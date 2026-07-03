use std::{fs, path::Path};

use crate::error::{EmulatorError, Result};

const TITLE_START: usize = 0x0134;
const TITLE_END_EXCLUSIVE: usize = 0x0144;
const CGB_FLAG_OFFSET: usize = 0x0143;
const CARTRIDGE_TYPE_OFFSET: usize = 0x0147;
const ROM_SIZE_OFFSET: usize = 0x0148;
const RAM_SIZE_OFFSET: usize = 0x0149;
const ROM_BANK_SIZE: usize = 16 * 1024;
const RAM_BANK_SIZE: usize = 8 * 1024;
const MIN_ROM_SIZE: usize = 2 * ROM_BANK_SIZE;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Cartridge {
    #[serde(skip)]
    rom: Vec<u8>,
    ram: Vec<u8>,
    header: CartridgeHeader,
    mapper: Mapper,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CartridgeHeader {
    title: String,
    cartridge_type: u8,
    cgb_flag: u8,
    rom_size_code: u8,
    ram_size_code: u8,
    mapper_kind: MapperKind,
    rom_size: usize,
    ram_size: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum MapperKind {
    NoMbc,
    Mbc1,
    Mbc3,
    Mbc5,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
enum Mapper {
    NoMbc,
    Mbc1(Mbc1),
    Mbc3(Mbc3),
    Mbc5(Mbc5),
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct Mbc1 {
    ram_enabled: bool,
    rom_bank_low5: u8,
    bank_high2: u8,
    banking_mode: u8,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct Mbc3 {
    ram_rtc_enabled: bool,
    has_rtc: bool,
    rom_bank: u8,
    ram_rtc_select: u8,
    latch_armed: bool,
    rtc: Rtc,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct Mbc5 {
    ram_enabled: bool,
    rom_bank: u16,
    ram_bank: u8,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct Rtc {
    cycles: u32,
    seconds: u8,
    minutes: u8,
    hours: u8,
    day_counter: u16,
    halted: bool,
    carry: bool,
    latched: RtcRegisters,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct RtcRegisters {
    seconds: u8,
    minutes: u8,
    hours: u8,
    day_low: u8,
    day_high: u8,
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
            MapperKind::Mbc3 => Mapper::Mbc3(Mbc3 {
                has_rtc: has_rtc(header.cartridge_type),
                rom_bank: 1,
                ..Mbc3::default()
            }),
            MapperKind::Mbc5 => Mapper::Mbc5(Mbc5 {
                rom_bank: 1,
                ..Mbc5::default()
            }),
        };

        Ok(Self {
            rom,
            ram,
            header,
            mapper,
        })
    }

    pub fn reload_rom(&mut self, rom: Vec<u8>) -> Result<()> {
        if rom.len() < self.header.rom_size {
            return Err(EmulatorError::InvalidRom {
                reason: format!(
                    "declared ROM size is {} bytes, got {}",
                    self.header.rom_size,
                    rom.len()
                ),
            });
        }
        self.rom = rom[..self.header.rom_size].to_vec();
        Ok(())
    }

    pub fn header(&self) -> &CartridgeHeader {
        &self.header
    }

    pub fn has_battery(&self) -> bool {
        has_battery(self.header.cartridge_type)
    }

    pub fn has_rtc(&self) -> bool {
        has_rtc(self.header.cartridge_type)
    }

    pub fn save_ram(&self) -> Option<&[u8]> {
        if self.has_battery() && !self.ram.is_empty() {
            Some(&self.ram)
        } else {
            None
        }
    }

    pub fn load_save_ram(&mut self, bytes: &[u8]) -> Result<()> {
        if self.ram.is_empty() {
            return if bytes.is_empty() {
                Ok(())
            } else {
                Err(EmulatorError::InvalidRom {
                    reason: format!(
                        "save RAM is {} bytes but cartridge has no external RAM",
                        bytes.len()
                    ),
                })
            };
        }

        if bytes.len() != self.ram.len() {
            return Err(EmulatorError::InvalidRom {
                reason: format!(
                    "save RAM size mismatch: expected {} bytes, got {}",
                    self.ram.len(),
                    bytes.len()
                ),
            });
        }

        self.ram.copy_from_slice(bytes);
        Ok(())
    }

    pub fn save_rtc(&self) -> Option<Vec<u8>> {
        match &self.mapper {
            Mapper::Mbc3(mbc) if self.has_battery() && mbc.has_rtc => Some(mbc.rtc.to_bytes()),
            _ => None,
        }
    }

    pub fn load_save_rtc(&mut self, bytes: &[u8]) -> Result<()> {
        match &mut self.mapper {
            Mapper::Mbc3(mbc) if mbc.has_rtc => mbc.rtc.load_bytes(bytes),
            _ if bytes.is_empty() => Ok(()),
            _ => Err(EmulatorError::InvalidRom {
                reason: format!(
                    "RTC save is {} bytes but cartridge has no battery-backed RTC",
                    bytes.len()
                ),
            }),
        }
    }

    pub fn advance_cycles(&mut self, cycles: u32) {
        if let Mapper::Mbc3(mbc) = &mut self.mapper {
            if mbc.has_rtc {
                mbc.rtc.advance_cycles(cycles);
            }
        }
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
            Mapper::Mbc3(mbc) => {
                let bank = if address < ROM_BANK_SIZE {
                    0
                } else {
                    mbc.rom_bank(self.rom_bank_count())
                };
                let offset = address % ROM_BANK_SIZE;
                self.rom.get(bank * ROM_BANK_SIZE + offset).copied()
            }
            Mapper::Mbc5(mbc) => {
                let bank = if address < ROM_BANK_SIZE {
                    0
                } else {
                    mbc.rom_bank(self.rom_bank_count())
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
            Mapper::Mbc3(mbc) => match address {
                0x0000..=0x1FFF => mbc.ram_rtc_enabled = value & 0x0F == 0x0A,
                0x2000..=0x3FFF => {
                    mbc.rom_bank = value & 0x7F;
                    if mbc.rom_bank == 0 {
                        mbc.rom_bank = 1;
                    }
                }
                0x4000..=0x5FFF => mbc.ram_rtc_select = value,
                0x6000..=0x7FFF => mbc.write_latch(value),
                _ => {}
            },
            Mapper::Mbc5(mbc) => match address {
                0x0000..=0x1FFF => mbc.ram_enabled = value & 0x0F == 0x0A,
                0x2000..=0x2FFF => mbc.rom_bank = (mbc.rom_bank & 0x100) | value as u16,
                0x3000..=0x3FFF => {
                    mbc.rom_bank = (mbc.rom_bank & 0x0FF) | (((value & 0x01) as u16) << 8)
                }
                0x4000..=0x5FFF => mbc.ram_bank = value & 0x0F,
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
            Mapper::Mbc3(mbc) => {
                if !mbc.ram_rtc_enabled {
                    return Some(0xFF);
                }

                match mbc.ram_rtc_select {
                    0x00..=0x03 => {
                        let bank = mbc.ram_bank(self.ram_bank_count());
                        let offset = address as usize % RAM_BANK_SIZE;
                        self.ram.get(bank * RAM_BANK_SIZE + offset).copied()
                    }
                    0x08..=0x0C if mbc.has_rtc => Some(mbc.rtc.read_latched(mbc.ram_rtc_select)),
                    _ => Some(0xFF),
                }
            }
            Mapper::Mbc5(mbc) => {
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
        let ram_bank_count = self.ram_bank_count();
        match &mut self.mapper {
            Mapper::NoMbc => {
                if self.ram.is_empty() {
                    return;
                }

                let offset = (address as usize) % self.ram.len();
                self.ram[offset] = value;
            }
            Mapper::Mbc1(mbc) => {
                if self.ram.is_empty() {
                    return;
                }

                if !mbc.ram_enabled {
                    return;
                }

                let bank = mbc.ram_bank(ram_bank_count);
                let offset = address as usize % RAM_BANK_SIZE;
                let index = bank * RAM_BANK_SIZE + offset;
                if let Some(byte) = self.ram.get_mut(index) {
                    *byte = value;
                }
            }
            Mapper::Mbc3(mbc) => {
                if !mbc.ram_rtc_enabled {
                    return;
                }

                match mbc.ram_rtc_select {
                    0x00..=0x03 => {
                        if self.ram.is_empty() {
                            return;
                        }

                        let bank = mbc.ram_bank(ram_bank_count);
                        let offset = address as usize % RAM_BANK_SIZE;
                        let index = bank * RAM_BANK_SIZE + offset;
                        if let Some(byte) = self.ram.get_mut(index) {
                            *byte = value;
                        }
                    }
                    0x08..=0x0C if mbc.has_rtc => mbc.rtc.write_register(mbc.ram_rtc_select, value),
                    _ => {}
                }
            }
            Mapper::Mbc5(mbc) => {
                if self.ram.is_empty() || !mbc.ram_enabled {
                    return;
                }

                let bank = mbc.ram_bank(ram_bank_count);
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
        let cgb_flag = rom[CGB_FLAG_OFFSET];
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
            cgb_flag,
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

    pub fn cgb_flag(&self) -> u8 {
        self.cgb_flag
    }

    pub fn supports_cgb(&self) -> bool {
        matches!(self.cgb_flag, 0x80 | 0xC0)
    }

    pub fn requires_cgb(&self) -> bool {
        self.cgb_flag == 0xC0
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

impl Mbc3 {
    fn rom_bank(&self, rom_bank_count: usize) -> usize {
        let bank = if self.rom_bank == 0 { 1 } else { self.rom_bank };
        bank as usize % rom_bank_count
    }

    fn ram_bank(&self, ram_bank_count: usize) -> usize {
        (self.ram_rtc_select as usize & 0x03) % ram_bank_count
    }

    fn write_latch(&mut self, value: u8) {
        if value == 0 {
            self.latch_armed = true;
        } else if value == 1 && self.latch_armed {
            self.rtc.latch();
            self.latch_armed = false;
        } else {
            self.latch_armed = false;
        }
    }
}

impl Mbc5 {
    fn rom_bank(&self, rom_bank_count: usize) -> usize {
        self.rom_bank as usize % rom_bank_count
    }

    fn ram_bank(&self, ram_bank_count: usize) -> usize {
        (self.ram_bank as usize & 0x0F) % ram_bank_count
    }
}

impl Rtc {
    const CYCLES_PER_SECOND: u32 = 4_194_304;
    const SAVE_LEN: usize = 10;

    fn advance_cycles(&mut self, cycles: u32) {
        if self.halted {
            return;
        }

        self.cycles += cycles;
        while self.cycles >= Self::CYCLES_PER_SECOND {
            self.cycles -= Self::CYCLES_PER_SECOND;
            self.tick_second();
        }
    }

    fn tick_second(&mut self) {
        self.seconds += 1;
        if self.seconds < 60 {
            return;
        }

        self.seconds = 0;
        self.minutes += 1;
        if self.minutes < 60 {
            return;
        }

        self.minutes = 0;
        self.hours += 1;
        if self.hours < 24 {
            return;
        }

        self.hours = 0;
        self.day_counter += 1;
        if self.day_counter > 0x01FF {
            self.day_counter &= 0x01FF;
            self.carry = true;
        }
    }

    fn latch(&mut self) {
        self.latched = RtcRegisters {
            seconds: self.seconds,
            minutes: self.minutes,
            hours: self.hours,
            day_low: self.day_counter as u8,
            day_high: ((self.day_counter >> 8) as u8 & 0x01)
                | (u8::from(self.halted) << 6)
                | (u8::from(self.carry) << 7),
        };
    }

    fn read_latched(&self, register: u8) -> u8 {
        match register {
            0x08 => self.latched.seconds,
            0x09 => self.latched.minutes,
            0x0A => self.latched.hours,
            0x0B => self.latched.day_low,
            0x0C => self.latched.day_high,
            _ => 0xFF,
        }
    }

    fn write_register(&mut self, register: u8, value: u8) {
        match register {
            0x08 => self.seconds = value % 60,
            0x09 => self.minutes = value % 60,
            0x0A => self.hours = value % 24,
            0x0B => self.day_counter = (self.day_counter & 0x0100) | value as u16,
            0x0C => {
                self.day_counter = (self.day_counter & 0x00FF) | (((value & 0x01) as u16) << 8);
                self.halted = value & 0x40 != 0;
                self.carry = value & 0x80 != 0;
            }
            _ => {}
        }

        self.latch();
    }

    fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(Self::SAVE_LEN);
        bytes.extend_from_slice(&self.cycles.to_le_bytes());
        bytes.push(self.seconds);
        bytes.push(self.minutes);
        bytes.push(self.hours);
        bytes.extend_from_slice(&self.day_counter.to_le_bytes());
        bytes.push(u8::from(self.halted) | (u8::from(self.carry) << 1));
        bytes
    }

    fn load_bytes(&mut self, bytes: &[u8]) -> Result<()> {
        if bytes.len() != Self::SAVE_LEN {
            return Err(EmulatorError::InvalidRom {
                reason: format!(
                    "RTC save size mismatch: expected {} bytes, got {}",
                    Self::SAVE_LEN,
                    bytes.len()
                ),
            });
        }

        self.cycles = u32::from_le_bytes(bytes[0..4].try_into().expect("four cycle bytes"));
        self.seconds = bytes[4] % 60;
        self.minutes = bytes[5] % 60;
        self.hours = bytes[6] % 24;
        self.day_counter =
            u16::from_le_bytes(bytes[7..9].try_into().expect("two day bytes")) & 0x01FF;
        self.halted = bytes[9] & 0x01 != 0;
        self.carry = bytes[9] & 0x02 != 0;
        self.latch();
        Ok(())
    }
}

fn mapper_kind(cartridge_type: u8) -> Result<MapperKind> {
    match cartridge_type {
        0x00 | 0x08 | 0x09 => Ok(MapperKind::NoMbc),
        0x01..=0x03 => Ok(MapperKind::Mbc1),
        0x0F..=0x13 => Ok(MapperKind::Mbc3),
        0x19..=0x1E => Ok(MapperKind::Mbc5),
        _ => Err(EmulatorError::UnsupportedCartridge { cartridge_type }),
    }
}

fn has_battery(cartridge_type: u8) -> bool {
    matches!(
        cartridge_type,
        0x03 | 0x06 | 0x09 | 0x0F | 0x10 | 0x13 | 0x1B | 0x1E
    )
}

fn has_rtc(cartridge_type: u8) -> bool {
    matches!(cartridge_type, 0x0F | 0x10)
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
    fn cgb_flag_distinguishes_dmg_compatible_from_cgb_only() {
        let mut dmg_only = synthetic_rom("DMG", &[(0, &[0x00])]);
        dmg_only[CGB_FLAG_OFFSET] = 0x00;
        let cart = Cartridge::from_bytes(dmg_only).expect("valid ROM");
        assert!(!cart.header().supports_cgb());
        assert!(!cart.header().requires_cgb());

        let mut cgb_compatible = synthetic_rom("COMPAT", &[(0, &[0x00])]);
        cgb_compatible[CGB_FLAG_OFFSET] = 0x80;
        let cart = Cartridge::from_bytes(cgb_compatible).expect("valid ROM");
        assert!(cart.header().supports_cgb());
        assert!(!cart.header().requires_cgb());

        let mut cgb_only = synthetic_rom("CGBONLY", &[(0, &[0x00])]);
        cgb_only[CGB_FLAG_OFFSET] = 0xC0;
        let cart = Cartridge::from_bytes(cgb_only).expect("valid ROM");
        assert!(cart.header().supports_cgb());
        assert!(cart.header().requires_cgb());
    }

    #[test]
    fn rejects_small_roms() {
        let err = Cartridge::from_bytes(vec![0; 16]).expect_err("small ROM should fail");

        assert!(matches!(err, EmulatorError::InvalidRom { .. }));
    }

    #[test]
    fn rejects_unsupported_cartridge_type() {
        let mut rom = synthetic_rom("UNSUPPORTED", &[(0, &[0x00])]);
        rom[CARTRIDGE_TYPE_OFFSET] = 0xFC;

        assert_eq!(
            Cartridge::from_bytes(rom),
            Err(EmulatorError::UnsupportedCartridge {
                cartridge_type: 0xFC
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

    #[test]
    fn mbc3_switches_rom_and_ram_banks() {
        let rom = banked_rom(0x13, 0x02, 0x03);
        let mut cartridge = Cartridge::from_bytes(rom).expect("valid MBC3 ROM");

        assert_eq!(cartridge.header().mapper_kind(), MapperKind::Mbc3);
        assert_eq!(cartridge.read_rom(0x0000), Some(0));
        assert_eq!(cartridge.read_rom(0x4000), Some(1));

        cartridge.write_rom(0x2000, 0x03);
        assert_eq!(cartridge.read_rom(0x4000), Some(3));

        cartridge.write_rom(0x2000, 0x00);
        assert_eq!(cartridge.read_rom(0x4000), Some(1));

        cartridge.write_ram(0x0000, 0x11);
        assert_eq!(cartridge.read_ram(0x0000), Some(0xFF));

        cartridge.write_rom(0x0000, 0x0A);
        cartridge.write_rom(0x4000, 0x00);
        cartridge.write_ram(0x0000, 0x11);
        cartridge.write_rom(0x4000, 0x02);
        cartridge.write_ram(0x0000, 0x22);

        cartridge.write_rom(0x4000, 0x00);
        assert_eq!(cartridge.read_ram(0x0000), Some(0x11));
        cartridge.write_rom(0x4000, 0x02);
        assert_eq!(cartridge.read_ram(0x0000), Some(0x22));
    }

    #[test]
    fn mbc3_latches_and_halts_rtc_registers() {
        let rom = banked_rom(0x10, 0x00, 0x03);
        let mut cartridge = Cartridge::from_bytes(rom).expect("valid MBC3 RTC ROM");

        assert!(cartridge.has_rtc());
        cartridge.write_rom(0x0000, 0x0A);
        cartridge.advance_cycles(Rtc::CYCLES_PER_SECOND * 2);
        cartridge.write_rom(0x6000, 0x00);
        cartridge.write_rom(0x6000, 0x01);
        cartridge.write_rom(0x4000, 0x08);
        assert_eq!(cartridge.read_ram(0x0000), Some(2));

        cartridge.write_ram(0x0000, 58);
        cartridge.advance_cycles(Rtc::CYCLES_PER_SECOND * 2);
        cartridge.write_rom(0x6000, 0x00);
        cartridge.write_rom(0x6000, 0x01);
        assert_eq!(cartridge.read_ram(0x0000), Some(0));

        cartridge.write_rom(0x4000, 0x09);
        assert_eq!(cartridge.read_ram(0x0000), Some(1));

        cartridge.write_rom(0x4000, 0x0C);
        cartridge.write_ram(0x0000, 0x40);
        cartridge.advance_cycles(Rtc::CYCLES_PER_SECOND);
        cartridge.write_rom(0x6000, 0x00);
        cartridge.write_rom(0x6000, 0x01);
        cartridge.write_rom(0x4000, 0x08);
        assert_eq!(cartridge.read_ram(0x0000), Some(0));
    }

    #[test]
    fn mbc3_without_timer_does_not_expose_rtc_registers() {
        let rom = banked_rom(0x13, 0x00, 0x03);
        let mut cartridge = Cartridge::from_bytes(rom).expect("valid MBC3 ROM");

        assert!(!cartridge.has_rtc());
        assert_eq!(cartridge.save_rtc(), None);

        cartridge.write_rom(0x0000, 0x0A);
        cartridge.write_rom(0x4000, 0x08);
        cartridge.write_ram(0x0000, 12);
        cartridge.write_rom(0x6000, 0x00);
        cartridge.write_rom(0x6000, 0x01);
        assert_eq!(cartridge.read_ram(0x0000), Some(0xFF));
    }

    #[test]
    fn mbc5_uses_nine_bit_rom_bank_and_four_bit_ram_bank() {
        let mut rom = banked_rom(0x1B, 0x08, 0x04);
        rom[0x0123] = 0x00;
        rom[ROM_BANK_SIZE + 0x0123] = 0x11;
        rom[0x101 * ROM_BANK_SIZE + 0x0123] = 0xA5;
        let mut cartridge = Cartridge::from_bytes(rom).expect("valid MBC5 ROM");

        assert_eq!(cartridge.header().mapper_kind(), MapperKind::Mbc5);
        assert_eq!(cartridge.read_rom(0x0123), Some(0x00));
        assert_eq!(cartridge.read_rom(0x4123), Some(0x11));

        cartridge.write_rom(0x2000, 0x01);
        cartridge.write_rom(0x3000, 0x01);
        assert_eq!(cartridge.read_rom(0x4123), Some(0xA5));

        cartridge.write_ram(0x0000, 0x11);
        assert_eq!(cartridge.read_ram(0x0000), Some(0xFF));

        cartridge.write_rom(0x0000, 0x0A);
        cartridge.write_rom(0x4000, 0x00);
        cartridge.write_ram(0x0000, 0x11);
        cartridge.write_rom(0x4000, 0x0F);
        cartridge.write_ram(0x0000, 0x22);

        cartridge.write_rom(0x4000, 0x00);
        assert_eq!(cartridge.read_ram(0x0000), Some(0x11));
        cartridge.write_rom(0x4000, 0x0F);
        assert_eq!(cartridge.read_ram(0x0000), Some(0x22));
    }

    #[test]
    fn battery_save_ram_round_trips_exact_external_ram() {
        let rom = banked_rom(0x03, 0x00, 0x03);
        let mut cartridge = Cartridge::from_bytes(rom).expect("valid battery ROM");

        assert!(cartridge.has_battery());
        assert_eq!(cartridge.save_ram().map(|ram| ram.len()), Some(32 * 1024));

        let save = vec![0x5A; 32 * 1024];
        cartridge.load_save_ram(&save).expect("load save RAM");
        assert_eq!(cartridge.save_ram(), Some(save.as_slice()));

        assert!(matches!(
            cartridge.load_save_ram(&save[..save.len() - 1]),
            Err(EmulatorError::InvalidRom { .. })
        ));
    }

    #[test]
    fn battery_rtc_save_round_trips_timer_state() {
        let rom = banked_rom(0x10, 0x00, 0x03);
        let mut cartridge = Cartridge::from_bytes(rom).expect("valid RTC ROM");

        cartridge.advance_cycles(Rtc::CYCLES_PER_SECOND * 7);
        let save = cartridge.save_rtc().expect("RTC save data");

        let mut restored =
            Cartridge::from_bytes(banked_rom(0x10, 0x00, 0x03)).expect("valid RTC ROM");
        restored.load_save_rtc(&save).expect("load RTC save");
        restored.write_rom(0x0000, 0x0A);
        restored.write_rom(0x6000, 0x00);
        restored.write_rom(0x6000, 0x01);
        restored.write_rom(0x4000, 0x08);
        assert_eq!(restored.read_ram(0x0000), Some(7));

        assert!(matches!(
            restored.load_save_rtc(&save[..save.len() - 1]),
            Err(EmulatorError::InvalidRom { .. })
        ));
    }

    #[test]
    fn non_battery_cartridges_do_not_export_save_ram() {
        let rom = banked_rom(0x02, 0x00, 0x03);
        let cartridge = Cartridge::from_bytes(rom).expect("valid non-battery ROM");

        assert!(!cartridge.has_battery());
        assert_eq!(cartridge.save_ram(), None);
    }
}
