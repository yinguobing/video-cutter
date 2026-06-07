use eframe::egui;
use egui_file_dialog::FileDialog;
use crate::player::MpvPlayer;
use crate::types::{DnxProfile, Project, VideoInfo};
use crate::export::{ExportJob, default_output_path};

/// Main application state.
pub struct DnClipApp {
    project: Project,
    player: MpvPlayer,
    // UI state
    paused: bool,
    current_time: f64,
    player_ready: bool,
    export_status: String,
    show_help: bool,
    file_dialog: FileDialog,
}

impl Default for DnClipApp {
    fn default() -> Self {
        let file_dialog = FileDialog::default()
            .initial_directory(std::env::current_dir().unwrap_or_default())
            .add_file_filter_extensions(
                "Video files",
                vec!["mp4", "mov", "mkv", "avi", "webm",
                     "mts", "m2ts", "ts", "flv", "wmv"],
            )
            .default_file_filter("Video files");

        Self {
            project: Project::default(),
            player: MpvPlayer::new(),
            paused: true,
            current_time: 0.0,
            player_ready: false,
            export_status: String::new(),
            show_help: false,
            file_dialog,
        }
    }
}

impl DnClipApp {
    fn format_time(secs: f64) -> String {
        if secs < 0.0 {
            return "00:00:00.000".to_string();
        }
        let total_ms = (secs * 1000.0) as i64;
        let h = total_ms / 3_600_000;
        let m = (total_ms % 3_600_000) / 60_000;
        let s = (total_ms % 60_000) / 1_000;
        let ms = total_ms % 1_000;
        format!("{:02}:{:02}:{:02}.{:03}", h, m, s, ms)
    }

    /// Open a video file.
    fn open_file(&mut self, path: &str) {
        // Stop current player if any
        self.player.stop();
        self.player_ready = false;
        self.project = Project::default();

        // Launch mpv standalone window
        if let Err(e) = self.player.launch(path) {
            self.export_status = format!("Error: {}", e);
            return;
        }
        self.project.source_path = Some(std::path::PathBuf::from(path));

        match self.player.get_duration() {
            Ok(d) => {
                let info = VideoInfo {
                    width: 0,  // we'll get this later
                    height: 0,
                    fps: 0.0,
                    duration: d,
                };
                // Try to get resolution
                if let Ok((w, h)) = self.player.get_resolution() {
                    self.project.video_info = Some(VideoInfo {
                        width: w,
                        height: h,
                        ..info
                    });
                } else {
                    self.project.video_info = Some(info);
                }
            }
            Err(e) => {
                self.export_status = format!("Failed to get video info: {}", e);
            }
        }

        self.player_ready = true;
        self.paused = true;
        self.export_status = format!("Loaded: {}", path);
    }

    /// Execute export.
    fn do_export(&mut self) {
        let source = match &self.project.source_path {
            Some(p) => p.to_string_lossy().to_string(),
            None => {
                self.export_status = "No file loaded".to_string();
                return;
            }
        };

        let in_pt = self.project.in_point.unwrap_or(0.0);
        let out_pt = self.project.out_point.unwrap_or(
            self.project.video_info.as_ref().map(|i| i.duration).unwrap_or(0.0)
        );

        if out_pt <= in_pt {
            self.export_status = "Out point must be after in point".to_string();
            return;
        }

        if out_pt - in_pt < 0.1 {
            self.export_status = "Segment too short (min 0.1s)".to_string();
            return;
        }

        // Determine output path
        let out_path = self.project.export_params.output_path.clone()
            .unwrap_or_else(|| {
                let input = std::path::Path::new(&source);
                std::path::PathBuf::from(default_output_path(input, &self.project.export_params.profile))
            });

        let (width, height) = match &self.project.video_info {
            Some(info) => (info.width, info.height),
            None => (1920, 1080),
        };

        let job = ExportJob {
            input_path: source,
            output_path: out_path.to_string_lossy().to_string(),
            in_point: in_pt,
            out_point: out_pt,
            profile: self.project.export_params.profile,
            width,
            height,
        };

        self.export_status = "Exporting... (this may take a while)".to_string();

        match job.run() {
            Ok(()) => {
                self.export_status = format!("✅ Exported: {}", out_path.display());
            }
            Err(e) => {
                self.export_status = format!("❌ Export failed: {}", e);
            }
        }
    }
}

