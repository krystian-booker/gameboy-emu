use std::{env, fs::File, io::Write, process};

use gameboy_core::{
    ppu::{SCREEN_HEIGHT, SCREEN_WIDTH},
    Emulator,
};

fn main() {
    let Some(path) = env::args().nth(1) else {
        eprintln!(
            "usage: cargo run -p gameboy-core --example dump_frame -- <rom.gb> [frames] [out.ppm]"
        );
        process::exit(2);
    };
    let frames = env::args()
        .nth(2)
        .map(|value| value.parse::<u64>().expect("valid frame count"))
        .unwrap_or(600);
    let out = env::args()
        .nth(3)
        .unwrap_or_else(|| "frame.ppm".to_string());

    let rom = std::fs::read(&path).unwrap_or_else(|err| {
        eprintln!("failed to read {path}: {err}");
        process::exit(1);
    });
    let mut emulator = Emulator::new();
    emulator.load_rom(rom).unwrap_or_else(|err| {
        eprintln!("{err}");
        process::exit(1);
    });

    for _ in 0..frames {
        emulator.run_frame().unwrap();
    }

    let mut file = File::create(&out).unwrap();
    writeln!(file, "P6\n{} {}\n255", SCREEN_WIDTH, SCREEN_HEIGHT).unwrap();
    for pixel in emulator.framebuffer() {
        let rgb = pixel.to_be_bytes();
        file.write_all(&rgb[1..]).unwrap();
    }
}
