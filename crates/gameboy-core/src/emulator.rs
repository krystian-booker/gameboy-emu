use crate::{
    bus::Bus,
    cartridge::Cartridge,
    cpu::{Cpu, Registers},
    error::Result,
};

pub type CycleCount = u8;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Emulator {
    cpu: Cpu,
    bus: Bus,
}

impl Emulator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn load_rom(&mut self, bytes: Vec<u8>) -> Result<()> {
        let cartridge = Cartridge::from_bytes(bytes)?;
        self.bus.insert_cartridge(cartridge);
        self.cpu.registers_mut().pc = 0;
        Ok(())
    }

    pub fn step(&mut self) -> Result<CycleCount> {
        self.cpu.step(&mut self.bus)
    }

    pub fn registers(&self) -> &Registers {
        self.cpu.registers()
    }

    pub fn bus(&self) -> &Bus {
        &self.bus
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{cartridge::synthetic_rom, error::EmulatorError};

    #[test]
    fn loads_rom_and_steps_nop() {
        let mut emulator = Emulator::new();

        emulator
            .load_rom(synthetic_rom("TEST", &[0x00]))
            .expect("load ROM");

        assert_eq!(emulator.step(), Ok(4));
        assert_eq!(emulator.registers().pc, 1);
    }

    #[test]
    fn step_without_rom_is_memory_error() {
        let mut emulator = Emulator::new();

        assert_eq!(
            emulator.step(),
            Err(EmulatorError::InvalidMemoryAccess { address: 0 })
        );
    }
}
