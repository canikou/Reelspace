use crate::error::{app_error, user_error, AppError, AppResult, ErrorCode};
use crate::models::{AudioTrack, MediaMetadata, ProbeData, ToolHealth};
use serde::Deserialize;
use std::io::Read;
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{path::BaseDirectory, AppHandle, Manager};

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug, Clone)]
pub struct Ffmpeg {
    ffmpeg_path: Option<PathBuf>,
    ffprobe_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy)]
pub enum AudioPreviewMethod {
    Cache,
    Copy,
    Transcode,
}

impl AudioPreviewMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cache => "cache",
            Self::Copy => "copy",
            Self::Transcode => "transcode",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum TrimMethod {
    Nvenc,
    Cpu,
    StreamCopy,
}

impl TrimMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Nvenc => "nvenc",
            Self::Cpu => "libx264",
            Self::StreamCopy => "stream_copy",
        }
    }
}

impl Ffmpeg {
    /// Creates a tool runner from explicit executable paths. This is used by
    /// diagnostics and benchmark harnesses that run without a Tauri app handle.
    pub fn from_paths(ffmpeg_path: PathBuf, ffprobe_path: PathBuf) -> AppResult<Self> {
        if !ffmpeg_path.is_file() {
            return Err(app_error(
                ErrorCode::FfmpegUnavailable,
                "The configured FFmpeg executable was not found",
            ));
        }
        if !ffprobe_path.is_file() {
            return Err(app_error(
                ErrorCode::FfprobeUnavailable,
                "The configured FFprobe executable was not found",
            ));
        }
        Ok(Self {
            ffmpeg_path: Some(ffmpeg_path),
            ffprobe_path: Some(ffprobe_path),
        })
    }

    pub fn discover(app: &AppHandle) -> Self {
        let ffmpeg_name = format!("ffmpeg{}", std::env::consts::EXE_SUFFIX);
        let ffprobe_name = format!("ffprobe{}", std::env::consts::EXE_SUFFIX);
        let bundled = |name: &str| {
            app.path()
                .resolve(format!("vendor/ffmpeg/bin/{name}"), BaseDirectory::Resource)
                .ok()
                .filter(|path| path.exists())
        };
        let bundled_ffmpeg = bundled(&ffmpeg_name);
        let bundled_ffprobe = bundled(&ffprobe_name);

        // `cargo tauri dev` and packaged builds do not share a predictable
        // working directory. Anchor the development fallback to this crate so
        // media features keep working regardless of how Reelspace was launched.
        #[cfg(debug_assertions)]
        let (ffmpeg_path, ffprobe_path) = {
            let manifest_vendor = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("vendor")
                .join("ffmpeg")
                .join("bin");
            (
                bundled_ffmpeg
                    .or_else(|| existing_file(manifest_vendor.join(&ffmpeg_name)))
                    .or_else(|| {
                        existing_file(PathBuf::from("vendor/ffmpeg/bin").join(&ffmpeg_name))
                    })
                    .or_else(|| which::which("ffmpeg").ok()),
                bundled_ffprobe
                    .or_else(|| existing_file(manifest_vendor.join(&ffprobe_name)))
                    .or_else(|| {
                        existing_file(PathBuf::from("vendor/ffmpeg/bin").join(&ffprobe_name))
                    })
                    .or_else(|| which::which("ffprobe").ok()),
            )
        };
        #[cfg(not(debug_assertions))]
        let (ffmpeg_path, ffprobe_path) = (bundled_ffmpeg, bundled_ffprobe);

        Self {
            ffmpeg_path,
            ffprobe_path,
        }
    }

    pub fn health(&self) -> ToolHealth {
        let ready = self.ffmpeg_path.is_some() && self.ffprobe_path.is_some();
        ToolHealth {
            ffmpeg_path: self.ffmpeg_path.as_deref().map(display_path),
            ffprobe_path: self.ffprobe_path.as_deref().map(display_path),
            ready,
            message: if ready {
                "FFmpeg and FFprobe are ready".to_string()
            } else {
                format!(
                    "Copy ffmpeg{0} and ffprobe{0} into vendor/ffmpeg/bin",
                    std::env::consts::EXE_SUFFIX
                )
            },
        }
    }

    pub fn probe(&self, path: &Path) -> AppResult<MediaMetadata> {
        Ok(self.probe_data(path)?.metadata)
    }

    pub fn probe_data(&self, path: &Path) -> AppResult<ProbeData> {
        Ok(self.probe_response(path, None)?.into_probe_data())
    }

