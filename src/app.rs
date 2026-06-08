use crate::embed::EmbeddedWindow;
use crate::export::{default_output_path, ExportJob};
use crate::player::MpvPlayer;
use crate::types::{DnxProfile, Project, Segment, VideoInfo};
use eframe::egui;
use egui_alignments::center_horizontal;
use egui_file_dialog::FileDialog;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

/// Main application state.
pub struct DnClipApp {
    project: Project,
    player: MpvPlayer,
    embedded: EmbeddedWindow,
    // UI state
    paused: bool,
    current_time: f64,
    player_ready: bool,
    export_status: String,
    show_help: bool,
    file_dialog: FileDialog,
    preview_rect: Option<egui::Rect>,
    embedded_initialized: bool,
    pending_open: Option<String>,
    segments: Vec<Segment>,
    debug_hover: bool,
}

impl Default for DnClipApp {
    fn default() -> Self {
        let file_dialog = FileDialog::default()
            .initial_directory(std::env::current_dir().unwrap_or_default())
            .add_file_filter_extensions(
                "Video files",
                vec![
                    "mp4", "mov", "mkv", "avi", "webm", "mts", "m2ts", "ts", "flv", "wmv",
                ],
            )
            .default_file_filter("Video files");

        Self {
            project: Project::default(),
            player: MpvPlayer::new(),
            embedded: EmbeddedWindow::new(),
            paused: true,
            current_time: 0.0,
            player_ready: false,
            export_status: String::new(),
            show_help: false,
            file_dialog,
            preview_rect: None,
            embedded_initialized: false,
            pending_open: None,
            segments: Vec::new(),
            debug_hover: false,
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
        self.embedded.destroy();
        self.player_ready = false;
        self.project = Project::default();

        // Try embedded launch first, fall back to standalone
        let launch_result = if self.embedded_initialized {
            if let Some(rect) = self.preview_rect {
                // Use last-frame's preview rect (will be corrected next frame)
                let ppp = 1.0; // approximate — will be fixed by reposition next frame
                let child_x = (rect.min.x * ppp) as i32;
                let child_y = (rect.min.y * ppp) as i32;
                let child_w = (rect.width() * ppp) as u32;
                let child_h = (rect.height() * ppp) as u32;
                if let Some(child_xid) = self.embedded.create(child_x, child_y, child_w, child_h) {
                    self.player.launch_embedded(path, child_xid)
                } else {
                    self.player.launch(path)
                }
            } else {
                self.player.launch(path)
            }
        } else {
            self.player.launch(path)
        };

        if let Err(e) = launch_result {
            self.export_status = format!("Error: {}", e);
            self.embedded.destroy();
            return;
        }
        self.project.source_path = Some(std::path::PathBuf::from(path));

        match self.player.get_duration() {
            Ok(d) => {
                let fps = self.player.get_fps().unwrap_or(0.0);
                let info = VideoInfo {
                    width: 0,
                    height: 0,
                    fps,
                    duration: d,
                };
                // Try to get resolution
                let (w, h) = self.player.get_resolution().unwrap_or((0, 0));
                self.project.video_info = Some(VideoInfo {
                    width: w,
                    height: h,
                    ..info
                });
            }
            Err(e) => {
                self.export_status = format!("Failed to get video info: {}", e);
            }
        }

        self.player_ready = true;
        self.paused = true;
        self.export_status = format!("Loaded: {}", path);
    }

    /// Execute export for all saved segments, or the current I/O pair.
    fn do_export(&mut self) {
        let source = match &self.project.source_path {
            Some(p) => p.to_string_lossy().to_string(),
            None => {
                self.export_status = "No file loaded".to_string();
                return;
            }
        };

        // Build export list: segments or current I/O
        let exports: Vec<(f64, f64)> = if !self.segments.is_empty() {
            self.segments
                .iter()
                .map(|s| (s.in_point, s.out_point))
                .collect()
        } else {
            let in_pt = self.project.in_point.unwrap_or(0.0);
            let out_pt = self.project.out_point.unwrap_or(
                self.project
                    .video_info
                    .as_ref()
                    .map(|i| i.duration)
                    .unwrap_or(0.0),
            );
            if out_pt <= in_pt {
                self.export_status = "Out point must be after in point".to_string();
                return;
            }
            vec![(in_pt, out_pt)]
        };

        if exports.is_empty() {
            self.export_status = "No segments to export".to_string();
            return;
        }

        let (width, height) = match &self.project.video_info {
            Some(info) => (info.width, info.height),
            None => (1920, 1080),
        };

        let profile = self.project.export_params.profile;

        let mut errors = Vec::new();
        for (idx, (in_pt, out_pt)) in exports.iter().enumerate() {
            let dur = out_pt - in_pt;
            if dur < 0.1 {
                errors.push(format!("#{}: segment too short", idx + 1));
                continue;
            }

            let input_path = std::path::Path::new(&source);
            let out_path = if exports.len() > 1 {
                let stem = input_path
                    .file_stem()
                    .map(|s| s.to_string_lossy())
                    .unwrap_or(std::borrow::Cow::Borrowed("output"));
                let profile_tag = profile.ffmpeg_profile().replace("dnxhr_", "");
                format!("{}_{}_{:03}.mov", stem, profile_tag, idx + 1)
            } else {
                default_output_path(input_path, &profile)
            };

            self.export_status = format!("Exporting {} / {}...", idx + 1, exports.len());

            let job = ExportJob {
                input_path: source.clone(),
                output_path: out_path.clone(),
                in_point: *in_pt,
                out_point: *out_pt,
                profile,
                width,
                height,
            };

            match job.run() {
                Ok(()) => {
                    log::info!("Exported: {}", out_path);
                }
                Err(e) => {
                    errors.push(format!("#{}: {}", idx + 1, e));
                }
            }
        }

        if errors.is_empty() {
            self.segments.clear();
            self.export_status = format!("✅ Exported {} segment(s)", exports.len());
        } else {
            self.export_status = format!("❌ {} error(s): {}", errors.len(), errors.join("; "));
        }
    }
}

impl eframe::App for DnClipApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        // Debug mode: show widget rects + names on hover
        if self.debug_hover {
            #[cfg(debug_assertions)]
            ctx.global_style_mut(|s| s.debug.debug_on_hover = true);
            // Also show an overlay panel with widget tree info
            egui::Window::new("🔍 UI Debug")
                .default_pos([10.0, 40.0])
                .show(ui.ctx(), |ui| {
                    let ppp = ui.ctx().pixels_per_point();
                    if let Some(r) = self.preview_rect {
                        ui.label(format!(
                            "Preview: {:.0}x{:.0} @ ({:.0},{:.0})",
                            r.width(),
                            r.height(),
                            r.min.x,
                            r.min.y
                        ));
                        ui.label(format!(
                            "  physical: {:.0}x{:.0}",
                            r.width() * ppp,
                            r.height() * ppp
                        ));
                    }
                    ui.label(format!("pixels_per_point: {:.2}", ppp));
                    ui.label(format!("screen_rect: {:?}", ctx.content_rect()));
                    if let Some(vp) = ui.ctx().input(|i| i.viewport().inner_rect) {
                        ui.label(format!("viewport inner: {:?}", vp));
                    }
                    if let Some(vp) = ui.ctx().input(|i| i.viewport().outer_rect) {
                        ui.label(format!("viewport outer: {:?}", vp));
                    }
                    ui.label(format!("embedded active: {}", self.embedded.is_active()));
                });
        }

