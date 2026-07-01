use crate::{
    bus::Bus,
    emulator::CycleCount,
    error::{EmulatorError, Result},
};

const FLAG_ZERO: u8 = 0b1000_0000;
const FLAG_SUBTRACT: u8 = 0b0100_0000;
const FLAG_HALF_CARRY: u8 = 0b0010_0000;
const FLAG_CARRY: u8 = 0b0001_0000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flag {
    Zero,
    Subtract,
    HalfCarry,
    Carry,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Registers {
    pub a: u8,
    pub f: u8,
    pub b: u8,
    pub c: u8,
    pub d: u8,
    pub e: u8,
    pub h: u8,
    pub l: u8,
    pub pc: u16,
    pub sp: u16,
}

impl Registers {
    pub fn af(&self) -> u16 {
        u16::from_be_bytes([self.a, self.f & 0xF0])
    }

    pub fn set_af(&mut self, value: u16) {
        let [a, f] = value.to_be_bytes();
        self.a = a;
        self.f = f & 0xF0;
    }

    pub fn bc(&self) -> u16 {
        u16::from_be_bytes([self.b, self.c])
    }

    pub fn set_bc(&mut self, value: u16) {
        let [b, c] = value.to_be_bytes();
        self.b = b;
        self.c = c;
    }

    pub fn de(&self) -> u16 {
        u16::from_be_bytes([self.d, self.e])
    }

    pub fn set_de(&mut self, value: u16) {
        let [d, e] = value.to_be_bytes();
        self.d = d;
        self.e = e;
    }

    pub fn hl(&self) -> u16 {
        u16::from_be_bytes([self.h, self.l])
    }

    pub fn set_hl(&mut self, value: u16) {
        let [h, l] = value.to_be_bytes();
        self.h = h;
        self.l = l;
    }

    pub fn flag(&self, flag: Flag) -> bool {
        self.f & flag.mask() != 0
    }

    pub fn set_flag(&mut self, flag: Flag, set: bool) {
        if set {
            self.f |= flag.mask();
        } else {
            self.f &= !flag.mask();
        }
        self.f &= 0xF0;
    }
}

impl Flag {
    fn mask(self) -> u8 {
        match self {
            Self::Zero => FLAG_ZERO,
            Self::Subtract => FLAG_SUBTRACT,
            Self::HalfCarry => FLAG_HALF_CARRY,
            Self::Carry => FLAG_CARRY,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Cpu {
    registers: Registers,
}

impl Cpu {
    pub fn registers(&self) -> &Registers {
        &self.registers
    }

    pub fn registers_mut(&mut self) -> &mut Registers {
        &mut self.registers
    }

    pub fn step(&mut self, bus: &mut Bus) -> Result<CycleCount> {
        let pc = self.registers.pc;
        let opcode = self.fetch_byte(bus)?;

        match opcode {
            0x00 => Ok(4),
            _ => Err(EmulatorError::UnimplementedOpcode { opcode, pc }),
        }
    }

    fn fetch_byte(&mut self, bus: &Bus) -> Result<u8> {
        let byte = bus.read_byte(self.registers.pc)?;
        self.registers.pc = self.registers.pc.wrapping_add(1);
        Ok(byte)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cartridge::{synthetic_rom, Cartridge};

    fn bus_with_program(program: &[u8]) -> Bus {
        let mut bus = Bus::default();
        bus.insert_cartridge(Cartridge::from_bytes(synthetic_rom("TEST", program)).unwrap());
        bus
    }

    #[test]
    fn register_pairs_read_and_write_big_endian_values() {
        let mut registers = Registers::default();

        registers.set_af(0x12F3);
        registers.set_bc(0x3456);
        registers.set_de(0x789A);
        registers.set_hl(0xBCDE);

        assert_eq!(registers.af(), 0x12F0);
        assert_eq!(registers.bc(), 0x3456);
        assert_eq!(registers.de(), 0x789A);
        assert_eq!(registers.hl(), 0xBCDE);
    }

    #[test]
    fn flags_can_be_set_cleared_and_read() {
        let mut registers = Registers::default();

        registers.set_flag(Flag::Zero, true);
        registers.set_flag(Flag::Carry, true);
        registers.set_flag(Flag::Zero, false);

        assert!(!registers.flag(Flag::Zero));
        assert!(registers.flag(Flag::Carry));
        assert_eq!(registers.f & 0x0F, 0);
    }

    #[test]
    fn nop_fetch_increments_pc() {
        let mut cpu = Cpu::default();
        let mut bus = bus_with_program(&[0x00]);

        assert_eq!(cpu.step(&mut bus), Ok(4));
        assert_eq!(cpu.registers().pc, 1);
    }

    #[test]
    fn unimplemented_opcode_reports_original_pc() {
        let mut cpu = Cpu::default();
        let mut bus = bus_with_program(&[0xFF]);

        assert_eq!(
            cpu.step(&mut bus),
            Err(EmulatorError::UnimplementedOpcode {
                opcode: 0xFF,
                pc: 0
            })
        );
        assert_eq!(cpu.registers().pc, 1);
    }
}
