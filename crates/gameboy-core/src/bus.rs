use crate::{
    apu::{Apu, AudioSample},
    cartridge::Cartridge,
    error::{EmulatorError, Result},
    joypad::JoypadState,
    memory::MemoryRegion,
    ppu::{Ppu, PpuMode},
    serial::Serial,
};

const VRAM_START: u16 = 0x8000;
const VRAM_END: u16 = 0x9FFF;
const ERAM_START: u16 = 0xA000;
const ERAM_END: u16 = 0xBFFF;
const WRAM_START: u16 = 0xC000;
const WRAM_END: u16 = 0xDFFF;
const ECHO_RAM_START: u16 = 0xE000;
const ECHO_RAM_END: u16 = 0xFDFF;
const OAM_START: u16 = 0xFE00;
const OAM_END: u16 = 0xFE9F;
const UNUSABLE_START: u16 = 0xFEA0;
const UNUSABLE_END: u16 = 0xFEFF;
const IO_START: u16 = 0xFF00;
const IO_END: u16 = 0xFF7F;
const JOYPAD: u16 = 0xFF00;
const SERIAL_DATA: u16 = 0xFF01;
const SERIAL_CONTROL: u16 = 0xFF02;
const DIV: u16 = 0xFF04;
const TIMA: u16 = 0xFF05;
const TMA: u16 = 0xFF06;
const TAC: u16 = 0xFF07;
const INTERRUPT_FLAG: u16 = 0xFF0F;
const DMA: u16 = 0xFF46;
const KEY1: u16 = 0xFF4D;
const VBK: u16 = 0xFF4F;
const HDMA1: u16 = 0xFF51;
const HDMA2: u16 = 0xFF52;
const HDMA3: u16 = 0xFF53;
const HDMA4: u16 = 0xFF54;
const HDMA5: u16 = 0xFF55;
const SVBK: u16 = 0xFF70;
const HRAM_START: u16 = 0xFF80;
const HRAM_END: u16 = 0xFFFE;
const INTERRUPT_ENABLE: u16 = 0xFFFF;
const OAM_DMA_BYTES: u16 = 0xA0;
const OAM_DMA_STARTUP_CYCLES: u32 = 4;
const OAM_DMA_BYTE_CYCLES: u32 = 4;

