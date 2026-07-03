use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

use gameboy_core::{Emulator, JoypadState};

use crate::audio::AudioOutput;

pub struct Session {
    emulator: Emulator,
    save_path: PathBuf,
    rtc_path: PathBuf,
    audio: Option<AudioOutput>,
    rom: Vec<u8>,
}

impl Session {
    pub fn start(rom_path: &Path) -> Result<Self, String> {
        let rom = fs::read(rom_path)
            .map_err(|err| format!("failed to read {}: {err}", rom_path.display()))?;

        let mut emulator = Emulator::new();
        emulator
            .load_rom(rom.clone())
            .map_err(|err| err.to_string())?;

        let save_path = rom_path.with_extension("sav");
        let rtc_path = rom_path.with_extension("rtc");

        if emulator.has_battery_save() && save_path.exists() {
            let save = fs::read(&save_path)
                .map_err(|err| format!("failed to read save {}: {err}", save_path.display()))?;
            emulator
                .load_save_ram(&save)
                .map_err(|err| format!("failed to load save {}: {err}", save_path.display()))?;
        }
        if emulator.has_battery_rtc() && rtc_path.exists() {
            let save = fs::read(&rtc_path)
                .map_err(|err| format!("failed to read RTC save {}: {err}", rtc_path.display()))?;
            emulator
                .load_save_rtc(&save)
                .map_err(|err| format!("failed to load RTC save {}: {err}", rtc_path.display()))?;
        }

        let audio = AudioOutput::new();
        if audio.is_none() {
            eprintln!("no audio output available; running without sound");
        }

        Ok(Self {
            emulator,
            save_path,
            rtc_path,
            audio,
            rom,
        })
    }

    pub fn snapshot(&self) -> Result<Vec<u8>, String> {
        bincode::serialize(&self.emulator)
            .map_err(|err| format!("failed to serialize save state: {err}"))
    }

    pub fn restore(rom_path: &Path, state: &[u8]) -> Result<Self, String> {
        let rom = fs::read(rom_path)
            .map_err(|err| format!("failed to read {}: {err}", rom_path.display()))?;

        let mut emulator: Emulator = bincode::deserialize(state)
            .map_err(|err| format!("failed to load save state: {err}"))?;
        emulator
            .reload_rom_bytes(rom.clone())
            .map_err(|err| err.to_string())?;

        let save_path = rom_path.with_extension("sav");
        let rtc_path = rom_path.with_extension("rtc");

        let audio = AudioOutput::new();
        if audio.is_none() {
            eprintln!("no audio output available; running without sound");
        }

        Ok(Self {
            emulator,
            save_path,
            rtc_path,
            audio,
            rom,
        })
    }

    pub fn restore_into(&mut self, state: &[u8]) -> Result<(), String> {
        let mut emulator: Emulator = bincode::deserialize(state)
            .map_err(|err| format!("failed to load rewind state: {err}"))?;
        emulator
            .reload_rom_bytes(self.rom.clone())
            .map_err(|err| err.to_string())?;
        self.emulator = emulator;
        if let Some(audio) = self.audio.as_mut() {
            audio.clear();
        }
        Ok(())
    }

    pub fn clear_audio(&mut self) {
        if let Some(audio) = self.audio.as_mut() {
            audio.clear();
        }
    }

    pub fn has_audio(&self) -> bool {
        self.audio.is_some()
    }

    pub fn set_speed(&mut self, speed: f32) {
        if let Some(audio) = self.audio.as_mut() {
            audio.set_speed(speed);
        }
    }

    pub fn ready_for_more(&self) -> bool {
        self.audio
            .as_ref()
            .map(|a| a.ready_for_more())
            .unwrap_or(true)
    }

    pub fn run_frame(&mut self) -> Result<(), String> {
        self.emulator.run_frame().map_err(|err| err.to_string())?;

        let samples = self.emulator.take_audio_samples();
        if let Some(audio) = self.audio.as_mut() {
            audio.queue(&samples);
        }

        let serial = self.emulator.take_serial_output();
        if !serial.is_empty() {
            let mut stdout = io::stdout();
            let _ = stdout.write_all(&serial).and_then(|_| stdout.flush());
        }

        Ok(())
    }

    pub fn set_joypad_state(&mut self, state: JoypadState) {
        self.emulator.set_joypad_state(state);
    }

    pub fn framebuffer(&self) -> &[u32] {
        self.emulator.framebuffer()
    }

    pub fn persist_saves(&self) -> io::Result<()> {
        if let Some(save_ram) = self.emulator.save_ram() {
            fs::write(&self.save_path, save_ram)?;
        }
        if let Some(save_rtc) = self.emulator.save_rtc() {
            fs::write(&self.rtc_path, save_rtc)?;
        }
        Ok(())
    }
}
