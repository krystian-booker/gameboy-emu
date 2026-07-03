# CheddyGB

[![CI](https://github.com/krystian-booker/gameboy-emu/actions/workflows/ci.yml/badge.svg)](https://github.com/krystian-booker/gameboy-emu/actions/workflows/ci.yml)

A Game Boy / Game Boy Color emulator written in Rust.

## Features

- Original Game Boy (DMG) and Game Boy Color support
- Save states, and rewind
- Turbo / fast-forward toggle
- Audio output 
- Gamepad support

## Crates

- `gameboy-core` - the emulation core: CPU, PPU, APU, cartridge, and MMU.
- `gameboy-frontend` - the desktop app (built on `eframe`/`egui`) that wires up
  the ROM library, input, and audio.

## Running

```sh
cargo run --release -p gameboy-frontend
```

Prebuilt binaries for Linux, Windows, and macOS are on the
[Releases](https://github.com/krystian-booker/gameboy-emu/releases) page.

## License

MIT see [LICENSE](LICENSE).