    pub fn probe_data_cancellable(
        &self,
        path: &Path,
        cancelled: &AtomicBool,
    ) -> AppResult<ProbeData> {
        Ok(self
            .probe_response(path, Some(cancelled))?
            .into_probe_data())
    }

    pub fn audio_tracks(&self, path: &Path) -> AppResult<Vec<AudioTrack>> {
        Ok(self.probe_response(path, None)?.audio_tracks())
    }

    fn probe_response(
        &self,
        path: &Path,
        cancelled: Option<&AtomicBool>,
    ) -> AppResult<ProbeResponse> {
        let ffprobe = self.ffprobe_path.as_ref().ok_or_else(|| {
            app_error(
                ErrorCode::FfprobeUnavailable,
                "FFprobe is not available in the Reelspace installation",
            )
        })?;
        let mut command = media_command(ffprobe);
        command
            .args([
                "-v",
                "error",
                "-print_format",
                "json",
                "-show_format",
                "-show_streams",
            ])
            .arg(path);
        let output =
            run_with_timeout_cancellable(command, Duration::from_secs(30), "FFprobe", cancelled)?;
        if !output.success {
            return Err(app_error(
                ErrorCode::FfprobeFailed,
                "FFprobe could not read this media file",
            )
            .with_detail(String::from_utf8_lossy(&output.stderr).trim()));
        }
        serde_json::from_slice(&output.stdout).map_err(|error| {
            app_error(
                ErrorCode::CorruptMedia,
                "FFprobe returned invalid metadata for this media file",
            )
            .with_detail(error.to_string())
        })
    }

    pub fn thumbnail_cancellable(
        &self,
        source: &Path,
        output_path: &Path,
        at_seconds: f64,
        cancelled: Option<&AtomicBool>,
    ) -> AppResult<()> {
        let ffmpeg = self.ffmpeg_path.as_ref().ok_or_else(|| {
            app_error(
                ErrorCode::FfmpegUnavailable,
                "FFmpeg is not available in the Reelspace installation",
            )
        })?;
        if let Some(parent) = output_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let timestamp = format!("{:.3}", at_seconds.max(0.0));
        let command = thumbnail_command(ffmpeg, source, output_path, &timestamp);
        let output = run_with_timeout_cancellable(
            command,
            Duration::from_secs(30),
            "Thumbnail generation",
            cancelled,
        )?;
        if output.success {
            Ok(())
        } else {
            Err(app_error(
                ErrorCode::ThumbnailGenerationFailed,
                "FFmpeg could not generate a thumbnail",
            )
            .with_detail(String::from_utf8_lossy(&output.stderr)))
        }
    }

    pub fn audio_preview(
        &self,
        source: &Path,
        output_path: &Path,
        stream_index: u32,
        cancelled: &AtomicBool,
    ) -> AppResult<AudioPreviewMethod> {
        let ffmpeg = self.ffmpeg_path.as_ref().ok_or_else(|| {
            app_error(
                ErrorCode::FfmpegUnavailable,
                "FFmpeg is not available in the Reelspace installation",
            )
        })?;
        if is_usable_media_file(output_path) {
            return Ok(AudioPreviewMethod::Cache);
        }
        if let Some(parent) = output_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let temp_path = unique_audio_temp_path(output_path);
        let run = |codec_args: &[&str]| -> AppResult<ProcessResult> {
            let mut command = media_command(ffmpeg);
            command.args(["-y", "-hide_banner", "-loglevel", "error", "-i"]);
            command.arg(source);
            command.arg("-map").arg(format!("0:{stream_index}"));
            command
                .arg("-vn")
                .args(codec_args)
                .args(["-movflags", "+faststart"]);
            command.arg(&temp_path);
            let result = run_cancellable(command, cancelled);
            if result.is_err() {
                let _ = std::fs::remove_file(&temp_path);
            }
            result
        };

        let mut method = AudioPreviewMethod::Transcode;
        let mut result = None;
        let copied = run(&["-c:a", "copy"])?;
        if copied.success && is_usable_media_file(&temp_path) {
            method = AudioPreviewMethod::Copy;
            result = Some(copied);
        } else {
            let _ = std::fs::remove_file(&temp_path);
        }
        let output = match result {
            Some(output) => output,
            None => run(&["-c:a", "aac", "-b:a", "160k"])?,
        };
        if !output.success {
            let _ = std::fs::remove_file(&temp_path);
            return Err(app_error(
                ErrorCode::FfmpegFailed,
                "FFmpeg could not prepare the audio preview",
            )
            .with_detail(output.stderr.trim()));
        }
        if !is_usable_media_file(&temp_path) {
            let _ = std::fs::remove_file(&temp_path);
            return Err(app_error(
                ErrorCode::FfmpegFailed,
                "FFmpeg produced an invalid audio preview",
            ));
        }
        if output_path.exists() {
            std::fs::remove_file(output_path)?;
        }
        if let Err(error) = std::fs::rename(&temp_path, output_path) {
            let _ = std::fs::remove_file(&temp_path);
            return Err(error.into());
        }
        Ok(method)
    }

