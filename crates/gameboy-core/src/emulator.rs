use crate::{
    apu::AudioSample,
    bus::Bus,
    cartridge::Cartridge,
    cpu::{Cpu, Registers},
    error::Result,
    joypad::JoypadState,
    ppu::PpuMode,
};

pub type CycleCount = u32;
pub const DOTS_PER_FRAME: CycleCount = 456 * 154;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Emulator {
    cpu: Cpu,
    bus: Bus,
}

impl Emulator {
    pub fn new() -> Self {
        Self {
            cpu: Cpu::new_without_boot_rom(),
            bus: Bus::default(),
        }
    }

    pub fn load_rom(&mut self, bytes: Vec<u8>) -> Result<()> {
        let cartridge = Cartridge::from_bytes(bytes)?;
        let cgb_mode = cartridge.header().supports_cgb();
        self.bus.insert_cartridge(cartridge);
        self.bus.set_cgb_mode(cgb_mode);
        self.cpu = if cgb_mode {
            Cpu::new_cgb_without_boot_rom()
        } else {
            Cpu::new_without_boot_rom()
        };
        Ok(())
    }

    pub fn has_battery_save(&self) -> bool {
        self.bus
            .cartridge()
            .is_some_and(|cartridge| cartridge.save_ram().is_some())
    }

    pub fn load_save_ram(&mut self, bytes: &[u8]) -> Result<()> {
        if let Some(cartridge) = self.bus.cartridge_mut() {
            cartridge.load_save_ram(bytes)
        } else {
            Ok(())
        }
    }

    pub fn save_ram(&self) -> Option<&[u8]> {
        self.bus
            .cartridge()
            .and_then(|cartridge| cartridge.save_ram())
    }

    pub fn has_battery_rtc(&self) -> bool {
        self.bus
            .cartridge()
            .is_some_and(|cartridge| cartridge.save_rtc().is_some())
    }

    pub fn load_save_rtc(&mut self, bytes: &[u8]) -> Result<()> {
        if let Some(cartridge) = self.bus.cartridge_mut() {
            cartridge.load_save_rtc(bytes)
        } else {
            Ok(())
        }
    }

    pub fn save_rtc(&self) -> Option<Vec<u8>> {
        self.bus
            .cartridge()
            .and_then(|cartridge| cartridge.save_rtc())
    }

    pub fn step(&mut self) -> Result<CycleCount> {
        self.cpu.step(&mut self.bus)
    }

    pub fn run_frame(&mut self) -> Result<CycleCount> {
        let mut elapsed = 0;

        while elapsed < DOTS_PER_FRAME {
            elapsed += self.step()?;
            if self.take_frame_ready() {
                break;
            }
        }

        Ok(elapsed)
    }

    pub fn registers(&self) -> &Registers {
        self.cpu.registers()
    }

    pub fn bus(&self) -> &Bus {
        &self.bus
    }

    pub fn ppu_mode(&self) -> PpuMode {
        self.bus.ppu_mode()
    }

    pub fn framebuffer(&self) -> &[u32] {
        self.bus.framebuffer()
    }

    pub fn take_frame_ready(&mut self) -> bool {
        self.bus.take_frame_ready()
    }

    pub fn set_joypad_state(&mut self, state: JoypadState) {
        self.bus.set_joypad_state(state);
    }

    pub fn joypad_state(&self) -> JoypadState {
        self.bus.joypad_state()
    }

    pub fn take_serial_output(&mut self) -> Vec<u8> {
        self.bus.take_serial_output()
    }

    pub fn take_audio_samples(&mut self) -> Vec<AudioSample> {
        self.bus.take_audio_samples()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        bus::INTERRUPT_TIMER, cartridge::synthetic_rom, error::EmulatorError,
        ppu::FRAMEBUFFER_PIXELS,
    };

    #[test]
    fn loads_rom_and_steps_nop() {
        let mut emulator = Emulator::new();

        emulator
            .load_rom(synthetic_rom("TEST", &[(0x0100, &[0x00])]))
            .expect("load ROM");

        assert_eq!(emulator.step(), Ok(4));
        assert_eq!(emulator.registers().pc, 0x0101);
    }

