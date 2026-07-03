use std::{env, process};

use gameboy_core::test_harness::{run_blargg_test_rom, TestRomConfig};

fn main() {
    let Some(path) = env::args().nth(1) else {
        eprintln!(
            "usage: cargo run -p gameboy-core --example run_test_rom -- <rom.gb> [max_frames]"
        );
        process::exit(2);
    };

    let max_frames = env::args()
        .nth(2)
        .map(|value| {
            value.parse::<u64>().unwrap_or_else(|err| {
                eprintln!("invalid max_frames value {value:?}: {err}");
                process::exit(2);
            })
        })
        .unwrap_or_else(|| TestRomConfig::default().max_frames);

    let run = match run_blargg_test_rom(
        &path,
        TestRomConfig {
            max_frames,
            ..TestRomConfig::default()
        },
    ) {
        Ok(run) => run,
        Err(err) => {
            eprintln!("{err}");
            process::exit(1);
        }
    };

    print!("{}", run.serial_text());
    eprintln!(
        "\nstatus: {:?}, frames: {}, cycles: {}",
        run.status, run.frames, run.cycles
    );
}