        // ── Embedded window init ──
        if !self.embedded_initialized {
            if let Ok(wh) = frame.window_handle() {
                if let RawWindowHandle::Xlib(xwh) = wh.as_raw() {
                    self.embedded.init(xwh.window);
                    self.embedded_initialized = true;
                    log::info!("Embedded window initialized, parent=0x{:x}", xwh.window);
                }
            }
        }

        // ── Reposition embedded window ──
        // Child window coords are relative to parent, no screen offset needed.
        if self.embedded.is_active() {
            if let Some(rect) = self.preview_rect {
                let ppp = ctx.pixels_per_point();
                let x = (rect.min.x * ppp) as i32;
                let y = (rect.min.y * ppp) as i32;
                let w = (rect.width() * ppp) as u32;
                let h = (rect.height() * ppp) as u32;
                self.embedded.reposition(x, y, w, h);
            }
        }

        // ── File dialog ──
        self.file_dialog.update(&ctx);
        if let Some(path) = self.file_dialog.take_picked() {
            self.pending_open = Some(path.to_string_lossy().to_string());
        }

        // ── Drag-and-drop ──
        if self.pending_open.is_none() {
            let dropped = ctx.input(|i| i.raw.dropped_files.clone());
            for file in dropped {
                if let Some(path) = file.path {
                    let valid_extensions = [
                        "mp4", "mov", "mkv", "avi", "webm", "mts", "m2ts", "ts", "flv", "wmv",
                    ];
                    if let Some(ext) = path
                        .extension()
                        .and_then(|e| e.to_str())
                        .map(|e| e.to_lowercase())
                    {
                        if valid_extensions.contains(&ext.as_str()) {
                            self.pending_open = Some(path.to_string_lossy().to_string());
                            break;
                        }
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

        // ── Help popup ──
        if self.show_help {
            egui::Window::new("Help / Shortcuts")
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(&ctx, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        ui.label("Shortcuts:");
                        ui.label("  Space  — Play / Pause");
                        ui.label("  I      — Mark IN point");
                        ui.label("  O      — Mark OUT point");
                        ui.label("  ←/→   — Seek -5s / +5s");
                        ui.label("  ↑/↓   — Frame step forward/back");
                        ui.label("  Enter  — Export segment");
                        ui.label("  Ctrl+O — Open file");
                        ui.label("  Ctrl+D — Debug hover");
                        ui.label("  H      — Toggle help");
                    });
                });
        }

        // ── Right sidebar ──
        egui::Panel::right("sidebar")
            .min_size(220.0)
            .resizable(true)
            .show_inside(ui, |ui| {
                if ui.button("📂 Open File").clicked() {
                    self.file_dialog.pick_file();
                }

                ui.separator();
                ui.heading("Segments");
                ui.add_space(4.0);

                // Segment list
                let mut remove_idx: Option<usize> = None;
                egui::ScrollArea::vertical()
                    .max_height(ui.available_height() - 120.0)
                    .show(ui, |ui| {
                        if self.segments.is_empty() {
                            ui.label("No segments saved.");
                            ui.label("Set I/O points and click");
                            ui.label("\"➕ Capture I/O\" below.");
                        } else {
                            for (i, seg) in self.segments.iter().enumerate() {
                                ui.horizontal(|ui| {
                                    ui.label(format!("#{}", i + 1));
                                    ui.label(format!(
                                        "{} → {}",
                                        DnClipApp::format_time(seg.in_point),
                                        DnClipApp::format_time(seg.out_point),
                                    ));
                                    ui.label(format!("({:.1}s)", seg.duration()));
                                    if ui.button("✕").clicked() {
                                        remove_idx = Some(i);
                                    }
                                });
                            }
                        }
                    });
                if let Some(i) = remove_idx {
                    self.segments.remove(i);
                }

                ui.add_space(4.0);

                // Capture current I/O
                let have_io = self.project.in_point.is_some() && self.project.out_point.is_some();
                if ui
                    .add_enabled(have_io, egui::Button::new("➕ Capture I/O"))
                    .clicked()
                {
                    let seg = Segment {
                        in_point: self.project.in_point.unwrap(),
                        out_point: self.project.out_point.unwrap(),
                    };
                    self.segments.push(seg);
                    self.project.in_point = None;
                    self.project.out_point = None;
                }

                ui.separator();

                // Profile selector
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

                ui.add_space(4.0);

                // Export button
                let can_export = have_io || !self.segments.is_empty();
                if ui
                    .add_enabled(can_export, egui::Button::new("💾 Export"))
                    .clicked()
                {
                    self.do_export();
                }

                if !self.export_status.is_empty() {
                    ui.add_space(4.0);
                    ui.label(&self.export_status);
                }
            });

        // ── Main content: preview fills, controls pinned to bottom ──
        egui::CentralPanel::default().show_inside(ui, |ui| {
            let total_dur = self
                .project
                .video_info
                .as_ref()
                .map(|i| i.duration)
                .unwrap_or(0.0);
            let has_video = self.project.video_info.is_some();

            // Controls pinned to bottom
            egui::Panel::bottom("playback_controls")
                .show_separator_line(false)
                .show_inside(ui, |ui| {
                    if has_video && total_dur > 0.0 {
                        // Timeline slider
                        let slider =
                            egui::Slider::new(&mut self.current_time, 0.0..=total_dur as f64)
                                .clamping(egui::SliderClamping::Always)
                                .show_value(false)
                                .trailing_fill(true);
                        let slider_w = ui.max_rect().width();
                        ui.spacing_mut().slider_width = slider_w;
                        let resp = ui.add(slider);
                        if resp.changed() {
                            let _ = self.player.seek(self.current_time);
                        }
                        if let (Some(in_pt), Some(out_pt)) =
                            (self.project.in_point, self.project.out_point)
                        {
                            let r = resp.rect;
                            let x_in =
                                r.left() + ((in_pt / total_dur).clamp(0.0, 1.0) as f32) * r.width();
                            let x_out = r.left()
                                + ((out_pt / total_dur).clamp(0.0, 1.0) as f32) * r.width();
                            ui.painter().rect_filled(
                                egui::Rect::from_min_max(
                                    egui::pos2(x_in, r.bottom() + 2.0),
                                    egui::pos2(x_out, r.bottom() + 6.0),
                                ),
                                egui::CornerRadius::same(2),
                                egui::Color32::from_rgb(0, 180, 255),
                            );
                        }

                        // 3-column layout: time | controls | I/O
                        ui.columns(3, |cols| {
                            // Column 0: current time / total time
                            cols[0].vertical_centered(|ui| {
                                ui.with_layout(
                                    egui::Layout::left_to_right(egui::Align::LEFT),
                                    |ui| {
                                        ui.label(format!(
                                            "{} / {}",
                                            Self::format_time(self.current_time),
                                            Self::format_time(total_dur),
                                        ));
                                    },
                                );
                            });

                            // Column 1: playback controls
                            cols[1].vertical_centered(|ui| {
                                center_horizontal(ui, |ui| {
                                    ui.spacing_mut().item_spacing.x = 4.0;
                                    if ui.button("⏮").clicked() {
                                        let _ = self.player.seek_relative(-30.0);
                                    }
                                    if ui.button("◀◀").clicked() {
                                        let _ = self.player.seek_relative(-5.0);
                                    }
                                    let play_label = if self.paused { "▶" } else { "⏸" };
                                    if ui.button(play_label).clicked() {
                                        let _ = self.player.toggle_pause();
                                        self.paused = !self.paused;
                                    }
                                    if ui.button("▶▶").clicked() {
                                        let _ = self.player.seek_relative(5.0);
                                    }
                                    if ui.button("⏭").clicked() {
                                        let _ = self.player.seek_relative(30.0);
                                    }
                                    if ui.button("◀F").clicked() {
                                        let _ = self.player.frame_step(false);
                                        if let Ok(t) = self.player.get_time_pos() {
                                            self.current_time = t;
                                        }
                                    }
                                    if ui.button("F▶").clicked() {
                                        let _ = self.player.frame_step(true);
                                        if let Ok(t) = self.player.get_time_pos() {
                                            self.current_time = t;
                                        }
                                    }
                                });
                            });

                            // Column 2: I/O timestamps
                            cols[2].vertical_centered(|ui| {
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if self.project.in_point.is_some()
                                            || self.project.out_point.is_some()
                                        {
                                            if ui.button("✕").clicked() {
                                                self.project.in_point = None;
                                                self.project.out_point = None;
                                            }
                                        }
                                        if let Some(dur) = self.project.segment_duration() {
                                            ui.label(format!("Dur:{}", Self::format_time(dur)));
                                        }
                                        ui.label(format!(
                                            "OUT:{}",
                                            Self::format_time(
                                                self.project.out_point.unwrap_or(0.0)
                                            )
                                        ));
                                        if ui.button("O").clicked() {
                                            self.project.out_point = Some(self.current_time);
                                        }
                                        ui.label(format!(
                                            "IN:{}",
                                            Self::format_time(self.project.in_point.unwrap_or(0.0))
                                        ));
                                        if ui.button("I").clicked() {
                                            self.project.in_point = Some(self.current_time);
                                        }
                                    },
                                );
                            });
                        });
                    }
                });

            // Preview fills remaining space
            egui::CentralPanel::default().show_inside(ui, |ui| {
                egui::Frame::dark_canvas(ui.style()).show(ui, |ui| {
                    let avail = ui.available_size();
                    let (pw, ph) = if avail.y > avail.x * 0.5625 {
                        (avail.x, avail.x * 0.5625)
                    } else {
                        (avail.y / 0.5625, avail.y)
                    };
                    ui.centered_and_justified(|ui| {
                        let (rect, _) =
                            ui.allocate_exact_size(egui::vec2(pw, ph), egui::Sense::hover());
                        self.preview_rect = Some(rect);
                        ui.painter().rect_filled(
                            rect,
                            egui::CornerRadius::same(4),
                            egui::Color32::from_rgb(20, 20, 30),
                        );

                        if !self.player_ready {
                            ui.painter().text(
                                rect.center(),
                                egui::Align2::CENTER_CENTER,
                                "Drop a video file or use Ctrl+O to open",
                                egui::FontId::proportional(18.0),
                                egui::Color32::GRAY,
                            );
                        }
                    });
                });
            });
        });

        // ── Process deferred file open (after preview_rect is known) ──
        if let Some(path) = self.pending_open.take() {
            self.open_file(&path);
        }

        // ── Keyboard shortcuts ──
        ctx.input_mut(|i| {
            for event in &i.events {
                if let egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } = event
                {
                    if modifiers.ctrl && *key == egui::Key::D {
                        self.debug_hover = !self.debug_hover;
                    } else if modifiers.ctrl && *key == egui::Key::O {
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