    pub fn trim(
        &self,
        source: &Path,
        output_path: &Path,
        start_seconds: f64,
        end_seconds: f64,
        accurate: bool,
        audio_track_indices: &[u32],
    ) -> AppResult<TrimMethod> {
        let ffmpeg = self.ffmpeg_path.as_ref().ok_or_else(|| {
            app_error(
                ErrorCode::FfmpegUnavailable,
                "FFmpeg is not available in the Reelspace installation",
            )
        })?;
        if end_seconds <= start_seconds {
            return Err(user_error("trim end must be after trim start"));
        }
        if let Some(parent) = output_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let start = format!("{:.3}", start_seconds.max(0.0));
        let duration = format!("{:.3}", end_seconds - start_seconds);
        let timeout = Duration::from_secs_f64(
            ((end_seconds - start_seconds) * 4.0 + 60.0).clamp(120.0, 7_200.0),
        );
        let run = |video_args: &[&str]| -> AppResult<CapturedProcessResult> {
            let mut command = media_command(ffmpeg);
            command.args(["-y", "-hide_banner", "-loglevel", "error", "-ss", &start]);
            command.arg("-i").arg(source);
            command.args(["-t", &duration]);
            command.args(["-map", "0:v:0"]);
            match audio_track_indices {
                [] => {
                    command.arg("-an");
                }
                [index] => {
                    command.arg("-map").arg(format!("0:{index}"));
                }
                indices => {
                    let inputs = indices
                        .iter()
                        .map(|index| format!("[0:{index}]"))
                        .collect::<String>();
                    let filter = format!(
                        "{inputs}amix=inputs={}:duration=longest:dropout_transition=0:normalize=1[aout]",
                        indices.len()
                    );
                    command.args(["-filter_complex", &filter, "-map", "[aout]"]);
                }
            }
            command.args(video_args);
            command.args(["-map_metadata", "0", "-movflags", "+faststart"]);
            command.arg(output_path);
            run_with_timeout(command, timeout, "clip export")
        };
        let (output, method) = if accurate {
            let nvenc = run(&[
                "-c:v",
                "h264_nvenc",
                "-preset",
                "p5",
                "-cq",
                "18",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
                "-b:a",
                "192k",
            ])?;
            if nvenc.success {
                (nvenc, TrimMethod::Nvenc)
            } else {
                (
                    run(&[
                        "-c:v", "libx264", "-preset", "medium", "-crf", "18", "-pix_fmt",
                        "yuv420p", "-c:a", "aac", "-b:a", "192k",
                    ])?,
                    TrimMethod::Cpu,
                )
            }
        } else {
            (
                run(&["-c", "copy", "-avoid_negative_ts", "make_zero"])?,
                TrimMethod::StreamCopy,
            )
        };
        if output.success {
            Ok(method)
        } else if accurate {
            Err(
                app_error(ErrorCode::ExportFailed, "FFmpeg could not export the clip")
                    .with_detail(String::from_utf8_lossy(&output.stderr)),
            )
        } else {
            Err(app_error(
                ErrorCode::ExportFailed,
                "FFmpeg could not export the lossless clip",
            )
            .with_detail(String::from_utf8_lossy(&output.stderr)))
        }
    }
}

fn thumbnail_command(ffmpeg: &Path, source: &Path, output_path: &Path, timestamp: &str) -> Command {
    let mut command = media_command(ffmpeg);
    command.args([
        "-y",
        "-hide_banner",
        "-loglevel",
        "error",
        "-ss",
        timestamp,
        "-i",
    ]);
    command.arg(source);
    command.args([
        "-map",
        "0:v:0",
        "-frames:v",
        "1",
        "-vf",
        "scale=480:-2",
        "-q:v",
        "3",
    ]);
    command.arg(output_path);
    command
}

fn media_command(executable: &Path) -> Command {
    #[cfg_attr(not(windows), allow(unused_mut))]
    let mut command = Command::new(executable);
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);
    command
}

struct ProcessResult {
    success: bool,
    stderr: String,
}

