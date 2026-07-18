use crate::db::Database;
use crate::error::{app_error, user_error, AppResult, ErrorCode};
use crate::models::ArchiveAccount;
use crate::state::AppState;
use crate::windows_integration::open_external_url;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use chrono::Utc;
use keyring::Entry;
use rand::{distr::Alphanumeric, Rng};
use reqwest::blocking::{Client, Response};
use reqwest::header::{CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE, LOCATION, RANGE};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::{Read, Seek, SeekFrom, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tauri::State;
use url::Url;

const CREDENTIAL_SERVICE: &str = "Reelspace Archive";
const CREDENTIAL_ACCOUNT: &str = "youtube-oauth";
const YOUTUBE_SCOPE: &str = "https://www.googleapis.com/auth/youtube.upload https://www.googleapis.com/auth/youtube.readonly";
const DEFAULT_CLIENT_ID: &str =
    "213662861772-slkhvmapt8hpv7r6f1ll3tv4gvcnfcd5.apps.googleusercontent.com";
const BUNDLED_CLIENT_ID: Option<&str> = option_env!("REELSPACE_YOUTUBE_CLIENT_ID");
const UPLOAD_CHUNK_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredCredentials {
    client_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    client_secret: Option<String>,
    refresh_token: String,
    access_token: String,
    expires_at: i64,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: i64,
    refresh_token: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct ArchiveWork {
    id: String,
    media_id: String,
    path: PathBuf,
    filename: String,
    total_bytes: u64,
    bytes_uploaded: u64,
    session_url: Option<String>,
    provider_remote_id: Option<String>,
    attempts: u32,
}

impl Database {
    pub fn get_youtube_remote_id(&self, media_id: &str) -> AppResult<String> {
        self.conn()?.query_row(
            "SELECT provider_remote_id FROM remote_assets WHERE media_id = ?1 AND provider = 'youtube' ORDER BY verified_at DESC LIMIT 1",
            [media_id], |row| row.get(0),
        ).optional()?.ok_or_else(|| user_error("no YouTube archive exists for this media item"))
    }

    pub fn remove_youtube_remote(&self, media_id: &str, remote_id: &str) -> AppResult<()> {
        let now = Utc::now().timestamp();
        let mut conn = self.conn()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute("DELETE FROM remote_assets WHERE media_id = ?1 AND provider = 'youtube' AND provider_remote_id = ?2", params![media_id, remote_id])?;
        tx.execute("UPDATE upload_jobs SET state = 'canceled', error = 'Remote copy deleted', updated_at = ?1 WHERE media_id = ?2 AND provider_remote_id = ?3", params![now, media_id, remote_id])?;
        tx.execute("INSERT INTO media_events (media_id, event, detail, created_at) VALUES (?1, 'remote_copy_deleted', ?2, ?3)", params![media_id, format!("YouTube video {remote_id}"), now])?;
        tx.commit()?;
        Ok(())
    }

    pub fn save_archive_account(&self, account: &ArchiveAccount) -> AppResult<()> {
        self.conn()?.execute(
            "INSERT OR REPLACE INTO archive_accounts (provider, account_id, display_name, connected_at) VALUES (?1, ?2, ?3, ?4)",
            params![account.provider, account.account_id, account.display_name, account.connected_at],
        )?;
        Ok(())
    }

    pub fn get_archive_account(&self) -> AppResult<Option<ArchiveAccount>> {
        self.conn()?.query_row(
            "SELECT provider, account_id, display_name, connected_at FROM archive_accounts WHERE provider = 'youtube'",
            [],
            |row| Ok(ArchiveAccount { provider: row.get(0)?, account_id: row.get(1)?, display_name: row.get(2)?, connected_at: row.get(3)? }),
        ).optional().map_err(Into::into)
    }

    pub fn delete_archive_account(&self) -> AppResult<()> {
        let conn = self.conn()?;
        conn.execute(
            "DELETE FROM archive_accounts WHERE provider = 'youtube'",
            [],
        )?;
        conn.execute("UPDATE upload_jobs SET state = 'needs_sign_in', error = 'Connect YouTube to continue', updated_at = ?1 WHERE state NOT IN ('archived', 'canceled')", [Utc::now().timestamp()])?;
        Ok(())
    }

    pub fn resume_authenticated_jobs(&self) -> AppResult<()> {
        self.conn()?.execute(
            "UPDATE upload_jobs SET state = 'queued', error = NULL, next_attempt_at = 0, updated_at = ?1 WHERE state = 'needs_sign_in'",
            [Utc::now().timestamp()],
        )?;
        Ok(())
    }

    pub(crate) fn claim_archive_work(&self) -> AppResult<Option<ArchiveWork>> {
        let mut conn = self.conn()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let now = Utc::now().timestamp();
        let work = tx.query_row(
            "SELECT uj.id, uj.media_id, m.path, m.filename, uj.total_bytes, uj.bytes_uploaded, uj.session_url, uj.provider_remote_id, uj.attempts
             FROM upload_jobs uj INNER JOIN media m ON m.id = uj.media_id
             WHERE uj.state IN ('queued', 'retry', 'processing_remote') AND uj.next_attempt_at <= ?1
             ORDER BY uj.priority DESC, uj.created_at LIMIT 1",
            [now],
            |row| Ok(ArchiveWork {
                id: row.get(0)?, media_id: row.get(1)?, path: PathBuf::from(row.get::<_, String>(2)?), filename: row.get(3)?,
                total_bytes: row.get::<_, i64>(4)?.max(0) as u64, bytes_uploaded: row.get::<_, i64>(5)?.max(0) as u64,
                session_url: row.get(6)?, provider_remote_id: row.get(7)?, attempts: row.get::<_, i64>(8)?.max(0) as u32,
            }),
        ).optional()?;
        if let Some(work) = &work {
            let state = if work.provider_remote_id.is_some() {
                "verifying"
            } else {
                "preparing"
            };
            tx.execute("UPDATE upload_jobs SET state = ?1, attempts = attempts + 1, updated_at = ?2 WHERE id = ?3", params![state, now, work.id])?;
        }
        tx.commit()?;
        Ok(work)
    }

    pub(crate) fn set_upload_session(&self, job_id: &str, session_url: &str) -> AppResult<()> {
        self.conn()?.execute("UPDATE upload_jobs SET session_url = ?1, state = 'uploading', updated_at = ?2 WHERE id = ?3", params![session_url, Utc::now().timestamp(), job_id])?;
        Ok(())
    }

    pub(crate) fn set_upload_progress(&self, job_id: &str, bytes: u64) -> AppResult<()> {
        self.conn()?.execute("UPDATE upload_jobs SET bytes_uploaded = ?1, state = 'uploading', updated_at = ?2 WHERE id = ?3", params![bytes as i64, Utc::now().timestamp(), job_id])?;
        Ok(())
    }

    pub(crate) fn set_remote_processing(&self, job_id: &str, remote_id: &str) -> AppResult<()> {
        let now = Utc::now().timestamp();
        self.conn()?.execute(
            "UPDATE upload_jobs SET provider_remote_id = ?1, bytes_uploaded = total_bytes, state = 'processing_remote', next_attempt_at = ?2, error = NULL, updated_at = ?3 WHERE id = ?4",
            params![remote_id, now + 30, now, job_id],
        )?;
        Ok(())
    }

    pub(crate) fn complete_archive(&self, work: &ArchiveWork) -> AppResult<()> {
        let remote_id = work
            .provider_remote_id
            .as_deref()
            .ok_or_else(|| user_error("the upload has no remote identifier"))?;
        let now = Utc::now().timestamp();
        let mut conn = self.conn()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "INSERT OR REPLACE INTO remote_assets (id, media_id, provider, provider_remote_id, browser_url, privacy_status, verification_status, processing_status, verified_at, last_checked_at)
             VALUES (?1, ?2, 'youtube', ?3, ?4, 'private', 'verified', 'processed', ?5, ?5)",
            params![format!("youtube:{remote_id}"), work.media_id, remote_id, format!("https://www.youtube.com/watch?v={remote_id}"), now],
        )?;
        tx.execute("UPDATE upload_jobs SET state = 'archived', error = NULL, next_attempt_at = 0, updated_at = ?1 WHERE id = ?2", params![now, work.id])?;
        tx.execute("INSERT INTO media_events (media_id, event, detail, created_at) VALUES (?1, 'archive_verified', ?2, ?3)", params![work.media_id, format!("YouTube video {remote_id}"), now])?;
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn reschedule_archive(
        &self,
        work: &ArchiveWork,
        state: &str,
        message: &str,
        delay_seconds: i64,
    ) -> AppResult<()> {
        let now = Utc::now().timestamp();
        self.conn()?.execute(
            "UPDATE upload_jobs SET state = ?1, error = ?2, next_attempt_at = ?3, updated_at = ?4 WHERE id = ?5",
            params![state, message, now + delay_seconds, now, work.id],
        )?;
        Ok(())
    }
}

#[tauri::command]
pub fn get_archive_account(state: State<'_, AppState>) -> AppResult<Option<ArchiveAccount>> {
    state.db.get_archive_account()
}

#[tauri::command]
pub async fn connect_youtube(state: State<'_, AppState>) -> AppResult<ArchiveAccount> {
    let client_id = bundled_oauth_client_id()?;
    let runtime = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || connect_youtube_inner(&runtime, client_id))
        .await
        .map_err(|error| {
            app_error(
                ErrorCode::Internal,
                "The YouTube connection task stopped unexpectedly",
            )
            .with_detail(error.to_string())
        })?
}

#[tauri::command]
pub async fn disconnect_youtube(state: State<'_, AppState>) -> AppResult<()> {
    let runtime = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || disconnect_youtube_inner(&runtime))
        .await
        .map_err(|error| {
            app_error(
                ErrorCode::Internal,
                "The YouTube disconnect task stopped unexpectedly",
            )
            .with_detail(error.to_string())
        })?
}

