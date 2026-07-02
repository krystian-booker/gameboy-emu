use std::{
    env, fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process, thread,
    time::{Duration, Instant},
};

use gameboy_core::{
    ppu::{SCREEN_HEIGHT, SCREEN_WIDTH},
    Emulator, JoypadButton, JoypadState,
};
use gilrs::{Axis, Button, Gilrs};
use minifb::{Key, Scale, Window, WindowOptions};

const DMG_REFRESH_RATE_HZ: f64 = 4_194_304.0 / (456.0 * 154.0);
const FRAME_TIME: Duration = Duration::from_nanos((1_000_000_000.0 / DMG_REFRESH_RATE_HZ) as u64);
const ANALOG_DEADZONE: f32 = 0.5;

#[derive(Debug, Clone, Copy)]
enum InputBinding {
    Keyboard(Key),
    GamepadButton(Button),
    GamepadAxis {
        axis: Axis,
        direction: AxisDirection,
    },
}

#[derive(Debug, Clone, Copy)]
enum AxisDirection {
    Negative,
    Positive,
}

#[derive(Debug, Clone)]
struct ControlBinding {
    button: JoypadButton,
    inputs: Vec<InputBinding>,
}

fn main() {
    let Some(path) = env::args().nth(1) else {
        eprintln!("usage: gameboy-frontend <rom.gb>");
        return;
    };

    let rom_path = PathBuf::from(&path);
    let rom = match fs::read(&rom_path) {
        Ok(rom) => rom,
        Err(err) => {
            eprintln!("failed to read {path}: {err}");
            process::exit(1);
        }
    };

    let mut emulator = Emulator::new();
    if let Err(err) = emulator.load_rom(rom) {
        eprintln!("{err}");
        process::exit(1);
    }

    let save_path = save_path_for_rom(&rom_path);
    let rtc_path = rtc_path_for_rom(&rom_path);
    if emulator.has_battery_save() && save_path.exists() {
        let save = match fs::read(&save_path) {
            Ok(save) => save,
            Err(err) => {
                eprintln!("failed to read save {}: {err}", save_path.display());
                process::exit(1);
            }
        };

        if let Err(err) = emulator.load_save_ram(&save) {
            eprintln!("failed to load save {}: {err}", save_path.display());
            process::exit(1);
        }
    }
    if emulator.has_battery_rtc() && rtc_path.exists() {
        let save = match fs::read(&rtc_path) {
            Ok(save) => save,
            Err(err) => {
                eprintln!("failed to read RTC save {}: {err}", rtc_path.display());
                process::exit(1);
            }
        };

        if let Err(err) = emulator.load_save_rtc(&save) {
            eprintln!("failed to load RTC save {}: {err}", rtc_path.display());
            process::exit(1);
        }
    }

    let mut window = match Window::new(
        "Game Boy Emulator",
        SCREEN_WIDTH,
        SCREEN_HEIGHT,
        WindowOptions {
            scale: Scale::X4,
            resize: true,
            ..WindowOptions::default()
        },
    ) {
        Ok(window) => window,
        Err(err) => {
            eprintln!("failed to create window: {err}");
            process::exit(1);
        }
    };

    let controls = default_controls();
    let mut gilrs = Gilrs::new().ok();

    while window.is_open() && !window.is_key_down(Key::Escape) {
        let frame_start = Instant::now();
        emulator.set_joypad_state(read_joypad_state(&window, gilrs.as_mut(), &controls));

        if let Err(err) = emulator.run_frame() {
            eprintln!("{err}");
            process::exit(1);
        }

        let serial_output = emulator.take_serial_output();
        if !serial_output.is_empty() {
            if let Err(err) = io::stdout()
                .write_all(&serial_output)
                .and_then(|_| io::stdout().flush())
            {
                eprintln!("failed to write serial output: {err}");
                process::exit(1);
            }
        }

        if let Err(err) =
            window.update_with_buffer(emulator.framebuffer(), SCREEN_WIDTH, SCREEN_HEIGHT)
        {
            eprintln!("failed to update window: {err}");
            process::exit(1);
        }

        let elapsed = frame_start.elapsed();
        if elapsed < FRAME_TIME {
            thread::sleep(FRAME_TIME - elapsed);
        }
    }

    if let Some(save_ram) = emulator.save_ram() {
        if let Err(err) = fs::write(&save_path, save_ram) {
            eprintln!("failed to write save {}: {err}", save_path.display());
            process::exit(1);
        }
    }
    if let Some(save_rtc) = emulator.save_rtc() {
        if let Err(err) = fs::write(&rtc_path, save_rtc) {
            eprintln!("failed to write RTC save {}: {err}", rtc_path.display());
            process::exit(1);
        }
    }
}

