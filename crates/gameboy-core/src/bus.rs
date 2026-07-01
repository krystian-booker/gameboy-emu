use crate::{
    cartridge::Cartridge,
    error::{EmulatorError, Result},
    memory::MemoryRegion,
    ppu::{Ppu, PpuMode},
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
const DIV: u16 = 0xFF04;
const TIMA: u16 = 0xFF05;
const TMA: u16 = 0xFF06;
const TAC: u16 = 0xFF07;
const INTERRUPT_FLAG: u16 = 0xFF0F;
const DMA: u16 = 0xFF46;
const HRAM_START: u16 = 0xFF80;
const HRAM_END: u16 = 0xFFFE;
const INTERRUPT_ENABLE: u16 = 0xFFFF;

pub const INTERRUPT_VBLANK: u8 = 0;
pub const INTERRUPT_LCD_STAT: u8 = 1;
pub const INTERRUPT_TIMER: u8 = 2;
pub const INTERRUPT_SERIAL: u8 = 3;
pub const INTERRUPT_JOYPAD: u8 = 4;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Timer {
    div: u8,
    div_cycles: u32,
    tima: u8,
    tma: u8,
    tac: u8,
    tima_cycles: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Bus {
    cartridge: Option<Cartridge>,
    eram: MemoryRegion<0x2000>,
    wram: MemoryRegion<0x2000>,
    io: MemoryRegion<0x80>,
    hram: MemoryRegion<0x7F>,
    interrupt_enable: u8,
    timer: Timer,
    ppu: Ppu,
}

impl Bus {
    pub fn insert_cartridge(&mut self, cartridge: Cartridge) {
        self.cartridge = Some(cartridge);
    }

    pub fn cartridge(&self) -> Option<&Cartridge> {
        self.cartridge.as_ref()
    }

    pub fn advance_cycles(&mut self, cycles: u32) {
        self.timer.div_cycles += cycles;
        while self.timer.div_cycles >= 256 {
            self.timer.div_cycles -= 256;
            self.timer.div = self.timer.div.wrapping_add(1);
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

        let ppu_interrupts = self.ppu.advance_cycles(cycles);
        if ppu_interrupts.vblank {
            self.request_interrupt(INTERRUPT_VBLANK);
        }
        if ppu_interrupts.stat {
            self.request_interrupt(INTERRUPT_LCD_STAT);
        }
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

    pub fn read_byte(&self, address: u16) -> Result<u8> {
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
            WRAM_START..=WRAM_END => self
                .wram
                .read((address - WRAM_START) as usize)
                .ok_or(EmulatorError::InvalidMemoryAccess { address }),
            ECHO_RAM_START..=ECHO_RAM_END => self
                .wram
                .read((address - ECHO_RAM_START) as usize)
                .ok_or(EmulatorError::InvalidMemoryAccess { address }),
            OAM_START..=OAM_END => self
                .ppu
                .read_oam(address - OAM_START)
                .ok_or(EmulatorError::InvalidMemoryAccess { address }),
            UNUSABLE_START..=UNUSABLE_END => Ok(0xFF),
            DIV => Ok(self.timer.div),
            TIMA => Ok(self.timer.tima),
            TMA => Ok(self.timer.tma),
            TAC => Ok(self.timer.tac | 0xF8),
            INTERRUPT_FLAG => Ok(self.interrupt_flag() | 0xE0),
            0xFF40..=0xFF4B => self
                .ppu
                .read_register(address)
                .or_else(|| self.io.read((address - IO_START) as usize))
                .ok_or(EmulatorError::InvalidMemoryAccess { address }),
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

    pub fn write_byte(&mut self, address: u16, value: u8) -> Result<()> {
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
            WRAM_START..=WRAM_END => self.wram.write((address - WRAM_START) as usize, value),
            ECHO_RAM_START..=ECHO_RAM_END => {
                self.wram.write((address - ECHO_RAM_START) as usize, value)
            }
            OAM_START..=OAM_END => self.ppu.write_oam(address - OAM_START, value),
            UNUSABLE_START..=UNUSABLE_END => true,
            DIV => {
                self.timer.div = 0;
                self.timer.div_cycles = 0;
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
            INTERRUPT_FLAG => self
                .io
                .write((INTERRUPT_FLAG - IO_START) as usize, value & 0x1F),
            DMA => {
                let wrote = self.io.write((DMA - IO_START) as usize, value);
                self.dma_transfer(value);
                wrote
            }
            0xFF40..=0xFF4B => {
                if self.ppu.write_register(address, value) {
                    true
                } else {
                    self.io.write((address - IO_START) as usize, value)
                }
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

    fn dma_transfer(&mut self, source_high: u8) {
        let source = (source_high as u16) << 8;
        for offset in 0..0xA0 {
            let value = self.dma_read_byte(source + offset);
            self.ppu.write_oam_raw(offset, value);
        }
    }

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
            WRAM_START..=WRAM_END => self
                .wram
                .read((address - WRAM_START) as usize)
                .unwrap_or(0xFF),
            ECHO_RAM_START..=ECHO_RAM_END => self
                .wram
                .read((address - ECHO_RAM_START) as usize)
                .unwrap_or(0xFF),
            OAM_START..=OAM_END => self.ppu.read_oam_raw(address - OAM_START).unwrap_or(0xFF),
            UNUSABLE_START..=UNUSABLE_END => 0xFF,
            DIV => self.timer.div,
            TIMA => self.timer.tima,
            TMA => self.timer.tma,
            TAC => self.timer.tac | 0xF8,
            INTERRUPT_FLAG => self.interrupt_flag() | 0xE0,
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
    use crate::cartridge::synthetic_rom;

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
        bus.write_byte(0xFF00, 0x34).expect("write io");
        bus.write_byte(0xFF80, 0x56).expect("write hram");
        bus.write_byte(0xFFFF, 0x1F).expect("write ie");

        assert_eq!(bus.read_byte(0xFE00), Ok(0x12));
        assert_eq!(bus.read_byte(0xFF00), Ok(0x34));
        assert_eq!(bus.read_byte(0xFF80), Ok(0x56));
        assert_eq!(bus.read_byte(0xFFFF), Ok(0x1F));
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

        bus.write_byte(0xC000, 0x12).expect("write source");
        bus.write_byte(0xC09F, 0x34).expect("write source");
        bus.write_byte(0xFF46, 0xC0).expect("start dma");

        bus.advance_cycles(80 + 172);
        assert_eq!(bus.read_byte(0xFE00), Ok(0x12));
        assert_eq!(bus.read_byte(0xFE9F), Ok(0x34));
        assert_eq!(bus.read_byte(0xFF46), Ok(0xC0));
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
}