pub const INTERRUPT_VBLANK: u8 = 0;
pub const INTERRUPT_LCD_STAT: u8 = 1;
pub const INTERRUPT_TIMER: u8 = 2;
pub const INTERRUPT_SERIAL: u8 = 3;
pub const INTERRUPT_JOYPAD: u8 = 4;

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct Timer {
    div_counter: u16,
    tima: u8,
    tma: u8,
    tac: u8,
    tima_cycles: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct Joypad {
    select: u8,
    state: JoypadState,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct CgbState {
    enabled: bool,
    double_speed: bool,
    prepare_speed_switch: bool,
    svbk: u8,
    div_remainder: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct HdmaState {
    source: u16,
    dest: u16,
    remaining_blocks: u8,
    hblank_mode: bool,
    active: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct DmaState {
    source_high: u8,
    active: bool,
    startup_cycles: u32,
    byte_cycles: u32,
    offset: u16,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Bus {
    cartridge: Option<Cartridge>,
    eram: MemoryRegion<0x2000>,
    wram: [MemoryRegion<0x1000>; 8],
    io: MemoryRegion<0x80>,
    hram: MemoryRegion<0x7F>,
    interrupt_enable: u8,
    timer: Timer,
    joypad: Joypad,
    serial: Serial,
    cgb: CgbState,
    hdma: HdmaState,
    apu: Apu,
    dma: DmaState,
    ppu: Ppu,
    prev_ppu_mode: PpuMode,
}

impl Bus {
    pub fn insert_cartridge(&mut self, cartridge: Cartridge) {
        self.cartridge = Some(cartridge);
    }

    pub fn cartridge(&self) -> Option<&Cartridge> {
        self.cartridge.as_ref()
    }

    pub fn cartridge_mut(&mut self) -> Option<&mut Cartridge> {
        self.cartridge.as_mut()
    }

    pub fn advance_cycles(&mut self, cycles: u32) {
        if let Some(cartridge) = self.cartridge.as_mut() {
            cartridge.advance_cycles(cycles);
        }

        let display_cycles = if self.cgb.double_speed {
            let total = cycles + self.cgb.div_remainder;
            self.cgb.div_remainder = total & 1;
            total / 2
        } else {
            cycles
        };

        self.apu.advance_cycles(display_cycles);
        self.advance_dma(cycles);

        let fs_bit = if self.cgb.double_speed { 13 } else { 12 };
        let period = 1u32 << (fs_bit + 1);
        let start = self.timer.div_counter as u32;
        let end = start + cycles;
        let fs_edges = end / period - start / period;
        self.timer.div_counter = (end & 0xFFFF) as u16;
        for _ in 0..fs_edges {
            self.apu.step_frame_sequencer();
        }

        if self.timer.tac & 0b100 != 0 {
            self.timer.tima_cycles += cycles;
            let period = timer_period(self.timer.tac);
            while self.timer.tima_cycles >= period {
                self.timer.tima_cycles -= period;
                let (next, overflowed) = self.timer.tima.overflowing_add(1);
                if overflowed {
                    self.timer.tima = self.timer.tma;
                    self.request_interrupt(INTERRUPT_TIMER);
                } else {
                    self.timer.tima = next;
                }
            }
        }

        let ppu_interrupts = self.ppu.advance_cycles(display_cycles);
        if ppu_interrupts.vblank {
            self.request_interrupt(INTERRUPT_VBLANK);
        }
        if ppu_interrupts.stat {
            self.request_interrupt(INTERRUPT_LCD_STAT);
        }

        let mode_after = self.ppu.mode();
        if self.hdma.active
            && self.hdma.hblank_mode
            && self.prev_ppu_mode != PpuMode::HBlank
            && mode_after == PpuMode::HBlank
        {
            self.hdma_transfer_block();
        }
        self.prev_ppu_mode = mode_after;
    }

    pub fn request_interrupt(&mut self, bit: u8) {
        let interrupt_flag = self.interrupt_flag() | (1 << bit);
        self.set_interrupt_flag(interrupt_flag);
    }

    pub fn pending_interrupts(&self) -> u8 {
        self.interrupt_enable & self.interrupt_flag() & 0x1F
    }

    pub fn clear_interrupt(&mut self, bit: u8) {
        let interrupt_flag = self.interrupt_flag() & !(1 << bit);
        self.set_interrupt_flag(interrupt_flag);
    }

    pub fn ppu_mode(&self) -> PpuMode {
        self.ppu.mode()
    }

    pub fn framebuffer(&self) -> &[u32] {
        self.ppu.framebuffer()
    }

    pub fn take_frame_ready(&mut self) -> bool {
        self.ppu.take_frame_ready()
    }

    pub fn set_joypad_state(&mut self, state: JoypadState) {
        let old_value = self.joypad.read();
        self.joypad.set_state(state);
        let new_value = self.joypad.read();

        if old_value & !new_value & 0x0F != 0 {
            self.request_interrupt(INTERRUPT_JOYPAD);
        }
    }

    pub fn joypad_state(&self) -> JoypadState {
        self.joypad.state()
    }

    pub fn take_serial_output(&mut self) -> Vec<u8> {
        self.serial.drain_output()
    }

    pub fn take_audio_samples(&mut self) -> Vec<AudioSample> {
        self.apu.drain_samples()
    }

    pub fn set_cgb_mode(&mut self, enabled: bool) {
        self.cgb.enabled = enabled;
        self.cgb.double_speed = false;
        self.cgb.prepare_speed_switch = false;
        self.cgb.svbk = 0;
        self.cgb.div_remainder = 0;
        self.ppu.set_cgb(enabled);
        self.apu.set_cgb(enabled);
    }

    pub fn stop(&mut self) -> bool {
        if self.cgb.enabled && self.cgb.prepare_speed_switch {
            self.cgb.double_speed = !self.cgb.double_speed;
            self.cgb.prepare_speed_switch = false;
            true
        } else {
            false
        }
    }

    fn wram_slot(&self, offset: usize) -> (usize, usize) {
        if offset < 0x1000 {
            (0, offset)
        } else {
            let bank = (self.cgb.svbk as usize & 0x07).max(1);
            (bank, offset - 0x1000)
        }
    }

    #[allow(clippy::match_overlapping_arm)]
    pub fn read_byte(&self, address: u16) -> Result<u8> {
        if self.dma_blocks_cpu_access(address) {
            return Ok(0xFF);
        }

        match address {
            0x0000..=0x7FFF => self
                .cartridge
                .as_ref()
                .and_then(|cart| cart.read_rom(address))
                .ok_or(EmulatorError::InvalidMemoryAccess { address }),
            VRAM_START..=VRAM_END => self
                .ppu
                .read_vram(address - VRAM_START)
                .ok_or(EmulatorError::InvalidMemoryAccess { address }),
            ERAM_START..=ERAM_END => self
                .cartridge
                .as_ref()
                .and_then(|cart| cart.read_ram(address - ERAM_START))
                .ok_or(EmulatorError::InvalidMemoryAccess { address }),
            WRAM_START..=WRAM_END => {
                let (bank, offset) = self.wram_slot((address - WRAM_START) as usize);
                self.wram[bank]
                    .read(offset)
                    .ok_or(EmulatorError::InvalidMemoryAccess { address })
            }
            ECHO_RAM_START..=ECHO_RAM_END => {
                let (bank, offset) = self.wram_slot((address - ECHO_RAM_START) as usize);
                self.wram[bank]
                    .read(offset)
                    .ok_or(EmulatorError::InvalidMemoryAccess { address })
            }
            OAM_START..=OAM_END => self
                .ppu
                .read_oam(address - OAM_START)
                .ok_or(EmulatorError::InvalidMemoryAccess { address }),
            UNUSABLE_START..=UNUSABLE_END => Ok(0xFF),
            DIV => Ok((self.timer.div_counter >> 8) as u8),
            TIMA => Ok(self.timer.tima),
            TMA => Ok(self.timer.tma),
            TAC => Ok(self.timer.tac | 0xF8),
            JOYPAD => Ok(self.joypad.read()),
            SERIAL_DATA => Ok(self.serial.read_data()),
            SERIAL_CONTROL => Ok(self.serial.read_control()),
            0xFF10..=0xFF3F => self
                .apu
                .read_register(address)
                .or_else(|| self.io.read((address - IO_START) as usize))
                .ok_or(EmulatorError::InvalidMemoryAccess { address }),
            INTERRUPT_FLAG => Ok(self.interrupt_flag() | 0xE0),
            KEY1 => Ok(self.cgb.read_key1()),
            0xFF40..=0xFF4B | VBK | 0xFF68..=0xFF6B => self
                .ppu
                .read_register(address)
                .or_else(|| self.io.read((address - IO_START) as usize))
                .ok_or(EmulatorError::InvalidMemoryAccess { address }),
            HDMA1..=HDMA5 => Ok(self.read_hdma(address)),
            SVBK => Ok(if self.cgb.enabled {
                0xF8 | (self.cgb.svbk & 0x07)
            } else {
                0xFF
            }),
            IO_START..=IO_END => self
                .io
                .read((address - IO_START) as usize)
                .ok_or(EmulatorError::InvalidMemoryAccess { address }),
            HRAM_START..=HRAM_END => self
                .hram
                .read((address - HRAM_START) as usize)
                .ok_or(EmulatorError::InvalidMemoryAccess { address }),
            INTERRUPT_ENABLE => Ok(self.interrupt_enable),
        }
    }

    #[allow(clippy::match_overlapping_arm)]
    pub fn write_byte(&mut self, address: u16, value: u8) -> Result<()> {
        if self.dma_blocks_cpu_access(address) {
            return Ok(());
        }

        let wrote = match address {
            0x0000..=0x7FFF => {
                if let Some(cartridge) = self.cartridge.as_mut() {
                    cartridge.write_rom(address, value);
                    true
                } else {
                    false
                }
            }
            VRAM_START..=VRAM_END => self.ppu.write_vram(address - VRAM_START, value),
            ERAM_START..=ERAM_END => {
                if let Some(cartridge) = self.cartridge.as_mut() {
                    cartridge.write_ram(address - ERAM_START, value);
                    true
                } else {
                    false
                }
            }
            WRAM_START..=WRAM_END => {
                let (bank, offset) = self.wram_slot((address - WRAM_START) as usize);
                self.wram[bank].write(offset, value)
            }
            ECHO_RAM_START..=ECHO_RAM_END => {
                let (bank, offset) = self.wram_slot((address - ECHO_RAM_START) as usize);
                self.wram[bank].write(offset, value)
            }
            OAM_START..=OAM_END => self.ppu.write_oam(address - OAM_START, value),
            UNUSABLE_START..=UNUSABLE_END => true,
            DIV => {
                let fs_bit = if self.cgb.double_speed { 13 } else { 12 };
                if self.timer.div_counter & (1 << fs_bit) != 0 {
                    self.apu.step_frame_sequencer();
                }
                self.timer.div_counter = 0;
                true
            }
            TIMA => {
                self.timer.tima = value;
                true
            }
            TMA => {
                self.timer.tma = value;
                true
            }
            TAC => {
                self.timer.tac = value & 0b111;
                self.timer.tima_cycles = 0;
                true
            }
            JOYPAD => {
                self.joypad.write(value);
                true
            }
            SERIAL_DATA => {
                self.serial.write_data(value);
                true
            }
            SERIAL_CONTROL => {
                if self.serial.write_control(value) {
                    self.request_interrupt(INTERRUPT_SERIAL);
                }
                true
            }
            INTERRUPT_FLAG => self
                .io
                .write((INTERRUPT_FLAG - IO_START) as usize, value & 0x1F),
            0xFF10..=0xFF3F => {
                let _ = self.io.write((address - IO_START) as usize, value);
                self.apu.write_register(address, value)
            }
            DMA => {
                let wrote = self.io.write((DMA - IO_START) as usize, value);
                self.start_dma(value);
                wrote
            }
            0xFF40..=0xFF4B | VBK | 0xFF68..=0xFF6B => {
                if self.ppu.write_register(address, value) {
                    true
                } else {
                    self.io.write((address - IO_START) as usize, value)
                }
            }
            KEY1 => {
                self.cgb.write_key1(value);
                true
            }
            HDMA1..=HDMA5 => {
                self.write_hdma(address, value);
                true
            }
            SVBK => {
                if self.cgb.enabled {
                    self.cgb.svbk = value & 0x07;
                }
                true
            }
            IO_START..=IO_END => self.io.write((address - IO_START) as usize, value),
            HRAM_START..=HRAM_END => self.hram.write((address - HRAM_START) as usize, value),
            INTERRUPT_ENABLE => {
                self.interrupt_enable = value;
                true
            }
        };

        if wrote {
            Ok(())
        } else {
            Err(EmulatorError::InvalidMemoryAccess { address })
        }
    }

    fn interrupt_flag(&self) -> u8 {
        self.io
            .read((INTERRUPT_FLAG - IO_START) as usize)
            .unwrap_or(0)
            & 0x1F
    }

    fn set_interrupt_flag(&mut self, value: u8) {
        let _ = self
            .io
            .write((INTERRUPT_FLAG - IO_START) as usize, value & 0x1F);
    }

    fn start_dma(&mut self, source_high: u8) {
        self.dma = DmaState {
            source_high,
            active: true,
            startup_cycles: OAM_DMA_STARTUP_CYCLES,
            byte_cycles: 0,
            offset: 0,
        };
    }

    fn advance_dma(&mut self, cycles: u32) {
        if !self.dma.active {
            return;
        }

        let mut remaining = cycles;
        if self.dma.startup_cycles > 0 {
            let elapsed = self.dma.startup_cycles.min(remaining);
            self.dma.startup_cycles -= elapsed;
            remaining -= elapsed;
        }

        self.dma.byte_cycles += remaining;
        while self.dma.active
            && self.dma.startup_cycles == 0
            && self.dma.byte_cycles >= OAM_DMA_BYTE_CYCLES
        {
            self.dma.byte_cycles -= OAM_DMA_BYTE_CYCLES;
            self.dma_copy_next_byte();
        }
    }

    fn dma_copy_next_byte(&mut self) {
        let source = ((self.dma.source_high as u16) << 8).wrapping_add(self.dma.offset);
        let value = self.dma_read_byte(source);
        self.ppu.write_oam_raw(self.dma.offset, value);
        self.dma.offset += 1;

        if self.dma.offset >= OAM_DMA_BYTES {
            self.dma.active = false;
            self.dma.byte_cycles = 0;
        }
    }

    fn write_hdma(&mut self, address: u16, value: u8) {
        match address {
            HDMA1 => self.hdma.source = (self.hdma.source & 0x00FF) | ((value as u16) << 8),
            HDMA2 => self.hdma.source = (self.hdma.source & 0xFF00) | (value as u16 & 0xF0),
            HDMA3 => {
                self.hdma.dest = 0x8000 | ((value as u16 & 0x1F) << 8) | (self.hdma.dest & 0x00F0)
            }
            HDMA4 => self.hdma.dest = (self.hdma.dest & 0xFF00) | (value as u16 & 0xF0),
            HDMA5 => self.start_hdma(value),
            _ => {}
        }
    }

    fn read_hdma(&self, address: u16) -> u8 {
        if address == HDMA5 && self.hdma.active && self.hdma.hblank_mode {
            self.hdma.remaining_blocks.wrapping_sub(1) & 0x7F
        } else {
            0xFF
        }
    }

    fn start_hdma(&mut self, value: u8) {
        let blocks = (value & 0x7F) + 1;

        if value & 0x80 != 0 {
            self.hdma.remaining_blocks = blocks;
            self.hdma.hblank_mode = true;
            self.hdma.active = true;
        } else if self.hdma.active && self.hdma.hblank_mode {
            self.hdma.active = false;
            self.hdma.hblank_mode = false;
        } else {
            self.hdma.remaining_blocks = blocks;
            self.hdma.hblank_mode = false;
            self.hdma.active = true;
            while self.hdma.active {
                self.hdma_transfer_block();
            }
        }
    }

    fn hdma_transfer_block(&mut self) {
        let bank = self.ppu.vram_bank() as usize;
        for i in 0..0x10 {
            let byte = self.dma_read_byte(self.hdma.source.wrapping_add(i));
            let dest = self.hdma.dest.wrapping_add(i) & 0x1FFF;
            self.ppu.write_vram_bank_raw(bank, dest, byte);
        }

        self.hdma.source = self.hdma.source.wrapping_add(0x10);
        self.hdma.dest = self.hdma.dest.wrapping_add(0x10);
        self.hdma.remaining_blocks = self.hdma.remaining_blocks.saturating_sub(1);
        if self.hdma.remaining_blocks == 0 {
            self.hdma.active = false;
            self.hdma.hblank_mode = false;
        }
    }

    fn dma_blocks_cpu_access(&self, address: u16) -> bool {
        self.dma.active && self.dma.startup_cycles == 0 && !matches!(address, HRAM_START..=HRAM_END)
    }

    #[allow(clippy::match_overlapping_arm)]
    fn dma_read_byte(&self, address: u16) -> u8 {
        match address {
            0x0000..=0x7FFF => self
                .cartridge
                .as_ref()
                .and_then(|cart| cart.read_rom(address))
                .unwrap_or(0xFF),
            VRAM_START..=VRAM_END => self.ppu.read_vram_raw(address - VRAM_START).unwrap_or(0xFF),
            ERAM_START..=ERAM_END => self
                .cartridge
                .as_ref()
                .and_then(|cart| cart.read_ram(address - ERAM_START))
                .unwrap_or(0xFF),
            WRAM_START..=WRAM_END => {
                let (bank, offset) = self.wram_slot((address - WRAM_START) as usize);
                self.wram[bank].read(offset).unwrap_or(0xFF)
            }
            ECHO_RAM_START..=ECHO_RAM_END => {
                let (bank, offset) = self.wram_slot((address - ECHO_RAM_START) as usize);
                self.wram[bank].read(offset).unwrap_or(0xFF)
            }
            OAM_START..=OAM_END => self.ppu.read_oam_raw(address - OAM_START).unwrap_or(0xFF),
            UNUSABLE_START..=UNUSABLE_END => 0xFF,
            DIV => (self.timer.div_counter >> 8) as u8,
            TIMA => self.timer.tima,
            TMA => self.timer.tma,
            TAC => self.timer.tac | 0xF8,
            JOYPAD => self.joypad.read(),
            SERIAL_DATA => self.serial.read_data(),
            SERIAL_CONTROL => self.serial.read_control(),
            0xFF10..=0xFF3F => self
                .apu
                .read_register(address)
                .or_else(|| self.io.read((address - IO_START) as usize))
                .unwrap_or(0xFF),
            INTERRUPT_FLAG => self.interrupt_flag() | 0xE0,
            KEY1 => self.cgb.read_key1(),
            0xFF40..=0xFF4B => self
                .ppu
                .read_register(address)
                .or_else(|| self.io.read((address - IO_START) as usize))
                .unwrap_or(0xFF),
            IO_START..=IO_END => self.io.read((address - IO_START) as usize).unwrap_or(0xFF),
            HRAM_START..=HRAM_END => self
                .hram
                .read((address - HRAM_START) as usize)
                .unwrap_or(0xFF),
            INTERRUPT_ENABLE => self.interrupt_enable,
        }
    }
}

impl Default for Joypad {
    fn default() -> Self {
        Self {
            select: 0x30,
            state: JoypadState::default(),
        }
    }
}

impl Joypad {
    fn read(&self) -> u8 {
        let mut lower = 0x0F;

        if self.select & 0x10 == 0 {
            lower &= self.state.direction_nibble();
        }
        if self.select & 0x20 == 0 {
            lower &= self.state.action_nibble();
        }

        0xC0 | self.select | lower
    }

    fn write(&mut self, value: u8) {
        self.select = value & 0x30;
    }

    fn set_state(&mut self, state: JoypadState) {
        self.state = state;
    }

    fn state(&self) -> JoypadState {
        self.state
    }
}

impl CgbState {
    fn read_key1(&self) -> u8 {
        if !self.enabled {
            return 0xFF;
        }

        0x7E | (u8::from(self.double_speed) << 7) | u8::from(self.prepare_speed_switch)
    }

    fn write_key1(&mut self, value: u8) {
        if self.enabled {
            self.prepare_speed_switch = value & 0x01 != 0;
        }
    }
}

fn timer_period(tac: u8) -> u32 {
    match tac & 0b11 {
        0b00 => 1024,
        0b01 => 16,
        0b10 => 64,
        0b11 => 256,
        _ => unreachable!("timer frequency uses two bits"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        cartridge::synthetic_rom,
        joypad::{JoypadButton, JoypadState},
    };

    #[test]
    fn reads_from_cartridge_rom() {
        let mut bus = Bus::default();
        let cartridge =
            Cartridge::from_bytes(synthetic_rom("TEST", &[(0, &[0x42])])).expect("valid ROM");
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
    fn echo_ram_mirrors_work_ram() {
        let mut bus = Bus::default();

        bus.write_byte(0xC123, 0x77).expect("write wram");
        assert_eq!(bus.read_byte(0xE123), Ok(0x77));

        bus.write_byte(0xE124, 0x88).expect("write echo");
        assert_eq!(bus.read_byte(0xC124), Ok(0x88));
    }

    #[test]
    fn reads_and_writes_oam_io_hram_and_interrupt_enable() {
        let mut bus = Bus::default();

        bus.write_byte(0xFF40, 0x00).expect("disable lcd");
        bus.write_byte(0xFE00, 0x12).expect("write oam");
        bus.write_byte(0xFF03, 0x34).expect("write io");
        bus.write_byte(0xFF80, 0x56).expect("write hram");
        bus.write_byte(0xFFFF, 0x1F).expect("write ie");

        assert_eq!(bus.read_byte(0xFE00), Ok(0x12));
        assert_eq!(bus.read_byte(0xFF03), Ok(0x34));
        assert_eq!(bus.read_byte(0xFF80), Ok(0x56));
        assert_eq!(bus.read_byte(0xFFFF), Ok(0x1F));
    }

    #[test]
    fn joypad_register_reads_selected_active_low_button_group() {
        let mut bus = Bus::default();
        let state = JoypadState::new()
            .with(JoypadButton::Right, true)
            .with(JoypadButton::Left, true)
            .with(JoypadButton::A, true)
            .with(JoypadButton::Start, true);

        bus.set_joypad_state(state);
        assert_eq!(bus.read_byte(JOYPAD), Ok(0xFF));

        bus.write_byte(JOYPAD, 0x20).expect("select directions");
        assert_eq!(bus.read_byte(JOYPAD), Ok(0xEC));

        bus.write_byte(JOYPAD, 0x10).expect("select actions");
        assert_eq!(bus.read_byte(JOYPAD), Ok(0xD6));

        bus.write_byte(JOYPAD, 0x00).expect("select both");
        assert_eq!(bus.read_byte(JOYPAD), Ok(0xC4));
    }

    #[test]
    fn joypad_newly_pressed_selected_button_requests_interrupt() {
        let mut bus = Bus::default();
        bus.write_byte(JOYPAD, 0x20).expect("select directions");

        bus.set_joypad_state(JoypadState::new().with(JoypadButton::Right, true));
        assert_eq!(bus.read_byte(INTERRUPT_FLAG), Ok(0xF0));

        bus.write_byte(INTERRUPT_FLAG, 0).expect("clear if");
        bus.set_joypad_state(JoypadState::new().with(JoypadButton::Right, true));
        assert_eq!(bus.read_byte(INTERRUPT_FLAG), Ok(0xE0));
    }

    #[test]
    fn joypad_pressed_unselected_button_does_not_request_interrupt_until_selected_press_changes() {
        let mut bus = Bus::default();
        bus.write_byte(JOYPAD, 0x20).expect("select directions");

        bus.set_joypad_state(JoypadState::new().with(JoypadButton::A, true));
        assert_eq!(bus.read_byte(INTERRUPT_FLAG), Ok(0xE0));

        bus.set_joypad_state(
            JoypadState::new()
                .with(JoypadButton::A, true)
                .with(JoypadButton::Up, true),
        );
        assert_eq!(bus.read_byte(INTERRUPT_FLAG), Ok(0xF0));
    }

    #[test]
    fn serial_internal_clock_transfer_logs_byte_and_requests_interrupt() {
        let mut bus = Bus::default();

        bus.write_byte(SERIAL_DATA, b'A').expect("write sb");
        bus.write_byte(SERIAL_CONTROL, 0x81).expect("start serial");

        assert_eq!(bus.read_byte(SERIAL_DATA), Ok(b'A'));
        assert_eq!(bus.read_byte(SERIAL_CONTROL), Ok(0x7F));
        assert_eq!(bus.read_byte(INTERRUPT_FLAG), Ok(0xE8));
        assert_eq!(bus.take_serial_output(), b"A".to_vec());
        assert!(bus.take_serial_output().is_empty());
    }

    #[test]
    fn serial_external_clock_transfer_does_not_log_without_peer() {
        let mut bus = Bus::default();

        bus.write_byte(SERIAL_DATA, b'B').expect("write sb");
        bus.write_byte(SERIAL_CONTROL, 0x80)
            .expect("start external serial");

        assert_eq!(bus.read_byte(SERIAL_CONTROL), Ok(0xFE));
        assert_eq!(bus.read_byte(INTERRUPT_FLAG), Ok(0xE0));
        assert!(bus.take_serial_output().is_empty());
    }

    #[test]
    fn apu_registers_are_memory_mapped_and_generate_samples() {
        let mut bus = Bus::default();

        assert_eq!(bus.read_byte(0xFF26), Ok(0x70));
        bus.write_byte(0xFF26, 0x80).expect("power apu");
        bus.write_byte(0xFF24, 0x77).expect("route volume");
        bus.write_byte(0xFF25, 0x11).expect("route pulse 1");
        bus.write_byte(0xFF11, 0x80).expect("duty");
        bus.write_byte(0xFF12, 0xF0).expect("envelope");
        bus.write_byte(0xFF13, 0x00).expect("frequency low");
        bus.write_byte(0xFF14, 0x87).expect("trigger");

        assert_eq!(bus.read_byte(0xFF26).unwrap() & 0x81, 0x81);

        bus.advance_cycles(4_194_304 / 120);
        assert!(!bus.take_audio_samples().is_empty());
    }

    #[test]
    fn wave_ram_is_mapped_through_apu() {
        let mut bus = Bus::default();

        bus.write_byte(0xFF30, 0xAB).expect("write wave ram");

        assert_eq!(bus.read_byte(0xFF30), Ok(0xAB));
    }

    #[test]
    fn oam_reads_and_writes_are_routed_to_ppu() {
        let mut bus = Bus::default();

        bus.write_byte(0xFF40, 0x00).expect("disable lcd");
        bus.write_byte(0xFE10, 0xAB).expect("write oam");
        assert_eq!(bus.read_byte(0xFE10), Ok(0xAB));
    }

    #[test]
    fn oam_is_restricted_while_ppu_is_using_it() {
        let mut bus = Bus::default();

        bus.write_byte(0xFE10, 0xAB).expect("write ignored");
        assert_eq!(bus.read_byte(0xFE10), Ok(0xFF));
        bus.advance_cycles(80 + 172);
        bus.write_byte(0xFE10, 0xAB).expect("write hblank");
        assert_eq!(bus.read_byte(0xFE10), Ok(0xAB));
    }

    #[test]
    fn dma_copies_source_page_into_ppu_oam() {
        let mut bus = Bus::default();
        bus.write_byte(0xFF40, 0x00).expect("disable lcd");

        bus.write_byte(0xC000, 0x12).expect("write source");
        bus.write_byte(0xC09F, 0x34).expect("write source");
        bus.write_byte(0xFF46, 0xC0).expect("start dma");

        assert_eq!(bus.read_byte(0xFE00), Ok(0x00));

        bus.advance_cycles(OAM_DMA_STARTUP_CYCLES + OAM_DMA_BYTE_CYCLES);
        assert_eq!(bus.read_byte(0xFE00), Ok(0xFF));
        assert_eq!(bus.dma_read_byte(0xFE00), 0x12);
        assert_eq!(bus.dma_read_byte(0xFE01), 0x00);

        bus.advance_cycles((OAM_DMA_BYTES as u32 - 1) * OAM_DMA_BYTE_CYCLES);
        assert_eq!(bus.read_byte(0xFE00), Ok(0x12));
        assert_eq!(bus.read_byte(0xFE9F), Ok(0x34));
        assert_eq!(bus.read_byte(0xFF46), Ok(0xC0));
    }

    #[test]
    fn oam_dma_blocks_cpu_access_outside_hram_after_startup() {
        let mut bus = Bus::default();
        bus.write_byte(0xC000, 0x12).expect("write wram");
        bus.write_byte(0xFF80, 0x34).expect("write hram");

        bus.write_byte(DMA, 0xC0).expect("start dma");
        assert_eq!(bus.read_byte(0xC000), Ok(0x12));

        bus.advance_cycles(OAM_DMA_STARTUP_CYCLES);
        assert_eq!(bus.read_byte(0xC000), Ok(0xFF));
        assert_eq!(bus.read_byte(INTERRUPT_ENABLE), Ok(0xFF));
        assert_eq!(bus.read_byte(0xFF80), Ok(0x34));

        bus.write_byte(0xC000, 0x56).expect("blocked write");
        assert_eq!(bus.dma_read_byte(0xC000), 0x12);
    }

    #[test]
    fn oam_dma_finishes_after_all_bytes_are_copied() {
        let mut bus = Bus::default();
        bus.write_byte(0xFF40, 0x00).expect("disable lcd");
        bus.write_byte(0xC000, 0x12).expect("write source");
        bus.write_byte(0xC09F, 0x34).expect("write source");

        bus.write_byte(DMA, 0xC0).expect("start dma");
        bus.advance_cycles(OAM_DMA_STARTUP_CYCLES + OAM_DMA_BYTES as u32 * OAM_DMA_BYTE_CYCLES);

        assert_eq!(bus.read_byte(0xC000), Ok(0x12));
        assert_eq!(bus.read_byte(0xFE00), Ok(0x12));
        assert_eq!(bus.read_byte(0xFE9F), Ok(0x34));
    }

    #[test]
    fn timer_registers_have_memory_mapped_behavior() {
        let mut bus = Bus::default();

        bus.write_byte(TIMA, 0x12).expect("write tima");
        bus.write_byte(TMA, 0x34).expect("write tma");
        bus.write_byte(TAC, 0xFF).expect("write tac");

        assert_eq!(bus.read_byte(TIMA), Ok(0x12));
        assert_eq!(bus.read_byte(TMA), Ok(0x34));
        assert_eq!(bus.read_byte(TAC), Ok(0xFF));

        bus.advance_cycles(256);
        assert_eq!(bus.read_byte(DIV), Ok(1));

        bus.write_byte(DIV, 0xFF).expect("reset div");
        assert_eq!(bus.read_byte(DIV), Ok(0));
    }

    #[test]
    fn lcd_registers_are_routed_to_ppu() {
        let mut bus = Bus::default();

        assert_eq!(bus.read_byte(0xFF40), Ok(0x91));
        assert_eq!(bus.read_byte(0xFF44), Ok(0));
        bus.write_byte(0xFF42, 0x12).expect("write scy");
        bus.write_byte(0xFF43, 0x34).expect("write scx");

        assert_eq!(bus.read_byte(0xFF42), Ok(0x12));
        assert_eq!(bus.read_byte(0xFF43), Ok(0x34));
        assert_eq!(bus.ppu_mode(), PpuMode::OamScan);
    }

    #[test]
    fn vram_reads_and_writes_are_routed_to_ppu_framebuffer_renderer() {
        let mut bus = Bus::default();

        bus.write_byte(0x9800, 1).expect("write tile map");
        bus.write_byte(0x8010, 0x80).expect("write tile low byte");
        bus.advance_cycles(456);

        assert_eq!(bus.read_byte(0x9800), Ok(1));
        assert_ne!(bus.framebuffer()[0], bus.framebuffer()[1]);
    }

    #[test]
    fn vram_is_restricted_during_ppu_drawing() {
        let mut bus = Bus::default();

        bus.write_byte(0x8000, 0xAB).expect("write vram");
        bus.advance_cycles(80);

        assert_eq!(bus.ppu_mode(), PpuMode::Drawing);
        assert_eq!(bus.read_byte(0x8000), Ok(0xFF));
        bus.write_byte(0x8000, 0x12).expect("ignored write");
        bus.advance_cycles(172);
        assert_eq!(bus.read_byte(0x8000), Ok(0xAB));
    }

    #[test]
    fn ppu_vblank_and_stat_interrupts_are_requested_from_cycle_advance() {
        let mut bus = Bus::default();
        bus.write_byte(0xFF41, 0x10).expect("enable vblank stat");

        for _ in 0..144 {
            bus.advance_cycles(456);
        }

        assert_eq!(bus.read_byte(0xFF44), Ok(144));
        assert_eq!(bus.ppu_mode(), PpuMode::VBlank);
        assert_eq!(bus.read_byte(INTERRUPT_FLAG), Ok(0xE3));
    }

    #[test]
    fn enabled_timer_overflow_reloads_tma_and_requests_interrupt() {
        let mut bus = Bus::default();

        bus.write_byte(TIMA, 0xFF).expect("write tima");
        bus.write_byte(TMA, 0x42).expect("write tma");
        bus.write_byte(TAC, 0b101).expect("enable timer fast");
        bus.advance_cycles(16);

        assert_eq!(bus.read_byte(TIMA), Ok(0x42));
        assert_eq!(bus.read_byte(INTERRUPT_FLAG), Ok(0xE4));
    }

    #[test]
    fn pending_interrupts_are_masked_by_interrupt_enable() {
        let mut bus = Bus::default();

        bus.request_interrupt(INTERRUPT_TIMER);
        assert_eq!(bus.pending_interrupts(), 0);

        bus.write_byte(INTERRUPT_ENABLE, 1 << INTERRUPT_TIMER)
            .expect("write ie");
        assert_eq!(bus.pending_interrupts(), 1 << INTERRUPT_TIMER);

        bus.clear_interrupt(INTERRUPT_TIMER);
        assert_eq!(bus.pending_interrupts(), 0);
    }

    #[test]
    fn unusable_memory_reads_as_ff_and_ignores_writes() {
        let mut bus = Bus::default();

        assert_eq!(bus.read_byte(0xFEA0), Ok(0xFF));
        bus.write_byte(0xFEA0, 0x00).expect("ignored write");
        assert_eq!(bus.read_byte(0xFEA0), Ok(0xFF));
    }

    #[test]
    fn rejects_rom_writes() {
        let mut bus = Bus::default();

        assert_eq!(
            bus.write_byte(0x0000, 0x12),
            Err(EmulatorError::InvalidMemoryAccess { address: 0x0000 })
        );
    }

    #[test]
    fn cartridge_rom_writes_are_forwarded_to_mapper() {
        let mut bus = Bus::default();
        let mut rom = crate::cartridge::synthetic_rom_with_header("MBC1", 0x01, 0x01, 0x00, &[]);
        rom[0x4000] = 1;
        rom[0x8000] = 2;
        bus.insert_cartridge(Cartridge::from_bytes(rom).expect("valid ROM"));

        assert_eq!(bus.read_byte(0x4000), Ok(1));
        bus.write_byte(0x2000, 0x02).expect("bank switch");
        assert_eq!(bus.read_byte(0x4000), Ok(2));
    }

    #[test]
    fn external_ram_reads_and_writes_are_forwarded_to_cartridge() {
        let mut bus = Bus::default();
        let rom = crate::cartridge::synthetic_rom_with_header("RAM", 0x03, 0x00, 0x02, &[]);
        bus.insert_cartridge(Cartridge::from_bytes(rom).expect("valid ROM"));

        assert_eq!(bus.read_byte(0xA000), Ok(0xFF));
        bus.write_byte(0x0000, 0x0A).expect("enable ram");
        bus.write_byte(0xA000, 0x5A).expect("write ram");
        assert_eq!(bus.read_byte(0xA000), Ok(0x5A));
    }

    #[test]
    fn cgb_wram_banks_switch_with_svbk() {
        let mut bus = Bus::default();
        bus.set_cgb_mode(true);

        bus.write_byte(0xD000, 0x11).expect("write bank 1");
        bus.write_byte(0xFF70, 2).expect("select bank 2");
        bus.write_byte(0xD000, 0x22).expect("write bank 2");
        assert_eq!(bus.read_byte(0xD000), Ok(0x22));

        bus.write_byte(0xFF70, 1).expect("select bank 1");
        assert_eq!(bus.read_byte(0xD000), Ok(0x11));
        assert_eq!(bus.read_byte(0xFF70), Ok(0xF9));

        bus.write_byte(0xC000, 0x33).expect("write bank 0");
        bus.write_byte(0xFF70, 5).expect("select bank 5");
        assert_eq!(bus.read_byte(0xC000), Ok(0x33));
    }

    #[test]
    fn cgb_general_hdma_copies_immediately_to_vram() {
        let mut bus = Bus::default();
        bus.set_cgb_mode(true);
        for i in 0..0x10u16 {
            bus.write_byte(0xC000 + i, (i + 1) as u8)
                .expect("seed wram");
        }

        bus.write_byte(0xFF51, 0xC0).expect("src high");
        bus.write_byte(0xFF52, 0x00).expect("src low");
        bus.write_byte(0xFF53, 0x00).expect("dst high");
        bus.write_byte(0xFF54, 0x00).expect("dst low");
        bus.write_byte(0xFF55, 0x00).expect("start hdma");

        for i in 0..0x10u16 {
            assert_eq!(bus.read_byte(0x8000 + i), Ok((i + 1) as u8));
        }
        assert_eq!(bus.read_byte(0xFF55), Ok(0xFF));
    }

    #[test]
    fn cgb_hblank_hdma_transfers_one_block_per_hblank() {
        let mut bus = Bus::default();
        bus.set_cgb_mode(true);
        for i in 0..0x20u16 {
            bus.write_byte(0xC000 + i, (i + 1) as u8)
                .expect("seed wram");
        }

        bus.write_byte(0xFF51, 0xC0).expect("src high");
        bus.write_byte(0xFF52, 0x00).expect("src low");
        bus.write_byte(0xFF53, 0x00).expect("dst high");
        bus.write_byte(0xFF54, 0x00).expect("dst low");
        bus.write_byte(0xFF55, 0x81).expect("start hblank hdma");

        for _ in 0..(456 * 3 / 4) {
            bus.advance_cycles(4);
        }

        assert_eq!(bus.read_byte(0x8000), Ok(0x01));
        assert_eq!(bus.read_byte(0x8010), Ok(0x11));
        assert_eq!(bus.read_byte(0xFF55), Ok(0xFF));
    }

    #[test]
    fn cgb_double_speed_halves_ppu_progress() {
        let mut single = Bus::default();
        single.set_cgb_mode(true);

        let mut double = Bus::default();
        double.set_cgb_mode(true);
        double.write_byte(0xFF4D, 0x01).expect("arm speed switch");
        assert!(double.stop());
        assert_eq!(double.read_byte(0xFF4D), Ok(0xFE));

        for _ in 0..(912 / 4) {
            single.advance_cycles(4);
            double.advance_cycles(4);
        }

        assert_eq!(single.read_byte(0xFF44), Ok(2));
        assert_eq!(double.read_byte(0xFF44), Ok(1));
        assert_eq!(single.read_byte(0xFF04), double.read_byte(0xFF04));
    }
}
