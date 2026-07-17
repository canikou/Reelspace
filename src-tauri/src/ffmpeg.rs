use crate::error::{user_error, AppResult};
use crate::models::{AudioTrack, MediaMetadata, ToolHealth};
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

        // `cargo tauri dev` and packaged builds do not share a predictable
        // working directory. Anchor the development fallback to this crate so
        // media features keep working regardless of how Reelspace was launched.
        #[cfg(debug_assertions)]
        let manifest_vendor = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("vendor")
            .join("ffmpeg")
            .join("bin");
        #[cfg(debug_assertions)]
        let manifest_ffmpeg = existing_file(manifest_vendor.join("ffmpeg.exe"));
        #[cfg(debug_assertions)]
        let manifest_ffprobe = existing_file(manifest_vendor.join("ffprobe.exe"));
        #[cfg(not(debug_assertions))]
        let manifest_ffmpeg = None;
        #[cfg(not(debug_assertions))]
        let manifest_ffprobe = None;

        let ffmpeg_path = bundled_ffmpeg
            .or(manifest_ffmpeg)
            .or_else(|| {
                first_existing([
                    "vendor/ffmpeg/bin/ffmpeg.exe",
                    "../vendor/ffmpeg/bin/ffmpeg.exe",
                ])
            })
            .or_else(|| which::which("ffmpeg").ok());
        let ffprobe_path = bundled_ffprobe
            .or(manifest_ffprobe)
            .or_else(|| {
                first_existing([
                    "vendor/ffmpeg/bin/ffprobe.exe",
                    "../vendor/ffmpeg/bin/ffprobe.exe",
                ])
            })
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
        Ok(self.probe_response(path)?.into_metadata())
    }

    pub fn audio_tracks(&self, path: &Path) -> AppResult<Vec<AudioTrack>> {
        Ok(self.probe_response(path)?.audio_tracks())
    }

    fn probe_response(&self, path: &Path) -> AppResult<ProbeResponse> {
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
            return Err(user_error(format!(
                "FFprobe failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        serde_json::from_slice(&output.stdout).map_err(Into::into)
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
            .args([
                "-y",
                "-hide_banner",
                "-loglevel",
                "error",
                "-ss",
                &timestamp,
            ])
            .arg(source)
            .args([
                "-map",
                "0:v:0",
                "-frames:v",
                "1",
                "-vf",
                "scale=480:-2",
                "-q:v",
                "3",
            ])
            .arg(output_path)
            .output()?;
        if output.status.success() {
            Ok(())
        } else {
            Err(user_error(
                String::from_utf8_lossy(&output.stderr).to_string(),
            ))
        }
    }

    pub fn audio_preview(
        &self,
        source: &Path,
        output_path: &Path,
        stream_index: u32,
    ) -> AppResult<()> {
        let ffmpeg = self
            .ffmpeg_path
            .as_ref()
            .ok_or_else(|| user_error("FFmpeg is not configured"))?;
        if output_path
            .metadata()
            .map(|metadata| metadata.is_file() && metadata.len() > 1024)
            .unwrap_or(false)
        {
            return Ok(());
        }
        if let Some(parent) = output_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let temp_path = output_path.with_extension("part.m4a");
        let _ = std::fs::remove_file(&temp_path);
        let mut command = Command::new(ffmpeg);
        command.args(["-y", "-hide_banner", "-loglevel", "error", "-i"]);
        command.arg(source);
        command.arg("-map").arg(format!("0:{stream_index}"));
        command.args([
            "-vn",
            "-c:a",
            "aac",
            "-b:a",
            "160k",
            "-movflags",
            "+faststart",
        ]);
        command.arg(&temp_path);
        let output = command.output()?;
        if !output.status.success() {
            let _ = std::fs::remove_file(&temp_path);
            return Err(user_error(format!(
                "audio preview failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        if !temp_path
            .metadata()
            .map(|metadata| metadata.len() > 1024)
            .unwrap_or(false)
        {
            let _ = std::fs::remove_file(&temp_path);
            return Err(user_error("FFmpeg produced an empty audio preview"));
        }
        if output_path.exists() {
            std::fs::remove_file(output_path)?;
        }
        std::fs::rename(temp_path, output_path)?;
        Ok(())
    }

    pub fn trim(
        &self,
        source: &Path,
        output_path: &Path,
        start_seconds: f64,
        end_seconds: f64,
        accurate: bool,
        audio_track_indices: &[u32],
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
        let run = |video_args: &[&str]| -> AppResult<std::process::Output> {
            let mut command = Command::new(ffmpeg);
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
            command.output().map_err(Into::into)
        };
        let output = if accurate {
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
            if nvenc.status.success() {
                nvenc
            } else {
                run(&[
                    "-c:v", "libx264", "-preset", "medium", "-crf", "18", "-pix_fmt", "yuv420p",
                    "-c:a", "aac", "-b:a", "192k",
                ])?
            }
        } else {
            run(&["-c", "copy", "-avoid_negative_ts", "make_zero"])?
        };
        if output.status.success() {
            Ok(())
        } else if accurate {
            Err(user_error(
                String::from_utf8_lossy(&output.stderr).to_string(),
            ))
        } else {
            Err(user_error(format!(
                "lossless trim failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )))
        }
    }
}

#[cfg(debug_assertions)]
fn existing_file(path: PathBuf) -> Option<PathBuf> {
    path.is_file().then_some(path)
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
    index: u32,
    codec_type: String,
    codec_name: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    channels: Option<u32>,
    duration: Option<String>,
    tags: Option<ProbeTags>,
    disposition: Option<ProbeDisposition>,
}

#[derive(Debug, Deserialize)]
struct ProbeTags {
    language: Option<String>,
    handler_name: Option<String>,
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ProbeDisposition {
    default: u8,
}