#[tauri::command]
pub async fn delete_youtube_remote(state: State<'_, AppState>, media_id: String) -> AppResult<()> {
    let runtime = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || delete_youtube_remote_inner(&runtime, &media_id))
        .await
        .map_err(|error| {
            app_error(
                ErrorCode::Internal,
                "The remote deletion task stopped unexpectedly",
            )
            .with_detail(error.to_string())
        })?
}

pub(crate) fn start_youtube_worker(state: AppState) {
    std::thread::Builder::new()
        .name("reelspace-youtube-archive".into())
        .spawn(move || loop {
            let pause_for_media = state
                .db
                .load_settings()
                .map(|settings| settings.pause_uploads_while_recording)
                .unwrap_or(true)
                && (state.media_jobs.active_count() > 0
                    || state.export_jobs.active_count() > 0
                    || state.background_jobs.active_count() > 0);
            if pause_for_media {
                std::thread::sleep(Duration::from_secs(3));
                continue;
            }
            match state.db.claim_archive_work() {
                Ok(Some(work)) => {
                    if let Err(error) = process_archive_work(&state, &work) {
                        state.telemetry.event(
                            "error",
                            "archive.job.failed",
                            json!({ "mediaId": work.media_id, "code": error.code }),
                        );
                        let delay = retry_delay(work.attempts);
                        let next_state = if error.code == ErrorCode::PermissionDenied {
                            "needs_sign_in"
                        } else if error.message.contains("allowance") {
                            "waiting_for_quota"
                        } else {
                            "retry"
                        };
                        let next_delay = if next_state == "waiting_for_quota" {
                            24 * 60 * 60
                        } else {
                            delay
                        };
                        let _ = state.db.reschedule_archive(
                            &work,
                            next_state,
                            &error.message,
                            next_delay,
                        );
                    }
                }
                Ok(None) => std::thread::sleep(Duration::from_secs(5)),
                Err(_) => std::thread::sleep(Duration::from_secs(10)),
            }
        })
        .ok();
}