struct CapturedProcessResult {
    success: bool,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

fn run_with_timeout(
    command: Command,
    timeout: Duration,
    operation: &str,
) -> AppResult<CapturedProcessResult> {
    run_with_timeout_cancellable(command, timeout, operation, None)
}

fn run_with_timeout_cancellable(
    mut command: Command,
    timeout: Duration,
    operation: &str,
    cancelled: Option<&AtomicBool>,
) -> AppResult<CapturedProcessResult> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn()?;
    let stdout_reader = child
        .stdout
        .take()
        .map(|pipe| thread::spawn(move || drain_reader(pipe, 8 * 1024 * 1024)));
    let stderr_reader = child
        .stderr
        .take()
        .map(|pipe| thread::spawn(move || drain_reader(pipe, 16 * 1024)));
    let started = Instant::now();
    loop {
        if cancelled.is_some_and(|flag| flag.load(Ordering::Acquire)) {
            let _ = child.kill();
            let _ = child.wait();
            if let Some(reader) = stdout_reader {
                let _ = reader.join();
            }
            if let Some(reader) = stderr_reader {
                let _ = reader.join();
            }
            return Err(AppError::cancelled(operation));
        }
        if let Some(status) = child.try_wait()? {
            let stdout = stdout_reader
                .and_then(|reader| reader.join().ok())
                .unwrap_or_default();
            let stderr = stderr_reader
                .and_then(|reader| reader.join().ok())
                .unwrap_or_default();
            return Ok(CapturedProcessResult {
                success: status.success(),
                stdout,
                stderr,
            });
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            if let Some(reader) = stdout_reader {
                let _ = reader.join();
            }
            if let Some(reader) = stderr_reader {
                let _ = reader.join();
            }
            return Err(
                AppError::timeout(operation).with_detail(format!("timeout={}s", timeout.as_secs()))
            );
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn run_cancellable(mut command: Command, cancelled: &AtomicBool) -> AppResult<ProcessResult> {
    command.stdout(Stdio::null()).stderr(Stdio::piped());
    let mut child = command.spawn()?;
    let stderr_reader = child
        .stderr
        .take()
        .map(|pipe| thread::spawn(move || drain_reader(pipe, 4096)));
    let started = Instant::now();
    loop {
        if cancelled.load(Ordering::Acquire) {
            let _ = child.kill();
            let _ = child.wait();
            if let Some(reader) = stderr_reader {
                let _ = reader.join();
            }
            return Err(AppError::cancelled("Audio preview"));
        }
        if started.elapsed() >= Duration::from_secs(30 * 60) {
            let _ = child.kill();
            let _ = child.wait();
            if let Some(reader) = stderr_reader {
                let _ = reader.join();
            }
            return Err(AppError::timeout("Audio preview").with_detail("timeout=1800s"));
        }
        if let Some(status) = child.try_wait()? {
            let stderr = stderr_reader
                .and_then(|reader| reader.join().ok())
                .unwrap_or_default();
            return Ok(ProcessResult {
                success: status.success(),
                stderr: String::from_utf8_lossy(&stderr).into_owned(),
            });
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn drain_reader(mut reader: impl Read, capture_limit: usize) -> Vec<u8> {
    let mut captured = Vec::with_capacity(capture_limit.min(4096));
    let mut buffer = [0_u8; 4096];
    while let Ok(read) = reader.read(&mut buffer) {
        if read == 0 {
            break;
        }
        let remaining = capture_limit.saturating_sub(captured.len());
        captured.extend_from_slice(&buffer[..read.min(remaining)]);
    }
    captured
}

fn is_usable_media_file(path: &Path) -> bool {
    path.metadata()
        .map(|metadata| metadata.is_file() && metadata.len() > 1024)
        .unwrap_or(false)
}

fn unique_audio_temp_path(output_path: &Path) -> PathBuf {
    static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(1);
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_micros())
        .unwrap_or_default();
    let stem = output_path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("audio-preview");
    output_path.with_file_name(format!(
        ".{stem}-{}-{nonce}-{sequence}.part.m4a",
        std::process::id()
    ))
}

#[cfg(debug_assertions)]
fn existing_file(path: PathBuf) -> Option<PathBuf> {
    path.is_file().then_some(path)
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

#[derive(Debug, Deserialize)]
struct ProbeResponse {
    streams: Vec<ProbeStream>,
    format: Option<ProbeFormat>,
}

impl ProbeResponse {
    fn into_probe_data(self) -> ProbeData {
        let audio_tracks = self.audio_tracks();
        let metadata = self.into_metadata();
        ProbeData {
            metadata,
            audio_tracks,
        }
    }

    fn audio_tracks(&self) -> Vec<AudioTrack> {
        self.streams
            .iter()
            .filter(|stream| stream.codec_type == "audio")
            .map(|stream| AudioTrack {
                index: stream.index,
                codec: stream.codec_name.clone(),
                channels: stream.channels,
                language: stream
                    .tags
                    .as_ref()
                    .and_then(|tags| tags.language.clone())
                    .filter(|language| !language.eq_ignore_ascii_case("und")),
                title: stream
                    .tags
                    .as_ref()
                    .and_then(|tags| tags.name.clone().or_else(|| tags.handler_name.clone())),
                is_default: stream
                    .disposition
                    .as_ref()
                    .map(|disposition| disposition.default == 1)
                    .unwrap_or(false),
            })
            .collect()
    }

    fn into_metadata(self) -> MediaMetadata {
        let video = self
            .streams
            .iter()
            .find(|stream| stream.codec_type == "video");
        let format = self.format;
        let duration = format
            .as_ref()
            .and_then(|format| format.duration.as_ref())
            .and_then(|value| value.parse::<f64>().ok())
            .unwrap_or_else(|| {
                video
                    .and_then(|stream| stream.duration.as_ref())
                    .and_then(|value| value.parse::<f64>().ok())
                    .unwrap_or_default()
            });
        MediaMetadata {
            duration_seconds: duration,
            width: video.and_then(|stream| stream.width),
            height: video.and_then(|stream| stream.height),
            codec: video.and_then(|stream| stream.codec_name.clone()),
            container: format
                .as_ref()
                .and_then(|format| format.format_name.clone()),
            frame_rate: video
                .and_then(|stream| stream.avg_frame_rate.as_deref())
                .and_then(parse_frame_rate),
            rotation_degrees: video
                .and_then(|stream| stream.tags.as_ref())
                .and_then(|tags| tags.rotate.as_deref())
                .and_then(|value| value.parse::<i32>().ok()),
            creation_timestamp: format
                .as_ref()
                .and_then(|format| format.tags.as_ref())
                .and_then(|tags| tags.creation_time.clone()),
        }
    }
}

#[derive(Debug, Deserialize)]
struct ProbeFormat {
    duration: Option<String>,
    format_name: Option<String>,
    tags: Option<ProbeTags>,
}

#[derive(Debug, Deserialize)]
struct ProbeStream {
    index: u32,
    codec_type: String,
    codec_name: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    channels: Option<u32>,
    duration: Option<String>,
    avg_frame_rate: Option<String>,
    tags: Option<ProbeTags>,
    disposition: Option<ProbeDisposition>,
}

#[derive(Debug, Deserialize)]
struct ProbeTags {
    language: Option<String>,
    handler_name: Option<String>,
    name: Option<String>,
    rotate: Option<String>,
    creation_time: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ProbeDisposition {
    default: u8,
}

fn parse_frame_rate(value: &str) -> Option<f64> {
    let (numerator, denominator) = value.split_once('/')?;
    let numerator = numerator.parse::<f64>().ok()?;
    let denominator = denominator.parse::<f64>().ok()?;
    (denominator > 0.0)
        .then_some(numerator / denominator)
        .filter(|rate| rate.is_finite() && *rate > 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn media_processes_are_created_without_a_console_window() {
        const CHILD_MARKER: &str = "REELSPACE_NO_CONSOLE_CHILD";
        if std::env::var_os(CHILD_MARKER).is_some() {
            let console = unsafe { windows_sys::Win32::System::Console::GetConsoleWindow() };
            assert_eq!(console, std::ptr::null_mut());
            return;
        }

        let executable = std::env::current_exe().expect("current test executable");
        let mut command = media_command(&executable);
        let output = command
            .env(CHILD_MARKER, "1")
            .args([
                "--exact",
                "ffmpeg::tests::media_processes_are_created_without_a_console_window",
                "--nocapture",
            ])
            .output()
            .expect("spawn hidden child test");
        assert!(
            output.status.success(),
            "hidden child failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn thumbnail_source_is_an_explicit_ffmpeg_input() {
        let command = thumbnail_command(
            Path::new("ffmpeg.exe"),
            Path::new("source.mp4"),
            Path::new("thumbnail.jpg"),
            "5.000",
        );
        let args = command
            .get_args()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        let source_index = args
            .iter()
            .position(|argument| argument == "source.mp4")
            .expect("source argument");
        assert_eq!(args.get(source_index - 1).map(String::as_str), Some("-i"));
    }

    #[test]
    fn frame_rates_are_parsed_defensively() {
        assert_eq!(parse_frame_rate("60000/1001").unwrap().round(), 60.0);
        assert_eq!(parse_frame_rate("0/0"), None);
        assert_eq!(parse_frame_rate("invalid"), None);
    }
}
