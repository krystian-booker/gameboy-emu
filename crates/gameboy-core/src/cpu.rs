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
pub enum CpuMode {
    #[default]
    Running,
    Halted,
    Stopped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Register8 {
    B,
    C,
    D,
    E,
    H,
    L,
    AddressHl,
    A,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Register16 {
    BC,
    DE,
    HL,
    SP,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AluOperation {
    Add,
    Adc,
    Sub,
    Sbc,
    And,
    Xor,
    Or,
    Cp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JumpCondition {
    NotZero,
    Zero,
    NotCarry,
    Carry,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RotateOperation {
    Rlc,
    Rrc,
    Rl,
    Rr,
    Sla,
    Sra,
    Swap,
    Srl,
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

    fn read16(&self, register: Register16) -> u16 {
        match register {
            Register16::BC => self.bc(),
            Register16::DE => self.de(),
            Register16::HL => self.hl(),
            Register16::SP => self.sp,
        }
    }

    fn write16(&mut self, register: Register16, value: u16) {
        match register {
            Register16::BC => self.set_bc(value),
            Register16::DE => self.set_de(value),
            Register16::HL => self.set_hl(value),
            Register16::SP => self.sp = value,
        }
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
    mode: CpuMode,
    interrupt_master_enabled: bool,
    ime_enable_delay: Option<u8>,
    elapsed_cycles: CycleCount,
    halt_bug: bool,
}

impl Cpu {
    pub fn new_without_boot_rom() -> Self {
        let mut registers = Registers::default();
        registers.set_af(0x01B0);
        registers.set_bc(0x0013);
        registers.set_de(0x00D8);
        registers.set_hl(0x014D);
        registers.pc = 0x0100;
        registers.sp = 0xFFFE;

        Self {
            registers,
            mode: CpuMode::Running,
            interrupt_master_enabled: false,
            ime_enable_delay: None,
            elapsed_cycles: 0,
            halt_bug: false,
        }
    }

    pub fn new_cgb_without_boot_rom() -> Self {
        let mut cpu = Self::new_without_boot_rom();
        cpu.registers.a = 0x11;
        cpu
    }

    pub fn registers(&self) -> &Registers {
        &self.registers
    }

    pub fn registers_mut(&mut self) -> &mut Registers {
        &mut self.registers
    }

    pub fn mode(&self) -> CpuMode {
        self.mode
    }

    pub fn interrupt_master_enabled(&self) -> bool {
        self.interrupt_master_enabled
    }

    #[cfg(test)]
    pub(crate) fn set_interrupt_master_enabled(&mut self, enabled: bool) {
        self.interrupt_master_enabled = enabled;
        if !enabled {
            self.ime_enable_delay = None;
        }
    }

    pub fn step(&mut self, bus: &mut Bus) -> Result<CycleCount> {
        self.elapsed_cycles = 0;

        if bus.pending_interrupts() != 0 {
            self.mode = CpuMode::Running;

            if self.interrupt_master_enabled {
                let cycles = self.service_interrupt(bus)?;
                self.advance_remaining_cycles(bus, cycles);
                return Ok(cycles);
            }
        }

        if self.mode == CpuMode::Halted {
            self.idle_cycle(bus);
            self.finish_instruction();
            return Ok(4);
        }

        let pc = self.registers.pc;
        let opcode = self.fetch_byte(bus)?;

        let cycles = if (0x40..=0x7F).contains(&opcode) && opcode != 0x76 {
            self.load_register_from_register(bus, opcode)?
        } else if (0x80..=0xBF).contains(&opcode) {
            self.execute_register_alu(bus, opcode)?
        } else {
            match opcode {
                0x00 => Ok(4),
                0x01 => {
                    let value = self.fetch_word(bus)?;
                    self.registers.set_bc(value);
                    Ok(12)
                }
                0x02 => {
                    self.write_byte(bus, self.registers.bc(), self.registers.a)?;
                    Ok(8)
                }
                0x03 => self.increment_register16(Register16::BC),
                0x04 => self.increment_register(bus, Register8::B),
                0x05 => self.decrement_register(bus, Register8::B),
                0x06 => {
                    self.registers.b = self.fetch_byte(bus)?;
                    Ok(8)
                }
                0x07 => {
                    self.rotate_accumulator(RotateOperation::Rlc);
                    Ok(4)
                }
                0x08 => {
                    let address = self.fetch_word(bus)?;
                    let [low, high] = self.registers.sp.to_le_bytes();
                    self.write_byte(bus, address, low)?;
                    self.write_byte(bus, address.wrapping_add(1), high)?;
                    Ok(20)
                }
                0x0A => {
                    self.registers.a = self.read_byte(bus, self.registers.bc())?;
                    Ok(8)
                }
                0x0B => self.decrement_register16(Register16::BC),
                0x0C => self.increment_register(bus, Register8::C),
                0x0D => self.decrement_register(bus, Register8::C),
                0x0E => {
                    self.registers.c = self.fetch_byte(bus)?;
                    Ok(8)
                }
                0x0F => {
                    self.rotate_accumulator(RotateOperation::Rrc);
                    Ok(4)
                }
                0x10 => {
                    let _ = self.fetch_byte(bus)?;
                    if !bus.stop() {
                        self.mode = CpuMode::Stopped;
                    }
                    Ok(4)
                }
                0x09 => self.add_hl(self.registers.bc()),
                0x11 => {
                    let value = self.fetch_word(bus)?;
                    self.registers.set_de(value);
                    Ok(12)
                }
                0x12 => {
                    self.write_byte(bus, self.registers.de(), self.registers.a)?;
                    Ok(8)
                }
                0x13 => self.increment_register16(Register16::DE),
                0x14 => self.increment_register(bus, Register8::D),
                0x15 => self.decrement_register(bus, Register8::D),
                0x16 => {
                    self.registers.d = self.fetch_byte(bus)?;
                    Ok(8)
                }
                0x17 => {
                    self.rotate_accumulator(RotateOperation::Rl);
                    Ok(4)
                }
                0x18 => {
                    self.jump_relative(bus)?;
                    Ok(12)
                }
                0x1A => {
                    self.registers.a = self.read_byte(bus, self.registers.de())?;
                    Ok(8)
                }
                0x1B => self.decrement_register16(Register16::DE),
                0x1C => self.increment_register(bus, Register8::E),
                0x1D => self.decrement_register(bus, Register8::E),
                0x1E => {
                    self.registers.e = self.fetch_byte(bus)?;
                    Ok(8)
                }
                0x1F => {
                    self.rotate_accumulator(RotateOperation::Rr);
                    Ok(4)
                }
                0x19 => self.add_hl(self.registers.de()),
                0x20 => self.jump_relative_if(bus, JumpCondition::NotZero),
                0x21 => {
                    let value = self.fetch_word(bus)?;
                    self.registers.set_hl(value);
                    Ok(12)
                }
                0x22 => {
                    let address = self.registers.hl();
                    self.write_byte(bus, address, self.registers.a)?;
                    self.registers.set_hl(address.wrapping_add(1));
                    Ok(8)
                }
                0x23 => self.increment_register16(Register16::HL),
                0x24 => self.increment_register(bus, Register8::H),
                0x25 => self.decrement_register(bus, Register8::H),
                0x26 => {
                    self.registers.h = self.fetch_byte(bus)?;
                    Ok(8)
                }
                0x27 => {
                    self.decimal_adjust_accumulator();
                    Ok(4)
                }
                0x28 => self.jump_relative_if(bus, JumpCondition::Zero),
                0x2A => {
                    let address = self.registers.hl();
                    self.registers.a = self.read_byte(bus, address)?;
                    self.registers.set_hl(address.wrapping_add(1));
                    Ok(8)
                }
                0x2B => self.decrement_register16(Register16::HL),
                0x2C => self.increment_register(bus, Register8::L),
                0x2D => self.decrement_register(bus, Register8::L),
                0x2E => {
                    self.registers.l = self.fetch_byte(bus)?;
                    Ok(8)
                }
                0x2F => {
                    self.registers.a = !self.registers.a;
                    self.registers.set_flag(Flag::Subtract, true);
                    self.registers.set_flag(Flag::HalfCarry, true);
                    Ok(4)
                }
                0x29 => self.add_hl(self.registers.hl()),
                0x30 => self.jump_relative_if(bus, JumpCondition::NotCarry),
                0x31 => {
                    self.registers.sp = self.fetch_word(bus)?;
                    Ok(12)
                }
                0x32 => {
                    let address = self.registers.hl();
                    self.write_byte(bus, address, self.registers.a)?;
                    self.registers.set_hl(address.wrapping_sub(1));
                    Ok(8)
                }
                0x33 => self.increment_register16(Register16::SP),
                0x34 => self.increment_register(bus, Register8::AddressHl),
                0x35 => self.decrement_register(bus, Register8::AddressHl),
                0x36 => {
                    let value = self.fetch_byte(bus)?;
                    self.write_byte(bus, self.registers.hl(), value)?;
                    Ok(12)
                }
                0x37 => {
                    self.registers.set_flag(Flag::Subtract, false);
                    self.registers.set_flag(Flag::HalfCarry, false);
                    self.registers.set_flag(Flag::Carry, true);
                    Ok(4)
                }
                0x38 => self.jump_relative_if(bus, JumpCondition::Carry),
                0x3A => {
                    let address = self.registers.hl();
                    self.registers.a = self.read_byte(bus, address)?;
                    self.registers.set_hl(address.wrapping_sub(1));
                    Ok(8)
                }
                0x3B => self.decrement_register16(Register16::SP),
                0x3C => self.increment_register(bus, Register8::A),
                0x3D => self.decrement_register(bus, Register8::A),
                0x3E => {
                    self.registers.a = self.fetch_byte(bus)?;
                    Ok(8)
                }
                0x3F => {
                    let carry = self.registers.flag(Flag::Carry);
                    self.registers.set_flag(Flag::Subtract, false);
                    self.registers.set_flag(Flag::HalfCarry, false);
                    self.registers.set_flag(Flag::Carry, !carry);
                    Ok(4)
                }
                0x39 => self.add_hl(self.registers.sp),
                0x76 => {
                    if !self.interrupt_master_enabled && bus.pending_interrupts() != 0 {
                        self.halt_bug = true;
                    } else {
                        self.mode = CpuMode::Halted;
                    }
                    Ok(4)
                }
                0x7E => {
                    self.registers.a = self.read_byte(bus, self.registers.hl())?;
                    Ok(8)
                }
                0xAF => {
                    self.registers.a ^= self.registers.a;
                    self.registers.set_flag(Flag::Zero, true);
                    self.registers.set_flag(Flag::Subtract, false);
                    self.registers.set_flag(Flag::HalfCarry, false);
                    self.registers.set_flag(Flag::Carry, false);
                    Ok(4)
                }
                0xC0 => self.return_if(bus, JumpCondition::NotZero),
                0xC1 => {
                    let value = self.pop_stack(bus)?;
                    self.registers.set_bc(value);
                    Ok(12)
                }
                0xC2 => self.jump_absolute_if(bus, JumpCondition::NotZero),
                0xC3 => {
                    self.registers.pc = self.fetch_word(bus)?;
                    Ok(16)
                }
                0xC4 => self.call_if(bus, JumpCondition::NotZero),
                0xC5 => {
                    self.push_stack(bus, self.registers.bc())?;
                    Ok(16)
                }
                0xC6 => {
                    let value = self.fetch_byte(bus)?;
                    self.execute_alu(AluOperation::Add, value);
                    Ok(8)
                }
                0xC7 => {
                    self.restart(bus, 0x00)?;
                    Ok(16)
                }
                0xC8 => self.return_if(bus, JumpCondition::Zero),
                0xC9 => {
                    self.registers.pc = self.pop_stack(bus)?;
                    Ok(16)
                }
                0xCA => self.jump_absolute_if(bus, JumpCondition::Zero),
                0xCB => self.execute_cb_prefixed(bus),
                0xCC => self.call_if(bus, JumpCondition::Zero),
                0xCD => {
                    let address = self.fetch_word(bus)?;
                    self.push_stack(bus, self.registers.pc)?;
                    self.registers.pc = address;
                    Ok(24)
                }
                0xCE => {
                    let value = self.fetch_byte(bus)?;
                    self.execute_alu(AluOperation::Adc, value);
                    Ok(8)
                }
                0xCF => {
                    self.restart(bus, 0x08)?;
                    Ok(16)
                }
                0xD0 => self.return_if(bus, JumpCondition::NotCarry),
                0xD1 => {
                    let value = self.pop_stack(bus)?;
                    self.registers.set_de(value);
                    Ok(12)
                }
                0xD2 => self.jump_absolute_if(bus, JumpCondition::NotCarry),
                0xD4 => self.call_if(bus, JumpCondition::NotCarry),
                0xD5 => {
                    self.push_stack(bus, self.registers.de())?;
                    Ok(16)
                }
                0xD6 => {
                    let value = self.fetch_byte(bus)?;
                    self.execute_alu(AluOperation::Sub, value);
                    Ok(8)
                }
                0xD7 => {
                    self.restart(bus, 0x10)?;
                    Ok(16)
                }
                0xD8 => self.return_if(bus, JumpCondition::Carry),
                0xD9 => {
                    self.registers.pc = self.pop_stack(bus)?;
                    self.interrupt_master_enabled = true;
                    self.ime_enable_delay = None;
                    Ok(16)
                }
                0xDA => self.jump_absolute_if(bus, JumpCondition::Carry),
                0xDC => self.call_if(bus, JumpCondition::Carry),
                0xDE => {
                    let value = self.fetch_byte(bus)?;
                    self.execute_alu(AluOperation::Sbc, value);
                    Ok(8)
                }
                0xDF => {
                    self.restart(bus, 0x18)?;
                    Ok(16)
                }
                0xE0 => {
                    let offset = self.fetch_byte(bus)?;
                    self.write_byte(bus, 0xFF00 + offset as u16, self.registers.a)?;
                    Ok(12)
                }
                0xE1 => {
                    let value = self.pop_stack(bus)?;
                    self.registers.set_hl(value);
                    Ok(12)
                }
                0xE2 => {
                    self.write_byte(bus, 0xFF00 + self.registers.c as u16, self.registers.a)?;
                    Ok(8)
                }
                0xE5 => {
                    self.push_stack(bus, self.registers.hl())?;
                    Ok(16)
                }
                0xE6 => {
                    let value = self.fetch_byte(bus)?;
                    self.execute_alu(AluOperation::And, value);
                    Ok(8)
                }
                0xE7 => {
                    self.restart(bus, 0x20)?;
                    Ok(16)
                }
                0xE8 => {
                    let offset = self.fetch_byte(bus)? as i8;
                    self.add_signed_offset_to_sp(offset);
                    Ok(16)
                }
                0xE9 => {
                    self.registers.pc = self.registers.hl();
                    Ok(4)
                }
                0xEA => {
                    let address = self.fetch_word(bus)?;
                    self.write_byte(bus, address, self.registers.a)?;
                    Ok(16)
                }
                0xEE => {
                    let value = self.fetch_byte(bus)?;
                    self.execute_alu(AluOperation::Xor, value);
                    Ok(8)
                }
                0xEF => {
                    self.restart(bus, 0x28)?;
                    Ok(16)
                }
                0xF0 => {
                    let offset = self.fetch_byte(bus)?;
                    self.registers.a = self.read_byte(bus, 0xFF00 + offset as u16)?;
                    Ok(12)
                }
                0xF1 => {
                    let value = self.pop_stack(bus)?;
                    self.registers.set_af(value);
                    Ok(12)
                }
                0xF2 => {
                    self.registers.a = self.read_byte(bus, 0xFF00 + self.registers.c as u16)?;
                    Ok(8)
                }
                0xF3 => {
                    self.interrupt_master_enabled = false;
                    self.ime_enable_delay = None;
                    Ok(4)
                }
                0xF5 => {
                    self.push_stack(bus, self.registers.af())?;
                    Ok(16)
                }
                0xF6 => {
                    let value = self.fetch_byte(bus)?;
                    self.execute_alu(AluOperation::Or, value);
                    Ok(8)
                }
                0xF7 => {
                    self.restart(bus, 0x30)?;
                    Ok(16)
                }
                0xF8 => {
                    let offset = self.fetch_byte(bus)? as i8;
                    let value = self.sp_plus_signed_offset(offset);
                    self.registers.set_hl(value);
                    Ok(12)
                }
                0xF9 => {
                    self.registers.sp = self.registers.hl();
                    Ok(8)
                }
                0xFA => {
                    let address = self.fetch_word(bus)?;
                    self.registers.a = self.read_byte(bus, address)?;
                    Ok(16)
                }
                0xFB => {
                    self.ime_enable_delay = Some(2);
                    Ok(4)
                }
                0xFE => {
                    let value = self.fetch_byte(bus)?;
                    self.execute_alu(AluOperation::Cp, value);
                    Ok(8)
                }
                0xFF => {
                    self.restart(bus, 0x38)?;
                    Ok(16)
                }
                _ => Err(EmulatorError::UnimplementedOpcode { opcode, pc }),
            }?
        };

        self.advance_remaining_cycles(bus, cycles);
        self.finish_instruction();
        Ok(cycles)
    }

    fn fetch_byte(&mut self, bus: &mut Bus) -> Result<u8> {
        let byte = self.read_byte(bus, self.registers.pc)?;
        if self.halt_bug {
            self.halt_bug = false;
        } else {
            self.registers.pc = self.registers.pc.wrapping_add(1);
        }
        Ok(byte)
    }

    fn fetch_word(&mut self, bus: &mut Bus) -> Result<u16> {
        let low = self.fetch_byte(bus)?;
        let high = self.fetch_byte(bus)?;
        Ok(u16::from_le_bytes([low, high]))
    }

    fn read_byte(&mut self, bus: &mut Bus, address: u16) -> Result<u8> {
        let value = bus.read_byte(address)?;
        self.idle_cycle(bus);
        Ok(value)
    }

    fn write_byte(&mut self, bus: &mut Bus, address: u16, value: u8) -> Result<()> {
        bus.write_byte(address, value)?;
        self.idle_cycle(bus);
        Ok(())
    }

    fn read8(&mut self, bus: &mut Bus, register: Register8) -> Result<u8> {
        match register {
            Register8::B => Ok(self.registers.b),
            Register8::C => Ok(self.registers.c),
            Register8::D => Ok(self.registers.d),
            Register8::E => Ok(self.registers.e),
            Register8::H => Ok(self.registers.h),
            Register8::L => Ok(self.registers.l),
            Register8::AddressHl => self.read_byte(bus, self.registers.hl()),
            Register8::A => Ok(self.registers.a),
        }
    }

    fn write8(&mut self, bus: &mut Bus, register: Register8, value: u8) -> Result<()> {
        match register {
            Register8::B => self.registers.b = value,
            Register8::C => self.registers.c = value,
            Register8::D => self.registers.d = value,
            Register8::E => self.registers.e = value,
            Register8::H => self.registers.h = value,
            Register8::L => self.registers.l = value,
            Register8::AddressHl => {
                self.write_byte(bus, self.registers.hl(), value)?;
            }
            Register8::A => self.registers.a = value,
        }

        Ok(())
    }

    fn idle_cycle(&mut self, bus: &mut Bus) {
        bus.advance_cycles(4);
        self.elapsed_cycles += 4;
    }

    fn advance_remaining_cycles(&mut self, bus: &mut Bus, cycles: CycleCount) {
        while self.elapsed_cycles < cycles {
            self.idle_cycle(bus);
        }
    }

    fn load_register_from_register(&mut self, bus: &mut Bus, opcode: u8) -> Result<CycleCount> {
        let destination = decode_register8((opcode >> 3) & 0b111);
        let source = decode_register8(opcode & 0b111);
        let value = self.read8(bus, source)?;
        self.write8(bus, destination, value)?;

        if destination == Register8::AddressHl || source == Register8::AddressHl {
            Ok(8)
        } else {
            Ok(4)
        }
    }

    fn execute_register_alu(&mut self, bus: &mut Bus, opcode: u8) -> Result<CycleCount> {
        let operation = match opcode >> 3 {
            0x10 => AluOperation::Add,
            0x11 => AluOperation::Adc,
            0x12 => AluOperation::Sub,
            0x13 => AluOperation::Sbc,
            0x14 => AluOperation::And,
            0x15 => AluOperation::Xor,
            0x16 => AluOperation::Or,
            0x17 => AluOperation::Cp,
            _ => unreachable!("ALU opcode range must decode to an ALU operation"),
        };
        let source = decode_register8(opcode & 0b111);
        let value = self.read8(bus, source)?;
        self.execute_alu(operation, value);

        if source == Register8::AddressHl {
            Ok(8)
        } else {
            Ok(4)
        }
    }

    fn execute_alu(&mut self, operation: AluOperation, value: u8) {
        match operation {
            AluOperation::Add => self.add(value, false),
            AluOperation::Adc => self.add(value, true),
            AluOperation::Sub => self.subtract(value, false),
            AluOperation::Sbc => self.subtract(value, true),
            AluOperation::And => {
                self.registers.a &= value;
                self.registers.set_flag(Flag::Zero, self.registers.a == 0);
                self.registers.set_flag(Flag::Subtract, false);
                self.registers.set_flag(Flag::HalfCarry, true);
                self.registers.set_flag(Flag::Carry, false);
            }
            AluOperation::Xor => {
                self.registers.a ^= value;
                self.registers.set_flag(Flag::Zero, self.registers.a == 0);
                self.registers.set_flag(Flag::Subtract, false);
                self.registers.set_flag(Flag::HalfCarry, false);
                self.registers.set_flag(Flag::Carry, false);
            }
            AluOperation::Or => {
                self.registers.a |= value;
                self.registers.set_flag(Flag::Zero, self.registers.a == 0);
                self.registers.set_flag(Flag::Subtract, false);
                self.registers.set_flag(Flag::HalfCarry, false);
                self.registers.set_flag(Flag::Carry, false);
            }
            AluOperation::Cp => self.compare(value),
        }
    }

    fn execute_cb_prefixed(&mut self, bus: &mut Bus) -> Result<CycleCount> {
        let opcode = self.fetch_byte(bus)?;
        let register = decode_register8(opcode & 0b111);

        match opcode {
            0x00..=0x3F => {
                let operation = match opcode >> 3 {
                    0 => RotateOperation::Rlc,
                    1 => RotateOperation::Rrc,
                    2 => RotateOperation::Rl,
                    3 => RotateOperation::Rr,
                    4 => RotateOperation::Sla,
                    5 => RotateOperation::Sra,
                    6 => RotateOperation::Swap,
                    7 => RotateOperation::Srl,
                    _ => unreachable!("CB rotate/shift group is 3 bits"),
                };
                let value = self.read8(bus, register)?;
                let result = self.rotate_shift_value(operation, value, true);
                self.write8(bus, register, result)?;
            }
            0x40..=0x7F => {
                let bit = (opcode >> 3) & 0b111;
                let value = self.read8(bus, register)?;
                self.registers.set_flag(Flag::Zero, value & (1 << bit) == 0);
                self.registers.set_flag(Flag::Subtract, false);
                self.registers.set_flag(Flag::HalfCarry, true);
            }
            0x80..=0xBF => {
                let bit = (opcode >> 3) & 0b111;
                let value = self.read8(bus, register)? & !(1 << bit);
                self.write8(bus, register, value)?;
            }
            0xC0..=0xFF => {
                let bit = (opcode >> 3) & 0b111;
                let value = self.read8(bus, register)? | (1 << bit);
                self.write8(bus, register, value)?;
            }
        }

        if register == Register8::AddressHl {
            match opcode {
                0x40..=0x7F => Ok(12),
                _ => Ok(16),
            }
        } else {
            Ok(8)
        }
    }

    fn rotate_accumulator(&mut self, operation: RotateOperation) {
        self.registers.a = self.rotate_shift_value(operation, self.registers.a, false);
        self.registers.set_flag(Flag::Zero, false);
    }

    fn rotate_shift_value(&mut self, operation: RotateOperation, value: u8, set_zero: bool) -> u8 {
        let old_carry = u8::from(self.registers.flag(Flag::Carry));
        let (result, carry) = match operation {
            RotateOperation::Rlc => (value.rotate_left(1), value & 0x80 != 0),
            RotateOperation::Rrc => (value.rotate_right(1), value & 0x01 != 0),
            RotateOperation::Rl => ((value << 1) | old_carry, value & 0x80 != 0),
            RotateOperation::Rr => ((value >> 1) | (old_carry << 7), value & 0x01 != 0),
            RotateOperation::Sla => (value << 1, value & 0x80 != 0),
            RotateOperation::Sra => ((value >> 1) | (value & 0x80), value & 0x01 != 0),
            RotateOperation::Swap => (value.rotate_left(4), false),
            RotateOperation::Srl => (value >> 1, value & 0x01 != 0),
        };

        self.registers.set_flag(Flag::Zero, set_zero && result == 0);
        self.registers.set_flag(Flag::Subtract, false);
        self.registers.set_flag(Flag::HalfCarry, false);
        self.registers.set_flag(Flag::Carry, carry);
        result
    }

    fn decimal_adjust_accumulator(&mut self) {
        let mut adjustment = 0;
        let mut carry = self.registers.flag(Flag::Carry);

        if self.registers.flag(Flag::HalfCarry)
            || (!self.registers.flag(Flag::Subtract) && self.registers.a & 0x0F > 9)
        {
            adjustment |= 0x06;
        }

        if carry || (!self.registers.flag(Flag::Subtract) && self.registers.a > 0x99) {
            adjustment |= 0x60;
            carry = true;
        }

        if self.registers.flag(Flag::Subtract) {
            self.registers.a = self.registers.a.wrapping_sub(adjustment);
        } else {
            self.registers.a = self.registers.a.wrapping_add(adjustment);
        }

        self.registers.set_flag(Flag::Zero, self.registers.a == 0);
        self.registers.set_flag(Flag::HalfCarry, false);
        self.registers.set_flag(Flag::Carry, carry);
    }

    fn add(&mut self, value: u8, include_carry: bool) {
        let a = self.registers.a;
        let carry = u8::from(include_carry && self.registers.flag(Flag::Carry));
        let result = a.wrapping_add(value).wrapping_add(carry);

        self.registers.a = result;
        self.registers.set_flag(Flag::Zero, result == 0);
        self.registers.set_flag(Flag::Subtract, false);
        self.registers
            .set_flag(Flag::HalfCarry, (a & 0x0F) + (value & 0x0F) + carry > 0x0F);
        self.registers
            .set_flag(Flag::Carry, a as u16 + value as u16 + carry as u16 > 0xFF);
    }

    fn subtract(&mut self, value: u8, include_carry: bool) {
        let a = self.registers.a;
        let carry = u8::from(include_carry && self.registers.flag(Flag::Carry));
        let result = a.wrapping_sub(value).wrapping_sub(carry);

        self.registers.a = result;
        self.registers.set_flag(Flag::Zero, result == 0);
        self.registers.set_flag(Flag::Subtract, true);
        self.registers
            .set_flag(Flag::HalfCarry, (a & 0x0F) < ((value & 0x0F) + carry));
        self.registers
            .set_flag(Flag::Carry, (a as u16) < (value as u16 + carry as u16));
    }

    fn compare(&mut self, value: u8) {
        let a = self.registers.a;
        let result = a.wrapping_sub(value);

        self.registers.set_flag(Flag::Zero, result == 0);
        self.registers.set_flag(Flag::Subtract, true);
        self.registers
            .set_flag(Flag::HalfCarry, (a & 0x0F) < (value & 0x0F));
        self.registers.set_flag(Flag::Carry, a < value);
    }

    fn increment_register(&mut self, bus: &mut Bus, register: Register8) -> Result<CycleCount> {
        let value = self.read8(bus, register)?;
        let result = value.wrapping_add(1);
        self.write8(bus, register, result)?;

        self.registers.set_flag(Flag::Zero, result == 0);
        self.registers.set_flag(Flag::Subtract, false);
        self.registers
            .set_flag(Flag::HalfCarry, (value & 0x0F) + 1 > 0x0F);

        if register == Register8::AddressHl {
            Ok(12)
        } else {
            Ok(4)
        }
    }

    fn decrement_register(&mut self, bus: &mut Bus, register: Register8) -> Result<CycleCount> {
        let value = self.read8(bus, register)?;
        let result = value.wrapping_sub(1);
        self.write8(bus, register, result)?;

        self.registers.set_flag(Flag::Zero, result == 0);
        self.registers.set_flag(Flag::Subtract, true);
        self.registers
            .set_flag(Flag::HalfCarry, (value & 0x0F) == 0);

        if register == Register8::AddressHl {
            Ok(12)
        } else {
            Ok(4)
        }
    }

    fn increment_register16(&mut self, register: Register16) -> Result<CycleCount> {
        let value = self.registers.read16(register).wrapping_add(1);
        self.registers.write16(register, value);
        Ok(8)
    }

    fn decrement_register16(&mut self, register: Register16) -> Result<CycleCount> {
        let value = self.registers.read16(register).wrapping_sub(1);
        self.registers.write16(register, value);
        Ok(8)
    }

    fn add_hl(&mut self, value: u16) -> Result<CycleCount> {
        let hl = self.registers.hl();
        let result = hl.wrapping_add(value);

        self.registers.set_hl(result);
        self.registers.set_flag(Flag::Subtract, false);
        self.registers
            .set_flag(Flag::HalfCarry, (hl & 0x0FFF) + (value & 0x0FFF) > 0x0FFF);
        self.registers
            .set_flag(Flag::Carry, hl as u32 + value as u32 > 0xFFFF);
        Ok(8)
    }

    fn push_stack(&mut self, bus: &mut Bus, value: u16) -> Result<()> {
        let [high, low] = value.to_be_bytes();
        self.registers.sp = self.registers.sp.wrapping_sub(1);
        self.write_byte(bus, self.registers.sp, high)?;
        self.registers.sp = self.registers.sp.wrapping_sub(1);
        self.write_byte(bus, self.registers.sp, low)
    }

    fn pop_stack(&mut self, bus: &mut Bus) -> Result<u16> {
        let low = self.read_byte(bus, self.registers.sp)?;
        self.registers.sp = self.registers.sp.wrapping_add(1);
        let high = self.read_byte(bus, self.registers.sp)?;
        self.registers.sp = self.registers.sp.wrapping_add(1);
        Ok(u16::from_be_bytes([high, low]))
    }

    fn restart(&mut self, bus: &mut Bus, vector: u16) -> Result<()> {
        self.push_stack(bus, self.registers.pc)?;
        self.registers.pc = vector;
        Ok(())
    }

    fn service_interrupt(&mut self, bus: &mut Bus) -> Result<CycleCount> {
        let pending = bus.pending_interrupts();
        let bit = pending.trailing_zeros() as u8;
        let vector = interrupt_vector(bit);

        self.interrupt_master_enabled = false;
        self.ime_enable_delay = None;
        bus.clear_interrupt(bit);
        self.push_stack(bus, self.registers.pc)?;
        self.registers.pc = vector;
        Ok(20)
    }

    fn jump_relative(&mut self, bus: &mut Bus) -> Result<()> {
        let offset = self.fetch_byte(bus)? as i8;
        self.registers.pc = self.registers.pc.wrapping_add_signed(offset as i16);
        Ok(())
    }

    fn jump_relative_if(&mut self, bus: &mut Bus, condition: JumpCondition) -> Result<CycleCount> {
        let should_jump = match condition {
            JumpCondition::NotZero => !self.registers.flag(Flag::Zero),
            JumpCondition::Zero => self.registers.flag(Flag::Zero),
            JumpCondition::NotCarry => !self.registers.flag(Flag::Carry),
            JumpCondition::Carry => self.registers.flag(Flag::Carry),
        };

        if should_jump {
            self.jump_relative(bus)?;
            Ok(12)
        } else {
            let _ = self.fetch_byte(bus)?;
            Ok(8)
        }
    }

    fn jump_absolute_if(&mut self, bus: &mut Bus, condition: JumpCondition) -> Result<CycleCount> {
        let address = self.fetch_word(bus)?;
        if self.condition_is_met(condition) {
            self.registers.pc = address;
            Ok(16)
        } else {
            Ok(12)
        }
    }

    fn call_if(&mut self, bus: &mut Bus, condition: JumpCondition) -> Result<CycleCount> {
        let address = self.fetch_word(bus)?;
        if self.condition_is_met(condition) {
            self.push_stack(bus, self.registers.pc)?;
            self.registers.pc = address;
            Ok(24)
        } else {
            Ok(12)
        }
    }

    fn return_if(&mut self, bus: &mut Bus, condition: JumpCondition) -> Result<CycleCount> {
        if self.condition_is_met(condition) {
            self.registers.pc = self.pop_stack(bus)?;
            Ok(20)
        } else {
            Ok(8)
        }
    }

    fn condition_is_met(&self, condition: JumpCondition) -> bool {
        match condition {
            JumpCondition::NotZero => !self.registers.flag(Flag::Zero),
            JumpCondition::Zero => self.registers.flag(Flag::Zero),
            JumpCondition::NotCarry => !self.registers.flag(Flag::Carry),
            JumpCondition::Carry => self.registers.flag(Flag::Carry),
        }
    }

    fn sp_plus_signed_offset(&mut self, offset: i8) -> u16 {
        let sp = self.registers.sp;
        let offset = offset as i16 as u16;
        let result = sp.wrapping_add(offset);

        self.registers.set_flag(Flag::Zero, false);
        self.registers.set_flag(Flag::Subtract, false);
        self.registers
            .set_flag(Flag::HalfCarry, (sp & 0x000F) + (offset & 0x000F) > 0x000F);
        self.registers
            .set_flag(Flag::Carry, (sp & 0x00FF) + (offset & 0x00FF) > 0x00FF);
        result
    }

    fn add_signed_offset_to_sp(&mut self, offset: i8) {
        self.registers.sp = self.sp_plus_signed_offset(offset);
    }

    fn finish_instruction(&mut self) {
        let Some(delay) = self.ime_enable_delay else {
            return;
        };

        if delay == 1 {
            self.interrupt_master_enabled = true;
            self.ime_enable_delay = None;
        } else {
            self.ime_enable_delay = Some(delay - 1);
        }
    }
}

fn decode_register8(code: u8) -> Register8 {
    match code {
        0 => Register8::B,
        1 => Register8::C,
        2 => Register8::D,
        3 => Register8::E,
        4 => Register8::H,
        5 => Register8::L,
        6 => Register8::AddressHl,
        7 => Register8::A,
        _ => unreachable!("3-bit register code must be in 0..=7"),
    }
}

fn interrupt_vector(bit: u8) -> u16 {
    match bit {
        0 => 0x0040,
        1 => 0x0048,
        2 => 0x0050,
        3 => 0x0058,
        4 => 0x0060,
        _ => unreachable!("pending interrupts are masked to five bits"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        bus::{INTERRUPT_JOYPAD, INTERRUPT_TIMER, INTERRUPT_VBLANK},
        cartridge::{synthetic_rom, Cartridge},
    };

    fn bus_with_program(program: &[u8]) -> Bus {
        let mut bus = Bus::default();
        bus.insert_cartridge(
            Cartridge::from_bytes(synthetic_rom("TEST", &[(0x0000, program)])).unwrap(),
        );
        bus
    }

    fn bus_with_program_at(address: u16, program: &[u8]) -> Bus {
        let mut bus = Bus::default();
        bus.insert_cartridge(
            Cartridge::from_bytes(synthetic_rom("TEST", &[(address, program)])).unwrap(),
        );
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
        let mut bus = bus_with_program(&[0xD3]);

        assert_eq!(
            cpu.step(&mut bus),
            Err(EmulatorError::UnimplementedOpcode {
                opcode: 0xD3,
                pc: 0
            })
        );
        assert_eq!(cpu.registers().pc, 1);
    }

    #[test]
    fn post_boot_constructor_uses_dmg_register_defaults() {
        let cpu = Cpu::new_without_boot_rom();

        assert_eq!(cpu.registers().af(), 0x01B0);
        assert_eq!(cpu.registers().bc(), 0x0013);
        assert_eq!(cpu.registers().de(), 0x00D8);
        assert_eq!(cpu.registers().hl(), 0x014D);
        assert_eq!(cpu.registers().pc, 0x0100);
        assert_eq!(cpu.registers().sp, 0xFFFE);
        assert_eq!(cpu.mode(), CpuMode::Running);
        assert!(!cpu.interrupt_master_enabled());
    }

    #[test]
    fn immediate_loads_update_8_bit_registers() {
        let mut cpu = Cpu::default();
        let mut bus = bus_with_program(&[
            0x06, 0x12, 0x0E, 0x34, 0x16, 0x56, 0x1E, 0x78, 0x26, 0x9A, 0x2E, 0xBC, 0x3E, 0xDE,
        ]);

        for _ in 0..7 {
            assert_eq!(cpu.step(&mut bus), Ok(8));
        }

        assert_eq!(cpu.registers().b, 0x12);
        assert_eq!(cpu.registers().c, 0x34);
        assert_eq!(cpu.registers().d, 0x56);
        assert_eq!(cpu.registers().e, 0x78);
        assert_eq!(cpu.registers().h, 0x9A);
        assert_eq!(cpu.registers().l, 0xBC);
        assert_eq!(cpu.registers().a, 0xDE);
    }

    #[test]
    fn immediate_loads_update_16_bit_registers() {
        let mut cpu = Cpu::default();
        let mut bus = bus_with_program(&[
            0x01, 0x34, 0x12, 0x11, 0x78, 0x56, 0x21, 0xBC, 0x9A, 0x31, 0xFE, 0xFF,
        ]);

        for _ in 0..4 {
            assert_eq!(cpu.step(&mut bus), Ok(12));
        }

        assert_eq!(cpu.registers().bc(), 0x1234);
        assert_eq!(cpu.registers().de(), 0x5678);
        assert_eq!(cpu.registers().hl(), 0x9ABC);
        assert_eq!(cpu.registers().sp, 0xFFFE);
    }

    #[test]
    fn inc_and_dec_16_bit_registers_do_not_change_flags() {
        let mut cpu = Cpu::default();
        cpu.registers_mut().set_bc(0xFFFF);
        cpu.registers_mut().set_de(0x0000);
        cpu.registers_mut().set_hl(0x1234);
        cpu.registers_mut().sp = 0xABCD;
        cpu.registers_mut().set_flag(Flag::Zero, true);
        cpu.registers_mut().set_flag(Flag::Carry, true);
        let mut bus = bus_with_program(&[0x03, 0x1B, 0x23, 0x3B]);

        assert_eq!(cpu.step(&mut bus), Ok(8));
        assert_eq!(cpu.step(&mut bus), Ok(8));
        assert_eq!(cpu.step(&mut bus), Ok(8));
        assert_eq!(cpu.step(&mut bus), Ok(8));

        assert_eq!(cpu.registers().bc(), 0x0000);
        assert_eq!(cpu.registers().de(), 0xFFFF);
        assert_eq!(cpu.registers().hl(), 0x1235);
        assert_eq!(cpu.registers().sp, 0xABCC);
        assert!(cpu.registers().flag(Flag::Zero));
        assert!(cpu.registers().flag(Flag::Carry));
    }

    #[test]
    fn add_hl_updates_16_bit_flags_and_preserves_zero() {
        let mut cpu = Cpu::default();
        cpu.registers_mut().set_hl(0x0FFF);
        cpu.registers_mut().set_bc(0x0001);
        cpu.registers_mut().sp = 0xF000;
        cpu.registers_mut().set_flag(Flag::Zero, true);
        let mut bus = bus_with_program(&[0x09, 0x39]);

        assert_eq!(cpu.step(&mut bus), Ok(8));
        assert_eq!(cpu.registers().hl(), 0x1000);
        assert!(cpu.registers().flag(Flag::Zero));
        assert!(!cpu.registers().flag(Flag::Subtract));
        assert!(cpu.registers().flag(Flag::HalfCarry));
        assert!(!cpu.registers().flag(Flag::Carry));

        assert_eq!(cpu.step(&mut bus), Ok(8));
        assert_eq!(cpu.registers().hl(), 0x0000);
        assert!(cpu.registers().flag(Flag::Zero));
        assert!(!cpu.registers().flag(Flag::Subtract));
        assert!(!cpu.registers().flag(Flag::HalfCarry));
        assert!(cpu.registers().flag(Flag::Carry));
    }

    #[test]
    fn push_and_pop_use_little_endian_stack_memory() {
        let mut cpu = Cpu::default();
        cpu.registers_mut().set_bc(0x1234);
        cpu.registers_mut().sp = 0xC010;
        let mut bus = bus_with_program(&[0xC5, 0xD1]);

        assert_eq!(cpu.step(&mut bus), Ok(16));
        assert_eq!(cpu.registers().sp, 0xC00E);
        assert_eq!(bus.read_byte(0xC00E), Ok(0x34));
        assert_eq!(bus.read_byte(0xC00F), Ok(0x12));

        assert_eq!(cpu.step(&mut bus), Ok(12));
        assert_eq!(cpu.registers().de(), 0x1234);
        assert_eq!(cpu.registers().sp, 0xC010);
    }

    #[test]
    fn pop_af_masks_lower_flag_nibble() {
        let mut cpu = Cpu::default();
        cpu.registers_mut().sp = 0xC000;
        let mut bus = bus_with_program(&[0xF1]);
        bus.write_byte(0xC000, 0xFF).expect("write low");
        bus.write_byte(0xC001, 0xAB).expect("write high");

        assert_eq!(cpu.step(&mut bus), Ok(12));
        assert_eq!(cpu.registers().af(), 0xABF0);
    }

    #[test]
    fn jump_absolute_sets_pc_to_little_endian_target() {
        let mut cpu = Cpu::default();
        let mut bus = bus_with_program(&[0xC3, 0x34, 0x12]);

        assert_eq!(cpu.step(&mut bus), Ok(16));
        assert_eq!(cpu.registers().pc, 0x1234);
    }

    #[test]
    fn conditional_absolute_jumps_use_taken_and_not_taken_cycles() {
        let mut cpu = Cpu::default();
        cpu.registers_mut().set_flag(Flag::Zero, true);
        let mut bus = bus_with_program(&[0xC2, 0x06, 0x00, 0xCA, 0x08, 0x00]);

        assert_eq!(cpu.step(&mut bus), Ok(12));
        assert_eq!(cpu.registers().pc, 3);

        assert_eq!(cpu.step(&mut bus), Ok(16));
        assert_eq!(cpu.registers().pc, 8);
    }

    #[test]
    fn call_and_ret_round_trip_through_stack() {
        let mut cpu = Cpu::default();
        cpu.registers_mut().sp = 0xC010;
        let mut bus = bus_with_program(&[0xCD, 0x05, 0x00, 0xFF, 0xFF, 0xC9]);

        assert_eq!(cpu.step(&mut bus), Ok(24));
        assert_eq!(cpu.registers().pc, 5);
        assert_eq!(cpu.registers().sp, 0xC00E);
        assert_eq!(bus.read_byte(0xC00E), Ok(0x03));
        assert_eq!(bus.read_byte(0xC00F), Ok(0x00));

        assert_eq!(cpu.step(&mut bus), Ok(16));
        assert_eq!(cpu.registers().pc, 3);
        assert_eq!(cpu.registers().sp, 0xC010);
    }

    #[test]
    fn conditional_call_and_ret_use_expected_cycles() {
        let mut cpu = Cpu::default();
        cpu.registers_mut().sp = 0xC010;
        cpu.registers_mut().set_flag(Flag::Zero, true);
        let mut bus = bus_with_program(&[0xC4, 0x06, 0x00, 0xCC, 0x08, 0x00, 0xC0, 0xC8]);

        assert_eq!(cpu.step(&mut bus), Ok(12));
        assert_eq!(cpu.registers().pc, 3);
        assert_eq!(cpu.registers().sp, 0xC010);

        assert_eq!(cpu.step(&mut bus), Ok(24));
        assert_eq!(cpu.registers().pc, 8);
        assert_eq!(cpu.registers().sp, 0xC00E);
        assert_eq!(bus.read_byte(0xC00E), Ok(0x06));

        cpu.registers_mut().pc = 6;
        assert_eq!(cpu.step(&mut bus), Ok(8));
        assert_eq!(cpu.registers().pc, 7);

        assert_eq!(cpu.step(&mut bus), Ok(20));
        assert_eq!(cpu.registers().pc, 6);
        assert_eq!(cpu.registers().sp, 0xC010);
    }

    #[test]
    fn rst_pushes_return_address_and_jumps_to_vector() {
        let mut cpu = Cpu::default();
        cpu.registers_mut().sp = 0xC010;
        let mut bus = bus_with_program(&[0xFF]);

        assert_eq!(cpu.step(&mut bus), Ok(16));
        assert_eq!(cpu.registers().pc, 0x0038);
        assert_eq!(cpu.registers().sp, 0xC00E);
        assert_eq!(bus.read_byte(0xC00E), Ok(0x01));
        assert_eq!(bus.read_byte(0xC00F), Ok(0x00));
    }

    #[test]
    fn reti_returns_and_enables_interrupts() {
        let mut cpu = Cpu::default();
        cpu.registers_mut().sp = 0xC000;
        let mut bus = bus_with_program(&[0xD9]);
        bus.write_byte(0xC000, 0x34).expect("write low");
        bus.write_byte(0xC001, 0x12).expect("write high");

        assert_eq!(cpu.step(&mut bus), Ok(16));
        assert_eq!(cpu.registers().pc, 0x1234);
        assert!(cpu.interrupt_master_enabled());
    }

    #[test]
    fn jp_hl_loads_pc_from_hl() {
        let mut cpu = Cpu::default();
        cpu.registers_mut().set_hl(0x4567);
        let mut bus = bus_with_program(&[0xE9]);

        assert_eq!(cpu.step(&mut bus), Ok(4));
        assert_eq!(cpu.registers().pc, 0x4567);
    }

    #[test]
    fn signed_sp_arithmetic_sets_flags_from_low_byte() {
        let mut cpu = Cpu::default();
        cpu.registers_mut().sp = 0x00FF;
        let mut bus = bus_with_program(&[0xE8, 0x01, 0xF8, 0xFF, 0xF9]);

        assert_eq!(cpu.step(&mut bus), Ok(16));
        assert_eq!(cpu.registers().sp, 0x0100);
        assert!(!cpu.registers().flag(Flag::Zero));
        assert!(!cpu.registers().flag(Flag::Subtract));
        assert!(cpu.registers().flag(Flag::HalfCarry));
        assert!(cpu.registers().flag(Flag::Carry));

        assert_eq!(cpu.step(&mut bus), Ok(12));
        assert_eq!(cpu.registers().hl(), 0x00FF);
        assert!(!cpu.registers().flag(Flag::Zero));
        assert!(!cpu.registers().flag(Flag::Subtract));

        assert_eq!(cpu.step(&mut bus), Ok(8));
        assert_eq!(cpu.registers().sp, 0x00FF);
    }

    #[test]
    fn xor_a_zeroes_a_and_sets_flags() {
        let mut cpu = Cpu::default();
        cpu.registers_mut().a = 0xA5;
        cpu.registers_mut().set_flag(Flag::Carry, true);
        let mut bus = bus_with_program(&[0xAF]);

        assert_eq!(cpu.step(&mut bus), Ok(4));

        assert_eq!(cpu.registers().a, 0);
        assert!(cpu.registers().flag(Flag::Zero));
        assert!(!cpu.registers().flag(Flag::Subtract));
        assert!(!cpu.registers().flag(Flag::HalfCarry));
        assert!(!cpu.registers().flag(Flag::Carry));
    }

    #[test]
    fn accumulator_rotates_reset_zero_and_update_carry() {
        let mut cpu = Cpu::default();
        cpu.registers_mut().a = 0x80;
        let mut bus = bus_with_program(&[0x07, 0x17, 0x0F, 0x1F]);

        assert_eq!(cpu.step(&mut bus), Ok(4));
        assert_eq!(cpu.registers().a, 0x01);
        assert!(!cpu.registers().flag(Flag::Zero));
        assert!(cpu.registers().flag(Flag::Carry));

        assert_eq!(cpu.step(&mut bus), Ok(4));
        assert_eq!(cpu.registers().a, 0x03);
        assert!(!cpu.registers().flag(Flag::Carry));

        assert_eq!(cpu.step(&mut bus), Ok(4));
        assert_eq!(cpu.registers().a, 0x81);
        assert!(cpu.registers().flag(Flag::Carry));

        assert_eq!(cpu.step(&mut bus), Ok(4));
        assert_eq!(cpu.registers().a, 0xC0);
        assert!(cpu.registers().flag(Flag::Carry));
    }

    #[test]
    fn daa_adjusts_after_add_and_subtract() {
        let mut cpu = Cpu::default();
        cpu.registers_mut().a = 0x09;
        let mut bus = bus_with_program(&[0xC6, 0x01, 0x27, 0xD6, 0x01, 0x27]);

        assert_eq!(cpu.step(&mut bus), Ok(8));
        assert_eq!(cpu.registers().a, 0x0A);
        assert_eq!(cpu.step(&mut bus), Ok(4));
        assert_eq!(cpu.registers().a, 0x10);
        assert!(!cpu.registers().flag(Flag::Subtract));

        assert_eq!(cpu.step(&mut bus), Ok(8));
        assert_eq!(cpu.step(&mut bus), Ok(4));
        assert_eq!(cpu.registers().a, 0x09);
        assert!(cpu.registers().flag(Flag::Subtract));
    }

    #[test]
    fn complement_and_carry_flag_ops_update_flags() {
        let mut cpu = Cpu::default();
        cpu.registers_mut().a = 0x55;
        cpu.registers_mut().set_flag(Flag::Zero, true);
        let mut bus = bus_with_program(&[0x2F, 0x37, 0x3F]);

        assert_eq!(cpu.step(&mut bus), Ok(4));
        assert_eq!(cpu.registers().a, 0xAA);
        assert!(cpu.registers().flag(Flag::Zero));
        assert!(cpu.registers().flag(Flag::Subtract));
        assert!(cpu.registers().flag(Flag::HalfCarry));

        assert_eq!(cpu.step(&mut bus), Ok(4));
        assert!(cpu.registers().flag(Flag::Carry));
        assert!(!cpu.registers().flag(Flag::Subtract));
        assert!(!cpu.registers().flag(Flag::HalfCarry));

        assert_eq!(cpu.step(&mut bus), Ok(4));
        assert!(!cpu.registers().flag(Flag::Carry));
    }

    #[test]
    fn stop_enters_stopped_mode_and_consumes_padding_byte() {
        let mut cpu = Cpu::default();
        let mut bus = bus_with_program(&[0x10, 0x00, 0xFF]);

        assert_eq!(cpu.step(&mut bus), Ok(4));
        assert_eq!(cpu.mode(), CpuMode::Stopped);
        assert_eq!(cpu.registers().pc, 2);
    }

    #[test]
    fn cb_rotate_shift_and_swap_registers() {
        let mut cpu = Cpu::default();
        cpu.registers_mut().b = 0x80;
        cpu.registers_mut().c = 0x01;
        cpu.registers_mut().d = 0x80;
        cpu.registers_mut().e = 0x01;
        cpu.registers_mut().h = 0x81;
        cpu.registers_mut().l = 0xF0;
        cpu.registers_mut().a = 0x01;
        let mut bus = bus_with_program(&[
            0xCB, 0x00, 0xCB, 0x09, 0xCB, 0x12, 0xCB, 0x1B, 0xCB, 0x24, 0xCB, 0x35, 0xCB, 0x3F,
        ]);

        assert_eq!(cpu.step(&mut bus), Ok(8));
        assert_eq!(cpu.registers().b, 0x01);
        assert!(cpu.registers().flag(Flag::Carry));

        assert_eq!(cpu.step(&mut bus), Ok(8));
        assert_eq!(cpu.registers().c, 0x80);
        assert!(cpu.registers().flag(Flag::Carry));

        assert_eq!(cpu.step(&mut bus), Ok(8));
        assert_eq!(cpu.registers().d, 0x01);
        assert!(cpu.registers().flag(Flag::Carry));

        assert_eq!(cpu.step(&mut bus), Ok(8));
        assert_eq!(cpu.registers().e, 0x80);
        assert!(cpu.registers().flag(Flag::Carry));

        assert_eq!(cpu.step(&mut bus), Ok(8));
        assert_eq!(cpu.registers().h, 0x02);
        assert!(cpu.registers().flag(Flag::Carry));

        assert_eq!(cpu.step(&mut bus), Ok(8));
        assert_eq!(cpu.registers().l, 0x0F);

        assert_eq!(cpu.step(&mut bus), Ok(8));
        assert_eq!(cpu.registers().a, 0x00);
        assert!(cpu.registers().flag(Flag::Zero));
        assert!(cpu.registers().flag(Flag::Carry));
    }

    #[test]
    fn cb_bit_res_and_set_registers_preserve_expected_flags() {
        let mut cpu = Cpu::default();
        cpu.registers_mut().b = 0b0000_0010;
        cpu.registers_mut().set_flag(Flag::Carry, true);
        let mut bus = bus_with_program(&[0xCB, 0x48, 0xCB, 0x80, 0xCB, 0xC0]);

        assert_eq!(cpu.step(&mut bus), Ok(8));
        assert!(!cpu.registers().flag(Flag::Zero));
        assert!(!cpu.registers().flag(Flag::Subtract));
        assert!(cpu.registers().flag(Flag::HalfCarry));
        assert!(cpu.registers().flag(Flag::Carry));

        assert_eq!(cpu.step(&mut bus), Ok(8));
        assert_eq!(cpu.registers().b, 0b0000_0010);

        assert_eq!(cpu.step(&mut bus), Ok(8));
        assert_eq!(cpu.registers().b, 0b0000_0011);
    }

    #[test]
    fn cb_operations_on_hl_use_memory_cycles() {
        let mut cpu = Cpu::default();
        cpu.registers_mut().set_hl(0xC000);
        let mut bus = bus_with_program(&[0xCB, 0x46, 0xCB, 0xC6, 0xCB, 0x86, 0xCB, 0x36]);
        bus.write_byte(0xC000, 0x00).expect("seed memory");

        assert_eq!(cpu.step(&mut bus), Ok(12));
        assert!(cpu.registers().flag(Flag::Zero));

        assert_eq!(cpu.step(&mut bus), Ok(16));
        assert_eq!(bus.read_byte(0xC000), Ok(0x01));

        assert_eq!(cpu.step(&mut bus), Ok(16));
        assert_eq!(bus.read_byte(0xC000), Ok(0x00));

        bus.write_byte(0xC000, 0xF0).expect("seed memory");
        assert_eq!(cpu.step(&mut bus), Ok(16));
        assert_eq!(bus.read_byte(0xC000), Ok(0x0F));
    }

    #[test]
    fn inc_and_dec_update_flags_without_changing_carry() {
        let mut cpu = Cpu::default();
        cpu.registers_mut().b = 0x0F;
        cpu.registers_mut().c = 0x00;
        cpu.registers_mut().set_flag(Flag::Carry, true);
        let mut bus = bus_with_program(&[0x04, 0x0D]);

        assert_eq!(cpu.step(&mut bus), Ok(4));
        assert_eq!(cpu.registers().b, 0x10);
        assert!(!cpu.registers().flag(Flag::Zero));
        assert!(!cpu.registers().flag(Flag::Subtract));
        assert!(cpu.registers().flag(Flag::HalfCarry));
        assert!(cpu.registers().flag(Flag::Carry));

        assert_eq!(cpu.step(&mut bus), Ok(4));
        assert_eq!(cpu.registers().c, 0xFF);
        assert!(!cpu.registers().flag(Flag::Zero));
        assert!(cpu.registers().flag(Flag::Subtract));
        assert!(cpu.registers().flag(Flag::HalfCarry));
        assert!(cpu.registers().flag(Flag::Carry));
    }

    #[test]
    fn inc_and_dec_through_hl_use_memory_cycles() {
        let mut cpu = Cpu::default();
        cpu.registers_mut().set_hl(0xC000);
        let mut bus = bus_with_program(&[0x34, 0x35]);
        bus.write_byte(0xC000, 0xFF).expect("seed memory");

        assert_eq!(cpu.step(&mut bus), Ok(12));
        assert_eq!(bus.read_byte(0xC000), Ok(0x00));
        assert!(cpu.registers().flag(Flag::Zero));
        assert!(cpu.registers().flag(Flag::HalfCarry));

        assert_eq!(cpu.step(&mut bus), Ok(12));
        assert_eq!(bus.read_byte(0xC000), Ok(0xFF));
        assert!(!cpu.registers().flag(Flag::Zero));
        assert!(cpu.registers().flag(Flag::Subtract));
        assert!(cpu.registers().flag(Flag::HalfCarry));
    }

    #[test]
    fn add_and_adc_set_half_carry_and_carry_flags() {
        let mut cpu = Cpu::default();
        cpu.registers_mut().a = 0x8F;
        cpu.registers_mut().b = 0x71;
        cpu.registers_mut().c = 0x00;
        let mut bus = bus_with_program(&[0x80, 0x89]);

        assert_eq!(cpu.step(&mut bus), Ok(4));
        assert_eq!(cpu.registers().a, 0x00);
        assert!(cpu.registers().flag(Flag::Zero));
        assert!(!cpu.registers().flag(Flag::Subtract));
        assert!(cpu.registers().flag(Flag::HalfCarry));
        assert!(cpu.registers().flag(Flag::Carry));

        assert_eq!(cpu.step(&mut bus), Ok(4));
        assert_eq!(cpu.registers().a, 0x01);
        assert!(!cpu.registers().flag(Flag::Zero));
        assert!(!cpu.registers().flag(Flag::Subtract));
        assert!(!cpu.registers().flag(Flag::HalfCarry));
        assert!(!cpu.registers().flag(Flag::Carry));
    }

    #[test]
    fn sub_and_sbc_set_borrow_flags() {
        let mut cpu = Cpu::default();
        cpu.registers_mut().a = 0x10;
        cpu.registers_mut().b = 0x01;
        cpu.registers_mut().c = 0x0F;
        let mut bus = bus_with_program(&[0x90, 0x99]);

        assert_eq!(cpu.step(&mut bus), Ok(4));
        assert_eq!(cpu.registers().a, 0x0F);
        assert!(!cpu.registers().flag(Flag::Zero));
        assert!(cpu.registers().flag(Flag::Subtract));
        assert!(cpu.registers().flag(Flag::HalfCarry));
        assert!(!cpu.registers().flag(Flag::Carry));

        assert_eq!(cpu.step(&mut bus), Ok(4));
        assert_eq!(cpu.registers().a, 0x00);
        assert!(cpu.registers().flag(Flag::Zero));
        assert!(cpu.registers().flag(Flag::Subtract));
        assert!(!cpu.registers().flag(Flag::HalfCarry));
        assert!(!cpu.registers().flag(Flag::Carry));
    }

    #[test]
    fn logical_alu_operations_set_expected_flags() {
        let mut cpu = Cpu::default();
        cpu.registers_mut().a = 0b1010_0000;
        cpu.registers_mut().b = 0b1000_1111;
        cpu.registers_mut().c = 0b0111_0000;
        cpu.registers_mut().d = 0b1111_0000;
        let mut bus = bus_with_program(&[0xA0, 0xB1, 0xAA]);

        assert_eq!(cpu.step(&mut bus), Ok(4));
        assert_eq!(cpu.registers().a, 0b1000_0000);
        assert!(!cpu.registers().flag(Flag::Zero));
        assert!(!cpu.registers().flag(Flag::Subtract));
        assert!(cpu.registers().flag(Flag::HalfCarry));
        assert!(!cpu.registers().flag(Flag::Carry));

        assert_eq!(cpu.step(&mut bus), Ok(4));
        assert_eq!(cpu.registers().a, 0b1111_0000);
        assert!(!cpu.registers().flag(Flag::HalfCarry));

        assert_eq!(cpu.step(&mut bus), Ok(4));
        assert_eq!(cpu.registers().a, 0);
        assert!(cpu.registers().flag(Flag::Zero));
        assert!(!cpu.registers().flag(Flag::Subtract));
        assert!(!cpu.registers().flag(Flag::HalfCarry));
        assert!(!cpu.registers().flag(Flag::Carry));
    }

    #[test]
    fn cp_sets_subtraction_flags_without_changing_a() {
        let mut cpu = Cpu::default();
        cpu.registers_mut().a = 0x10;
        cpu.registers_mut().b = 0x11;
        let mut bus = bus_with_program(&[0xB8]);

        assert_eq!(cpu.step(&mut bus), Ok(4));
        assert_eq!(cpu.registers().a, 0x10);
        assert!(!cpu.registers().flag(Flag::Zero));
        assert!(cpu.registers().flag(Flag::Subtract));
        assert!(cpu.registers().flag(Flag::HalfCarry));
        assert!(cpu.registers().flag(Flag::Carry));
    }

    #[test]
    fn immediate_alu_operations_execute_against_a() {
        let mut cpu = Cpu::default();
        cpu.registers_mut().a = 0x01;
        let mut bus =
            bus_with_program(&[0xC6, 0x0F, 0xCE, 0xF0, 0xE6, 0x0F, 0xF6, 0x80, 0xFE, 0x8F]);

        assert_eq!(cpu.step(&mut bus), Ok(8));
        assert_eq!(cpu.registers().a, 0x10);
        assert!(cpu.registers().flag(Flag::HalfCarry));

        assert_eq!(cpu.step(&mut bus), Ok(8));
        assert_eq!(cpu.registers().a, 0x00);
        assert!(cpu.registers().flag(Flag::Zero));
        assert!(cpu.registers().flag(Flag::Carry));

        assert_eq!(cpu.step(&mut bus), Ok(8));
        assert_eq!(cpu.registers().a, 0x00);
        assert!(cpu.registers().flag(Flag::Zero));

        assert_eq!(cpu.step(&mut bus), Ok(8));
        assert_eq!(cpu.registers().a, 0x80);

        assert_eq!(cpu.step(&mut bus), Ok(8));
        assert_eq!(cpu.registers().a, 0x80);
        assert!(cpu.registers().flag(Flag::Carry));
    }

    #[test]
    fn alu_operations_can_read_from_hl() {
        let mut cpu = Cpu::default();
        cpu.registers_mut().a = 0x01;
        cpu.registers_mut().set_hl(0xC000);
        let mut bus = bus_with_program(&[0x86]);
        bus.write_byte(0xC000, 0x02).expect("seed memory");

        assert_eq!(cpu.step(&mut bus), Ok(8));
        assert_eq!(cpu.registers().a, 0x03);
    }

    #[test]
    fn relative_jump_uses_signed_offset() {
        let mut cpu = Cpu::default();
        let mut bus = bus_with_program(&[0x18, 0x02, 0xFF, 0xFF, 0x18, 0xFC]);

        assert_eq!(cpu.step(&mut bus), Ok(12));
        assert_eq!(cpu.registers().pc, 4);

        assert_eq!(cpu.step(&mut bus), Ok(12));
        assert_eq!(cpu.registers().pc, 2);
    }

    #[test]
    fn conditional_relative_jumps_report_taken_and_not_taken_cycles() {
        let mut cpu = Cpu::default();
        let mut bus = bus_with_program(&[0x20, 0x02, 0x30, 0x02, 0x28, 0x02, 0x38, 0x02]);

        cpu.registers_mut().set_flag(Flag::Zero, true);
        assert_eq!(cpu.step(&mut bus), Ok(8));
        assert_eq!(cpu.registers().pc, 2);

        cpu.registers_mut().set_flag(Flag::Carry, true);
        assert_eq!(cpu.step(&mut bus), Ok(8));
        assert_eq!(cpu.registers().pc, 4);

        assert_eq!(cpu.step(&mut bus), Ok(12));
        assert_eq!(cpu.registers().pc, 8);

        cpu.registers_mut().pc = 6;
        assert_eq!(cpu.step(&mut bus), Ok(12));
        assert_eq!(cpu.registers().pc, 10);
    }

    #[test]
    fn load_through_hl_reads_and_writes_bus_memory() {
        let mut cpu = Cpu::default();
        let mut bus = bus_with_program_at(0, &[0x21, 0x00, 0xC0, 0x36, 0x5A, 0x7E]);

        assert_eq!(cpu.step(&mut bus), Ok(12));
        assert_eq!(cpu.step(&mut bus), Ok(12));
        assert_eq!(cpu.step(&mut bus), Ok(8));

        assert_eq!(cpu.registers().a, 0x5A);
    }

    #[test]
    fn register_to_register_loads_follow_opcode_matrix() {
        let mut cpu = Cpu::default();
        cpu.registers_mut().b = 0x12;
        cpu.registers_mut().c = 0x34;
        let mut bus = bus_with_program(&[0x41, 0x78]);

        assert_eq!(cpu.step(&mut bus), Ok(4));
        assert_eq!(cpu.registers().b, 0x34);

        assert_eq!(cpu.step(&mut bus), Ok(4));
        assert_eq!(cpu.registers().a, 0x34);
    }

    #[test]
    fn register_to_hl_loads_take_memory_cycles() {
        let mut cpu = Cpu::default();
        cpu.registers_mut().set_hl(0xC000);
        cpu.registers_mut().a = 0x5A;
        let mut bus = bus_with_program(&[0x77, 0x46]);

        assert_eq!(cpu.step(&mut bus), Ok(8));
        cpu.registers_mut().b = 0;

        assert_eq!(cpu.step(&mut bus), Ok(8));
        assert_eq!(cpu.registers().b, 0x5A);
    }

    #[test]
    fn accumulator_loads_through_bc_de_and_absolute_addresses() {
        let mut cpu = Cpu::default();
        cpu.registers_mut().a = 0x9A;
        cpu.registers_mut().set_bc(0xC000);
        cpu.registers_mut().set_de(0xC001);
        let mut bus = bus_with_program(&[
            0x02, 0x12, 0xEA, 0x02, 0xC0, 0x3E, 0x00, 0x0A, 0x3E, 0x00, 0x1A, 0x3E, 0x00, 0xFA,
            0x02, 0xC0,
        ]);

        assert_eq!(cpu.step(&mut bus), Ok(8));
        assert_eq!(cpu.step(&mut bus), Ok(8));
        assert_eq!(cpu.step(&mut bus), Ok(16));

        assert_eq!(cpu.step(&mut bus), Ok(8));
        assert_eq!(cpu.step(&mut bus), Ok(8));
        assert_eq!(cpu.registers().a, 0x9A);

        assert_eq!(cpu.step(&mut bus), Ok(8));
        assert_eq!(cpu.step(&mut bus), Ok(8));
        assert_eq!(cpu.registers().a, 0x9A);

        assert_eq!(cpu.step(&mut bus), Ok(8));
        assert_eq!(cpu.step(&mut bus), Ok(16));
        assert_eq!(cpu.registers().a, 0x9A);
    }

    #[test]
    fn high_memory_accumulator_loads_use_ff00_base() {
        let mut cpu = Cpu::default();
        cpu.registers_mut().a = 0xAB;
        cpu.registers_mut().c = 0x42;
        let mut bus =
            bus_with_program(&[0xE0, 0x40, 0xE2, 0x3E, 0x00, 0xF0, 0x40, 0x3E, 0x00, 0xF2]);

        assert_eq!(cpu.step(&mut bus), Ok(12));
        assert_eq!(cpu.step(&mut bus), Ok(8));

        assert_eq!(cpu.step(&mut bus), Ok(8));
        assert_eq!(cpu.step(&mut bus), Ok(12));
        assert_eq!(cpu.registers().a, 0xAB);

        assert_eq!(cpu.step(&mut bus), Ok(8));
        assert_eq!(cpu.step(&mut bus), Ok(8));
        assert_eq!(cpu.registers().a, 0xAB);
    }

    #[test]
    fn hl_auto_increment_and_decrement_loads_update_hl() {
        let mut cpu = Cpu::default();
        cpu.registers_mut().a = 0x22;
        cpu.registers_mut().set_hl(0xC000);
        let mut bus = bus_with_program(&[0x22, 0x3E, 0x00, 0x2A, 0x32, 0x3E, 0x00, 0x3A]);

        assert_eq!(cpu.step(&mut bus), Ok(8));
        assert_eq!(cpu.registers().hl(), 0xC001);

        assert_eq!(cpu.step(&mut bus), Ok(8));
        assert_eq!(cpu.step(&mut bus), Ok(8));
        assert_eq!(cpu.registers().a, 0);
        assert_eq!(cpu.registers().hl(), 0xC002);

        cpu.registers_mut().a = 0x33;
        cpu.registers_mut().set_hl(0xC010);
        assert_eq!(cpu.step(&mut bus), Ok(8));
        assert_eq!(cpu.registers().hl(), 0xC00F);

        assert_eq!(cpu.step(&mut bus), Ok(8));
        assert_eq!(cpu.step(&mut bus), Ok(8));
        assert_eq!(cpu.registers().a, 0);
        assert_eq!(cpu.registers().hl(), 0xC00E);
    }

    #[test]
    fn store_sp_to_absolute_address_writes_little_endian_bytes() {
        let mut cpu = Cpu::default();
        cpu.registers_mut().sp = 0xBEEF;
        let mut bus = bus_with_program(&[0x08, 0x00, 0xC0]);

        assert_eq!(cpu.step(&mut bus), Ok(20));
        assert_eq!(bus.read_byte(0xC000), Ok(0xEF));
        assert_eq!(bus.read_byte(0xC001), Ok(0xBE));
    }

    #[test]
    fn halt_enters_halted_mode_and_subsequent_steps_do_not_fetch() {
        let mut cpu = Cpu::default();
        let mut bus = bus_with_program(&[0x76, 0xFF]);

        assert_eq!(cpu.step(&mut bus), Ok(4));
        assert_eq!(cpu.mode(), CpuMode::Halted);
        assert_eq!(cpu.registers().pc, 1);
        assert_eq!(cpu.step(&mut bus), Ok(4));
        assert_eq!(cpu.registers().pc, 1);
    }

    #[test]
    fn halt_bug_repeats_next_opcode_fetch_when_interrupt_is_pending_without_ime() {
        let mut cpu = Cpu::default();
        let mut bus = bus_with_program(&[0x76, 0x3C]);
        bus.write_byte(0xFFFF, 1 << INTERRUPT_JOYPAD)
            .expect("enable joypad interrupt");
        bus.request_interrupt(INTERRUPT_JOYPAD);

        assert_eq!(cpu.step(&mut bus), Ok(4));
        assert_eq!(cpu.mode(), CpuMode::Running);
        assert_eq!(cpu.registers().pc, 1);

        assert_eq!(cpu.step(&mut bus), Ok(4));
        assert_eq!(cpu.registers().a, 1);
        assert_eq!(cpu.registers().pc, 1);

        assert_eq!(cpu.step(&mut bus), Ok(4));
        assert_eq!(cpu.registers().a, 2);
        assert_eq!(cpu.registers().pc, 2);
    }

    #[test]
    fn ei_enables_interrupts_after_following_instruction() {
        let mut cpu = Cpu::default();
        let mut bus = bus_with_program(&[0xFB, 0x00]);

        assert_eq!(cpu.step(&mut bus), Ok(4));
        assert!(!cpu.interrupt_master_enabled());
        assert_eq!(cpu.step(&mut bus), Ok(4));
        assert!(cpu.interrupt_master_enabled());
    }

    #[test]
    fn di_disables_interrupts_and_cancels_pending_ei() {
        let mut cpu = Cpu::default();
        let mut bus = bus_with_program(&[0xFB, 0xF3, 0x00]);

        assert_eq!(cpu.step(&mut bus), Ok(4));
        assert_eq!(cpu.step(&mut bus), Ok(4));
        assert_eq!(cpu.step(&mut bus), Ok(4));
        assert!(!cpu.interrupt_master_enabled());
    }

    #[test]
    fn enabled_interrupt_pushes_pc_and_jumps_to_priority_vector() {
        let mut cpu = Cpu::default();
        cpu.registers_mut().pc = 0x1234;
        cpu.registers_mut().sp = 0xC010;
        cpu.interrupt_master_enabled = true;
        let mut bus = bus_with_program(&[0x00]);
        bus.write_byte(0xFFFF, (1 << INTERRUPT_TIMER) | (1 << INTERRUPT_VBLANK))
            .expect("write ie");
        bus.request_interrupt(INTERRUPT_TIMER);
        bus.request_interrupt(INTERRUPT_VBLANK);

        assert_eq!(cpu.step(&mut bus), Ok(20));
        assert_eq!(cpu.registers().pc, 0x0040);
        assert_eq!(cpu.registers().sp, 0xC00E);
        assert_eq!(bus.read_byte(0xC00E), Ok(0x34));
        assert_eq!(bus.read_byte(0xC00F), Ok(0x12));
        assert_eq!(bus.read_byte(0xFF0F), Ok(0xE4));
        assert!(!cpu.interrupt_master_enabled());
    }

    #[test]
    fn pending_interrupt_wakes_halt_without_ime() {
        let mut cpu = Cpu::default();
        let mut bus = bus_with_program(&[0x76, 0x00]);
        bus.write_byte(0xFFFF, 1 << INTERRUPT_JOYPAD)
            .expect("write ie");

        assert_eq!(cpu.step(&mut bus), Ok(4));
        assert_eq!(cpu.mode(), CpuMode::Halted);

        bus.request_interrupt(INTERRUPT_JOYPAD);
        assert_eq!(cpu.step(&mut bus), Ok(4));
        assert_eq!(cpu.mode(), CpuMode::Running);
        assert_eq!(cpu.registers().pc, 2);
    }

    #[test]
    fn ei_allows_interrupt_service_on_following_step() {
        let mut cpu = Cpu::default();
        cpu.registers_mut().sp = 0xC010;
        let mut bus = bus_with_program(&[0xFB, 0x00]);
        bus.write_byte(0xFFFF, 1 << INTERRUPT_TIMER)
            .expect("write ie");
        bus.request_interrupt(INTERRUPT_TIMER);

        assert_eq!(cpu.step(&mut bus), Ok(4));
        assert!(!cpu.interrupt_master_enabled());

        assert_eq!(cpu.step(&mut bus), Ok(4));
        assert!(cpu.interrupt_master_enabled());

        assert_eq!(cpu.step(&mut bus), Ok(20));
        assert_eq!(cpu.registers().pc, 0x0050);
    }
}