fn connect_youtube_inner(state: &AppState, client_id: String) -> AppResult<ArchiveAccount> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|error| {
        app_error(
            ErrorCode::WindowsIntegrationFailed,
            "Reelspace could not start the local OAuth callback",
        )
        .with_detail(error.to_string())
    })?;
    let port = listener.local_addr()?.port();
    let redirect_uri = format!("http://127.0.0.1:{port}");
    let verifier = random_token(72);
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let expected_state = random_token(40);
    let mut authorize =
        Url::parse("https://accounts.google.com/o/oauth2/v2/auth").expect("static OAuth URL");
    authorize
        .query_pairs_mut()
        .append_pair("client_id", &client_id)
        .append_pair("redirect_uri", &redirect_uri)
        .append_pair("response_type", "code")
        .append_pair("scope", YOUTUBE_SCOPE)
        .append_pair("access_type", "offline")
        .append_pair("prompt", "consent")
        .append_pair("code_challenge", &challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", &expected_state);
    open_external_url(
        authorize.as_str(),
        "The Google authorization page could not be opened",
    )?;
    let (code, returned_state) = receive_oauth_callback(listener)?;
    if returned_state != expected_state {
        return Err(user_error(
            "the OAuth response did not match this Reelspace session",
        ));
    }
    let client = http_client()?;
    let response = client
        .post("https://oauth2.googleapis.com/token")
        .form(&[
            ("client_id", client_id.as_str()),
            ("code", code.as_str()),
            ("code_verifier", verifier.as_str()),
            ("grant_type", "authorization_code"),
            ("redirect_uri", redirect_uri.as_str()),
        ])
        .send()
        .map_err(network_error)?;
    let response = require_success(response, "Google rejected the OAuth token exchange")?;
    let token: TokenResponse = response.json().map_err(network_error)?;
    let refresh_token = token.refresh_token.ok_or_else(|| {
        user_error(
            "Google did not return an offline refresh token; revoke Reelspace access and try again",
        )
    })?;
    let credentials = StoredCredentials {
        client_id,
        client_secret: None,
        refresh_token,
        access_token: token.access_token,
        expires_at: Utc::now().timestamp() + token.expires_in,
    };
    let account = fetch_channel(&client, &credentials.access_token)?;
    save_credentials(&credentials)?;
    state.db.save_archive_account(&account)?;
    state.db.resume_authenticated_jobs()?;
    Ok(account)
}

