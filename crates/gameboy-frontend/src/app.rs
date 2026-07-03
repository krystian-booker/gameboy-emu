use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{mpsc, Arc, Mutex},
    time::{Duration, Instant},
};

use eframe::egui_glow;
use egui::{
    load::SizedTexture, pos2, vec2, Align, Align2, Color32, CornerRadius, FontId, Key, Layout,
    Margin, Rect, RichText, Sense, Shape, Stroke, StrokeKind,
};
use gameboy_core::ppu::{SCREEN_HEIGHT, SCREEN_WIDTH};
use gameboy_core::{JoypadButton, JoypadState};
use gilrs::{ev::Code, Button, EventType, Gilrs};

use crate::{
    browser::DirBrowser,
    config::Config,
    input::{bind_pressed, mappings, menu_pressed, read_joypad_state, Bind, ControlBinding, InputBinding},
    library::{spawn_scan, RomEntry, ScanResult},
    renderer::{Pipeline, ShaderParams},
    session::Session,
    states::{StateMeta, StateStore},
    theme::{self, Palette, PaletteKind},
};

const SHADER_CONTROLS: usize = 8;

const SETTINGS_BUTTONS: [Bind; 10] = [
    Bind::Pad(JoypadButton::Up),
    Bind::Pad(JoypadButton::Down),
    Bind::Pad(JoypadButton::Left),
    Bind::Pad(JoypadButton::Right),
    Bind::Pad(JoypadButton::A),
    Bind::Pad(JoypadButton::B),
    Bind::Pad(JoypadButton::Start),
    Bind::Pad(JoypadButton::Select),
    Bind::Menu,
    Bind::Pause,
];

const FRAME_TIME: Duration = Duration::from_nanos(16_742_706);
const MAX_FRAMES_PER_TICK: u32 = 8;
const BOOT_DURATION: Duration = Duration::from_millis(2200);

const SPEEDS: [f32; 5] = [1.0, 1.5, 2.0, 3.0, 5.0];
const SAVE_SLOTS: usize = 4;

const ERROR_COLOR: Color32 = Color32::from_rgb(224, 96, 96);

#[derive(Clone, Copy, PartialEq)]
enum Screen {
    Boot,
    Library,
    Settings,
    Browser,
    Playing,
}

#[derive(Clone, Copy, PartialEq)]
enum Listening {
    None,
    Key(Bind),
    Pad(Bind),
}

#[derive(Clone, Copy, PartialEq)]
enum PauseView {
    Menu,
    Save,
    Load,
}

#[derive(Clone, Copy, PartialEq)]
enum PauseItem {
    Speed,
    Save,
    Load,
}

const PAUSE_ITEMS: [PauseItem; 3] = [PauseItem::Speed, PauseItem::Save, PauseItem::Load];

#[derive(Clone, Copy, PartialEq)]
enum SetItem {
    ChangeFolder,
    Device,
    Map(Bind, bool),
    Palette,
    Shader(usize),
}

fn settings_items() -> Vec<SetItem> {
    let mut items = vec![SetItem::ChangeFolder, SetItem::Device];
    for button in SETTINGS_BUTTONS {
        items.push(SetItem::Map(button, false));
        items.push(SetItem::Map(button, true));
    }
    items.push(SetItem::Palette);
    for i in 0..SHADER_CONTROLS {
        items.push(SetItem::Shader(i));
    }
    items
}

fn focus_move(
    cur: SetItem,
    up: bool,
    down: bool,
    left: bool,
    right: bool,
) -> Option<SetItem> {
    use SetItem::*;
    let last_btn = SETTINGS_BUTTONS.len() - 1;
    match cur {
        Map(btn, is_gamepad) => {
            let bi = SETTINGS_BUTTONS.iter().position(|b| *b == btn)?;
            if right && !is_gamepad {
                Some(Map(btn, true))
            } else if left && is_gamepad {
                Some(Map(btn, false))
            } else if down {
                Some(if bi < last_btn {
                    Map(SETTINGS_BUTTONS[bi + 1], is_gamepad)
                } else {
                    Palette
                })
            } else if up {
                Some(if bi > 0 {
                    Map(SETTINGS_BUTTONS[bi - 1], is_gamepad)
                } else {
                    Device
                })
            } else {
                None
            }
        }
        ChangeFolder => {
            if down {
                Some(Device)
            } else if up {
                Some(Shader(SHADER_CONTROLS - 1))
            } else {
                None
            }
        }
        Device => {
            if down {
                Some(Map(SETTINGS_BUTTONS[0], false))
            } else if up {
                Some(ChangeFolder)
            } else {
                None
            }
        }
        Palette => {
            if down {
                Some(Shader(0))
            } else if up {
                Some(Map(SETTINGS_BUTTONS[last_btn], false))
            } else {
                None
            }
        }
        Shader(k) => {
            if down {
                Some(if k + 1 < SHADER_CONTROLS { Shader(k + 1) } else { ChangeFolder })
            } else if up {
                Some(if k > 0 { Shader(k - 1) } else { Palette })
            } else {
                None
            }
        }
    }
}

enum ScanState {
    Idle,
    Scanning(mpsc::Receiver<ScanResult>),
    Ready(Vec<RomEntry>),
    Error(String),
}

impl ScanState {
    fn entries(&self) -> &[RomEntry] {
        match self {
            ScanState::Ready(entries) => entries,
            _ => &[],
        }
    }
}

#[derive(Clone, Copy)]
enum LibRow {
    Rom(usize),
    Orphan(usize),
}

enum LibAction {
    Select(usize),
    Activate(LibRow),
    Discard(String),
    OpenBrowser,
}

pub struct App {
    screen: Screen,
    config: Config,
    browser: DirBrowser,
    scan: ScanState,
    session: Option<Session>,
    current: Option<RomEntry>,
    states: StateStore,
    state_textures: HashMap<String, egui::TextureHandle>,
    controls: Vec<ControlBinding>,
    gilrs: Option<Gilrs>,
    rgba: Vec<u8>,
    error: Option<String>,
    boot_started: Option<Instant>,
    sel: usize,
    device_idx: usize,
    play_accumulated: Duration,
    play_since: Option<Instant>,
    prev_pad: JoypadState,
    paused: bool,
    pause_prev: bool,
    speed_idx: usize,
    speed: f32,
    pause_view: PauseView,
    pause_sel: usize,
    pause_slot: usize,
    pause_slots: [Option<StateMeta>; SAVE_SLOTS],
    listening: Listening,
    lib_gear_focus: bool,
    browser_from: Screen,
    settings_focus: usize,
    settings_scroll: bool,
    pipeline: Arc<Mutex<Option<Pipeline>>>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::apply(&cc.egui_ctx);

        let mut config = Config::load(cc.storage);
        for default in crate::input::default_controls() {
            if !config.controls.iter().any(|c| c.button == default.button) {
                config.controls.push(default);
            }
        }
        let browser = DirBrowser::new(config.rom_dir.clone());

        let mut app = Self {
            screen: Screen::Boot,
            browser,
            scan: ScanState::Idle,
            session: None,
            current: None,
            states: StateStore::load(),
            state_textures: HashMap::new(),
            controls: config.controls.clone(),
            gilrs: Gilrs::new().ok(),
            rgba: vec![0; SCREEN_WIDTH * SCREEN_HEIGHT * 4],
            error: None,
            boot_started: Some(Instant::now()),
            sel: 0,
            device_idx: 0,
            play_accumulated: Duration::ZERO,
            play_since: None,
            prev_pad: JoypadState::new(),
            paused: false,
            pause_prev: false,
            speed_idx: 0,
            speed: SPEEDS[0],
            pause_view: PauseView::Menu,
            pause_sel: 0,
            pause_slot: 0,
            pause_slots: Default::default(),
            listening: Listening::None,
            lib_gear_focus: false,
            browser_from: Screen::Library,
            settings_focus: 0,
            settings_scroll: false,
            pipeline: Arc::new(Mutex::new(None)),
            config,
        };

        if let Some(dir) = app.config.rom_dir.clone() {
            if dir.is_dir() {
                app.start_scan(dir);
            }
        }

