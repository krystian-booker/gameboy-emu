use std::{
    collections::hash_map::DefaultHasher,
    fs,
    hash::{Hash, Hasher},
    io,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

const APP_ID: &str = "CheddyGB";
const FORMAT_VERSION: u32 = 1;
const AUTO_SLOT: u32 = 0;
const META_EXT: &str = "meta";
const STATE_EXT: &str = "state";

#[derive(Clone, Serialize, Deserialize)]
pub struct StateMeta {
    pub version: u32,
    pub rom_path: PathBuf,
    pub title: String,
    pub mapper: String,
    pub color: bool,
    pub playtime_secs: u64,
    pub saved_at_unix: u64,
    pub thumbnail: Vec<u8>,
    #[serde(skip)]
    slug: String,
    #[serde(skip)]
    slot: u32,
}

impl StateMeta {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        rom_path: PathBuf,
        title: String,
        mapper: String,
        color: bool,
        playtime_secs: u64,
        thumbnail: Vec<u8>,
        slot: u32,
    ) -> Self {
        let slug = slug_for(&rom_path, slot);
        Self {
            version: FORMAT_VERSION,
            rom_path,
            title,
            mapper,
            color,
            playtime_secs,
            saved_at_unix: now_unix(),
            thumbnail,
            slug,
            slot,
        }
    }

    pub fn slug(&self) -> &str {
        &self.slug
    }
}

pub struct StateStore {
    dir: Option<PathBuf>,
    entries: Vec<StateMeta>,
}

impl StateStore {
    pub fn load() -> Self {
        let dir = eframe::storage_dir(APP_ID).map(|d| d.join("states"));
        let mut store = Self {
            dir,
            entries: Vec::new(),
        };
        store.reload();
        store
    }

    fn reload(&mut self) {
        self.entries.clear();
        let Some(dir) = &self.dir else { return };
        let Ok(read_dir) = fs::read_dir(dir) else {
            return;
        };

        for entry in read_dir.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some(META_EXT) {
                continue;
            }
            match fs::read(&path).ok().and_then(|bytes| {
                bincode::deserialize::<StateMeta>(&bytes).ok()
            }) {
                Some(mut meta) if meta.version == FORMAT_VERSION => {
                    meta.slug = path
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    meta.slot = slot_from_slug(&meta.slug);
                    self.entries.push(meta);
                }
                Some(_) => eprintln!("skipping incompatible save state: {}", path.display()),
                None => eprintln!("skipping unreadable save state: {}", path.display()),
            }
        }

        self.sort();
    }

    fn sort(&mut self) {
        self.entries
            .sort_by_key(|m| std::cmp::Reverse(m.saved_at_unix));
    }

    pub fn entries(&self) -> &[StateMeta] {
        &self.entries
    }

    pub fn auto_entries(&self) -> Vec<&StateMeta> {
        self.entries.iter().filter(|m| m.slot == AUTO_SLOT).collect()
    }

    pub fn find(&self, rom_path: &Path) -> Option<&StateMeta> {
        self.find_slot(rom_path, AUTO_SLOT)
    }

    pub fn find_slot(&self, rom_path: &Path, slot: u32) -> Option<&StateMeta> {
        let slug = slug_for(rom_path, slot);
        self.entries.iter().find(|m| m.slug == slug)
    }

    pub fn slots_for(&self, rom_path: &Path) -> [Option<StateMeta>; 4] {
        std::array::from_fn(|i| self.find_slot(rom_path, i as u32 + 1).cloned())
    }

    pub fn save(&mut self, meta: StateMeta, state: &[u8]) -> io::Result<()> {
        let dir = self
            .dir
            .clone()
            .ok_or_else(|| io::Error::other("no save-state directory available"))?;
        fs::create_dir_all(&dir)?;

        let meta_bytes = bincode::serialize(&meta)
            .map_err(|err| io::Error::other(format!("serialize state meta: {err}")))?;
        fs::write(dir.join(format!("{}.{META_EXT}", meta.slug)), meta_bytes)?;
        fs::write(dir.join(format!("{}.{STATE_EXT}", meta.slug)), state)?;

        self.entries.retain(|m| m.slug != meta.slug);
        self.entries.push(meta);
        self.sort();
        Ok(())
    }

    pub fn read_state(&self, meta: &StateMeta) -> io::Result<Vec<u8>> {
        let dir = self
            .dir
            .as_ref()
            .ok_or_else(|| io::Error::other("no save-state directory available"))?;
        fs::read(dir.join(format!("{}.{STATE_EXT}", meta.slug)))
    }

    pub fn remove(&mut self, slug: &str) -> io::Result<()> {
        if let Some(dir) = &self.dir {
            let _ = fs::remove_file(dir.join(format!("{slug}.{META_EXT}")));
            let _ = fs::remove_file(dir.join(format!("{slug}.{STATE_EXT}")));
        }
        self.entries.retain(|m| m.slug != slug);
        Ok(())
    }
}

fn slot_from_slug(slug: &str) -> u32 {
    slug.rsplit_once("_s")
        .and_then(|(_, n)| n.parse().ok())
        .unwrap_or(AUTO_SLOT)
}

fn slug_for(rom_path: &Path, slot: u32) -> String {
    let normalized = fs::canonicalize(rom_path).unwrap_or_else(|_| rom_path.to_path_buf());
    let mut hasher = DefaultHasher::new();
    normalized.hash(&mut hasher);
    format!("{:016x}_s{slot}", hasher.finish())
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
