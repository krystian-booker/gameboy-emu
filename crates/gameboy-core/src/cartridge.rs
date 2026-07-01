use std::{fs, path::Path};

use crate::error::{EmulatorError, Result};

const TITLE_START: usize = 0x0134;
const TITLE_END_EXCLUSIVE: usize = 0x0144;
const CARTRIDGE_TYPE_OFFSET: usize = 0x0147;
const ROM_SIZE_OFFSET: usize = 0x0148;
const RAM_SIZE_OFFSET: usize = 0x0149;
const ROM_BANK_SIZE: usize = 16 * 1024;
const RAM_BANK_SIZE: usize = 8 * 1024;
const MIN_ROM_SIZE: usize = 2 * ROM_BANK_SIZE;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cartridge {
    rom: Vec<u8>,
    ram: Vec<u8>,
    header: CartridgeHeader,
    mapper: Mapper,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CartridgeHeader {
    title: String,
    cartridge_type: u8,
    rom_size_code: u8,
    ram_size_code: u8,
    mapper_kind: MapperKind,
    rom_size: usize,
    ram_size: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapperKind {
    NoMbc,
    Mbc1,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Mapper {
    NoMbc,
    Mbc1(Mbc1),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Mbc1 {
    ram_enabled: bool,
    rom_bank_low5: u8,
    bank_high2: u8,
    banking_mode: u8,
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

        let header = CartridgeHeader::parse(&rom)?;
        if rom.len() < header.rom_size {
            return Err(EmulatorError::InvalidRom {
                reason: format!(
                    "declared ROM size is {} bytes, got {}",
                    header.rom_size,
                    rom.len()
                ),
            });
        }

        let rom = rom[..header.rom_size].to_vec();
        let ram = vec![0; header.ram_size];
        let mapper = match header.mapper_kind {
            MapperKind::NoMbc => Mapper::NoMbc,
            MapperKind::Mbc1 => Mapper::Mbc1(Mbc1 {
                rom_bank_low5: 1,
                ..Mbc1::default()
            }),
        };

        Ok(Self {
            rom,
            ram,
            header,
            mapper,
        })
    }

    pub fn header(&self) -> &CartridgeHeader {
        &self.header
    }

    pub fn read_rom(&self, address: u16) -> Option<u8> {
        let address = address as usize;
        match &self.mapper {
            Mapper::NoMbc => self.rom.get(address).copied(),
            Mapper::Mbc1(mbc) => {
                let bank = if address < ROM_BANK_SIZE {
                    mbc.fixed_rom_bank(self.rom_bank_count())
                } else {
                    mbc.switchable_rom_bank(self.rom_bank_count())
                };
                let offset = address % ROM_BANK_SIZE;
                self.rom.get(bank * ROM_BANK_SIZE + offset).copied()
            }
        }
    }

    pub fn write_rom(&mut self, address: u16, value: u8) {
        match &mut self.mapper {
            Mapper::NoMbc => {}
            Mapper::Mbc1(mbc) => match address {
                0x0000..=0x1FFF => mbc.ram_enabled = value & 0x0F == 0x0A,
                0x2000..=0x3FFF => {
                    mbc.rom_bank_low5 = value & 0x1F;
                    if mbc.rom_bank_low5 == 0 {
                        mbc.rom_bank_low5 = 1;
                    }
                }
                0x4000..=0x5FFF => mbc.bank_high2 = value & 0x03,
                0x6000..=0x7FFF => mbc.banking_mode = value & 0x01,
                _ => {}
            },
        }
    }

    pub fn read_ram(&self, address: u16) -> Option<u8> {
        if self.ram.is_empty() {
            return Some(0xFF);
        }

        match &self.mapper {
            Mapper::NoMbc => self.ram.get((address as usize) % self.ram.len()).copied(),
            Mapper::Mbc1(mbc) => {
                if !mbc.ram_enabled {
                    return Some(0xFF);
                }

                let bank = mbc.ram_bank(self.ram_bank_count());
                let offset = address as usize % RAM_BANK_SIZE;
                self.ram.get(bank * RAM_BANK_SIZE + offset).copied()
            }
        }
    }

    pub fn write_ram(&mut self, address: u16, value: u8) {
        if self.ram.is_empty() {
            return;
        }

        match &self.mapper {
            Mapper::NoMbc => {
                let offset = (address as usize) % self.ram.len();
                self.ram[offset] = value;
            }
            Mapper::Mbc1(mbc) => {
                if !mbc.ram_enabled {
                    return;
                }

                let bank = mbc.ram_bank(self.ram_bank_count());
                let offset = address as usize % RAM_BANK_SIZE;
                let index = bank * RAM_BANK_SIZE + offset;
                if let Some(byte) = self.ram.get_mut(index) {
                    *byte = value;
                }
            }
        }
    }

    fn rom_bank_count(&self) -> usize {
        self.rom.len() / ROM_BANK_SIZE
    }

    fn ram_bank_count(&self) -> usize {
        (self.ram.len() / RAM_BANK_SIZE).max(1)
    }
}

impl CartridgeHeader {
    fn parse(rom: &[u8]) -> Result<Self> {
        if rom.len() <= RAM_SIZE_OFFSET {
            return Err(EmulatorError::InvalidRom {
                reason: "ROM does not contain a complete header".to_string(),
            });
        }

        let cartridge_type = rom[CARTRIDGE_TYPE_OFFSET];
        let mapper_kind = mapper_kind(cartridge_type)?;
        let rom_size_code = rom[ROM_SIZE_OFFSET];
        let ram_size_code = rom[RAM_SIZE_OFFSET];
        let rom_size = rom_size(rom_size_code)?;
        let ram_size = ram_size(ram_size_code)?;
        let title_bytes = &rom[TITLE_START..TITLE_END_EXCLUSIVE];
        let title_end = title_bytes
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(title_bytes.len());
        let title = String::from_utf8_lossy(&title_bytes[..title_end])
            .trim_end()
            .to_string();

        Ok(Self {
            title,
            cartridge_type,
            rom_size_code,
            ram_size_code,
            mapper_kind,
            rom_size,
            ram_size,
        })
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

    pub fn mapper_kind(&self) -> MapperKind {
        self.mapper_kind
    }

    pub fn rom_size(&self) -> usize {
        self.rom_size
    }

    pub fn ram_size(&self) -> usize {
        self.ram_size
    }
}

impl Mbc1 {
    fn fixed_rom_bank(&self, rom_bank_count: usize) -> usize {
        if self.banking_mode == 0 {
            0
        } else {
            ((self.bank_high2 as usize) << 5) % rom_bank_count
        }
    }

    fn switchable_rom_bank(&self, rom_bank_count: usize) -> usize {
        let mut bank = ((self.bank_high2 as usize) << 5) | self.rom_bank_low5 as usize;
        if bank & 0x1F == 0 {
            bank += 1;
        }
        bank % rom_bank_count
    }

    fn ram_bank(&self, ram_bank_count: usize) -> usize {
        if self.banking_mode == 0 {
            0
        } else {
            self.bank_high2 as usize % ram_bank_count
        }
    }
}

fn mapper_kind(cartridge_type: u8) -> Result<MapperKind> {
    match cartridge_type {
        0x00 | 0x08 | 0x09 => Ok(MapperKind::NoMbc),
        0x01..=0x03 => Ok(MapperKind::Mbc1),
        _ => Err(EmulatorError::UnsupportedCartridge { cartridge_type }),
    }
}

fn rom_size(code: u8) -> Result<usize> {
    match code {
        0x00..=0x08 => Ok(MIN_ROM_SIZE << code),
        _ => Err(EmulatorError::InvalidRom {
            reason: format!("unsupported ROM size code: 0x{code:02X}"),
        }),
    }
}

fn ram_size(code: u8) -> Result<usize> {
    match code {
        0x00 => Ok(0),
        0x01 => Ok(2 * 1024),
        0x02 => Ok(8 * 1024),
        0x03 => Ok(32 * 1024),
        0x04 => Ok(128 * 1024),
        0x05 => Ok(64 * 1024),
        _ => Err(EmulatorError::InvalidRom {
            reason: format!("unsupported RAM size code: 0x{code:02X}"),
        }),
    }
}

#[cfg(test)]
pub fn synthetic_rom(title: &str, segments: &[(u16, &[u8])]) -> Vec<u8> {
    synthetic_rom_with_header(title, 0x00, 0x00, 0x00, segments)
}

#[cfg(test)]
pub fn synthetic_rom_with_header(
    title: &str,
    cartridge_type: u8,
    rom_size_code: u8,
    ram_size_code: u8,
    segments: &[(u16, &[u8])],
) -> Vec<u8> {
    let mut rom = vec![0; rom_size(rom_size_code).unwrap_or(MIN_ROM_SIZE)];
    for (start, bytes) in segments {
        let start = *start as usize;
        let end = (start + bytes.len()).min(rom.len());
        let len = end.saturating_sub(start);
        if len > 0 {
            rom[start..end].copy_from_slice(&bytes[..len]);
        }
    }

    let title_bytes = title.as_bytes();
    let title_len = title_bytes.len().min(TITLE_END_EXCLUSIVE - TITLE_START);
    rom[TITLE_START..TITLE_START + title_len].copy_from_slice(&title_bytes[..title_len]);
    rom[CARTRIDGE_TYPE_OFFSET] = cartridge_type;
    rom[ROM_SIZE_OFFSET] = rom_size_code;
    rom[RAM_SIZE_OFFSET] = ram_size_code;
    rom
}

#[cfg(test)]
mod tests {
    use super::*;

    fn banked_rom(cartridge_type: u8, rom_size_code: u8, ram_size_code: u8) -> Vec<u8> {
        let mut rom =
            synthetic_rom_with_header("BANKED", cartridge_type, rom_size_code, ram_size_code, &[]);
        for bank in 0..rom.len() / ROM_BANK_SIZE {
            rom[bank * ROM_BANK_SIZE] = bank as u8;
        }
        rom
    }

    #[test]
    fn parses_header_from_synthetic_rom() {
        let rom = synthetic_rom("TEST", &[(0, &[0x00])]);
        let cartridge = Cartridge::from_bytes(rom).expect("valid ROM");

        assert_eq!(cartridge.header().title(), "TEST");
        assert_eq!(cartridge.header().cartridge_type(), 0x00);
        assert_eq!(cartridge.header().rom_size_code(), 0x00);
        assert_eq!(cartridge.header().ram_size_code(), 0x00);
        assert_eq!(cartridge.header().mapper_kind(), MapperKind::NoMbc);
        assert_eq!(cartridge.header().rom_size(), MIN_ROM_SIZE);
        assert_eq!(cartridge.header().ram_size(), 0);
    }

    #[test]
    fn rejects_small_roms() {
        let err = Cartridge::from_bytes(vec![0; 16]).expect_err("small ROM should fail");

        assert!(matches!(err, EmulatorError::InvalidRom { .. }));
    }

    #[test]
    fn rejects_unsupported_cartridge_type() {
        let mut rom = synthetic_rom("UNSUPPORTED", &[(0, &[0x00])]);
        rom[CARTRIDGE_TYPE_OFFSET] = 0x19;

        assert_eq!(
            Cartridge::from_bytes(rom),
            Err(EmulatorError::UnsupportedCartridge {
                cartridge_type: 0x19
            })
        );
    }

    #[test]
    fn rejects_rom_shorter_than_declared_size() {
        let mut rom = synthetic_rom_with_header("SHORT", 0x00, 0x01, 0x00, &[]);
        rom.truncate(MIN_ROM_SIZE);

        assert!(matches!(
            Cartridge::from_bytes(rom),
            Err(EmulatorError::InvalidRom { .. })
        ));
    }

    #[test]
    fn synthetic_rom_places_program_at_start() {
        let rom = synthetic_rom("TEST", &[(0, &[0x3E, 0x12])]);
        let cartridge = Cartridge::from_bytes(rom).expect("valid ROM");

        assert_eq!(cartridge.read_rom(0), Some(0x3E));
        assert_eq!(cartridge.read_rom(1), Some(0x12));
    }

    #[test]
    fn no_mbc_reads_fixed_rom_and_ignores_rom_writes() {
        let rom = synthetic_rom("ROM", &[(0x0000, &[0x12]), (0x4000, &[0x34])]);
        let mut cartridge = Cartridge::from_bytes(rom).expect("valid ROM");

        assert_eq!(cartridge.read_rom(0x0000), Some(0x12));
        assert_eq!(cartridge.read_rom(0x4000), Some(0x34));

        cartridge.write_rom(0x2000, 0x02);
        assert_eq!(cartridge.read_rom(0x4000), Some(0x34));
    }

    #[test]
    fn mbc1_switches_rom_banks_and_never_selects_bank_zero_in_switchable_region() {
        let rom = banked_rom(0x01, 0x01, 0x00);
        let mut cartridge = Cartridge::from_bytes(rom).expect("valid MBC1 ROM");

        assert_eq!(cartridge.header().mapper_kind(), MapperKind::Mbc1);
        assert_eq!(cartridge.read_rom(0x0000), Some(0));
        assert_eq!(cartridge.read_rom(0x4000), Some(1));

        cartridge.write_rom(0x2000, 0x02);
        assert_eq!(cartridge.read_rom(0x4000), Some(2));

        cartridge.write_rom(0x2000, 0x00);
        assert_eq!(cartridge.read_rom(0x4000), Some(1));
    }

    #[test]
    fn mbc1_ram_enable_and_banking_mode_select_ram_banks() {
        let rom = banked_rom(0x03, 0x00, 0x03);
        let mut cartridge = Cartridge::from_bytes(rom).expect("valid MBC1 ROM");

        cartridge.write_ram(0x0000, 0x11);
        assert_eq!(cartridge.read_ram(0x0000), Some(0xFF));

        cartridge.write_rom(0x0000, 0x0A);
        cartridge.write_ram(0x0000, 0x11);
        assert_eq!(cartridge.read_ram(0x0000), Some(0x11));

        cartridge.write_rom(0x6000, 0x01);
        cartridge.write_rom(0x4000, 0x01);
        cartridge.write_ram(0x0000, 0x22);
        assert_eq!(cartridge.read_ram(0x0000), Some(0x22));

        cartridge.write_rom(0x4000, 0x00);
        assert_eq!(cartridge.read_ram(0x0000), Some(0x11));
    }
}
