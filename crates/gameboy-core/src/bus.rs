use crate::{
    cartridge::Cartridge,
    error::{EmulatorError, Result},
    memory::MemoryRegion,
};

const VRAM_START: u16 = 0x8000;
const VRAM_END: u16 = 0x9FFF;
const WRAM_START: u16 = 0xC000;
const WRAM_END: u16 = 0xDFFF;
const HRAM_START: u16 = 0xFF80;
const HRAM_END: u16 = 0xFFFE;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Bus {
    cartridge: Option<Cartridge>,
    vram: MemoryRegion<0x2000>,
    wram: MemoryRegion<0x2000>,
    hram: MemoryRegion<0x7F>,
}

impl Bus {
    pub fn insert_cartridge(&mut self, cartridge: Cartridge) {
        self.cartridge = Some(cartridge);
    }

    pub fn cartridge(&self) -> Option<&Cartridge> {
        self.cartridge.as_ref()
    }

    pub fn read_byte(&self, address: u16) -> Result<u8> {
        match address {
            0x0000..=0x7FFF => self
                .cartridge
                .as_ref()
                .and_then(|cart| cart.read_rom(address))
                .ok_or(EmulatorError::InvalidMemoryAccess { address }),
            VRAM_START..=VRAM_END => self
                .vram
                .read((address - VRAM_START) as usize)
                .ok_or(EmulatorError::InvalidMemoryAccess { address }),
            WRAM_START..=WRAM_END => self
                .wram
                .read((address - WRAM_START) as usize)
                .ok_or(EmulatorError::InvalidMemoryAccess { address }),
            HRAM_START..=HRAM_END => self
                .hram
                .read((address - HRAM_START) as usize)
                .ok_or(EmulatorError::InvalidMemoryAccess { address }),
            _ => Err(EmulatorError::InvalidMemoryAccess { address }),
        }
    }

    pub fn write_byte(&mut self, address: u16, value: u8) -> Result<()> {
        let wrote = match address {
            VRAM_START..=VRAM_END => self.vram.write((address - VRAM_START) as usize, value),
            WRAM_START..=WRAM_END => self.wram.write((address - WRAM_START) as usize, value),
            HRAM_START..=HRAM_END => self.hram.write((address - HRAM_START) as usize, value),
            _ => false,
        };

        if wrote {
            Ok(())
        } else {
            Err(EmulatorError::InvalidMemoryAccess { address })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cartridge::synthetic_rom;

    #[test]
    fn reads_from_cartridge_rom() {
        let mut bus = Bus::default();
        let cartridge = Cartridge::from_bytes(synthetic_rom("TEST", &[0x42])).expect("valid ROM");
        bus.insert_cartridge(cartridge);

        assert_eq!(bus.read_byte(0x0000), Ok(0x42));
    }

    #[test]
    fn reads_and_writes_work_ram() {
        let mut bus = Bus::default();

        bus.write_byte(0xC123, 0x77).expect("write");

        assert_eq!(bus.read_byte(0xC123), Ok(0x77));
    }

    #[test]
    fn rejects_rom_writes() {
        let mut bus = Bus::default();

        assert_eq!(
            bus.write_byte(0x0000, 0x12),
            Err(EmulatorError::InvalidMemoryAccess { address: 0x0000 })
        );
    }
}
