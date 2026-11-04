//! Hands files, folders, and links to the desktop shell (Explorer, Finder, or the
//! freedesktop file manager / browser) through the official opener plugin.
use crate::error::{app_error, user_error, AppResult, ErrorCode};
use std::path::Path;
use url::Url;

pub(crate) fn open_external_url(url: &str, failure_message: &str) -> AppResult<()> {
    validate_external_url(url)?;
    tauri_plugin_opener::open_url(url, None::<&str>).map_err(|error| {
        app_error(ErrorCode::WindowsIntegrationFailed, failure_message)
            .with_detail(error.to_string())
    })
}

pub fn reveal(path: &Path) -> AppResult<()> {
    if !path.is_file() {
        return Err(app_error(
            ErrorCode::FileNotFound,
            "The file does not exist on disk",
        ));
    }
    let canonical = path.canonicalize()?;
    tauri_plugin_opener::reveal_item_in_dir(&canonical).map_err(|error| {
        app_error(
            ErrorCode::WindowsIntegrationFailed,
            "The file manager could not reveal the file",
        )
        .with_detail(error.to_string())
    })
}

pub(crate) fn open_folder(path: &Path, failure_message: &str) -> AppResult<()> {
    tauri_plugin_opener::open_path(path, None::<&str>).map_err(|error| {
        app_error(ErrorCode::WindowsIntegrationFailed, failure_message)
            .with_detail(error.to_string())
    })
}

fn validate_external_url(value: &str) -> AppResult<()> {
    if value.is_empty() || value.len() > 4096 || value.chars().any(char::is_control) {
        return Err(user_error("the external browser link is malformed"));
    }
    let parsed =
        Url::parse(value).map_err(|_| user_error("the external browser link is malformed"))?;
    if parsed.scheme() != "https"
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return Err(user_error(
            "the external browser link is not a safe HTTPS URL",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_browser_links_require_safe_https_urls() {
        assert!(
            validate_external_url("https://accounts.google.com/o/oauth2/v2/auth?state=abc").is_ok()
        );
        assert!(validate_external_url("https://www.youtube.com/watch?v=abc").is_ok());
        for invalid in [
            "",
            "http://accounts.google.com/",
            "file:///C:/Windows/System32/calc.exe",
            "https://user:password@example.com/",
            "https://example.com/\nnext",
            "not a url",
        ] {
            assert!(
                validate_external_url(invalid).is_err(),
                "accepted {invalid:?}"
            );
        }
    }
}
