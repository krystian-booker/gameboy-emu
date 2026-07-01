use std::fmt;

pub type Result<T> = std::result::Result<T, EmulatorError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmulatorError {
    InvalidRom { reason: String },
    UnsupportedCartridge { cartridge_type: u8 },
    InvalidMemoryAccess { address: u16 },
    UnimplementedOpcode { opcode: u8, pc: u16 },
}

impl fmt::Display for EmulatorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRom { reason } => write!(f, "invalid ROM: {reason}"),
            Self::UnsupportedCartridge { cartridge_type } => {
                write!(f, "unsupported cartridge type: 0x{cartridge_type:02X}")
            }
            Self::InvalidMemoryAccess { address } => {
                write!(f, "invalid memory access at 0x{address:04X}")
            }
            Self::UnimplementedOpcode { opcode, pc } => {
                write!(f, "unimplemented opcode 0x{opcode:02X} at 0x{pc:04X}")
            }
        }
    }
}

impl std::error::Error for EmulatorError {}
