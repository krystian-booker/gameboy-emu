use egui::{InputState, Key};
use gameboy_core::{JoypadButton, JoypadState};
use gilrs::{Axis, Button, Gilrs};

const ANALOG_DEADZONE: f32 = 0.5;

#[derive(Debug, Clone, Copy)]
pub enum InputBinding {
    Keyboard(Key),
    GamepadButton(Button),
    GamepadAxis { axis: Axis, direction: AxisDirection },
}

#[derive(Debug, Clone, Copy)]
pub enum AxisDirection {
    Negative,
    Positive,
}

#[derive(Debug, Clone)]
pub struct ControlBinding {
    pub button: JoypadButton,
    pub inputs: Vec<InputBinding>,
}

pub fn default_controls() -> Vec<ControlBinding> {
    vec![
        ControlBinding {
            button: JoypadButton::Right,
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
            button: JoypadButton::Left,
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
            button: JoypadButton::Up,
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
            button: JoypadButton::Down,
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

pub struct Mapping {
    pub id: JoypadButton,
    pub button: &'static str,
    pub keyboard: String,
    pub gamepad: String,
}

pub fn mappings(controls: &[ControlBinding]) -> Vec<Mapping> {
    const ORDER: [(JoypadButton, &str); 8] = [
        (JoypadButton::Up, "D-Pad Up"),
        (JoypadButton::Down, "D-Pad Down"),
        (JoypadButton::Left, "D-Pad Left"),
        (JoypadButton::Right, "D-Pad Right"),
        (JoypadButton::A, "A"),
        (JoypadButton::B, "B"),
        (JoypadButton::Start, "Start"),
        (JoypadButton::Select, "Select"),
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
        Button::Start => "Start".into(),
        Button::Select => "Select".into(),
        other => format!("{other:?}"),
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
        let pressed = control
            .inputs
            .iter()
            .any(|binding| binding_pressed(*binding, input, gilrs.as_deref()));
        state.set(control.button, pressed);
    }
    state
}

fn binding_pressed(binding: InputBinding, input: &InputState, gilrs: Option<&Gilrs>) -> bool {
    match binding {
        InputBinding::Keyboard(key) => input.key_down(key),
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