fn disconnect_youtube_inner(state: &AppState) -> AppResult<()> {
    if let Ok(credentials) = load_credentials() {
        let _ = http_client()?
            .post("https://oauth2.googleapis.com/revoke")
            .form(&[("token", credentials.refresh_token)])
            .send();
    }
    let entry = credential_entry()?;
    if let Err(error) = entry.delete_credential() {
        if !matches!(error, keyring::Error::NoEntry) {
            return Err(credential_error(error));
        }
    }
    state.db.delete_archive_account()
}

fn delete_youtube_remote_inner(state: &AppState, media_id: &str) -> AppResult<()> {
    let remote_id = state.db.get_youtube_remote_id(media_id)?;
    let client = http_client()?;
    let mut credentials = load_credentials()?;
    refresh_access_token(&client, &mut credentials)?;
    let response = client
        .delete("https://www.googleapis.com/youtube/v3/videos")
        .bearer_auth(&credentials.access_token)
        .query(&[("id", remote_id.as_str())])
        .send()
        .map_err(network_error)?;
    if !response.status().is_success() && response.status().as_u16() != 404 {
        return Err(youtube_response_error(response));
    }
    state.db.remove_youtube_remote(media_id, &remote_id)
}

fn process_archive_work(state: &AppState, work: &ArchiveWork) -> AppResult<()> {
    let client = http_client()?;
    let mut credentials = load_credentials()?;
    refresh_access_token(&client, &mut credentials)?;
    if let Some(remote_id) = &work.provider_remote_id {
        return verify_remote(state, work, &client, &credentials.access_token, remote_id);
    }
    let metadata = std::fs::metadata(&work.path).map_err(|error| {
        app_error(
            ErrorCode::SourceUnavailable,
            "The queued local file is unavailable",
        )
        .with_detail(error.to_string())
    })?;
    if metadata.len() != work.total_bytes {
        return Err(app_error(
            ErrorCode::FileChanged,
            "The queued file changed after upload was scheduled",
        ));
    }
    let mut session = work.session_url.clone();
    let mut offset = work.bytes_uploaded;
    if let Some(url) = &session {
        match query_upload_offset(&client, url, &credentials.access_token, work.total_bytes)? {
            UploadOffset::Complete(remote_id) => {
                state.db.set_remote_processing(&work.id, &remote_id)?;
                return Ok(());
            }
            UploadOffset::Resume(server_offset) => offset = server_offset,
            UploadOffset::Expired => {
                session = None;
                offset = 0;
            }
        }
    }
    if session.is_none() {
        let url = begin_upload(&client, &credentials.access_token, work)?;
        state.db.set_upload_session(&work.id, &url)?;
        session = Some(url);
    }
    let url = session.expect("upload session initialized");
    let mut file = std::fs::File::open(&work.path)?;
    while offset < work.total_bytes {
        file.seek(SeekFrom::Start(offset))?;
        let chunk_len = (work.total_bytes - offset).min(UPLOAD_CHUNK_BYTES as u64) as usize;
        let mut chunk = vec![0_u8; chunk_len];
        file.read_exact(&mut chunk)?;
        let last = offset + chunk_len as u64 - 1;
        let response = client
            .put(&url)
            .bearer_auth(&credentials.access_token)
            .header(CONTENT_TYPE, "application/octet-stream")
            .header(CONTENT_LENGTH, chunk_len)
            .header(
                CONTENT_RANGE,
                format!("bytes {offset}-{last}/{}", work.total_bytes),
            )
            .body(chunk)
            .send()
            .map_err(network_error)?;
        if response.status().as_u16() == 308 {
            offset = response
                .headers()
                .get(RANGE)
                .and_then(|value| value.to_str().ok())
                .and_then(parse_uploaded_range)
                .unwrap_or(last + 1);
            state.db.set_upload_progress(&work.id, offset)?;
            continue;
        }
        if response.status().is_success() {
            let payload: Value = response.json().map_err(network_error)?;
            let remote_id = payload.get("id").and_then(Value::as_str).ok_or_else(|| {
                app_error(
                    ErrorCode::Internal,
                    "YouTube completed the upload without returning a video id",
                )
            })?;
            state.db.set_remote_processing(&work.id, remote_id)?;
            return Ok(());
        }
        return Err(youtube_response_error(response));
    }
    Ok(())
}

