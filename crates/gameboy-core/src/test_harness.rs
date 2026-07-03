use std::{fs, path::Path};

use crate::{Emulator, HardwareModel, Result};

const EXTERNAL_RESULT_ADDRESS: u16 = 0xA000;
const EXTERNAL_TEXT_ADDRESS: u16 = 0xA004;
const EXTERNAL_RESULT_PENDING: u8 = 0x80;
const EXTERNAL_TEXT_MAGIC: [u8; 3] = [0xDE, 0xB0, 0x61];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestRomStatus {
    Passed,
    Failed,
    TimedOut,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestRomRun {
    pub status: TestRomStatus,
    pub frames: u64,
    pub cycles: u64,
    pub serial_output: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TestRomConfig {
    pub max_frames: u64,
    pub model: HardwareModel,
}

impl Default for TestRomConfig {
    fn default() -> Self {
        Self {
            max_frames: 3_600,
            model: HardwareModel::Auto,
        }
    }
}

impl TestRomRun {
    pub fn serial_text(&self) -> String {
        String::from_utf8_lossy(&self.serial_output).into_owned()
    }
}

pub fn run_blargg_test_rom(path: impl AsRef<Path>, config: TestRomConfig) -> Result<TestRomRun> {
    let rom = fs::read(path).map_err(|err| crate::EmulatorError::InvalidRom {
        reason: err.to_string(),
    })?;
    run_blargg_test_rom_bytes(rom, config)
}

pub fn run_blargg_test_rom_bytes(rom: Vec<u8>, config: TestRomConfig) -> Result<TestRomRun> {
    let mut emulator = Emulator::new();
    emulator.load_rom_with_model(rom, config.model)?;

    let mut serial_output = Vec::new();
    let mut cycles = 0;

    for frames in 1..=config.max_frames {
        cycles += emulator.run_frame()? as u64;
        serial_output.extend(emulator.take_serial_output());

        if let Some((status, external_output)) = external_result(&emulator) {
            if serial_output.is_empty() {
                serial_output = external_output;
            }

            return Ok(TestRomRun {
                status,
                frames,
                cycles,
                serial_output,
            });
        }

        if serial_indicates_failure(&serial_output) {
            return Ok(TestRomRun {
                status: TestRomStatus::Failed,
                frames,
                cycles,
                serial_output,
            });
        }

        if serial_indicates_success(&serial_output) {
            return Ok(TestRomRun {
                status: TestRomStatus::Passed,
                frames,
                cycles,
                serial_output,
            });
        }
    }

    Ok(TestRomRun {
        status: TestRomStatus::TimedOut,
        frames: config.max_frames,
        cycles,
        serial_output,
    })
}

fn serial_indicates_success(output: &[u8]) -> bool {
    let text = String::from_utf8_lossy(output).to_ascii_lowercase();
    text.contains("passed") || text.contains("passed all tests")
}

fn serial_indicates_failure(output: &[u8]) -> bool {
    let text = String::from_utf8_lossy(output).to_ascii_lowercase();
    text.contains("failed") || text.contains("failure")
}

fn external_result(emulator: &Emulator) -> Option<(TestRomStatus, Vec<u8>)> {
    let bus = emulator.bus();
    let magic = [
        bus.read_byte(EXTERNAL_RESULT_ADDRESS + 1).ok()?,
        bus.read_byte(EXTERNAL_RESULT_ADDRESS + 2).ok()?,
        bus.read_byte(EXTERNAL_RESULT_ADDRESS + 3).ok()?,
    ];
    if magic != EXTERNAL_TEXT_MAGIC {
        return None;
    }

    let result = bus.read_byte(EXTERNAL_RESULT_ADDRESS).ok()?;
    if result == EXTERNAL_RESULT_PENDING {
        return None;
    }

    let mut text = Vec::new();
    for offset in 0..0x1FFC {
        let byte = bus.read_byte(EXTERNAL_TEXT_ADDRESS + offset).ok()?;
        if byte == 0 {
            break;
        }
        text.push(byte);
    }

    let status = if result == 0 {
        TestRomStatus::Passed
    } else {
        TestRomStatus::Failed
    };
    Some((status, text))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cartridge::synthetic_rom;

    fn serial_program(message: &str) -> Vec<u8> {
        let mut program = Vec::new();
        for byte in message.bytes() {
            program.extend_from_slice(&[0x3E, byte, 0xE0, 0x01, 0x3E, 0x81, 0xE0, 0x02]);
        }
        program.push(0x76);

        synthetic_rom(
            "SERIAL",
            &[(0x0100, &[0xC3, 0x50, 0x01]), (0x0150, &program)],
        )
    }

    #[test]
    fn detects_pass_from_serial_output() {
        let run = run_blargg_test_rom_bytes(
            serial_program("Passed\n"),
            TestRomConfig {
                max_frames: 1,
                model: HardwareModel::Auto,
            },
        )
        .expect("run test ROM");

        assert_eq!(run.status, TestRomStatus::Passed);
        assert_eq!(run.serial_text(), "Passed\n");
    }

    #[test]
    fn detects_failure_from_serial_output() {
        let run = run_blargg_test_rom_bytes(
            serial_program("Failed #1\n"),
            TestRomConfig {
                max_frames: 1,
                model: HardwareModel::Auto,
            },
        )
        .expect("run test ROM");

        assert_eq!(run.status, TestRomStatus::Failed);
        assert_eq!(run.serial_text(), "Failed #1\n");
    }

    #[test]
    fn reports_timeout_when_no_result_is_emitted() {
        let run = run_blargg_test_rom_bytes(
            serial_program("Running\n"),
            TestRomConfig {
                max_frames: 1,
                model: HardwareModel::Auto,
            },
        )
        .expect("run test ROM");

        assert_eq!(run.status, TestRomStatus::TimedOut);
        assert_eq!(run.frames, 1);
        assert_eq!(run.serial_text(), "Running\n");
    }
}