fn save_path_for_rom(path: &Path) -> PathBuf {
    path.with_extension("sav")
}

fn rtc_path_for_rom(path: &Path) -> PathBuf {
    path.with_extension("rtc")
}

fn default_controls() -> Vec<ControlBinding> {
    vec![
        ControlBinding {
            button: JoypadButton::Right,
            inputs: vec![
                InputBinding::Keyboard(Key::Right),
                InputBinding::Keyboard(Key::D),
                InputBinding::GamepadButton(Button::DPadRight),
                InputBinding::GamepadAxis {
                    axis: Axis::LeftStickX,
                    direction: AxisDirection::Positive,
                },
            ],
        },
        ControlBinding {
            button: JoypadButton::Left,
            inputs: vec![
                InputBinding::Keyboard(Key::Left),
                InputBinding::Keyboard(Key::A),
                InputBinding::GamepadButton(Button::DPadLeft),
                InputBinding::GamepadAxis {
                    axis: Axis::LeftStickX,
                    direction: AxisDirection::Negative,
                },
            ],
        },
        ControlBinding {
            button: JoypadButton::Up,
            inputs: vec![
                InputBinding::Keyboard(Key::Up),
                InputBinding::Keyboard(Key::W),
                InputBinding::GamepadButton(Button::DPadUp),
                InputBinding::GamepadAxis {
                    axis: Axis::LeftStickY,
                    direction: AxisDirection::Positive,
                },
            ],
        },
        ControlBinding {
            button: JoypadButton::Down,
            inputs: vec![
                InputBinding::Keyboard(Key::Down),
                InputBinding::Keyboard(Key::S),
                InputBinding::GamepadButton(Button::DPadDown),
                InputBinding::GamepadAxis {
                    axis: Axis::LeftStickY,
                    direction: AxisDirection::Negative,
                },
            ],
        },
        ControlBinding {
            button: JoypadButton::A,
            inputs: vec![
                InputBinding::Keyboard(Key::Z),
                InputBinding::GamepadButton(Button::South),
            ],
        },
        ControlBinding {
            button: JoypadButton::B,
            inputs: vec![
                InputBinding::Keyboard(Key::X),
                InputBinding::GamepadButton(Button::East),
            ],
        },
        ControlBinding {
            button: JoypadButton::Select,
            inputs: vec![
                InputBinding::Keyboard(Key::Backspace),
                InputBinding::GamepadButton(Button::Select),
            ],
        },
        ControlBinding {
            button: JoypadButton::Start,
            inputs: vec![
                InputBinding::Keyboard(Key::Enter),
                InputBinding::GamepadButton(Button::Start),
            ],
        },
    ]
}

fn read_joypad_state(
    window: &Window,
    mut gilrs: Option<&mut Gilrs>,
    controls: &[ControlBinding],
) -> JoypadState {
    if let Some(gilrs) = gilrs.as_deref_mut() {
        while gilrs.next_event().is_some() {}
    }

    let mut state = JoypadState::new();
    for control in controls {
        let pressed = control
            .inputs
            .iter()
            .any(|input| input_pressed(*input, window, gilrs.as_deref()));
        state.set(control.button, pressed);
    }
    state
}

fn input_pressed(input: InputBinding, window: &Window, gilrs: Option<&Gilrs>) -> bool {
    match input {
        InputBinding::Keyboard(key) => window.is_key_down(key),
        InputBinding::GamepadButton(button) => gilrs
            .map(|gilrs| {
                gilrs
                    .gamepads()
                    .any(|(_, gamepad)| gamepad.is_pressed(button))
            })
            .unwrap_or(false),
        InputBinding::GamepadAxis { axis, direction } => gilrs
            .map(|gilrs| {
                gilrs.gamepads().any(|(_, gamepad)| {
                    let value = gamepad.value(axis);
                    match direction {
                        AxisDirection::Negative => value <= -ANALOG_DEADZONE,
                        AxisDirection::Positive => value >= ANALOG_DEADZONE,
                    }
                })
            })
            .unwrap_or(false),
    }
}