fn begin_upload(client: &Client, access_token: &str, work: &ArchiveWork) -> AppResult<String> {
    let title = Path::new(&work.filename)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("Reelspace recording");
    let response = client.post("https://www.googleapis.com/upload/youtube/v3/videos?uploadType=resumable&part=snippet,status")
        .bearer_auth(access_token).header(CONTENT_TYPE, "application/json; charset=UTF-8")
        .header("X-Upload-Content-Length", work.total_bytes).header("X-Upload-Content-Type", "application/octet-stream")
        .json(&json!({ "snippet": { "title": title, "description": "Private archive created by Reelspace", "categoryId": "20" }, "status": { "privacyStatus": "private", "embeddable": false } }))
        .send().map_err(network_error)?;
    let response = require_success(response, "YouTube rejected the resumable upload request")?;
    response
        .headers()
        .get(LOCATION)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
        .ok_or_else(|| {
            app_error(
                ErrorCode::Internal,
                "YouTube did not return a resumable upload location",
            )
        })
}

enum UploadOffset {
    Resume(u64),
    Complete(String),
    Expired,
}
fn query_upload_offset(
    client: &Client,
    url: &str,
    token: &str,
    total: u64,
) -> AppResult<UploadOffset> {
    let response = client
        .put(url)
        .bearer_auth(token)
        .header(CONTENT_LENGTH, 0)
        .header(CONTENT_RANGE, format!("bytes */{total}"))
        .send()
        .map_err(network_error)?;
    if response.status().as_u16() == 404 {
        return Ok(UploadOffset::Expired);
    }
    if response.status().as_u16() == 308 {
        let offset = response
            .headers()
            .get(RANGE)
            .and_then(|value| value.to_str().ok())
            .and_then(parse_uploaded_range)
            .unwrap_or(0);
        return Ok(UploadOffset::Resume(offset));
    }
    if response.status().is_success() {
        let payload: Value = response.json().map_err(network_error)?;
        if let Some(id) = payload.get("id").and_then(Value::as_str) {
            return Ok(UploadOffset::Complete(id.to_owned()));
        }
    }
    Err(app_error(
        ErrorCode::Internal,
        "YouTube reported a completed upload without a video id",
    ))
}

