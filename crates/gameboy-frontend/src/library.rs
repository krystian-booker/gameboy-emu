use std::{
    fs,
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
};

use gameboy_core::cartridge::{Cartridge, MapperKind};

#[derive(Debug, Clone)]
pub struct RomEntry {
    pub path: PathBuf,
    pub file_name: String,
    pub title: String,
    pub color: bool,
    pub mapper: String,
}

impl RomEntry {
    pub fn display_title(&self) -> &str {
        if self.title.is_empty() {
            &self.file_name
        } else {
            &self.title
        }
    }
}

pub type ScanResult = Result<Vec<RomEntry>, String>;

pub fn spawn_scan(dir: PathBuf) -> mpsc::Receiver<ScanResult> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let result = scan_roms(&dir);
        let _ = tx.send(result);
    });
    rx
}

pub fn scan_roms(dir: &Path) -> ScanResult {
    let mut entries = Vec::new();
    collect(dir, &mut entries).map_err(|err| format!("failed to scan {}: {err}", dir.display()))?;
    entries.sort_by(|a, b| {
        a.display_title()
            .to_lowercase()
            .cmp(&b.display_title().to_lowercase())
    });
    Ok(entries)
}

fn collect(dir: &Path, out: &mut Vec<RomEntry>) -> std::io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };

        if file_type.is_dir() {
            if is_hidden(&path) {
                continue;
            }
            let _ = collect(&path, out);
        } else if is_rom(&path) {
            if let Some(rom) = read_entry(path) {
                out.push(rom);
            }
        }
    }
    Ok(())
}

fn read_entry(path: PathBuf) -> Option<RomEntry> {
    let file_name = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();

    let bytes = fs::read(&path).ok()?;
    let cartridge = Cartridge::from_bytes(bytes).ok()?;
    let header = cartridge.header();

    Some(RomEntry {
        file_name,
        title: header.title().to_string(),
        color: header.supports_cgb(),
        mapper: mapper_name(header.mapper_kind()),
        path,
    })
}

fn is_rom(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_lowercase())
            .as_deref(),
        Some("gb") | Some("gbc")
    )
}

fn is_hidden(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with('.'))
}

fn mapper_name(kind: MapperKind) -> String {
    match kind {
        MapperKind::NoMbc => "ROM only".to_string(),
        MapperKind::Mbc1 => "MBC1".to_string(),
        MapperKind::Mbc3 => "MBC3".to_string(),
        MapperKind::Mbc5 => "MBC5".to_string(),
    }
}
