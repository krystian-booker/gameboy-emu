use std::path::PathBuf;

use serde::{Deserialize, Serialize};

const CONFIG_KEY: &str = "config";

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Config {
    pub rom_dir: Option<PathBuf>,
}

impl Config {
    pub fn load(storage: Option<&dyn eframe::Storage>) -> Self {
        storage
            .and_then(|storage| eframe::get_value(storage, CONFIG_KEY))
            .unwrap_or_default()
    }

    pub fn store(&self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, CONFIG_KEY, self);
    }
}
