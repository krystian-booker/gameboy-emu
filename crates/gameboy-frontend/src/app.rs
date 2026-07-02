use std::{path::PathBuf, sync::mpsc, time::Duration};

use egui::{
    load::SizedTexture, pos2, vec2, Align2, Color32, CornerRadius, FontId, Key, Margin, RichText,
    Sense, Stroke, StrokeKind,
};
use gameboy_core::ppu::{SCREEN_HEIGHT, SCREEN_WIDTH};
use gilrs::Gilrs;

use crate::{
    browser::DirBrowser,
    config::Config,
    input::{default_controls, read_joypad_state, ControlBinding},
    library::{spawn_scan, RomEntry, ScanResult},
    session::Session,
    theme,
};

/// ~59.7 Hz DMG refresh, used to pace the emulator when no audio device is
/// available to drive timing.
const FRAME_TIME: Duration = Duration::from_nanos(16_742_706);
/// Cap on frames run per UI tick so a stalled UI can't cause an unbounded
/// catch-up spike.
const MAX_FRAMES_PER_TICK: u32 = 8;

const ERROR_COLOR: Color32 = Color32::from_rgb(224, 96, 96);

/// Which screen the app is currently showing.
enum Screen {
    SetupRomDir,
    /// The library / game list. A ROM may still be loaded (paused) in the
    /// background, in which case it can be resumed from here.
    Menu,
    Playing,
}

/// State of the (background) ROM directory scan.
enum ScanState {
    Idle,
    Scanning(mpsc::Receiver<ScanResult>),
    Ready(Vec<RomEntry>),
    Error(String),
}

/// Deferred actions produced while rendering the menu, applied after the egui
/// closures release their borrows of `self`.
#[derive(Default)]
struct MenuActions {
    open_browser: bool,
    resume: bool,
    stop: bool,
    launch: Option<RomEntry>,
}

pub struct App {
    screen: Screen,
    config: Config,
    browser: DirBrowser,
    scan: ScanState,
    session: Option<Session>,
    /// The ROM currently loaded into `session` (running or paused).
    current: Option<RomEntry>,
    controls: Vec<ControlBinding>,
    gilrs: Option<Gilrs>,
    texture: Option<egui::TextureHandle>,
    /// Reusable RGBA scratch buffer for uploading the GB framebuffer.
    rgba: Vec<u8>,
    /// Last non-fatal error, surfaced in the UI.
    error: Option<String>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::apply(&cc.egui_ctx);

        let config = Config::load(cc.storage);
        let browser = DirBrowser::new(config.rom_dir.clone());

        let mut app = Self {
            screen: Screen::SetupRomDir,
            browser,
            scan: ScanState::Idle,
            session: None,
            current: None,
            controls: default_controls(),
            gilrs: Gilrs::new().ok(),
            texture: None,
            rgba: vec![0; SCREEN_WIDTH * SCREEN_HEIGHT * 4],
            error: None,
            config,
        };

        // If a valid ROM directory was remembered, go straight to the list.
        if let Some(dir) = app.config.rom_dir.clone() {
            if dir.is_dir() {
                app.start_scan(dir);
                app.screen = Screen::Menu;
            }
        }

