use crate::error::{user_error, AppResult};
use crate::models::{MediaMetadata, ToolHealth};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::Command;
use tauri::{path::BaseDirectory, AppHandle, Manager};

#[derive(Debug, Clone)]
pub struct Ffmpeg {
    ffmpeg_path: Option<PathBuf>,
    ffprobe_path: Option<PathBuf>,
}

impl Ffmpeg {
    pub fn discover(app: &AppHandle) -> Self {
        let bundled_ffmpeg = app
            .path()
            .resolve("vendor/ffmpeg/bin/ffmpeg.exe", BaseDirectory::Resource)
            .ok()
            .filter(|path| path.exists());
        let bundled_ffprobe = app
            .path()
            .resolve("vendor/ffmpeg/bin/ffprobe.exe", BaseDirectory::Resource)
            .ok()
            .filter(|path| path.exists());

        let ffmpeg_path = bundled_ffmpeg
            .or_else(|| first_existing(["vendor/ffmpeg/bin/ffmpeg.exe", "../vendor/ffmpeg/bin/ffmpeg.exe"]))
            .or_else(|| which::which("ffmpeg").ok());
        let ffprobe_path = bundled_ffprobe
            .or_else(|| first_existing(["vendor/ffmpeg/bin/ffprobe.exe", "../vendor/ffmpeg/bin/ffprobe.exe"]))
            .or_else(|| which::which("ffprobe").ok());

        Self {
            ffmpeg_path,
            ffprobe_path,
        }
    }

    pub fn health(&self) -> ToolHealth {
        let ready = self.ffmpeg_path.is_some() && self.ffprobe_path.is_some();
        ToolHealth {
            ffmpeg_path: self.ffmpeg_path.as_ref().map(display_path),
            ffprobe_path: self.ffprobe_path.as_ref().map(display_path),
            ready,
            message: if ready {
                "FFmpeg and FFprobe are ready".to_string()
            } else {
                "Copy ffmpeg.exe and ffprobe.exe into vendor/ffmpeg/bin".to_string()
            },
        }
    }

    pub fn probe(&self, path: &Path) -> AppResult<MediaMetadata> {
        let ffprobe = self
            .ffprobe_path
            .as_ref()
            .ok_or_else(|| user_error("FFprobe is not configured"))?;
        let output = Command::new(ffprobe)
            .args([
                "-v",
                "error",
                "-print_format",
                "json",
                "-show_format",
                "-show_streams",
            ])
            .arg(path)
            .output()?;
        if !output.status.success() {
            return Ok(MediaMetadata::default());
        }
        let response: ProbeResponse = serde_json::from_slice(&output.stdout)?;
        Ok(response.into_metadata())
    }

    pub fn thumbnail(&self, source: &Path, output_path: &Path, at_seconds: f64) -> AppResult<()> {
        let ffmpeg = self
            .ffmpeg_path
            .as_ref()
            .ok_or_else(|| user_error("FFmpeg is not configured"))?;
        if let Some(parent) = output_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let timestamp = format!("{:.3}", at_seconds.max(0.0));
        let output = Command::new(ffmpeg)
            .args(["-y", "-hide_banner", "-loglevel", "error", "-ss", &timestamp])
            .arg(source)
            .args(["-frames:v", "1", "-vf", "scale=480:-1"])
            .arg(output_path)
            .output()?;
        if output.status.success() {
            Ok(())
        } else {
            Err(user_error(String::from_utf8_lossy(&output.stderr).to_string()))
        }
    }

    pub fn trim(
        &self,
        source: &Path,
        output_path: &Path,
        start_seconds: f64,
        end_seconds: f64,
        accurate: bool,
    ) -> AppResult<()> {
        let ffmpeg = self
            .ffmpeg_path
            .as_ref()
            .ok_or_else(|| user_error("FFmpeg is not configured"))?;
        if end_seconds <= start_seconds {
            return Err(user_error("trim end must be after trim start"));
        }
        if let Some(parent) = output_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let start = format!("{:.3}", start_seconds.max(0.0));
        let duration = format!("{:.3}", end_seconds - start_seconds);
        let mut command = Command::new(ffmpeg);
        command.args(["-y", "-hide_banner", "-loglevel", "error", "-ss", &start]);
        command.arg("-i").arg(source);
        command.args(["-t", &duration]);
        if accurate {
            command.args([
                "-c:v",
                "h264_nvenc",
                "-preset",
                "p5",
                "-cq",
                "18",
                "-c:a",
                "aac",
                "-b:a",
                "192k",
            ]);
        } else {
            command.args(["-c", "copy", "-avoid_negative_ts", "make_zero"]);
        }
        command.arg(output_path);

        let output = command.output()?;
        if output.status.success() {
            Ok(())
        } else if accurate {
            Err(user_error(String::from_utf8_lossy(&output.stderr).to_string()))
        } else {
            Err(user_error(format!(
                "lossless trim failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )))
        }
    }
}

fn first_existing<const N: usize>(candidates: [&str; N]) -> Option<PathBuf> {
    candidates
        .into_iter()
        .map(PathBuf::from)
        .find(|path| path.exists())
}

fn display_path(path: &PathBuf) -> String {
    path.to_string_lossy().to_string()
}

#[derive(Debug, Deserialize)]
struct ProbeResponse {
    streams: Vec<ProbeStream>,
    format: Option<ProbeFormat>,
}

impl ProbeResponse {
    fn into_metadata(self) -> MediaMetadata {
        let video = self.streams.iter().find(|stream| stream.codec_type == "video");
        let duration = self
            .format
            .and_then(|format| format.duration)
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
        }
    }
}

#[derive(Debug, Deserialize)]
struct ProbeFormat {
    duration: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ProbeStream {
    codec_type: String,
    codec_name: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    duration: Option<String>,
}
