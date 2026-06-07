use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

use crate::types::DnxProfile;

/// FFmpeg export wrapper.
pub struct ExportJob {
    pub input_path: String,
    pub output_path: String,
    pub in_point: f64,    // seconds
    pub out_point: f64,   // seconds
    pub profile: DnxProfile,
    pub width: u32,
    pub height: u32,
}

impl ExportJob {
    /// Build and execute the ffmpeg command.
    pub fn run(&self) -> Result<()> {
        let duration = self.out_point - self.in_point;
        if duration <= 0.0 {
            anyhow::bail!("Invalid segment: out point must be after in point");
        }

        let profile_str = self.profile.ffmpeg_profile();

        log::info!(
            "Exporting: {} -> {} ({}s, {})",
            self.input_path,
            self.output_path,
            duration,
            profile_str
        );

        // DNxHD requires even dimensions and specific widths.
        let width = if self.width % 2 != 0 { self.width + 1 } else { self.width };
        let height = if self.height % 2 != 0 { self.height + 1 } else { self.height };

        let status = Command::new("ffmpeg")
            .args([
                "-ss",
                &format!("{:.3}", self.in_point),
                "-i",
                &self.input_path,
                "-t",
                &format!("{:.3}", duration),
                "-c:v",
                "dnxhd",
                "-profile:v",
                profile_str,
                "-pix_fmt",
                "yuv422p",
                "-vf",
                &format!("scale={width}:{height}:flags=lanczos,setsar=1"),
                "-c:a",
                "pcm_s16le",
                "-y",
                &self.output_path,
            ])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .context("Failed to start ffmpeg. Is it installed?")?;

        let output = status
            .wait_with_output()
            .context("ffmpeg process failed")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("ffmpeg failed:\n{}", stderr);
        }

        log::info!("Export complete: {}", self.output_path);
        Ok(())
    }
}

/// Generate a default output path based on the input file and profile.
pub fn default_output_path(input: &Path, profile: &DnxProfile) -> String {
    let stem = input
        .file_stem()
        .map(|s| s.to_string_lossy())
        .unwrap_or(std::borrow::Cow::Borrowed("output"));

    let profile_tag = profile.ffmpeg_profile().replace("dnxhr_", "");
    format!("{}_{}.mov", stem, profile_tag)
}