        app
    }

    fn start_scan(&mut self, dir: PathBuf) {
        self.scan = ScanState::Scanning(spawn_scan(dir));
    }

    /// Persists the running ROM's saves without tearing down the session.
    fn flush_saves(&mut self) {
        if let Some(session) = &self.session {
            if let Err(err) = session.persist_saves() {
                self.error = Some(format!("failed to write save: {err}"));
            }
        }
    }

    /// Escape during play: keep the ROM loaded but stop advancing it, and return
    /// to the library where it can be resumed.
    fn pause(&mut self) {
        self.flush_saves();
        self.screen = Screen::Menu;
    }

    fn resume(&mut self) {
        if self.session.is_some() {
            self.screen = Screen::Playing;
        }
    }

    /// Fully unloads the current ROM (flushing saves) and stays in the library.
    fn stop(&mut self) {
        self.flush_saves();
        self.session = None;
        self.current = None;
        if let Some(dir) = self.config.rom_dir.clone() {
            self.start_scan(dir);
        }
        self.screen = Screen::Menu;
    }

    /// Loads `entry` into a fresh session, replacing any ROM already loaded.
    fn launch(&mut self, entry: RomEntry) {
        // Save and drop any ROM that's currently loaded before switching.
        self.flush_saves();
        self.session = None;
        self.current = None;

        match Session::start(&entry.path) {
            Ok(session) => {
                self.session = Some(session);
                self.current = Some(entry);
                self.error = None;
                self.screen = Screen::Playing;
            }
            Err(err) => {
                self.error = Some(err);
                self.screen = Screen::Menu;
            }
        }
    }

    fn open_browser(&mut self) {
        self.browser = DirBrowser::new(self.config.rom_dir.clone());
        self.screen = Screen::SetupRomDir;
    }

    fn is_current(&self, entry: &RomEntry) -> bool {
        self.current
            .as_ref()
            .is_some_and(|current| current.path == entry.path)
    }

    // ---- screens ---------------------------------------------------------

    fn ui_setup(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("setup_header").show(ui, |ui| {
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("●").color(theme::GREEN).size(14.0));
                ui.heading("Choose your ROM folder");
            });
            ui.add_space(2.0);
            ui.label(
                RichText::new(
                    "Pick the folder that contains your Game Boy ROMs. \
                     It will be remembered for next time.",
                )
                .color(theme::WEAK),
            );
            ui.add_space(12.0);
        });

        egui::CentralPanel::default().show(ui, |ui| {
            if let Some(dir) = self.browser.ui(ui) {
                self.config.rom_dir = Some(dir.clone());
                self.start_scan(dir);
                self.screen = Screen::Menu;
            }
        });
    }

    fn ui_menu(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();

        // Advance a running scan.
        if let ScanState::Scanning(rx) = &self.scan {
            match rx.try_recv() {
                Ok(Ok(entries)) => self.scan = ScanState::Ready(entries),
                Ok(Err(err)) => self.scan = ScanState::Error(err),
                Err(mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(Duration::from_millis(50))
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.scan = ScanState::Error("scan thread stopped unexpectedly".to_string())
                }
            }
        }

        let mut actions = MenuActions::default();

        egui::Panel::top("menu_header").show(ui, |ui| {
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("●").color(theme::GREEN).size(15.0));
                ui.heading("Game Boy");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("📂  Change folder").clicked() {
                        actions.open_browser = true;
                    }
                });
            });
            if let Some(dir) = &self.config.rom_dir {
                ui.add_space(2.0);
                ui.label(RichText::new(dir.display().to_string()).color(theme::WEAK).small());
            }
            ui.add_space(12.0);
        });

        egui::CentralPanel::default().show(ui, |ui| {
            if let Some(error) = &self.error {
                ui.colored_label(ERROR_COLOR, error);
                ui.add_space(6.0);
            }

            // "Now Playing" banner for a paused/loaded ROM.
            if self.session.is_some() {
                self.now_playing_card(ui, &mut actions);
                ui.add_space(10.0);
            }

            match &self.scan {
                ScanState::Scanning(_) | ScanState::Idle => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(RichText::new("Scanning for ROMs…").color(theme::WEAK));
                    });
                }
                ScanState::Error(err) => {
                    ui.colored_label(ERROR_COLOR, err.clone());
                }
                ScanState::Ready(entries) if entries.is_empty() => {
                    ui.label("No .gb or .gbc ROMs found in this folder.");
                    ui.add_space(6.0);
                    if ui.button("Choose a different folder").clicked() {
                        actions.open_browser = true;
                    }
                }
                ScanState::Ready(entries) => {
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            for entry in entries {
                                let current = self.is_current(entry);
                                if rom_row(ui, entry, current).clicked() {
                                    if current {
                                        actions.resume = true;
                                    } else {
                                        actions.launch = Some(entry.clone());
                                    }
                                }
                            }
                        });
                }
            }
        });

        // Apply deferred actions now that borrows are released.
        if actions.open_browser {
            self.open_browser();
        } else if actions.stop {
            self.stop();
        } else if actions.resume {
            self.resume();
        } else if let Some(entry) = actions.launch {
            self.launch(entry);
        }
    }

    fn now_playing_card(&self, ui: &mut egui::Ui, actions: &mut MenuActions) {
        egui::Frame::default()
            .fill(theme::GREEN_TINT)
            .stroke(Stroke::new(1.0, theme::GREEN))
            .corner_radius(CornerRadius::same(12))
            .inner_margin(Margin::same(12))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if let Some(texture) = &self.texture {
                        let h = 54.0;
                        let w = h * SCREEN_WIDTH as f32 / SCREEN_HEIGHT as f32;
                        ui.image(SizedTexture::new(texture.id(), vec2(w, h)));
                    }
                    ui.add_space(6.0);
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new("NOW PLAYING")
                                .color(theme::GREEN)
                                .small()
                                .strong(),
                        );
                        let title = self
                            .current
                            .as_ref()
                            .map(|entry| entry.display_title())
                            .unwrap_or("");
                        ui.label(RichText::new(title).size(17.0).strong());
                        ui.label(RichText::new("Paused").color(theme::WEAK).small());
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("■  Stop").clicked() {
                            actions.stop = true;
                        }
                        let resume = egui::Button::new(
                            RichText::new("▶  Resume").strong().color(theme::WINDOW_BG),
                        )
                        .fill(theme::GREEN)
                        .min_size(vec2(0.0, 32.0));
                        if ui.add(resume).clicked() {
                            actions.resume = true;
                        }
                    });
                });
            });
    }

    fn ui_playing(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();

        // Escape pauses (keeps the ROM loaded) and returns to the library.
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.pause();
            return;
        }

        let joypad = ctx.input(|i| read_joypad_state(i, self.gilrs.as_mut(), &self.controls));

        let mut has_audio = false;
        if let Some(session) = self.session.as_mut() {
            session.set_joypad_state(joypad);
            has_audio = session.has_audio();

            let mut ran = 0;
            while ran < MAX_FRAMES_PER_TICK && session.ready_for_more() {
                if let Err(err) = session.run_frame() {
                    self.error = Some(err);
                    self.stop();
                    return;
                }
                ran += 1;
                if !has_audio {
                    // Without an audio clock, run exactly one frame per tick.
                    break;
                }
            }
        }

        self.upload_frame(&ctx);
        self.draw_screen(ui);

        // Keep frames flowing: poll the audio drain frequently, or pace off the
        // wall clock when there's no audio device.
        if has_audio {
            ctx.request_repaint_after(Duration::from_millis(1));
        } else {
            ctx.request_repaint_after(FRAME_TIME);
        }
    }

    // ---- rendering -------------------------------------------------------

    /// Converts the GB framebuffer (ARGB `0xAARRGGBB`) into the reusable RGBA
    /// buffer and uploads it to the GPU texture (nearest-neighbor for crisp
    /// pixels), reusing the same texture handle across frames.
    fn upload_frame(&mut self, ctx: &egui::Context) {
        let Some(session) = self.session.as_ref() else {
            return;
        };
        for (pixel, out) in session
            .framebuffer()
            .iter()
            .zip(self.rgba.chunks_exact_mut(4))
        {
            out[0] = (pixel >> 16) as u8; // R
            out[1] = (pixel >> 8) as u8; // G
            out[2] = *pixel as u8; // B
            out[3] = 0xFF; // A (force opaque)
        }

        let image =
            egui::ColorImage::from_rgba_unmultiplied([SCREEN_WIDTH, SCREEN_HEIGHT], &self.rgba);
        match &mut self.texture {
            Some(texture) => texture.set(image, egui::TextureOptions::NEAREST),
            None => {
                self.texture =
                    Some(ctx.load_texture("gb_screen", image, egui::TextureOptions::NEAREST))
            }
        }
    }

    fn draw_screen(&self, ui: &mut egui::Ui) {
        egui::CentralPanel::default()
            .frame(egui::Frame::default().fill(Color32::BLACK))
            .show(ui, |ui| {
                let Some(texture) = &self.texture else {
                    return;
                };
                let avail = ui.available_size();
                let scale = (avail.x / SCREEN_WIDTH as f32)
                    .min(avail.y / SCREEN_HEIGHT as f32)
                    .max(0.0);
                let size = vec2(SCREEN_WIDTH as f32 * scale, SCREEN_HEIGHT as f32 * scale);
                let (rect, _) = ui.allocate_exact_size(avail, Sense::hover());
                let image_rect = egui::Rect::from_center_size(rect.center(), size);
                egui::Image::new(SizedTexture::new(texture.id(), size)).paint_at(ui, image_rect);

                // Subtle hint at the bottom.
                ui.painter().text(
                    pos2(rect.center().x, rect.bottom() - 10.0),
                    Align2::CENTER_BOTTOM,
                    "Esc — library",
                    FontId::proportional(12.0),
                    Color32::from_white_alpha(70),
                );
            });
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        match self.screen {
            Screen::SetupRomDir => self.ui_setup(ui),
            Screen::Menu => self.ui_menu(ui),
            Screen::Playing => self.ui_playing(ui),
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.config.store(storage);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if let Some(session) = &self.session {
            let _ = session.persist_saves();
        }
    }
}