        app
    }

    fn palette(&self) -> Palette {
        self.config.palette.palette()
    }

    fn start_scan(&mut self, dir: PathBuf) {
        self.scan = ScanState::Scanning(spawn_scan(dir));
    }

    fn poll_scan(&mut self, ctx: &egui::Context) {
        if let ScanState::Scanning(rx) = &self.scan {
            match rx.try_recv() {
                Ok(Ok(entries)) => {
                    self.sel = self.sel.min(entries.len().saturating_sub(1));
                    self.scan = ScanState::Ready(entries);
                }
                Ok(Err(err)) => self.scan = ScanState::Error(err),
                Err(mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(Duration::from_millis(50))
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.scan = ScanState::Error("scan thread stopped unexpectedly".to_string())
                }
            }
        }
    }

    fn flush_saves(&mut self) {
        if let Some(session) = &self.session {
            if let Err(err) = session.persist_saves() {
                self.error = Some(format!("failed to write save: {err}"));
            }
        }
    }

    fn bank_playtime(&mut self) {
        if let Some(since) = self.play_since.take() {
            self.play_accumulated += since.elapsed();
        }
    }

    fn total_playtime(&self) -> Duration {
        self.play_accumulated + self.play_since.map(|s| s.elapsed()).unwrap_or_default()
    }

    fn pause(&mut self) {
        self.bank_playtime();
        self.flush_saves();
        self.suspend_current();
        self.session = None;
        self.current = None;
        self.play_accumulated = Duration::ZERO;
        self.play_since = None;
        self.paused = false;
        self.pause_prev = false;
        self.screen = Screen::Library;
    }

    fn suspend_current(&mut self) {
        self.write_state(0);
    }

    fn write_state(&mut self, slot: u32) {
        let Some(session) = self.session.as_ref() else {
            return;
        };
        let Some(entry) = self.current.clone() else {
            return;
        };
        let state = match session.snapshot() {
            Ok(state) => state,
            Err(err) => {
                self.error = Some(err);
                return;
            }
        };
        let meta = StateMeta::new(
            entry.path.clone(),
            entry.display_title().to_string(),
            entry.mapper.clone(),
            entry.color,
            self.total_playtime().as_secs(),
            self.rgba.clone(),
            slot,
        );
        self.state_textures.remove(meta.slug());
        if let Err(err) = self.states.save(meta, &state) {
            self.error = Some(format!("failed to write save state: {err}"));
        }
    }

    fn save_slot(&mut self, slot: u32) {
        self.write_state(slot);
        self.refresh_pause_slots();
    }

    fn load_slot(&mut self, slot: u32) {
        let Some(entry) = self.current.clone() else {
            return;
        };
        let Some(meta) = self.states.find_slot(&entry.path, slot).cloned() else {
            return;
        };
        let state = match self.states.read_state(&meta) {
            Ok(state) => state,
            Err(err) => {
                self.error = Some(format!("failed to read save state: {err}"));
                return;
            }
        };
        match Session::restore(&meta.rom_path, &state) {
            Ok(session) => {
                self.session = Some(session);
                self.error = None;
                self.paused = false;
            }
            Err(err) => {
                self.error = Some(err);
            }
        }
    }

    fn resume_state(&mut self, meta: StateMeta) {
        self.reset_playback();
        self.bank_playtime();
        self.flush_saves();

        let state = match self.states.read_state(&meta) {
            Ok(state) => state,
            Err(err) => {
                self.error = Some(format!("failed to read save state: {err}"));
                return;
            }
        };

        match Session::restore(&meta.rom_path, &state) {
            Ok(session) => {
                self.session = Some(session);
                self.current = Some(entry_from_meta(&meta));
                self.error = None;
                self.play_accumulated = Duration::from_secs(meta.playtime_secs);
                self.play_since = Some(Instant::now());
                self.screen = Screen::Playing;
            }
            Err(err) => {
                self.error = Some(err);
                self.screen = Screen::Library;
            }
        }
    }

    fn stop_state(&mut self, slug: &str) {
        if let Err(err) = self.states.remove(slug) {
            self.error = Some(format!("failed to remove save state: {err}"));
        }
        self.state_textures.remove(slug);
    }

    fn discard_session(&mut self) {
        self.bank_playtime();
        self.session = None;
        self.current = None;
        self.play_accumulated = Duration::ZERO;
        self.play_since = None;
        self.screen = Screen::Library;
    }

    fn reset_playback(&mut self) {
        self.speed_idx = 0;
        self.speed = SPEEDS[0];
        self.paused = false;
        self.pause_view = PauseView::Menu;
        self.pause_sel = 0;
    }

    fn launch(&mut self, entry: RomEntry) {
        self.lib_gear_focus = false;
        self.reset_playback();
        if let Some(meta) = self.states.find(&entry.path).cloned() {
            self.resume_state(meta);
            return;
        }

        self.bank_playtime();
        self.flush_saves();
        self.session = None;
        self.current = None;
        self.play_accumulated = Duration::ZERO;

        match Session::start(&entry.path) {
            Ok(session) => {
                self.session = Some(session);
                self.current = Some(entry);
                self.error = None;
                self.play_since = Some(Instant::now());
                self.screen = Screen::Playing;
            }
            Err(err) => {
                self.error = Some(err);
                self.screen = Screen::Library;
            }
        }
    }

    fn open_browser(&mut self, from: Screen) {
        self.browser = DirBrowser::new(self.config.rom_dir.clone());
        self.browser_from = from;
        self.screen = Screen::Browser;
    }

    fn open_settings(&mut self) {
        self.lib_gear_focus = false;
        self.settings_focus = 0;
        self.settings_scroll = true;
        self.screen = Screen::Settings;
    }

    fn after_boot(&mut self) {
        self.boot_started = None;
        if self.config.rom_dir.as_ref().is_some_and(|d| d.is_dir()) {
            self.screen = Screen::Library;
        } else {
            self.open_browser(Screen::Library);
        }
    }

    fn handle_menu_input(&mut self, ctx: &egui::Context, captured_pad: Option<(Button, Code)>) {
        let pad = ctx.input(|i| read_joypad_state(i, self.gilrs.as_mut(), &self.controls));
        let just = |b: JoypadButton| pad.is_pressed(b) && !self.prev_pad.is_pressed(b);
        let up = just(JoypadButton::Up);
        let down = just(JoypadButton::Down);
        let left = just(JoypadButton::Left);
        let right = just(JoypadButton::Right);
        let select = just(JoypadButton::A) || just(JoypadButton::Start);
        let back_btn = just(JoypadButton::B);
        let menu_btn = just(JoypadButton::Select);
        self.prev_pad = pad;

        let (esc, captured_key) = ctx.input(|i| {
            let esc = i.key_pressed(Key::Escape);
            let key = i.events.iter().find_map(|e| match e {
                egui::Event::Key {
                    key,
                    pressed: true,
                    repeat: false,
                    ..
                } if *key != Key::Escape => Some(*key),
                _ => None,
            });
            (esc, key)
        });

        match self.listening {
            Listening::Key(button) => {
                if esc {
                    self.listening = Listening::None;
                } else if let Some(key) = captured_key {
                    self.rebind_key(button, key);
                    self.listening = Listening::None;
                    self.sync_prev_pad(ctx);
                }
                return;
            }
            Listening::Pad(button) => {
                if esc {
                    self.listening = Listening::None;
                } else if let Some(button_input) = captured_pad {
                    self.rebind_pad(button, button_input);
                    self.listening = Listening::None;
                    self.sync_prev_pad(ctx);
                }
                return;
            }
            Listening::None => {}
        }

        if esc || back_btn {
            match self.screen {
                Screen::Settings => {
                    self.screen = Screen::Library;
                    return;
                }
                Screen::Browser
                    if self.config.rom_dir.as_ref().is_some_and(|d| d.is_dir()) =>
                {
                    self.screen = self.browser_from;
                    return;
                }
                _ => {}
            }
        }

        match self.screen {
            Screen::Library => {
                if menu_btn {
                    self.open_settings();
                    return;
                }

                if self.lib_gear_focus {
                    if select {
                        self.open_settings();
                    } else if down {
                        self.lib_gear_focus = false;
                    }
                    return;
                }
                let rows = self.library_rows();
                let count = rows.len();
                if up && (count == 0 || self.sel == 0) {
                    self.lib_gear_focus = true;
                    return;
                }
                if count > 0 {
                    if self.sel >= count {
                        self.sel = count - 1;
                    }
                    if down {
                        self.sel = (self.sel + 1) % count;
                    } else if up {
                        self.sel -= 1;
                    }
                    if select {
                        match rows[self.sel] {
                            LibRow::Rom(ri) => {
                                if let Some(entry) = self.scan.entries().get(ri).cloned() {
                                    self.launch(entry);
                                }
                            }
                            LibRow::Orphan(si) => {
                                if let Some(meta) = self.states.auto_entries().get(si).map(|m| (*m).clone()) {
                                    self.resume_state(meta);
                                }
                            }
                        }
                    }
                }
            }
            Screen::Settings => {
                if menu_btn {
                    self.screen = Screen::Library;
                    return;
                }
                self.navigate_settings(up, down, left, right, select);
            }
            _ => {}
        }
    }

    fn navigate_settings(&mut self, up: bool, down: bool, left: bool, right: bool, select: bool) {
        let items = settings_items();
        if self.settings_focus >= items.len() {
            self.settings_focus = 0;
        }
        let cur = items[self.settings_focus];

        if let Some(target) = focus_move(cur, up, down, left, right) {
            if let Some(idx) = items.iter().position(|it| *it == target) {
                self.settings_focus = idx;
                self.settings_scroll = true;
            }
            return;
        }

        match cur {
            SetItem::ChangeFolder => {
                if select {
                    self.open_browser(Screen::Settings);
                }
            }
            SetItem::Device => {
                let count = self
                    .gilrs
                    .as_ref()
                    .map(|g| g.gamepads().count())
                    .unwrap_or(0);
                if count > 0 {
                    if right {
                        self.device_idx = (self.device_idx + 1) % count;
                    } else if left {
                        self.device_idx = (self.device_idx + count - 1) % count;
                    }
                }
            }
            SetItem::Map(button, is_gamepad) => {
                if select {
                    self.listening = if is_gamepad {
                        Listening::Pad(button)
                    } else {
                        Listening::Key(button)
                    };
                }
            }
            SetItem::Palette => {
                let all = PaletteKind::ALL;
                let cur = all.iter().position(|p| *p == self.config.palette).unwrap_or(0);
                if right || select {
                    self.config.palette = all[(cur + 1) % all.len()];
                } else if left {
                    self.config.palette = all[(cur + all.len() - 1) % all.len()];
                }
            }
            SetItem::Shader(i) => {
                let s = &mut self.config.shaders;
                let flip = select || left || right;
                let adjust = |v: &mut f32| {
                    let step = if left { -0.05 } else { 0.05 };
                    if left || right || select {
                        *v = (*v + step).clamp(0.0, 1.0);
                    }
                };
                match i {
                    0 if flip => s.color_correct = !s.color_correct,
                    1 => adjust(&mut s.gamma_weight),
                    2 if flip => s.ghosting = !s.ghosting,
                    3 => adjust(&mut s.response_time),
                    4 if flip && !s.integer_scale => s.pixel_aa = !s.pixel_aa,
                    5 if flip => s.integer_scale = !s.integer_scale,
                    6 if flip => s.lcd_grid = !s.lcd_grid,
                    7 => adjust(&mut s.grid_intensity),
                    _ => {}
                }
            }
        }
    }

    fn sync_prev_pad(&mut self, ctx: &egui::Context) {
        self.prev_pad = ctx.input(|i| read_joypad_state(i, self.gilrs.as_mut(), &self.controls));
    }

    fn rebind_key(&mut self, button: Bind, key: Key) {
        if let Some(control) = self.controls.iter_mut().find(|c| c.button == button) {
            match control
                .inputs
                .iter_mut()
                .find(|b| matches!(b, InputBinding::Keyboard(_)))
            {
                Some(slot) => *slot = InputBinding::Keyboard(key),
                None => control.inputs.push(InputBinding::Keyboard(key)),
            }
        }
    }

    fn rebind_pad(&mut self, button: Bind, captured: (Button, Code)) {
        let (pad_button, code) = captured;
        let new = if pad_button == Button::Unknown {
            InputBinding::GamepadCode(code)
        } else {
            InputBinding::GamepadButton(pad_button)
        };
        if let Some(control) = self.controls.iter_mut().find(|c| c.button == button) {
            match control.inputs.iter_mut().find(|b| {
                matches!(
                    b,
                    InputBinding::GamepadButton(_) | InputBinding::GamepadCode(_)
                )
            }) {
                Some(slot) => *slot = new,
                None => control.inputs.push(new),
            }
        }
    }

    fn titlebar(&mut self, ui: &mut egui::Ui, pal: Palette) {
        let frame = egui::Frame::default()
            .fill(pal.bg2)
            .inner_margin(Margin::symmetric(15, 8));

        let mut go_settings = false;

        let bar = egui::Panel::top("titlebar")
            .frame(frame)
            .show_separator_line(false)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Cheddy GB").font(theme::silk(12.0)).color(pal.scr));
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new("DOT MATRIX WITH STEREO SOUND")
                            .font(theme::silk(9.0))
                            .color(pal.scr3),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let gear_focused =
                            self.screen == Screen::Library && self.lib_gear_focus;
                        let gear_color = if self.screen == Screen::Settings || gear_focused {
                            pal.acc
                        } else {
                            pal.scr2
                        };
                        let gear = icon_button(ui, gear_color, pal.acc, draw_gear);
                        if gear.clicked() {
                            go_settings = true;
                        }
                        if gear_focused {
                            focus_ring(ui, gear.rect, pal, false);
                        }
                    });
                });
            });

        let rect = bar.response.rect;
        ui.painter()
            .hline(rect.x_range(), rect.bottom(), Stroke::new(1.0, pal.line));

        if go_settings {
            self.lib_gear_focus = false;
            self.screen = if self.screen == Screen::Settings {
                Screen::Library
            } else {
                Screen::Settings
            };
        }
    }

    fn ui_boot(&mut self, ui: &mut egui::Ui, pal: Palette) {
        let elapsed = self
            .boot_started
            .map(|s| s.elapsed())
            .unwrap_or(BOOT_DURATION);
        if elapsed >= BOOT_DURATION {
            self.after_boot();
            return;
        }
        ui.ctx().request_repaint();

        let t = elapsed.as_secs_f32();
        let drop = (t / 0.9).clamp(0.0, 1.0);
        let ease = 1.0 - (1.0 - drop).powi(3);
        let y_off = -180.0 * (1.0 - ease);
        let alpha = ((t / 0.4).clamp(0.0, 1.0) * 255.0) as u8;

        let rect = ui.max_rect();
        let painter = ui.painter();
        let center = rect.center();

        let logo_color = with_alpha(pal.scr, alpha);
        painter.text(
            pos2(center.x, center.y + y_off),
            Align2::CENTER_CENTER,
            "CheddyGB",
            theme::pixel(30.0),
            logo_color,
        );
        if t > 1.75 {
            painter.text(
                pos2(center.x, center.y + 60.0),
                Align2::CENTER_CENTER,
                "\u{2122}",
                theme::silk(12.0),
                pal.scr3,
            );
        }

        if t < 0.25 {
            let flash = (0.25 - t) / 0.25 * 0.45;
            painter.rect_filled(
                rect,
                CornerRadius::ZERO,
                Color32::from_white_alpha((flash * 255.0) as u8),
            );
        }
    }

    fn library_rows(&self) -> Vec<LibRow> {
        let roms = self.scan.entries();
        let auto = self.states.auto_entries();
        let mut matched: HashSet<&str> = HashSet::new();
        let mut rows: Vec<LibRow> = Vec::with_capacity(roms.len() + auto.len());
        for (i, entry) in roms.iter().enumerate() {
            if let Some(meta) = self.states.find(&entry.path) {
                matched.insert(meta.slug());
            }
            rows.push(LibRow::Rom(i));
        }
        for (i, meta) in auto.iter().enumerate() {
            if !matched.contains(meta.slug()) {
                rows.push(LibRow::Orphan(i));
            }
        }
        rows
    }

    fn ui_library(&mut self, ui: &mut egui::Ui, pal: Palette) {
        self.poll_scan(&ui.ctx().clone());
        paint_dot_grid(ui, pal);

        let rows = self.library_rows();
        let total = rows.len();
        if total > 0 && self.sel >= total {
            self.sel = total - 1;
        }

        let ctx = ui.ctx().clone();
        let sel = if self.lib_gear_focus {
            usize::MAX
        } else {
            self.sel
        };
        let mut action: Option<LibAction> = None;

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(4.0);
                ui.vertical_centered(|ui| {
                    ui.label(
                        RichText::new("GAME BOY")
                            .font(theme::pixel(24.0))
                            .color(pal.scr),
                    );
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new("\u{25AA} SELECT A GAME")
                            .font(theme::silk(10.0))
                            .color(pal.scr3),
                    );
                });
                ui.add_space(18.0);

                {
                    let App {
                        scan,
                        states,
                        state_textures,
                        ..
                    } = &mut *self;
                    let roms = scan.entries();
                    let auto = states.auto_entries();

                    for (i, row) in rows.iter().enumerate() {
                        let selected = i == sel;
                        let owned;
                        let (entry, meta): (&RomEntry, Option<&StateMeta>) = match *row {
                            LibRow::Rom(ri) => (&roms[ri], states.find(&roms[ri].path)),
                            LibRow::Orphan(si) => {
                                let meta = auto[si];
                                owned = entry_from_meta(meta);
                                (&owned, Some(meta))
                            }
                        };
                        let thumb = meta.and_then(|m| thumb_texture(state_textures, &ctx, m));
                        let a = game_row(ui, pal, entry, meta.map(|m| (m, thumb)), selected);

                        if a.discard {
                            if let Some(m) = meta {
                                action = Some(LibAction::Discard(m.slug().to_string()));
                            }
                        } else if a.activate {
                            action = Some(LibAction::Activate(*row));
                        } else if a.select {
                            action = Some(LibAction::Select(i));
                        }
                    }
                }

                match &self.scan {
                    ScanState::Scanning(_) | ScanState::Idle => {
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(RichText::new("SCANNING\u{2026}").color(pal.scr2));
                        });
                    }
                    ScanState::Error(err) => {
                        ui.colored_label(ERROR_COLOR, err.clone());
                    }
                    ScanState::Ready(entries) if entries.is_empty() => {
                        ui.add_space(4.0);
                        ui.label(RichText::new("NO ROMS FOUND").color(pal.scr2));
                        ui.add_space(8.0);
                        if text_button(ui, "CHANGE FOLDER", theme::silk(9.0), pal.scr, pal.bg)
                            .clicked()
                            && action.is_none()
                        {
                            action = Some(LibAction::OpenBrowser);
                        }
                    }
                    ScanState::Ready(_) => {}
                }

                if let Some(err) = &self.error {
                    ui.add_space(8.0);
                    ui.colored_label(ERROR_COLOR, err);
                }
            });

        match action {
            Some(LibAction::Select(i)) => self.sel = i,
            Some(LibAction::Activate(LibRow::Rom(ri))) => {
                if let Some(entry) = self.scan.entries().get(ri).cloned() {
                    self.launch(entry);
                }
            }
            Some(LibAction::Activate(LibRow::Orphan(si))) => {
                if let Some(meta) = self.states.entries().get(si).cloned() {
                    self.resume_state(meta);
                }
            }
            Some(LibAction::Discard(slug)) => self.stop_state(&slug),
            Some(LibAction::OpenBrowser) => self.open_browser(Screen::Library),
            None => {}
        }
    }

    fn ui_settings(&mut self, ui: &mut egui::Ui, pal: Palette) {
        self.poll_scan(&ui.ctx().clone());
        paint_dot_grid(ui, pal);

        let items = settings_items();
        let focused = items[self.settings_focus.min(items.len() - 1)];
        let scroll = self.settings_scroll;
        let mut open_browser = false;

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if icon_button(ui, pal.scr, pal.acc, draw_back).clicked() {
                        self.screen = Screen::Library;
                    }
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new("OPTIONS")
                            .font(theme::pixel(16.0))
                            .color(pal.scr),
                    );
                });
                ui.add_space(20.0);

                section_label(ui, pal, "ROM FOLDER");
                egui::Frame::default()
                    .stroke(Stroke::new(2.0, pal.line))
                    .corner_radius(CornerRadius::same(4))
                    .inner_margin(Margin::symmetric(15, 11))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let path = self
                                .config
                                .rom_dir
                                .as_ref()
                                .map(|d| d.display().to_string())
                                .unwrap_or_else(|| "(not set)".into());
                            ui.label(
                                RichText::new(shorten(&path, 34))
                                    .font(theme::mono(13.0))
                                    .color(pal.scr),
                            );
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                let change =
                                    text_button(ui, "CHANGE", theme::silk(9.0), pal.scr, pal.bg);
                                if change.clicked() {
                                    open_browser = true;
                                }
                                if focused == SetItem::ChangeFolder {
                                    focus_ring(ui, change.rect, pal, scroll);
                                }
                            });
                        });
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new(format!("{} ROMS", self.scan.entries().len()))
                                .font(theme::mono(11.0))
                                .color(pal.scr3),
                        );
                    });
                ui.add_space(22.0);

                section_label(ui, pal, "CONTROLLER");
                self.controller_box(ui, pal, focused == SetItem::Device, scroll);
                ui.add_space(10.0);
                self.mapping_box(ui, pal, focused, scroll);
                ui.add_space(22.0);

                section_label(ui, pal, "SCREEN");
                self.shaders_box(ui, pal, focused, scroll);
                ui.add_space(8.0);
            });

        self.settings_scroll = false;
        if open_browser {
            self.open_browser(Screen::Settings);
        }
    }

    fn controller_box(&mut self, ui: &mut egui::Ui, pal: Palette, focused: bool, scroll: bool) {
        let devices: Vec<String> = self
            .gilrs
            .as_ref()
            .map(|g| g.gamepads().map(|(_, gp)| gp.name().to_uppercase()).collect())
            .unwrap_or_default();
        let has_pad = !devices.is_empty();
        if has_pad {
            self.device_idx %= devices.len();
        }
        let name = if has_pad {
            devices[self.device_idx].clone()
        } else {
            "KEYBOARD ONLY".to_string()
        };

        let border = if focused { pal.acc } else { pal.line };
        egui::Frame::default()
            .stroke(Stroke::new(2.0, border))
            .corner_radius(CornerRadius::same(4))
            .inner_margin(Margin::symmetric(14, 6))
            .show(ui, |ui| {
                let (rect, _) =
                    ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::hover());
                if focused && scroll {
                    ui.scroll_to_rect(rect, Some(Align::Center));
                }
                let left = Rect::from_min_size(rect.left_top(), vec2(30.0, rect.height()));
                let right = Rect::from_min_size(
                    pos2(rect.right() - 30.0, rect.top()),
                    vec2(30.0, rect.height()),
                );

                let prev = ui.interact(left, ui.id().with("dev_prev"), Sense::click());
                let next = ui.interact(right, ui.id().with("dev_next"), Sense::click());

                let painter = ui.painter();
                painter.text(
                    left.center(),
                    Align2::CENTER_CENTER,
                    "\u{25C0}",
                    theme::mono(15.0),
                    pal.acc,
                );
                painter.text(
                    right.center(),
                    Align2::CENTER_CENTER,
                    "\u{25B6}",
                    theme::mono(15.0),
                    pal.acc,
                );
                let name_pos = pos2(rect.center().x - 8.0, rect.center().y);
                let galley = painter.text(
                    name_pos,
                    Align2::CENTER_CENTER,
                    &name,
                    theme::mono(14.0),
                    pal.scr,
                );
                let dot_color = if has_pad { pal.scr } else { pal.scr3 };
                painter.circle_filled(
                    pos2(galley.right() + 12.0, rect.center().y),
                    3.5,
                    dot_color,
                );

                if prev.clicked() && has_pad {
                    self.device_idx = (self.device_idx + devices.len() - 1) % devices.len();
                }
                if next.clicked() && has_pad {
                    self.device_idx = (self.device_idx + 1) % devices.len();
                }
            });
    }

    fn mapping_box(&mut self, ui: &mut egui::Ui, pal: Palette, focused: SetItem, scroll: bool) {
        let rows = mappings(&self.controls);
        let listening = self.listening;
        let mut new_listening: Option<Listening> = None;

        egui::Frame::default()
            .stroke(Stroke::new(2.0, pal.line))
            .corner_radius(CornerRadius::same(4))
            .inner_margin(Margin::symmetric(15, 10))
            .show(ui, |ui| {
                let col_w = ((ui.available_width() - 32.0) / 3.0).max(1.0);
                egui::Grid::new("mapping_grid")
                    .num_columns(3)
                    .spacing([16.0, 9.0])
                    .min_col_width(col_w)
                    .show(ui, |ui| {
                        for h in ["BUTTON", "KEYBOARD", "GAMEPAD"] {
                            ui.label(RichText::new(h).font(theme::silk(8.5)).color(pal.scr2));
                        }
                        ui.end_row();

                        for m in &rows {
                            ui.label(
                                RichText::new(m.button).font(theme::mono(12.5)).color(pal.scr),
                            );

                            let key_on = listening == Listening::Key(m.id);
                            let key_chip = rebind_chip(ui, pal, &m.keyboard, key_on);
                            if focused == SetItem::Map(m.id, false) {
                                focus_ring(ui, key_chip.rect, pal, scroll);
                            }
                            if key_chip.clicked() {
                                new_listening = Some(toggle_listen(key_on, Listening::Key(m.id)));
                            }

                            let pad_on = listening == Listening::Pad(m.id);
                            let pad_chip = rebind_chip(ui, pal, &m.gamepad, pad_on);
                            if focused == SetItem::Map(m.id, true) {
                                focus_ring(ui, pad_chip.rect, pal, scroll);
                            }
                            if pad_chip.clicked() {
                                new_listening = Some(toggle_listen(pad_on, Listening::Pad(m.id)));
                            }
                            ui.end_row();
                        }
                    });
            });

        if let Some(next) = new_listening {
            self.listening = next;
        }
    }

    fn shaders_box(&mut self, ui: &mut egui::Ui, pal: Palette, focused: SetItem, scroll: bool) {
        egui::Frame::default()
            .stroke(Stroke::new(2.0, pal.line))
            .corner_radius(CornerRadius::same(4))
            .inner_margin(Margin::symmetric(16, 14))
            .show(ui, |ui| {
                ui.label(
                    RichText::new("SCREEN PALETTE")
                        .font(theme::mono(13.5))
                        .color(pal.scr),
                );
                ui.add_space(11.0);
                let row = ui.horizontal(|ui| {
                    for kind in PaletteKind::ALL {
                        if palette_swatch(ui, kind, self.config.palette == kind).clicked() {
                            self.config.palette = kind;
                        }
                        ui.add_space(6.0);
                    }
                });
                if focused == SetItem::Palette {
                    focus_ring(ui, row.response.rect, pal, scroll);
                }
                ui.add_space(10.0);

                let sh = &mut self.config.shaders;
                let f = |i: usize| focused == SetItem::Shader(i);

                shader_group(ui, pal, "COLOR & GAMMA");
                toggle_row(ui, pal, "COLOR CORRECTION", &mut sh.color_correct, f(0), scroll);
                slider_row(ui, pal, "GAMMA WEIGHT", &mut sh.gamma_weight, f(1), scroll);

                shader_group(ui, pal, "GHOSTING");
                toggle_row(ui, pal, "LCD GHOSTING", &mut sh.ghosting, f(2), scroll);
                slider_row(ui, pal, "RESPONSE TIME", &mut sh.response_time, f(3), scroll);

                shader_group(ui, pal, "SCALING");
                let integer_on = sh.integer_scale;
                toggle_row_enabled(
                    ui, pal, "PIXEL AA", &mut sh.pixel_aa, f(4), scroll, !integer_on, "N/A",
                );
                toggle_row(ui, pal, "INTEGER SCALE", &mut sh.integer_scale, f(5), scroll);

                shader_group(ui, pal, "LCD GRID");
                toggle_row(ui, pal, "LCD GRID", &mut sh.lcd_grid, f(6), scroll);
                slider_row(ui, pal, "GRID INTENSITY", &mut sh.grid_intensity, f(7), scroll);
            });
    }

    fn ui_browser(&mut self, ui: &mut egui::Ui, pal: Palette) {
        let can_cancel = self.config.rom_dir.as_ref().is_some_and(|d| d.is_dir());

        ui.horizontal(|ui| {
            if can_cancel && icon_button(ui, pal.scr, pal.acc, draw_back).clicked() {
                self.screen = self.browser_from;
            }
            ui.add_space(8.0);
            ui.label(
                RichText::new("ROM FOLDER")
                    .font(theme::pixel(16.0))
                    .color(pal.scr),
            );
        });
        ui.add_space(14.0);

        if let Some(dir) = self.browser.ui(ui) {
            self.config.rom_dir = Some(dir.clone());
            self.start_scan(dir);
            self.screen = Screen::Library;
        }
    }

    fn ui_playing(&mut self, ui: &mut egui::Ui, pal: Palette) {
        let ctx = ui.ctx().clone();

        if ctx.input(|i| menu_pressed(i, self.gilrs.as_ref(), &self.controls)) {
            self.pause();
            ctx.request_repaint();
            return;
        }

        let pause_now = ctx.input(|i| bind_pressed(i, self.gilrs.as_ref(), &self.controls, Bind::Pause));
        if pause_now && !self.pause_prev {
            self.paused = !self.paused;
            if self.paused {
                self.enter_pause_menu(&ctx);
            }
        }
        self.pause_prev = pause_now;

        if self.paused {
            self.handle_pause_input(&ctx);
        }

        let joypad = if self.paused {
            JoypadState::new()
        } else {
            ctx.input(|i| read_joypad_state(i, self.gilrs.as_mut(), &self.controls))
        };

        let mut has_audio = false;
        if let Some(session) = self.session.as_mut() {
            session.set_joypad_state(joypad);
            session.set_speed(self.speed);
            has_audio = session.has_audio();

            if !self.paused {
                let max_frames = if has_audio {
                    MAX_FRAMES_PER_TICK
                } else {
                    (self.speed.round() as u32).max(1)
                };
                let mut ran = 0;
                while ran < max_frames && session.ready_for_more() {
                    if let Err(err) = session.run_frame() {
                        self.error = Some(err);
                        self.discard_session();
                        return;
                    }
                    ran += 1;
                }
            }
        }

        self.upload_frame(pal);
        self.draw_screen(ui, pal);

        if self.paused {
            self.draw_pause_overlay(ui, pal);
            ctx.request_repaint_after(FRAME_TIME);
        } else if has_audio {
            ctx.request_repaint_after(Duration::from_millis(1));
        } else {
            ctx.request_repaint_after(FRAME_TIME.div_f32(self.speed));
        }
    }

    fn enter_pause_menu(&mut self, ctx: &egui::Context) {
        self.pause_view = PauseView::Menu;
        self.pause_sel = 0;
        self.pause_slot = 0;
        self.refresh_pause_slots();
        self.sync_prev_pad(ctx);
    }

    fn refresh_pause_slots(&mut self) {
        self.pause_slots = match self.current.as_ref() {
            Some(entry) => self.states.slots_for(&entry.path),
            None => Default::default(),
        };
    }

    fn handle_pause_input(&mut self, ctx: &egui::Context) {
        let pad = ctx.input(|i| read_joypad_state(i, self.gilrs.as_mut(), &self.controls));
        let just = |b: JoypadButton| pad.is_pressed(b) && !self.prev_pad.is_pressed(b);
        let up = just(JoypadButton::Up);
        let down = just(JoypadButton::Down);
        let left = just(JoypadButton::Left);
        let right = just(JoypadButton::Right);
        let select = just(JoypadButton::A) || just(JoypadButton::Start);
        let back = just(JoypadButton::B);
        self.prev_pad = pad;

        match self.pause_view {
            PauseView::Menu => {
                if back {
                    self.paused = false;
                    return;
                }
                let n = PAUSE_ITEMS.len();
                if down {
                    self.pause_sel = (self.pause_sel + 1) % n;
                } else if up {
                    self.pause_sel = (self.pause_sel + n - 1) % n;
                }
                match PAUSE_ITEMS[self.pause_sel] {
                    PauseItem::Speed => {
                        if right {
                            self.speed_idx = (self.speed_idx + 1).min(SPEEDS.len() - 1);
                        } else if left {
                            self.speed_idx = self.speed_idx.saturating_sub(1);
                        }
                        self.speed = SPEEDS[self.speed_idx];
                    }
                    PauseItem::Save if select => {
                        self.pause_view = PauseView::Save;
                        self.pause_slot = 0;
                        self.refresh_pause_slots();
                    }
                    PauseItem::Load if select => {
                        self.pause_view = PauseView::Load;
                        self.pause_slot = 0;
                        self.refresh_pause_slots();
                    }
                    _ => {}
                }
            }
            PauseView::Save | PauseView::Load => {
                if back {
                    self.pause_view = PauseView::Menu;
                    return;
                }
                if right {
                    self.pause_slot = (self.pause_slot + 1) % SAVE_SLOTS;
                } else if left {
                    self.pause_slot = (self.pause_slot + SAVE_SLOTS - 1) % SAVE_SLOTS;
                } else if down {
                    self.pause_slot = (self.pause_slot + 2) % SAVE_SLOTS;
                } else if up {
                    self.pause_slot = (self.pause_slot + SAVE_SLOTS - 2) % SAVE_SLOTS;
                }
                if select {
                    let slot = self.pause_slot as u32 + 1;
                    if matches!(self.pause_view, PauseView::Save) {
                        self.save_slot(slot);
                    } else if self.pause_slots[self.pause_slot].is_some() {
                        self.load_slot(slot);
                    }
                }
            }
        }
    }

    fn draw_pause_overlay(&mut self, ui: &mut egui::Ui, pal: Palette) {
        let rect = ui.max_rect();
        ui.painter()
            .rect_filled(rect, CornerRadius::ZERO, Color32::from_black_alpha(160));
        match self.pause_view {
            PauseView::Menu => self.draw_pause_list(ui, pal, rect),
            PauseView::Save | PauseView::Load => self.draw_pause_slots(ui, pal, rect),
        }
    }

    fn draw_pause_list(&self, ui: &mut egui::Ui, pal: Palette, rect: Rect) {
        let center = rect.center();
        let panel = Rect::from_center_size(center, vec2(360.0, 300.0));
        let painter = ui.painter();
        painter.rect_filled(panel, CornerRadius::same(8), pal.bg);
        painter.rect_stroke(
            panel,
            CornerRadius::same(8),
            Stroke::new(1.5, pal.scr),
            StrokeKind::Inside,
        );

        painter.text(
            pos2(center.x, panel.top() + 44.0),
            Align2::CENTER_CENTER,
            "PAUSED",
            theme::pixel(22.0),
            pal.scr,
        );

        let start_y = center.y - 8.0;
        let row_h = 48.0;
        for (i, item) in PAUSE_ITEMS.iter().enumerate() {
            let y = start_y + i as f32 * row_h;
            let selected = i == self.pause_sel;
            let label = match item {
                PauseItem::Speed => {
                    format!("SPEED   \u{25C4} {} \u{25BA}", speed_label(self.speed))
                }
                PauseItem::Save => "SAVE STATE".to_string(),
                PauseItem::Load => "LOAD STATE".to_string(),
            };
            let color = if selected {
                let hl = Rect::from_center_size(pos2(center.x, y), vec2(304.0, 36.0));
                painter.rect_filled(hl, CornerRadius::same(5), pal.scr);
                pal.bg
            } else {
                pal.scr
            };
            painter.text(
                pos2(center.x, y),
                Align2::CENTER_CENTER,
                label,
                theme::silk(18.0),
                color,
            );
        }
    }

    fn draw_pause_slots(&mut self, ui: &mut egui::Ui, pal: Palette, rect: Rect) {
        let ctx = ui.ctx().clone();
        let is_save = matches!(self.pause_view, PauseView::Save);
        let sel_slot = self.pause_slot;
        let center = rect.center();

        let panel = Rect::from_center_size(center, vec2(384.0, 372.0));
        ui.painter().rect_filled(panel, CornerRadius::same(8), pal.bg);
        ui.painter().rect_stroke(
            panel,
            CornerRadius::same(8),
            Stroke::new(1.5, pal.scr),
            StrokeKind::Inside,
        );

        ui.painter().text(
            pos2(center.x, panel.top() + 34.0),
            Align2::CENTER_CENTER,
            if is_save { "SAVE STATE" } else { "LOAD STATE" },
            theme::pixel(18.0),
            pal.scr,
        );

        let cell = vec2(150.0, 116.0);
        let gap = 18.0;
        let grid_w = cell.x * 2.0 + gap;
        let origin = pos2(center.x - grid_w / 2.0, panel.top() + 60.0);

        let App {
            pause_slots,
            state_textures,
            ..
        } = &mut *self;

        for (slot, meta) in pause_slots.iter().enumerate() {
            let col = (slot % 2) as f32;
            let row = (slot / 2) as f32;
            let cell_rect = Rect::from_min_size(
                pos2(origin.x + col * (cell.x + gap), origin.y + row * (cell.y + gap)),
                cell,
            );
            let selected = slot == sel_slot;

            let meta = meta.as_ref();
            let tex_id = meta
                .and_then(|m| thumb_texture(state_textures, &ctx, m))
                .map(|t| t.id());

            let painter = ui.painter();
            let cell_bg = if selected {
                with_alpha(pal.scr, 40)
            } else {
                with_alpha(pal.scr, 16)
            };
            painter.rect_filled(cell_rect, CornerRadius::same(5), cell_bg);

            let thumb_rect = Rect::from_min_size(
                cell_rect.min + vec2(8.0, 8.0),
                vec2(cell.x - 16.0, 78.0),
            );
            if let Some(id) = tex_id {
                egui::Image::new(SizedTexture::new(id, thumb_rect.size())).paint_at(ui, thumb_rect);
            } else {
                ui.painter()
                    .rect_filled(thumb_rect, CornerRadius::same(3), pal.bg2);
                ui.painter().text(
                    thumb_rect.center(),
                    Align2::CENTER_CENTER,
                    "EMPTY",
                    theme::silk(12.0),
                    pal.scr2,
                );
            }

            let label = match meta {
                Some(m) => format!(
                    "SLOT {}  \u{00B7}  {}",
                    slot + 1,
                    format_duration(Duration::from_secs(m.playtime_secs))
                ),
                None => format!("SLOT {}", slot + 1),
            };
            ui.painter().text(
                pos2(cell_rect.center().x, cell_rect.bottom() - 12.0),
                Align2::CENTER_CENTER,
                label,
                theme::silk(10.0),
                pal.scr,
            );

            if selected {
                ui.painter().rect_stroke(
                    cell_rect,
                    CornerRadius::same(5),
                    Stroke::new(2.0, pal.scr),
                    StrokeKind::Inside,
                );
            }
        }
    }

    fn upload_frame(&mut self, pal: Palette) {
        let Some(session) = self.session.as_ref() else {
            return;
        };
        let tint = self.current.as_ref().is_some_and(|e| !e.color);

        for (pixel, out) in session
            .framebuffer()
            .iter()
            .zip(self.rgba.chunks_exact_mut(4))
        {
            if tint {
                let luma = (*pixel & 0xFF) as u8;
                let color = pal.shade(luma);
                out[0] = color.r();
                out[1] = color.g();
                out[2] = color.b();
            } else {
                out[0] = (pixel >> 16) as u8;
                out[1] = (pixel >> 8) as u8;
                out[2] = *pixel as u8;
            }
            out[3] = 0xFF;
        }
    }

    fn draw_screen(&self, ui: &mut egui::Ui, _pal: Palette) {
        let avail = ui.available_size();
        let (area, _) = ui.allocate_exact_size(avail, Sense::hover());

        let aspect = SCREEN_WIDTH as f32 / SCREEN_HEIGHT as f32;
        let mut w = avail.x.min(avail.y * aspect);
        let mut h = w / aspect;
        if self.config.shaders.integer_scale {
            let scale = (h / SCREEN_HEIGHT as f32).floor().max(1.0);
            w = SCREEN_WIDTH as f32 * scale;
            h = SCREEN_HEIGHT as f32 * scale;
        }
        let rect = Rect::from_center_size(area.center(), vec2(w, h));
        if rect.width() < 1.0 || rect.height() < 1.0 {
            return;
        }

        let s = &self.config.shaders;
        let params = ShaderParams {
            color_correct: s.color_correct,
            gamma_weight: s.gamma_weight,
            ghosting: s.ghosting,
            response_time: s.response_time,
            pixel_aa: s.pixel_aa,
            lcd_grid: s.lcd_grid,
            grid_intensity: s.grid_intensity,
        };
        let rgba = self.rgba.clone();
        let pipeline = self.pipeline.clone();
        let callback = egui::PaintCallback {
            rect,
            callback: Arc::new(egui_glow::CallbackFn::new(move |info, painter| {
                let gl = painter.gl();
                let mut guard = pipeline.lock().unwrap();
                if guard.is_none() {
                    match Pipeline::new(gl) {
                        Ok(p) => *guard = Some(p),
                        Err(err) => {
                            eprintln!("failed to build shader pipeline: {err}");
                            return;
                        }
                    }
                }
                if let Some(p) = guard.as_mut() {
                    p.render(gl, &rgba, &params, &info);
                }
            })),
        };
        ui.painter().add(callback);
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        let mut captured_pad: Option<(Button, Code)> = None;
        if let Some(gilrs) = self.gilrs.as_mut() {
            while let Some(ev) = gilrs.next_event() {
                if let EventType::ButtonPressed(button, code) = ev.event {
                    captured_pad.get_or_insert((button, code));
                }
            }
        }

        let pal = self.palette();
        theme::style(&ctx, pal);

        if self.screen == Screen::Playing {
            egui::CentralPanel::default()
                .frame(egui::Frame::default().fill(pal.bg))
                .show(ui, |ui| self.ui_playing(ui, pal));
            return;
        }

        self.handle_menu_input(&ctx, captured_pad);

        if self.screen == Screen::Playing {
            egui::CentralPanel::default()
                .frame(egui::Frame::default().fill(pal.bg))
                .show(ui, |ui| self.ui_playing(ui, pal));
            return;
        }

        ctx.request_repaint_after(Duration::from_millis(33));

        self.titlebar(ui, pal);

        let frame = egui::Frame::default()
            .fill(pal.bg)
            .inner_margin(Margin::symmetric(30, 22));
        egui::CentralPanel::default()
            .frame(frame)
            .show(ui, |ui| match self.screen {
                Screen::Boot => self.ui_boot(ui, pal),
                Screen::Library => self.ui_library(ui, pal),
                Screen::Settings => self.ui_settings(ui, pal),
                Screen::Browser => self.ui_browser(ui, pal),
                Screen::Playing => {}
            });
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.config.controls = self.controls.clone();
        self.config.store(storage);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if self.session.is_some() {
            self.bank_playtime();
            self.flush_saves();
            self.suspend_current();
        }
    }
}

