use std::{
    path::{Path, PathBuf},
    process::Stdio,
};

use tokio::process::Command;

use crate::core::{AudioEncoderStatus, CoreConfig, PodcastError, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AudioFormat {
    Mp3,
    Other(String),
}

pub fn classify_audio_format(extension: &str, content_type: Option<&str>) -> AudioFormat {
    let extension = extension.trim_start_matches('.').to_ascii_lowercase();
    let content_type = content_type.unwrap_or_default().to_ascii_lowercase();

    if !content_type.is_empty() {
        return match content_type.as_str() {
            "audio/mpeg" | "audio/mp3" => AudioFormat::Mp3,
            _ if content_type.starts_with("audio/") || content_type.starts_with("video/") => {
                AudioFormat::Other(if extension.is_empty() {
                    "unknown".to_string()
                } else {
                    extension
                })
            }
            _ => AudioFormat::Other(if extension.is_empty() {
                "unknown".to_string()
            } else {
                extension
            }),
        };
    }

    match extension.as_str() {
        "mp3" => AudioFormat::Mp3,
        _ => AudioFormat::Other(if extension.is_empty() {
            "unknown".to_string()
        } else {
            extension
        }),
    }
}

pub fn configured_encoder_path(config: &CoreConfig) -> PathBuf {
    config
        .mp3_encoder_path
        .clone()
        .unwrap_or_else(|| PathBuf::from("ffmpeg"))
}

pub async fn encoder_status(config: &CoreConfig) -> AudioEncoderStatus {
    let path = configured_encoder_path(config);
    let output = Command::new(&path)
        .arg("-version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await;

    match output {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let version = stdout.lines().next().unwrap_or("ffmpeg").to_string();
            AudioEncoderStatus::Available { path, version }
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            AudioEncoderStatus::Error {
                path,
                error: if stderr.is_empty() {
                    format!("encoder exited with {}", output.status)
                } else {
                    stderr
                },
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            AudioEncoderStatus::Missing { path }
        }
        Err(error) => AudioEncoderStatus::Error {
            path,
            error: error.to_string(),
        },
    }
}

pub async fn convert_to_mp3(
    encoder_path: Option<&Path>,
    input_path: &Path,
    output_path: &Path,
) -> Result<()> {
    let encoder = encoder_path
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("ffmpeg"));

    let output = Command::new(&encoder)
        .arg("-y")
        .arg("-i")
        .arg(input_path)
        .arg("-vn")
        .arg("-codec:a")
        .arg("libmp3lame")
        .arg("-q:a")
        .arg("2")
        .arg(output_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .await;

    let output = match output {
        Ok(output) => output,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(PodcastError::Mp3EncoderUnavailable);
        }
        Err(error) => return Err(PodcastError::Io(error)),
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(PodcastError::Mp3ConversionFailed(if stderr.is_empty() {
            format!("encoder exited with {}", output.status)
        } else {
            stderr
        }));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_mp3_from_extension_or_content_type() {
        assert_eq!(classify_audio_format("mp3", None), AudioFormat::Mp3);
        assert_eq!(
            classify_audio_format("bin", Some("audio/mpeg")),
            AudioFormat::Mp3
        );
        assert_eq!(
            classify_audio_format("m4a", Some("audio/mp4")),
            AudioFormat::Other("m4a".to_string())
        );
    }

    #[test]
    fn content_type_overrides_misleading_extension() {
        assert_eq!(
            classify_audio_format("mp3", Some("audio/mp4")),
            AudioFormat::Other("mp3".to_string())
        );
        assert_eq!(
            classify_audio_format("", None),
            AudioFormat::Other("unknown".to_string())
        );
    }
}
