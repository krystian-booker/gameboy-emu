use std::{fs, path::Path};

use crate::error::{EmulatorError, Result};

const HEADER_END: usize = 0x014F;
const TITLE_START: usize = 0x0134;
const TITLE_END_EXCLUSIVE: usize = 0x0144;
const CARTRIDGE_TYPE_OFFSET: usize = 0x0147;
const ROM_SIZE_OFFSET: usize = 0x0148;
const RAM_SIZE_OFFSET: usize = 0x0149;
const MIN_ROM_SIZE: usize = 32 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cartridge {
    rom: Vec<u8>,
    header: CartridgeHeader,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CartridgeHeader {
    title: String,
    cartridge_type: u8,
    rom_size_code: u8,
    ram_size_code: u8,
}

impl Cartridge {
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self> {
        let bytes = fs::read(path).map_err(|err| EmulatorError::InvalidRom {
            reason: err.to_string(),
        })?;

        Self::from_bytes(bytes)
    }

    pub fn from_bytes(rom: Vec<u8>) -> Result<Self> {
        if rom.len() < MIN_ROM_SIZE {
            return Err(EmulatorError::InvalidRom {
                reason: format!("expected at least {MIN_ROM_SIZE} bytes, got {}", rom.len()),
            });
        }

        if HEADER_END >= rom.len() {
            return Err(EmulatorError::InvalidRom {
                reason: "ROM does not contain a complete header".to_string(),
            });
        }

        let header = CartridgeHeader::parse(&rom);
        if header.cartridge_type != 0x00 {
            return Err(EmulatorError::UnsupportedCartridge {
                cartridge_type: header.cartridge_type,
            });
        }

        Ok(Self { rom, header })
    }

    pub fn header(&self) -> &CartridgeHeader {
        &self.header
    }

    pub fn read_rom(&self, address: u16) -> Option<u8> {
        self.rom.get(address as usize).copied()
    }
}

impl CartridgeHeader {
    fn parse(rom: &[u8]) -> Self {
        let title_bytes = &rom[TITLE_START..TITLE_END_EXCLUSIVE];
        let title_end = title_bytes
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(title_bytes.len());
        let title = String::from_utf8_lossy(&title_bytes[..title_end])
            .trim_end()
            .to_string();

        Self {
            title,
            cartridge_type: rom[CARTRIDGE_TYPE_OFFSET],
            rom_size_code: rom[ROM_SIZE_OFFSET],
            ram_size_code: rom[RAM_SIZE_OFFSET],
        }
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn cartridge_type(&self) -> u8 {
        self.cartridge_type
    }

    pub fn rom_size_code(&self) -> u8 {
        self.rom_size_code
    }

    pub fn ram_size_code(&self) -> u8 {
        self.ram_size_code
    }
}

#[cfg(test)]
pub fn synthetic_rom(title: &str, program: &[u8]) -> Vec<u8> {
    let mut rom = vec![0; MIN_ROM_SIZE];
    let program_len = program.len().min(MIN_ROM_SIZE);
    rom[..program_len].copy_from_slice(&program[..program_len]);

    let title_bytes = title.as_bytes();
    let title_len = title_bytes.len().min(TITLE_END_EXCLUSIVE - TITLE_START);
    rom[TITLE_START..TITLE_START + title_len].copy_from_slice(&title_bytes[..title_len]);
    rom[CARTRIDGE_TYPE_OFFSET] = 0x00;
    rom[ROM_SIZE_OFFSET] = 0x00;
    rom[RAM_SIZE_OFFSET] = 0x00;
    rom
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_header_from_synthetic_rom() {
        let rom = synthetic_rom("TEST", &[0x00]);
        let cartridge = Cartridge::from_bytes(rom).expect("valid ROM");

        assert_eq!(cartridge.header().title(), "TEST");
        assert_eq!(cartridge.header().cartridge_type(), 0x00);
        assert_eq!(cartridge.header().rom_size_code(), 0x00);
        assert_eq!(cartridge.header().ram_size_code(), 0x00);
    }

    #[test]
    fn rejects_small_roms() {
        let err = Cartridge::from_bytes(vec![0; 16]).expect_err("small ROM should fail");

        assert!(matches!(err, EmulatorError::InvalidRom { .. }));
    }

    #[test]
    fn rejects_unsupported_cartridge_type() {
        let mut rom = synthetic_rom("MBC1", &[0x00]);
        rom[CARTRIDGE_TYPE_OFFSET] = 0x01;

        assert_eq!(
            Cartridge::from_bytes(rom),
            Err(EmulatorError::UnsupportedCartridge {
                cartridge_type: 0x01
            })
        );
    }

    #[test]
    fn synthetic_rom_places_program_at_start() {
        let rom = synthetic_rom("TEST", &[0x3E, 0x12]);
        let cartridge = Cartridge::from_bytes(rom).expect("valid ROM");

        assert_eq!(cartridge.read_rom(0), Some(0x3E));
        assert_eq!(cartridge.read_rom(1), Some(0x12));
    }
}