fn text_button(
    ui: &mut egui::Ui,
    label: &str,
    font: FontId,
    fill: Color32,
    text_color: Color32,
) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(label.to_string(), font, text_color);
    let pad = vec2(13.0, 9.0);
    let (rect, resp) = ui.allocate_at_least(galley.size() + pad * 2.0, Sense::click());
    let bg = if resp.hovered() { lighten(fill, 0.12) } else { fill };
    ui.painter().rect_filled(rect, CornerRadius::same(3), bg);
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, text_color);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn entry_from_meta(meta: &StateMeta) -> RomEntry {
    let file_name = meta
        .rom_path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    RomEntry {
        path: meta.rom_path.clone(),
        file_name,
        title: meta.title.clone(),
        color: meta.color,
        mapper: meta.mapper.clone(),
    }
}

struct RowAction {
    select: bool,
    activate: bool,
    discard: bool,
}

fn thumb_texture<'a>(
    cache: &'a mut HashMap<String, egui::TextureHandle>,
    ctx: &egui::Context,
    meta: &StateMeta,
) -> Option<&'a egui::TextureHandle> {
    if meta.thumbnail.len() != SCREEN_WIDTH * SCREEN_HEIGHT * 4 {
        return None;
    }
    Some(cache.entry(meta.slug().to_string()).or_insert_with(|| {
        let image =
            egui::ColorImage::from_rgba_unmultiplied([SCREEN_WIDTH, SCREEN_HEIGHT], &meta.thumbnail);
        ctx.load_texture(
            format!("thumb_{}", meta.slug()),
            image,
            egui::TextureOptions::NEAREST,
        )
    }))
}

