use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// MPV JSON IPC command.
#[derive(Serialize)]
struct MpvCommand<'a> {
    command: &'a [serde_json::Value],
    request_id: Option<u64>,
}

/// MPV JSON IPC response.
#[derive(Deserialize, Debug)]
struct MpvResponse {
    #[allow(dead_code)]
    error: Option<String>,
    #[allow(dead_code)]
    data: Option<serde_json::Value>,
    #[allow(dead_code)]
    request_id: Option<u64>,
}

/// Manages an mpv subprocess with JSON IPC control.
pub struct MpvPlayer {
    process: Option<Child>,
    socket_path: String,
    running: Arc<AtomicBool>,
}

impl MpvPlayer {
    const IPC_SOCKET: &'static str = "/tmp/dnclip-mpv.sock";

    pub fn new() -> Self {
        Self {
            process: None,
            socket_path: Self::IPC_SOCKET.to_string(),
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Launch mpv as a standalone window (no embedding).
    /// Call this first, then use IPC to control playback.
    pub fn launch(&mut self, file_path: &str) -> Result<()> {
        self.kill_mpv();
        let _ = std::fs::remove_file(&self.socket_path);

        self.running.store(true, Ordering::SeqCst);

        let ipc_arg = format!("--input-ipc-server={}", self.socket_path);
        let mut child = Command::new("mpv")
            .args([
                "--no-terminal",
                "--keep-open=yes",
                "--osd-level=0",
                "--osc=no",
                &ipc_arg,
                file_path,
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .context("Failed to launch mpv. Is it installed?")?;

        log::info!("mpv launched standalone, PID {}", child.id());

        // Wait for IPC socket to be ready
        let mut retries = 20;
        while retries > 0 {
            if std::path::Path::new(&self.socket_path).exists() {
                break;
            }
            if let Ok(Some(status)) = child.try_wait() {
                self.running.store(false, Ordering::SeqCst);
                anyhow::bail!(
                    "mpv exited early (status: {}). Check terminal above for mpv errors.",
                    status,
                );
            }
            std::thread::sleep(Duration::from_millis(100));
            retries -= 1;
        }

        if retries == 0 {
            self.running.store(false, Ordering::SeqCst);
            let _ = child.wait();
            anyhow::bail!("mpv failed to create IPC socket within timeout.");
        }

        self.process = Some(child);
        Ok(())
    }

    /// Launch mpv embedded in the given X11 window.
    /// Reserved for Phase 2 enhancement.
    #[allow(dead_code)]
    pub fn launch_embedded(&mut self, file_path: &str, wid: u64) -> Result<()> {
        self.kill_mpv();
        let _ = std::fs::remove_file(&self.socket_path);

        self.running.store(true, Ordering::SeqCst);

        let ipc_arg = format!("--input-ipc-server={}", self.socket_path);
        let wid_arg = format!("--wid={}", wid);
        let child = Command::new("mpv")
            .args([
                "--no-terminal",
                "--keep-open=yes",
                "--osd-level=0",
                "--osc=no",
                &ipc_arg,
                &wid_arg,
                file_path,
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("Failed to launch mpv. Is it installed?")?;

        log::info!("mpv spawned embedded, PID {}", child.id());

        let mut retries = 20;
        while retries > 0 {
            if std::path::Path::new(&self.socket_path).exists() {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
            retries -= 1;
        }

        if retries == 0 {
            self.running.store(false, Ordering::SeqCst);
            let _ = child.wait_with_output();
            anyhow::bail!("mpv failed to create IPC socket within timeout");
        }

        self.process = Some(child);
        Ok(())
    }

    /// Send a JSON command to mpv via the IPC socket.
    fn send_command(&self, cmd: &[serde_json::Value]) -> Result<MpvResponse> {
        let mut stream = UnixStream::connect(&self.socket_path)
            .context("Failed to connect to mpv IPC socket. Is mpv running?")?;

        stream.set_write_timeout(Some(Duration::from_secs(2)))?;
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;

        let req = MpvCommand {
            command: cmd,
            request_id: Some(1),
        };

        let payload = serde_json::to_string(&req)?;
        log::debug!("mpv send: {}", payload);

        stream.write_all(payload.as_bytes())?;
        stream.write_all(b"\n")?;

        let mut reader = BufReader::new(&stream);
        let mut line = String::new();
        reader.read_line(&mut line)?;

        log::debug!("mpv recv: {}", line.trim());

        let resp: MpvResponse = serde_json::from_str(&line)?;
        Ok(resp)
    }

    /// Load a new file into mpv (replaces current).
    pub fn load_file(&self, path: &str) -> Result<()> {
        self.send_command(&[
            serde_json::json!("loadfile"),
            serde_json::json!(path),
            serde_json::json!("replace"),
        ])?;
        Ok(())
    }

    pub fn play(&self) -> Result<()> {
        self.send_command(&[
            serde_json::json!("set"),
            serde_json::json!("pause"),
            serde_json::json!(false),
        ])?;
        Ok(())
    }

    pub fn pause(&self) -> Result<()> {
        self.send_command(&[
            serde_json::json!("set"),
            serde_json::json!("pause"),
            serde_json::json!(true),
        ])?;
        Ok(())
    }

    pub fn toggle_pause(&self) -> Result<()> {
        self.send_command(&[
            serde_json::json!("cycle"),
            serde_json::json!("pause"),
        ])?;
        Ok(())
    }

    pub fn seek(&self, seconds: f64) -> Result<()> {
        self.send_command(&[
            serde_json::json!("seek"),
            serde_json::json!(seconds),
            serde_json::json!("absolute"),
        ])?;
        Ok(())
    }

    pub fn seek_relative(&self, seconds: f64) -> Result<()> {
        self.send_command(&[
            serde_json::json!("seek"),
            serde_json::json!(seconds),
            serde_json::json!("relative"),
        ])?;
        Ok(())
    }

    pub fn frame_step(&self, forward: bool) -> Result<()> {
        let cmd = if forward {
            serde_json::json!("frame-step")
        } else {
            serde_json::json!("frame-back-step")
        };
        self.send_command(&[cmd])?;
        Ok(())
    }

    /// Get a property value from mpv.
    pub fn get_property(&self, name: &str) -> Result<serde_json::Value> {
        let resp = self.send_command(&[
            serde_json::json!("get_property"),
            serde_json::json!(name),
        ])?;
        Ok(resp.data.unwrap_or(serde_json::Value::Null))
    }

    /// Get the current playback time in seconds.
    pub fn get_time_pos(&self) -> Result<f64> {
        let val = self.get_property("time-pos")?;
        val.as_f64().context("Failed to get time-pos from mpv")
    }

    /// Get the file duration in seconds.
    pub fn get_duration(&self) -> Result<f64> {
        let val = self.get_property("duration")?;
        val.as_f64().context("Failed to get duration from mpv")
    }

    /// Get video FPS from container metadata.
    /// Falls back to estimated FPS if container metadata is unavailable.
    pub fn get_fps(&self) -> Result<f64> {
        // Try container-fps first (most reliable for CFR content)
        if let Ok(val) = self.get_property("container-fps") {
            if let Some(fps) = val.as_f64() {
                if fps > 0.0 {
                    return Ok(fps);
                }
            }
        }
        // Fallback to estimated-vf-fps
        let val = self.get_property("estimated-vf-fps")?;
        val.as_f64().context("Failed to get FPS from mpv")
    }

    /// Get video resolution.
    pub fn get_resolution(&self) -> Result<(u32, u32)> {
        let w = self
            .get_property("dwidth")?
            .as_f64()
            .context("Failed to get width")? as u32;
        let h = self
            .get_property("dheight")?
            .as_f64()
            .context("Failed to get height")? as u32;
        Ok((w, h))
    }

    /// Check if mpv is paused.
    pub fn is_paused(&self) -> Result<bool> {
        let val = self.get_property("pause")?;
        Ok(val.as_bool().unwrap_or(true))
    }

    /// Check if the mpv process is still alive.
    pub fn is_alive(&mut self) -> bool {
        self.process
            .as_mut()
            .map(|p| matches!(p.try_wait(), Ok(None)))
            .unwrap_or(false)
    }

    fn kill_mpv(&mut self) {
        if let Some(mut child) = self.process.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    /// Stop mpv and clean up.
    pub fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        self.kill_mpv();
        let _ = std::fs::remove_file(&self.socket_path);
    }
}

impl Drop for MpvPlayer {
    fn drop(&mut self) {
        self.stop();
    }
}