/// Renders one ROM as a modern full-width card (title, metadata, GB/GBC badge)
/// and returns its click response. The currently-loaded ROM is highlighted.
fn rom_row(ui: &mut egui::Ui, entry: &RomEntry, is_current: bool) -> egui::Response {
    let height = 56.0;
    let (rect, response) =
        ui.allocate_at_least(vec2(ui.available_width(), height), Sense::click());
    let hovered = response.hovered();

    let fill = if is_current {
        theme::GREEN_TINT
    } else if hovered {
        theme::CARD_HOVER
    } else {
        theme::CARD
    };

    let painter = ui.painter();
    let radius = CornerRadius::same(10);
    painter.rect_filled(rect, radius, fill);
    if is_current {
        painter.rect_stroke(rect, radius, Stroke::new(1.0, theme::GREEN), StrokeKind::Inside);
    } else if hovered {
        painter.rect_stroke(rect, radius, Stroke::new(1.0, theme::STROKE), StrokeKind::Inside);
    }

    let pad = 16.0;
    painter.text(
        pos2(rect.left() + pad, rect.top() + 11.0),
        Align2::LEFT_TOP,
        entry.display_title(),
        FontId::proportional(16.0),
        theme::TEXT,
    );
    painter.text(
        pos2(rect.left() + pad, rect.top() + 33.0),
        Align2::LEFT_TOP,
        format!("{}  ·  {}", entry.mapper, entry.file_name),
        FontId::proportional(12.0),
        theme::WEAK,
    );

    let (badge, badge_color) = if entry.color {
        ("GBC", theme::PURPLE)
    } else {
        ("GB", theme::GREEN)
    };
    painter.text(
        pos2(rect.right() - pad, rect.center().y),
        Align2::RIGHT_CENTER,
        badge,
        FontId::proportional(13.0),
        badge_color,
    );

    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}