fn game_row(
    ui: &mut egui::Ui,
    pal: Palette,
    entry: &RomEntry,
    suspend: Option<(&StateMeta, Option<&egui::TextureHandle>)>,
    selected: bool,
) -> RowAction {
    let pad = 16.0;
    let height = if suspend.is_some() { 58.0 } else { 46.0 };
    let (rect, resp) = ui.allocate_at_least(vec2(ui.available_width(), height), Sense::click());
    let hovered = resp.hovered();

    let (bg, fg, sub) = if selected {
        (pal.scr, pal.bg, pal.bg)
    } else if hovered {
        (pal.tint(22), pal.scr, pal.scr2)
    } else {
        (Color32::TRANSPARENT, pal.scr, pal.scr2)
    };

    let x_center = pos2(rect.right() - pad - 46.0, rect.center().y);
    let mut discard = false;
    let mut x_hovered = false;
    if suspend.is_some() {
        let x_resp = ui
            .interact(
                Rect::from_center_size(x_center, vec2(26.0, 26.0)),
                resp.id.with("discard"),
                Sense::click(),
            )
            .on_hover_text("Discard save state");
        discard = x_resp.clicked();
        x_hovered = x_resp.hovered();
    }

    ui.painter().rect_filled(rect, CornerRadius::same(3), bg);

    let title_x = if let Some((_, thumb)) = suspend {
        let (tw, th) = (52.0, height - 14.0);
        let trect =
            Rect::from_min_size(pos2(rect.left() + pad, rect.center().y - th / 2.0), vec2(tw, th));
        if let Some(tex) = thumb {
            egui::Image::new(SizedTexture::new(tex.id(), vec2(tw, th))).paint_at(ui, trect);
        } else {
            ui.painter().rect_filled(trect, CornerRadius::same(2), pal.scr);
        }
        trect.right() + 12.0
    } else {
        ui.painter().text(
            pos2(rect.left() + pad, rect.center().y),
            Align2::LEFT_CENTER,
            if selected { "\u{25BA}" } else { " " },
            theme::mono(14.0),
            fg,
        );
        rect.left() + pad + 26.0
    };

    if let Some((meta, _)) = suspend {
        ui.painter().text(
            pos2(title_x, rect.center().y - 9.0),
            Align2::LEFT_CENTER,
            entry.display_title().to_uppercase(),
            theme::mono(16.0),
            fg,
        );
        let paused = format!(
            "\u{275A}\u{275A} PAUSED {} \u{00B7} {}",
            format_duration(Duration::from_secs(meta.playtime_secs)),
            meta.mapper
        );
        ui.painter().text(
            pos2(title_x, rect.center().y + 11.0),
            Align2::LEFT_CENTER,
            paused,
            theme::mono(11.0),
            if selected { pal.bg } else { pal.acc },
        );
    } else {
        ui.painter().text(
            pos2(title_x, rect.center().y),
            Align2::LEFT_CENTER,
            entry.display_title().to_uppercase(),
            theme::mono(16.0),
            fg,
        );
    }

    let badge = if entry.color { "GBC" } else { "GB" };
    ui.painter().text(
        pos2(rect.right() - pad, rect.center().y),
        Align2::RIGHT_CENTER,
        badge,
        theme::silk(12.0),
        sub,
    );
    if suspend.is_some() {
        let x_col = if selected {
            pal.bg
        } else if x_hovered {
            ERROR_COLOR
        } else {
            sub
        };
        ui.painter()
            .text(x_center, Align2::CENTER_CENTER, "\u{2715}", theme::mono(19.0), x_col);
    }

    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    RowAction {
        select: resp.clicked() && !discard,
        activate: resp.double_clicked() && !discard,
        discard,
    }
}