fn verify_remote(
    state: &AppState,
    work: &ArchiveWork,
    client: &Client,
    token: &str,
    remote_id: &str,
) -> AppResult<()> {
    let response = client
        .get("https://www.googleapis.com/youtube/v3/videos")
        .bearer_auth(token)
        .query(&[("part", "status"), ("id", remote_id)])
        .send()
        .map_err(network_error)?;
    let response = require_success(response, "YouTube archive verification failed")?;
    let payload: Value = response.json().map_err(network_error)?;
    let status = payload
        .pointer("/items/0/status/uploadStatus")
        .and_then(Value::as_str);
    let privacy = payload
        .pointer("/items/0/status/privacyStatus")
        .and_then(Value::as_str);
    match (status, privacy) {
        (Some("processed"), Some("private")) => state.db.complete_archive(work),
        (Some("failed" | "rejected"), _) => state.db.reschedule_archive(
            work,
            "failed",
            "YouTube rejected or failed remote processing",
            0,
        ),
        (None, _) => state.db.reschedule_archive(
            work,
            "remote_missing",
            "The uploaded YouTube video is no longer accessible",
            0,
        ),
        _ => state.db.reschedule_archive(
            work,
            "processing_remote",
            "YouTube is still processing the private upload",
            45,
        ),
    }
}

fn fetch_channel(client: &Client, token: &str) -> AppResult<ArchiveAccount> {
    let response = client
        .get("https://www.googleapis.com/youtube/v3/channels")
        .bearer_auth(token)
        .query(&[("part", "snippet"), ("mine", "true")])
        .send()
        .map_err(network_error)?;
    let response = require_success(
        response,
        "The connected Google account has no accessible YouTube channel",
    )?;
    let payload: Value = response.json().map_err(network_error)?;
    let item = payload.pointer("/items/0").ok_or_else(|| {
        user_error("the connected Google account does not have a YouTube channel")
    })?;
    Ok(ArchiveAccount {
        provider: "youtube".into(),
        account_id: item
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .into(),
        display_name: item
            .pointer("/snippet/title")
            .and_then(Value::as_str)
            .unwrap_or("YouTube")
            .into(),
        connected_at: Utc::now().timestamp(),
    })
}

