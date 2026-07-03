use egui::{InputState, Key};
use gameboy_core::{JoypadButton, JoypadState};
use gilrs::{ev::Code, Axis, Button, Gilrs};
use serde::{Deserialize, Serialize};

const ANALOG_DEADZONE: f32 = 0.5;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum InputBinding {
    Keyboard(Key),
    GamepadButton(Button),
    GamepadCode(Code),
    GamepadAxis { axis: Axis, direction: AxisDirection },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum AxisDirection {
    Negative,
    Positive,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Bind {
    Pad(JoypadButton),
    Menu,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlBinding {
    pub button: Bind,
    pub inputs: Vec<InputBinding>,
}

pub fn default_controls() -> Vec<ControlBinding> {
    vec![
        ControlBinding {
            button: Bind::Pad(JoypadButton::Right),
            inputs: vec![
                InputBinding::Keyboard(Key::ArrowRight),
                InputBinding::Keyboard(Key::D),
                InputBinding::GamepadButton(Button::DPadRight),
                InputBinding::GamepadAxis {
                    axis: Axis::LeftStickX,
                    direction: AxisDirection::Positive,
                },
            ],
        },
        ControlBinding {
            button: Bind::Pad(JoypadButton::Left),
            inputs: vec![
                InputBinding::Keyboard(Key::ArrowLeft),
                InputBinding::Keyboard(Key::A),
                InputBinding::GamepadButton(Button::DPadLeft),
                InputBinding::GamepadAxis {
                    axis: Axis::LeftStickX,
                    direction: AxisDirection::Negative,
                },
            ],
        },
        ControlBinding {
            button: Bind::Pad(JoypadButton::Up),
            inputs: vec![
                InputBinding::Keyboard(Key::ArrowUp),
                InputBinding::Keyboard(Key::W),
                InputBinding::GamepadButton(Button::DPadUp),
                InputBinding::GamepadAxis {
                    axis: Axis::LeftStickY,
                    direction: AxisDirection::Positive,
                },
            ],
        },
        ControlBinding {
            button: Bind::Pad(JoypadButton::Down),
            inputs: vec![
                InputBinding::Keyboard(Key::ArrowDown),
                InputBinding::Keyboard(Key::S),
                InputBinding::GamepadButton(Button::DPadDown),
                InputBinding::GamepadAxis {
                    axis: Axis::LeftStickY,
                    direction: AxisDirection::Negative,
                },
            ],
        },
        ControlBinding {
            button: Bind::Pad(JoypadButton::A),
            inputs: vec![
                InputBinding::Keyboard(Key::Z),
                InputBinding::GamepadButton(Button::South),
            ],
        },
        ControlBinding {
            button: Bind::Pad(JoypadButton::B),
            inputs: vec![
                InputBinding::Keyboard(Key::X),
                InputBinding::GamepadButton(Button::East),
            ],
        },
        ControlBinding {
            button: Bind::Pad(JoypadButton::Select),
            inputs: vec![
                InputBinding::Keyboard(Key::Backspace),
                InputBinding::GamepadButton(Button::Select),
            ],
        },
        ControlBinding {
            button: Bind::Pad(JoypadButton::Start),
            inputs: vec![
                InputBinding::Keyboard(Key::Enter),
                InputBinding::GamepadButton(Button::Start),
            ],
        },
        ControlBinding {
            button: Bind::Menu,
            inputs: vec![
                InputBinding::Keyboard(Key::Escape),
                InputBinding::GamepadButton(Button::Mode),
            ],
        },
    ]
}

pub struct Mapping {
    pub id: Bind,
    pub button: &'static str,
    pub keyboard: String,
    pub gamepad: String,
}

pub fn mappings(controls: &[ControlBinding]) -> Vec<Mapping> {
    const ORDER: [(Bind, &str); 9] = [
        (Bind::Pad(JoypadButton::Up), "D-Pad Up"),
        (Bind::Pad(JoypadButton::Down), "D-Pad Down"),
        (Bind::Pad(JoypadButton::Left), "D-Pad Left"),
        (Bind::Pad(JoypadButton::Right), "D-Pad Right"),
        (Bind::Pad(JoypadButton::A), "A"),
        (Bind::Pad(JoypadButton::B), "B"),
        (Bind::Pad(JoypadButton::Start), "Start"),
        (Bind::Pad(JoypadButton::Select), "Select"),
        (Bind::Menu, "Menu"),
    ];

    ORDER
        .iter()
        .map(|(button, label)| {
            let binding = controls.iter().find(|c| c.button == *button);
            Mapping {
                id: *button,
                button: label,
                keyboard: binding.map(keyboard_label).unwrap_or_else(|| "—".into()),
                gamepad: binding.map(gamepad_label).unwrap_or_else(|| "—".into()),
            }
        })
        .collect()
}

fn keyboard_label(binding: &ControlBinding) -> String {
    binding
        .inputs
        .iter()
        .find_map(|input| match input {
            InputBinding::Keyboard(key) => Some(key_symbol(*key)),
            _ => None,
        })
        .unwrap_or_else(|| "—".into())
}

fn gamepad_label(binding: &ControlBinding) -> String {
    binding
        .inputs
        .iter()
        .find_map(|input| match input {
            InputBinding::GamepadButton(button) => Some(gamepad_button_label(*button)),
            InputBinding::GamepadCode(code) => Some(gamepad_code_label(code)),
            _ => None,
        })
        .unwrap_or_else(|| "—".into())
}

fn key_symbol(key: Key) -> String {
    match key {
        Key::ArrowUp => "↑".into(),
        Key::ArrowDown => "↓".into(),
        Key::ArrowLeft => "←".into(),
        Key::ArrowRight => "→".into(),
        Key::Enter => "Enter".into(),
        Key::Backspace => "Backspace".into(),
        Key::Escape => "Esc".into(),
        other => other.name().to_string(),
    }
}

fn gamepad_button_label(button: Button) -> String {
    match button {
        Button::DPadUp => "D-Pad ↑".into(),
        Button::DPadDown => "D-Pad ↓".into(),
        Button::DPadLeft => "D-Pad ←".into(),
        Button::DPadRight => "D-Pad →".into(),
        Button::South => "A / ✕".into(),
        Button::East => "B / ○".into(),
        Button::North => "Y / △".into(),
        Button::West => "X / □".into(),
        Button::Start => "Start".into(),
        Button::Select => "Select".into(),
        Button::Mode => "Home".into(),
        other => format!("{other:?}"),
    }
}

fn gamepad_code_label(code: &Code) -> String {
    let text = code.to_string();
    let usage = text
        .split_once('(')
        .and_then(|(_, rest)| rest.strip_suffix(')'))
        .and_then(|n| n.trim().parse::<u32>().ok());
    match usage {
        Some(7) => "L2".into(),
        Some(8) => "R2".into(),
        Some(14) => "Touchpad".into(),
        Some(n) => format!("Btn {n}"),
        None => format!("Btn {text}"),
    }
}

pub fn read_joypad_state(
    input: &InputState,
    mut gilrs: Option<&mut Gilrs>,
    controls: &[ControlBinding],
) -> JoypadState {
    if let Some(gilrs) = gilrs.as_deref_mut() {
        while gilrs.next_event().is_some() {}
    }

    let mut state = JoypadState::new();
    for control in controls {
        let Bind::Pad(button) = control.button else {
            continue;
        };
        let pressed = control
            .inputs
            .iter()
            .any(|binding| binding_pressed(*binding, input, gilrs.as_deref()));
        state.set(button, pressed);
    }
    state
}

pub fn menu_pressed(input: &InputState, gilrs: Option<&Gilrs>, controls: &[ControlBinding]) -> bool {
    controls
        .iter()
        .find(|c| c.button == Bind::Menu)
        .is_some_and(|c| {
            c.inputs
                .iter()
                .any(|binding| binding_pressed(*binding, input, gilrs))
        })
}

fn binding_pressed(binding: InputBinding, input: &InputState, gilrs: Option<&Gilrs>) -> bool {
    match binding {
        InputBinding::Keyboard(key) => input.key_down(key),
        InputBinding::GamepadButton(Button::Unknown) => false,
        InputBinding::GamepadButton(button) => gilrs
            .map(|gilrs| {
                gilrs
                    .gamepads()
                    .any(|(_, gamepad)| gamepad.is_pressed(button))
            })
            .unwrap_or(false),
        InputBinding::GamepadCode(code) => gilrs
            .map(|gilrs| {
                gilrs
                    .gamepads()
                    .any(|(_, gamepad)| gamepad.state().is_pressed(code))
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