fn rebind_chip(ui: &mut egui::Ui, pal: Palette, text: &str, listening: bool) -> egui::Response {
    let (label, fg, border) = if listening {
        ("PRESS\u{2026}".to_string(), pal.bg, pal.acc)
    } else {
        (text.to_string(), pal.acc, pal.line)
    };
    let fill = if listening { pal.acc } else { pal.tint(16) };
    let galley = ui.painter().layout_no_wrap(label, theme::mono(11.0), fg);
    let pad = vec2(8.0, 4.0);
    let (rect, resp) = ui.allocate_at_least(galley.size() + pad * 2.0, Sense::click());
    let stroke = if resp.hovered() { pal.scr } else { border };
    ui.painter().rect_filled(rect, CornerRadius::same(3), fill);
    ui.painter().rect_stroke(
        rect,
        CornerRadius::same(3),
        Stroke::new(1.0, stroke),
        StrokeKind::Inside,
    );
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, fg);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn toggle_listen(active: bool, target: Listening) -> Listening {
    if active {
        Listening::None
    } else {
        target
    }
}

fn palette_swatch(ui: &mut egui::Ui, kind: PaletteKind, active: bool) -> egui::Response {
    let pal = kind.palette();
    let (rect, resp) = ui.allocate_exact_size(vec2(42.0, 42.0), Sense::click());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(3), pal.sw2);
    painter.add(Shape::convex_polygon(
        vec![rect.left_top(), rect.right_top(), rect.left_bottom()],
        pal.sw1,
        Stroke::NONE,
    ));
    if active {
        painter.rect_stroke(
            rect.expand(2.0),
            CornerRadius::same(4),
            Stroke::new(2.0, pal.sw1),
            StrokeKind::Outside,
        );
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(kind.label())
}