fn refresh_access_token(client: &Client, credentials: &mut StoredCredentials) -> AppResult<()> {
    if credentials.expires_at > Utc::now().timestamp() + 90 {
        return Ok(());
    }
    let mut form = vec![
        ("client_id", credentials.client_id.as_str()),
        ("refresh_token", credentials.refresh_token.as_str()),
        ("grant_type", "refresh_token"),
    ];
    if let Some(secret) = credentials.client_secret.as_deref() {
        form.push(("client_secret", secret));
    }
    let response = client
        .post("https://oauth2.googleapis.com/token")
        .form(&form)
        .send()
        .map_err(network_error)?;
    let response = require_success(response, "Google sign-in expired; reconnect YouTube")?;
    let token: TokenResponse = response.json().map_err(network_error)?;
    credentials.access_token = token.access_token;
    credentials.expires_at = Utc::now().timestamp() + token.expires_in;
    save_credentials(credentials)
}

fn receive_oauth_callback(listener: TcpListener) -> AppResult<(String, String)> {
    listener.set_nonblocking(true)?;
    let deadline = Instant::now() + Duration::from_secs(180);
    loop {
        match listener.accept() {
            Ok((mut stream, _)) => {
                let mut buffer = [0_u8; 8192];
                let read = stream.read(&mut buffer)?;
                let request = String::from_utf8_lossy(&buffer[..read]);
                let target = request
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    .ok_or_else(|| user_error("the OAuth callback was malformed"))?;
                let url = Url::parse(&format!("http://127.0.0.1{target}"))
                    .map_err(|_| user_error("the OAuth callback URL was malformed"))?;
                let values = url
                    .query_pairs()
                    .collect::<std::collections::HashMap<_, _>>();
                let html = "<!doctype html><meta charset=utf-8><title>Reelspace connected</title><body style='font:16px system-ui;background:#0c0b12;color:#f4f1fa;padding:40px'><h1>Return to Reelspace</h1><p>You can close this tab.</p></body>";
                let response = format!("HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{html}", html.len());
                let _ = stream.write_all(response.as_bytes());
                if let Some(error) = values.get("error") {
                    return Err(user_error(format!(
                        "Google authorization was not completed: {error}"
                    )));
                }
                let code = values
                    .get("code")
                    .map(|value| value.to_string())
                    .ok_or_else(|| user_error("Google did not return an authorization code"))?;
                let state = values
                    .get("state")
                    .map(|value| value.to_string())
                    .ok_or_else(|| user_error("Google did not return the OAuth session state"))?;
                return Ok((code, state));
            }
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline =>
            {
                std::thread::sleep(Duration::from_millis(100))
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                return Err(user_error("Google authorization timed out"))
            }
            Err(error) => return Err(error.into()),
        }
    }
}

fn bundled_oauth_client_id() -> AppResult<String> {
    let client_id = BUNDLED_CLIENT_ID.unwrap_or(DEFAULT_CLIENT_ID).trim();
    validate_oauth_client_id(client_id).map_err(|error| {
        app_error(
            ErrorCode::InvalidRequest,
            "YouTube sign-in is not configured in this Reelspace build",
        )
        .with_detail(error.message)
    })?;
    Ok(client_id.to_owned())
}

fn validate_oauth_client_id(client_id: &str) -> AppResult<()> {
    let prefix = client_id
        .strip_suffix(".apps.googleusercontent.com")
        .unwrap_or_default();
    if client_id.is_empty()
        || client_id.len() > 300
        || client_id.trim() != client_id
        || prefix.is_empty()
        || !prefix
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        return Err(user_error("invalid Google Desktop OAuth client id"));
    }
    Ok(())
}

