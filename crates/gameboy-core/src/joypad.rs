#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum JoypadButton {
    Right,
    Left,
    Up,
    Down,
    A,
    B,
    Select,
    Start,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct JoypadState {
    bits: u8,
}

impl JoypadState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with(mut self, button: JoypadButton, pressed: bool) -> Self {
        self.set(button, pressed);
        self
    }

    pub fn set(&mut self, button: JoypadButton, pressed: bool) {
        if pressed {
            self.bits |= button.mask();
        } else {
            self.bits &= !button.mask();
        }
    }

    pub fn is_pressed(self, button: JoypadButton) -> bool {
        self.bits & button.mask() != 0
    }

    pub(crate) fn action_nibble(self) -> u8 {
        let mut nibble = 0x0F;
        if self.is_pressed(JoypadButton::A) {
            nibble &= !0x01;
        }
        if self.is_pressed(JoypadButton::B) {
            nibble &= !0x02;
        }
        if self.is_pressed(JoypadButton::Select) {
            nibble &= !0x04;
        }
        if self.is_pressed(JoypadButton::Start) {
            nibble &= !0x08;
        }
        nibble
    }

    pub(crate) fn direction_nibble(self) -> u8 {
        let mut nibble = 0x0F;
        if self.is_pressed(JoypadButton::Right) {
            nibble &= !0x01;
        }
        if self.is_pressed(JoypadButton::Left) {
            nibble &= !0x02;
        }
        if self.is_pressed(JoypadButton::Up) {
            nibble &= !0x04;
        }
        if self.is_pressed(JoypadButton::Down) {
            nibble &= !0x08;
        }
        nibble
    }
}

impl JoypadButton {
    fn mask(self) -> u8 {
        match self {
            Self::Right => 1 << 0,
            Self::Left => 1 << 1,
            Self::Up => 1 << 2,
            Self::Down => 1 << 3,
            Self::A => 1 << 4,
            Self::B => 1 << 5,
            Self::Select => 1 << 6,
            Self::Start => 1 << 7,
        }
    }
}