fn toggle_row(
    ui: &mut egui::Ui,
    pal: Palette,
    label: &str,
    value: &mut bool,
    focused: bool,
    scroll: bool,
) {
    toggle_row_enabled(ui, pal, label, value, focused, scroll, true, "OFF");
}

#[allow(clippy::too_many_arguments)]
fn toggle_row_enabled(
    ui: &mut egui::Ui,
    pal: Palette,
    label: &str,
    value: &mut bool,
    focused: bool,
    scroll: bool,
    enabled: bool,
    off_text: &str,
) {
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, resp) = ui.allocate_at_least(vec2(ui.available_width(), 34.0), sense);
    if enabled && resp.clicked() {
        *value = !*value;
    }
    if focused && scroll {
        ui.scroll_to_rect(rect, Some(Align::Center));
    }
    let painter = ui.painter();
    if focused {
        painter.rect_filled(rect, CornerRadius::same(3), pal.tint(20));
    }
    painter.hline(rect.x_range(), rect.top(), Stroke::new(1.0, pal.line));
    let label_color = if !enabled {
        pal.scr3
    } else if focused {
        pal.acc
    } else {
        pal.scr
    };
    painter.text(
        pos2(rect.left() + 4.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        theme::mono(13.5),
        label_color,
    );
    let (state, color) = if !enabled {
        (off_text, pal.scr3)
    } else if *value {
        ("\u{25BA} ON", pal.acc)
    } else {
        ("OFF", pal.scr3)
    };
    painter.text(
        pos2(rect.right() - 4.0, rect.center().y),
        Align2::RIGHT_CENTER,
        state,
        theme::mono(13.5),
        color,
    );
    if enabled {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    }
}