fn http_client() -> AppResult<Client> {
    Client::builder()
        .timeout(Duration::from_secs(90))
        .user_agent(concat!("Reelspace/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(network_error)
}
fn random_token(length: usize) -> String {
    rand::rng()
        .sample_iter(&Alphanumeric)
        .take(length)
        .map(char::from)
        .collect()
}
fn credential_entry() -> AppResult<Entry> {
    Entry::new(CREDENTIAL_SERVICE, CREDENTIAL_ACCOUNT).map_err(credential_error)
}
fn save_credentials(value: &StoredCredentials) -> AppResult<()> {
    credential_entry()?
        .set_password(&serde_json::to_string(value)?)
        .map_err(credential_error)
}
fn load_credentials() -> AppResult<StoredCredentials> {
    let raw = credential_entry()?
        .get_password()
        .map_err(credential_error)?;
    serde_json::from_str(&raw).map_err(|error| {
        app_error(
            ErrorCode::PermissionDenied,
            "Stored YouTube credentials are unreadable; reconnect the account",
        )
        .with_detail(error.to_string())
    })
}
fn credential_error(error: keyring::Error) -> crate::error::AppError {
    app_error(
        ErrorCode::PermissionDenied,
        "YouTube credentials could not be accessed in Windows Credential Manager",
    )
    .with_detail(error.to_string())
}
fn network_error(error: reqwest::Error) -> crate::error::AppError {
    app_error(
        ErrorCode::SourceUnavailable,
        "The YouTube service could not be reached",
    )
    .with_detail(error.to_string())
}
fn require_success(response: Response, message: &str) -> AppResult<Response> {
    if response.status().is_success() {
        Ok(response)
    } else {
        Err(youtube_response_error_with(response, message))
    }
}
fn youtube_response_error(response: Response) -> crate::error::AppError {
    youtube_response_error_with(response, "YouTube rejected the archive request")
}
fn youtube_response_error_with(response: Response, message: &str) -> crate::error::AppError {
    let status = response.status();
    let body = response.text().unwrap_or_default();
    let quota = body.contains("quotaExceeded") || body.contains("uploadLimitExceeded");
    let auth = status.as_u16() == 401 || body.contains("invalid_grant");
    let mut error = app_error(
        if auth {
            ErrorCode::PermissionDenied
        } else {
            ErrorCode::SourceUnavailable
        },
        if quota {
            "YouTube's upload allowance is exhausted; Reelspace will retry after the quota resets"
        } else {
            message
        },
    )
    .with_detail(format!(
        "HTTP {}: {}",
        status.as_u16(),
        redact_provider_error(&body)
    ));
    error.retryable = status.is_server_error() || quota;
    error
}
fn redact_provider_error(body: &str) -> String {
    body.chars()
        .filter(|ch| !ch.is_control() || *ch == ' ')
        .take(800)
        .collect()
}
fn parse_uploaded_range(value: &str) -> Option<u64> {
    value
        .rsplit_once('-')?
        .1
        .parse::<u64>()
        .ok()?
        .checked_add(1)
}
fn retry_delay(attempts: u32) -> i64 {
    (30_i64.saturating_mul(2_i64.saturating_pow(attempts.min(10)))).min(6 * 60 * 60)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resumable_ranges_continue_after_the_last_confirmed_byte() {
        assert_eq!(parse_uploaded_range("bytes=0-8388607"), Some(8_388_608));
    }
    #[test]
    fn retry_backoff_is_bounded() {
        assert_eq!(retry_delay(0), 30);
        assert_eq!(retry_delay(99), 6 * 60 * 60);
    }
    #[test]
    fn oauth_validation_accepts_only_well_formed_desktop_client_ids() {
        assert!(validate_oauth_client_id("123-abc.apps.googleusercontent.com").is_ok());
        for invalid in [
            "",
            "web.example",
            ".apps.googleusercontent.com",
            " 123.apps.googleusercontent.com",
            "123.apps.googleusercontent.com ",
            "123\n.apps.googleusercontent.com",
            "123/abc.apps.googleusercontent.com",
        ] {
            assert!(
                validate_oauth_client_id(invalid).is_err(),
                "accepted {invalid:?}"
            );
        }
    }

    #[test]
    fn legacy_stored_client_secrets_remain_readable() {
        let credentials: StoredCredentials = serde_json::from_value(json!({
            "client_id": "123.apps.googleusercontent.com",
            "client_secret": "legacy-secret",
            "refresh_token": "refresh",
            "access_token": "access",
            "expires_at": 1
        }))
        .unwrap();
        assert_eq!(credentials.client_secret.as_deref(), Some("legacy-secret"));
    }
}
