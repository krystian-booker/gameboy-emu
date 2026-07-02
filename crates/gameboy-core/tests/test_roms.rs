use std::path::{Path, PathBuf};

use gameboy_core::test_harness::{run_blargg_test_rom, TestRomConfig, TestRomStatus};

fn workspace_rom(path: impl AsRef<Path>) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(path)
}

fn assert_blargg_passes(path: impl AsRef<Path>, max_frames: u64) {
    let path = workspace_rom(path);
    let run = run_blargg_test_rom(&path, TestRomConfig { max_frames })
        .unwrap_or_else(|err| panic!("failed to run {}: {err}", path.display()));

    assert_eq!(
        run.status,
        TestRomStatus::Passed,
        "{} did not pass after {} frames.\nSerial output:\n{}",
        path.display(),
        run.frames,
        run.serial_text()
    );
}

#[test]
#[ignore = "external compatibility test; run explicitly after CPU/interrupt changes"]
fn blargg_cpu_instrs_01_special() {
    assert_blargg_passes(
        "roms/gb-test-roms/cpu_instrs/individual/01-special.gb",
        3_600,
    );
}

#[test]
#[ignore = "external compatibility test; run explicitly after CPU/interrupt changes"]
fn blargg_cpu_instrs_02_interrupts() {
    assert_blargg_passes(
        "roms/gb-test-roms/cpu_instrs/individual/02-interrupts.gb",
        3_600,
    );
}

#[test]
#[ignore = "external compatibility test; run explicitly after CPU/interrupt changes"]
fn blargg_cpu_instrs_full_suite() {
    assert_blargg_passes("roms/gb-test-roms/cpu_instrs/cpu_instrs.gb", 12_000);
}

#[test]
#[ignore = "external compatibility test; run explicitly after timing changes"]
fn blargg_instr_timing() {
    assert_blargg_passes("roms/gb-test-roms/instr_timing/instr_timing.gb", 3_600);
}

#[test]
#[ignore = "external compatibility test; run explicitly after timing changes"]
fn blargg_mem_timing_read() {
    assert_blargg_passes(
        "roms/gb-test-roms/mem_timing/individual/01-read_timing.gb",
        3_600,
    );
}

#[test]
#[ignore = "external compatibility test; run explicitly after timing changes"]
fn blargg_mem_timing_write() {
    assert_blargg_passes(
        "roms/gb-test-roms/mem_timing/individual/02-write_timing.gb",
        3_600,
    );
}

#[test]
#[ignore = "external compatibility test; run explicitly after timing changes"]
fn blargg_mem_timing_modify() {
    assert_blargg_passes(
        "roms/gb-test-roms/mem_timing/individual/03-modify_timing.gb",
        3_600,
    );
}

#[test]
#[ignore = "external compatibility test; run explicitly after timing changes"]
fn blargg_mem_timing_full_suite() {
    assert_blargg_passes("roms/gb-test-roms/mem_timing/mem_timing.gb", 3_600);
}