fn shader_group(ui: &mut egui::Ui, pal: Palette, text: &str) {
    ui.add_space(8.0);
    ui.label(RichText::new(text).font(theme::silk(8.5)).color(pal.scr2));
}

fn slider_row(
    ui: &mut egui::Ui,
    pal: Palette,
    label: &str,
    value: &mut f32,
    focused: bool,
    scroll: bool,
) {
    let (rect, resp) =
        ui.allocate_at_least(vec2(ui.available_width(), 34.0), Sense::click_and_drag());
    if focused && scroll {
        ui.scroll_to_rect(rect, Some(Align::Center));
    }

    let track_w = 120.0;
    let track = Rect::from_center_size(
        pos2(rect.right() - track_w * 0.5 - 44.0, rect.center().y),
        vec2(track_w, 5.0),
    );
    if resp.clicked() || resp.dragged() {
        if let Some(pos) = resp.interact_pointer_pos() {
            *value = ((pos.x - track.left()) / track.width()).clamp(0.0, 1.0);
        }
    }
    let t = value.clamp(0.0, 1.0);

    let painter = ui.painter();
    if focused {
        painter.rect_filled(rect, CornerRadius::same(3), pal.tint(20));
    }
    painter.hline(rect.x_range(), rect.top(), Stroke::new(1.0, pal.line));
    let label_color = if focused { pal.acc } else { pal.scr };
    painter.text(
        pos2(rect.left() + 4.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        theme::mono(13.5),
        label_color,
    );
    painter.rect_filled(track, CornerRadius::same(3), pal.tint(30));
    let fill = Rect::from_min_size(track.min, vec2(track.width() * t, track.height()));
    painter.rect_filled(fill, CornerRadius::same(3), pal.acc);
    painter.circle_filled(pos2(track.left() + track.width() * t, track.center().y), 5.0, pal.scr);
    painter.text(
        pos2(rect.right() - 4.0, rect.center().y),
        Align2::RIGHT_CENTER,
        format!("{:.0}%", t * 100.0),
        theme::mono(12.0),
        if focused { pal.acc } else { pal.scr2 },
    );
    resp.on_hover_cursor(egui::CursorIcon::PointingHand);
}

fn focus_ring(ui: &mut egui::Ui, rect: Rect, pal: Palette, scroll: bool) {
    ui.painter().rect_stroke(
        rect.expand(3.0),
        CornerRadius::same(4),
        Stroke::new(2.0, pal.acc),
        StrokeKind::Outside,
    );
    if scroll {
        ui.scroll_to_rect(rect.expand(24.0), Some(Align::Center));
    }
}

fn section_label(ui: &mut egui::Ui, pal: Palette, text: &str) {
    ui.label(
        RichText::new(format!("\u{25AA} {text}"))
            .font(theme::silk(10.0))
            .color(pal.scr2),
    );
    ui.add_space(8.0);
}

fn icon_button(
    ui: &mut egui::Ui,
    color: Color32,
    hover: Color32,
    draw: impl FnOnce(&egui::Painter, Rect, Color32),
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::click());
    let c = if resp.hovered() { hover } else { color };
    draw(ui.painter(), rect, c);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn draw_gear(p: &egui::Painter, r: Rect, c: Color32) {
    let center = r.center();
    let rad = r.width() * 0.24;
    let stroke = Stroke::new(1.7, c);
    p.circle_stroke(center, rad, stroke);
    p.circle_filled(center, rad * 0.42, c);
    for i in 0..8 {
        let a = std::f32::consts::TAU * i as f32 / 8.0;
        let dir = vec2(a.cos(), a.sin());
        p.line_segment([center + dir * rad, center + dir * (rad + 3.0)], stroke);
    }
}

fn draw_back(p: &egui::Painter, r: Rect, c: Color32) {
    let stroke = Stroke::new(2.0, c);
    let cx = r.center().x + 2.0;
    let cy = r.center().y;
    let w = 5.0;
    let h = 6.0;
    p.line_segment([pos2(cx + w, cy - h), pos2(cx - w, cy)], stroke);
    p.line_segment([pos2(cx - w, cy), pos2(cx + w, cy + h)], stroke);
}

fn paint_dot_grid(ui: &egui::Ui, pal: Palette) {
    let rect = ui.clip_rect();
    let painter = ui.painter();
    let color = pal.dot();
    let step = 12.0;
    let mut x = rect.left();
    while x <= rect.right() {
        painter.vline(x, rect.y_range(), Stroke::new(1.0, color));
        x += step;
    }
    let mut y = rect.top();
    while y <= rect.bottom() {
        painter.hline(rect.x_range(), y, Stroke::new(1.0, color));
        y += step;
    }
}

fn format_duration(d: Duration) -> String {
    let s = d.as_secs();
    format!("{:02}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60)
}

fn speed_label(speed: f32) -> String {
    if speed.fract() == 0.0 {
        format!("{}x", speed as u32)
    } else {
        format!("{speed}x")
    }
}

fn shorten(s: &str, max: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max {
        s.to_string()
    } else {
        let tail: String = chars[chars.len() - (max - 1)..].iter().collect();
        format!("\u{2026}{tail}")
    }
}

fn with_alpha(c: Color32, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a)
}

fn lighten(c: Color32, t: f32) -> Color32 {
    let mix = |x: u8| (x as f32 + (255.0 - x as f32) * t).round() as u8;
    Color32::from_rgb(mix(c.r()), mix(c.g()), mix(c.b()))
}
