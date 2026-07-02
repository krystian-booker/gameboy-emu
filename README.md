# CheddyGB

A Game Boy / Game Boy Color emulator written in Rust.

## Crates

- `gameboy-core` — CPU, PPU, APU, cartridge, and MMU emulation core.
- `gameboy-frontend` — desktop frontend (built with `eframe`/`egui`) providing the ROM library, input, and audio output.

## Running

```sh
cargo run --release -p gameboy-frontend
```

## License

MIT — see [LICENSE](LICENSE).
