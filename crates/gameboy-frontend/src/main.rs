use std::{env, fs, process};

use gameboy_core::Emulator;

fn main() {
    let Some(path) = env::args().nth(1) else {
        eprintln!("usage: gameboy-frontend <rom.gb>");
        return;
    };

    let rom = match fs::read(&path) {
        Ok(rom) => rom,
        Err(err) => {
            eprintln!("failed to read {path}: {err}");
            process::exit(1);
        }
    };

    let mut emulator = Emulator::new();
    if let Err(err) = emulator
        .load_rom(rom)
        .and_then(|_| emulator.step().map(|_| ()))
    {
        eprintln!("{err}");
        process::exit(1);
    }
}
