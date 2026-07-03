use std::{env, fs, path::PathBuf};

use gameboy_core::test_harness::{run_blargg_test_rom, TestRomConfig, TestRomStatus};

fn main() {
    let root = env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("roms/gb-test-roms"));
    let max_frames = env::args()
        .nth(2)
        .map(|value| value.parse::<u64>().expect("max_frames must be a number"))
        .unwrap_or(7_200);

    let mut roms = Vec::new();
    collect_roms(&root, &mut roms);
    roms.sort();

    let mut passed = 0;
    let mut failed = Vec::new();
    let mut timed_out = Vec::new();
    let mut errored = Vec::new();

    for rom in &roms {
        let rel = rom.strip_prefix(&root).unwrap_or(rom);
        match run_blargg_test_rom(
            rom,
            TestRomConfig {
                max_frames,
                ..TestRomConfig::default()
            },
        ) {
            Ok(run) => match run.status {
                TestRomStatus::Passed => {
                    passed += 1;
                    println!("PASS  {}", rel.display());
                }
                TestRomStatus::Failed => {
                    println!("FAIL  {}", rel.display());
                    println!("      serial: {}", run.serial_text().replace('\n', "\\n"));
                    failed.push(rel.display().to_string());
                }
                TestRomStatus::TimedOut => {
                    println!("TIME  {} ({} frames)", rel.display(), run.frames);
                    println!("      serial: {}", run.serial_text().replace('\n', "\\n"));
                    timed_out.push(rel.display().to_string());
                }
            },
            Err(err) => {
                println!("ERR   {}: {}", rel.display(), err);
                errored.push(rel.display().to_string());
            }
        }
    }

    println!();
    println!(
        "Summary: {} passed, {} failed, {} timed out, {} errored (of {})",
        passed,
        failed.len(),
        timed_out.len(),
        errored.len(),
        roms.len()
    );
    if !failed.is_empty() {
        println!("Failed:");
        for f in &failed {
            println!("  - {f}");
        }
    }
    if !timed_out.is_empty() {
        println!("Timed out:");
        for t in &timed_out {
            println!("  - {t}");
        }
    }
    if !errored.is_empty() {
        println!("Errored:");
        for e in &errored {
            println!("  - {e}");
        }
    }
}

fn collect_roms(dir: &PathBuf, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().and_then(|n| n.to_str()) == Some(".git") {
                continue;
            }
            collect_roms(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("gb") {
            out.push(path);
        }
    }
}