impl eframe::App for DnClipApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // ── File dialog ──
        self.file_dialog.update(ctx);
        if let Some(path) = self.file_dialog.take_picked() {
            let path_str = path.to_string_lossy().to_string();
            self.open_file(&path_str);
        }

        // ── Drag-and-drop ──
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        for file in dropped {
            if let Some(path) = file.path {
                let valid_extensions = ["mp4", "mov", "mkv", "avi", "webm",
                                        "mts", "m2ts", "ts", "flv", "wmv"];
                if let Some(ext) = path.extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.to_lowercase())
                {
                    if valid_extensions.contains(&ext.as_str()) {
                        let path_str = path.to_string_lossy().to_string();
                        self.open_file(&path_str);
                        break;
                    }
                }
            }
        }

        // Poll mpv state
        if self.player_ready {
            match self.player.get_time_pos() {
                Ok(t) => self.current_time = t,
                Err(_) => {
                    // Player might have exited; mark not ready
                    // but don't spam errors
                }
            }
            match self.player.is_paused() {
                Ok(p) => self.paused = p,
                Err(_) => {}
            }
        }

        // Request continuous repaint while player is active
        if self.player_ready {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }

        // ── Top panel ──
        egui::TopBottomPanel::top("top_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("📂 Open File").clicked() {
                    self.file_dialog.pick_file();
                }
                if ui.button("❓ Help").clicked() {
                    self.show_help = !self.show_help;
                }
                ui.separator();
                if let Some(path) = &self.project.source_path {
                    ui.label(format!("File: {}", path.file_name().unwrap_or_default().to_string_lossy()));
                } else {
                    ui.label("No file loaded");
                }
            });
        });

        // ── Help popup ──
        if self.show_help {
            egui::Window::new("Help / Shortcuts")
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        ui.label("Shortcuts:");
                        ui.label("  Space  — Play / Pause");
                        ui.label("  I      — Mark IN point");
                        ui.label("  O      — Mark OUT point");
                        ui.label("  ←/→   — Seek -5s / +5s");
                        ui.label("  ↑/↓   — Frame step forward/back");
                        ui.label("  Enter  — Export segment");
                        ui.label("  Ctrl+O — Open file");
                        ui.label("  H      — Toggle help");
                    });
                });
        }

        // ── Main content ──
        egui::CentralPanel::default().show(ctx, |ui| {
            // Video preview area (placeholder for now)
            egui::Frame::dark_canvas(ui.style()).show(ui, |ui| {
                let avail = ui.available_size();
                let (rect, _) = ui.allocate_exact_size(
                    egui::vec2(avail.x, avail.x * 0.5625), // 16:9
                    egui::Sense::hover(),
                );

                ui.painter().rect_filled(
                    rect,
                    egui::CornerRadius::same(4),
                    egui::Color32::from_rgb(20, 20, 30),
                );

                // Show current time overlay
                if self.player_ready {
                    let time_text = Self::format_time(self.current_time);
                    ui.painter().text(
                        egui::pos2(rect.left() + 10.0, rect.top() + 10.0),
                        egui::Align2::LEFT_TOP,
                        &time_text,
                        egui::FontId::monospace(18.0),
                        egui::Color32::WHITE,
                    );

                    // Show IN/OUT markers at top
                    let total_dur = self.project.video_info.as_ref()
                        .map(|i| i.duration).unwrap_or(1.0);
                    if total_dur > 0.0 {
                        let bar_w = rect.width() - 20.0;
                        let bar_x = rect.left() + 10.0;

                        if let Some(in_pt) = self.project.in_point {
                            let frac = (in_pt / total_dur).clamp(0.0, 1.0) as f32;
                            let x = bar_x + frac * bar_w;
                            ui.painter().line_segment(
                                [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                                egui::Stroke::new(2.0, egui::Color32::GREEN),
                            );
                            ui.painter().text(
                                egui::pos2(x, rect.top() + 5.0),
                                egui::Align2::CENTER_TOP,
                                "I",
                                egui::FontId::proportional(14.0),
                                egui::Color32::GREEN,
                            );
                        }

                        if let Some(out_pt) = self.project.out_point {
                            let frac = (out_pt / total_dur).clamp(0.0, 1.0) as f32;
                            let x = bar_x + frac * bar_w;
                            ui.painter().line_segment(
                                [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                                egui::Stroke::new(2.0, egui::Color32::RED),
                            );
                            ui.painter().text(
                                egui::pos2(x, rect.top() + 5.0),
                                egui::Align2::CENTER_TOP,
                                "O",
                                egui::FontId::proportional(14.0),
                                egui::Color32::RED,
                            );
                        }
                    }

                    // Center text "mpv window is separate"
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "🎬 mpv playback window (separate)",
                        egui::FontId::proportional(16.0),
                        egui::Color32::GRAY,
                    );
                } else {
                    // Drag-and-drop hint
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "Drop a video file or use Ctrl+O to open",
                        egui::FontId::proportional(18.0),
                        egui::Color32::GRAY,
                    );
                }
            });

            ui.add_space(8.0);

            // ── Timeline ──
            if let Some(info) = &self.project.video_info {
                let total_dur = info.duration;
                if total_dur > 0.0 {
                    // Compact time display
                    ui.horizontal(|ui| {
                        ui.label(Self::format_time(self.current_time));
                        ui.label(format!(" / {}", Self::format_time(total_dur)));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(format!("{:.3} fps", info.fps));
                        });
                    });

                    // Interactive timeline slider
                    let slider = egui::Slider::new(&mut self.current_time, 0.0..=total_dur as f64)
                        .clamping(egui::SliderClamping::Always)
                        .show_value(false)
                        .trailing_fill(true);
                    let resp = ui.add(slider);

                    if resp.changed() {
                        let _ = self.player.seek(self.current_time);
                    }

                    // Draw segment highlight below slider
                    if let (Some(in_pt), Some(out_pt)) = (self.project.in_point, self.project.out_point) {
                        let rect = resp.rect;
                        let frac_in = (in_pt / total_dur).clamp(0.0, 1.0) as f32;
                        let frac_out = (out_pt / total_dur).clamp(0.0, 1.0) as f32;
                        let x_in = rect.left() + frac_in * rect.width();
                        let x_out = rect.left() + frac_out * rect.width();
                        ui.painter().rect_filled(
                            egui::Rect::from_min_max(
                                egui::pos2(x_in, rect.bottom() + 2.0),
                                egui::pos2(x_out, rect.bottom() + 6.0),
                            ),
                            egui::CornerRadius::same(2),
                            egui::Color32::from_rgb(0, 180, 255),
                        );
                    }

                    ui.add_space(4.0);
                }
            }

            ui.separator();

            // ── I/O Point Display ──
            ui.horizontal(|ui| {
                ui.label(format!("IN:  {}", Self::format_time(self.project.in_point.unwrap_or(0.0))));
                if ui.button("I").clicked() {
                    self.project.in_point = Some(self.current_time);
                }
                ui.separator();
                ui.label(format!("OUT: {}", Self::format_time(self.project.out_point.unwrap_or(0.0))));
                if ui.button("O").clicked() {
                    self.project.out_point = Some(self.current_time);
                }
                ui.separator();
                if let Some(dur) = self.project.segment_duration() {
                    ui.label(format!("Duration: {}", Self::format_time(dur)));
                }
                if ui.button("Clear I/O").clicked() {
                    self.project.in_point = None;
                    self.project.out_point = None;
                }
            });

            ui.add_space(4.0);

            // ── Playback Controls ──
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;

                if ui.button("⏮ -30s").clicked() {
                    let _ = self.player.seek_relative(-30.0);
                }
                if ui.button("◀◀ -5s").clicked() {
                    let _ = self.player.seek_relative(-5.0);
                }

                let play_label = if self.paused { "▶ Play" } else { "⏸ Pause" };
                if ui.button(play_label).clicked() {
                    let _ = self.player.toggle_pause();
                    self.paused = !self.paused;
                }

                if ui.button("▶▶ +5s").clicked() {
                    let _ = self.player.seek_relative(5.0);
                }
                if ui.button("+30s ⏭").clicked() {
                    let _ = self.player.seek_relative(30.0);
                }

                ui.separator();

                if ui.button("◀ Frame").clicked() {
                    let _ = self.player.frame_step(false);
                    // Update current time after frame step
                    if let Ok(t) = self.player.get_time_pos() {
                        self.current_time = t;
                    }
                }
                if ui.button("Frame ▶").clicked() {
                    let _ = self.player.frame_step(true);
                    if let Ok(t) = self.player.get_time_pos() {
                        self.current_time = t;
                    }
                }
            });

            ui.add_space(8.0);

            // ── Export Panel ──
            egui::CollapsingHeader::new("⚙ Export Settings")
                .default_open(true)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Profile:");
                        let current = &mut self.project.export_params.profile;
                        egui::ComboBox::from_id_salt("profile_selector")
                            .selected_text(current.label())
                            .show_ui(ui, |ui| {
                                let profiles = [
                                    DnxProfile::DnxHR_LB,
                                    DnxProfile::DnxHR_SQ,
                                    DnxProfile::DnxHR_HQ,
                                    DnxProfile::DnxHR_HQX,
                                ];
                                for p in profiles {
                                    ui.selectable_value(current, p, p.label());
                                }
                            });
                    });

                    ui.horizontal(|ui| {
                        if ui.button("💾 Export Segment").clicked() {
                            self.do_export();
                        }
                        if !self.export_status.is_empty() {
                            ui.label(&self.export_status);
                        }
                    });
                });
        });

        // ── Keyboard shortcuts ──
        ctx.input_mut(|i| {
            for event in &i.events {
                if let egui::Event::Key { key, pressed: true, modifiers, .. } = event {
                    if modifiers.ctrl && *key == egui::Key::O {
                        self.file_dialog.pick_file();
                    } else if *key == egui::Key::H {
                        self.show_help = !self.show_help;
                    } else if *key == egui::Key::Space && self.player_ready {
                        let _ = self.player.toggle_pause();
                    } else if *key == egui::Key::I && self.player_ready {
                        self.project.in_point = Some(self.current_time);
                    } else if *key == egui::Key::O && self.player_ready {
                        self.project.out_point = Some(self.current_time);
                    } else if *key == egui::Key::ArrowLeft && self.player_ready {
                        let _ = self.player.seek_relative(-5.0);
                    } else if *key == egui::Key::ArrowRight && self.player_ready {
                        let _ = self.player.seek_relative(5.0);
                    } else if *key == egui::Key::ArrowUp && self.player_ready {
                        let _ = self.player.frame_step(true);
                    } else if *key == egui::Key::ArrowDown && self.player_ready {
                        let _ = self.player.frame_step(false);
                    } else if *key == egui::Key::Enter && self.player_ready {
                        self.do_export();
                    }
                }
            }
        });
    }
}