    #[test]
    fn step_without_rom_is_memory_error() {
        let mut emulator = Emulator::new();

        assert_eq!(
            emulator.step(),
            Err(EmulatorError::InvalidMemoryAccess { address: 0x0100 })
        );
    }

    #[test]
    fn new_uses_post_boot_cpu_state() {
        let emulator = Emulator::new();

        assert_eq!(emulator.registers().af(), 0x01B0);
        assert_eq!(emulator.registers().bc(), 0x0013);
        assert_eq!(emulator.registers().de(), 0x00D8);
        assert_eq!(emulator.registers().hl(), 0x014D);
        assert_eq!(emulator.registers().pc, 0x0100);
        assert_eq!(emulator.registers().sp, 0xFFFE);
    }

    #[test]
    fn step_advances_div_timer_by_cpu_cycles() {
        let mut emulator = Emulator::new();
        emulator
            .load_rom(synthetic_rom("TEST", &[(0x0100, &[0x00])]))
            .expect("load ROM");

        for _ in 0..64 {
            assert_eq!(emulator.step(), Ok(4));
        }

        assert_eq!(emulator.bus().read_byte(0xFF04), Ok(1));
    }

    #[test]
    fn timer_overflow_can_trigger_interrupt_service() {
        let mut emulator = Emulator::new();
        emulator
            .load_rom(synthetic_rom(
                "TEST",
                &[(0x0100, &[0x00, 0x00, 0x00, 0x00])],
            ))
            .expect("load ROM");

        emulator.bus.write_byte(0xFF05, 0xFF).expect("write tima");
        emulator.bus.write_byte(0xFF06, 0x42).expect("write tma");
        emulator.bus.write_byte(0xFF07, 0b101).expect("write tac");
        emulator
            .bus
            .write_byte(0xFFFF, 1 << INTERRUPT_TIMER)
            .expect("write ie");
        emulator.cpu.set_interrupt_master_enabled(true);
        emulator.cpu.registers_mut().sp = 0xC010;

        for _ in 0..4 {
            assert_eq!(emulator.step(), Ok(4));
        }

        assert_eq!(emulator.bus().read_byte(0xFF05), Ok(0x42));
        assert_eq!(emulator.step(), Ok(20));
        assert_eq!(emulator.registers().pc, 0x0050);
    }

    #[test]
    fn exposes_headless_framebuffer_and_ppu_mode() {
        let emulator = Emulator::new();

        assert_eq!(emulator.framebuffer().len(), FRAMEBUFFER_PIXELS);
        assert_eq!(emulator.ppu_mode(), PpuMode::OamScan);
    }

    #[test]
    fn step_advances_ppu_and_requests_vblank_interrupt() {
        let mut emulator = Emulator::new();
        emulator
            .load_rom(synthetic_rom("TEST", &[(0x0100, &[0x00])]))
            .expect("load ROM");

        for _ in 0..(456 * 144 / 4) {
            assert_eq!(emulator.step(), Ok(4));
        }

        assert_eq!(emulator.bus().read_byte(0xFF44), Ok(144));
        assert_eq!(emulator.ppu_mode(), PpuMode::VBlank);
        assert_eq!(emulator.bus().read_byte(0xFF0F), Ok(0xE1));
        assert!(emulator.take_frame_ready());
        assert!(!emulator.take_frame_ready());
    }

    #[test]
    fn run_frame_steps_until_framebuffer_is_ready() {
        let mut emulator = Emulator::new();
        emulator
            .load_rom(synthetic_rom("TEST", &[(0x0100, &[0x00])]))
            .expect("load ROM");

        assert_eq!(emulator.run_frame(), Ok(456 * 144));
        assert_eq!(emulator.bus().read_byte(0xFF44), Ok(144));
        assert!(!emulator.take_frame_ready());
    }

    #[test]
    fn framebuffer_reflects_vram_background_tiles_after_scanline() {
        let mut emulator = Emulator::new();
        emulator
            .load_rom(synthetic_rom("TEST", &[(0x0100, &[0x00])]))
            .expect("load ROM");

        emulator.bus.write_byte(0x9800, 1).expect("write map");
        emulator.bus.write_byte(0x8010, 0x80).expect("write tile");

        for _ in 0..114 {
            assert_eq!(emulator.step(), Ok(4));
        }

        assert_ne!(emulator.framebuffer()[0], emulator.framebuffer()[1]);
    }
}
