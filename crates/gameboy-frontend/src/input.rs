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
