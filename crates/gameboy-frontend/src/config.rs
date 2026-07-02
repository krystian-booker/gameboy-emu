use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::theme::PaletteKind;

const CONFIG_KEY: &str = "config";

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(default)]
pub struct Shaders {
    pub color_correct: bool,
    pub gamma_weight: f32,

    pub ghosting: bool,
    pub response_time: f32,

    pub pixel_aa: bool,
    pub integer_scale: bool,

    pub lcd_grid: bool,
    pub grid_intensity: f32,
}

impl Default for Shaders {
    fn default() -> Self {
        Self {
            color_correct: true,
            gamma_weight: 0.5,
            ghosting: true,
            response_time: 0.35,
            pixel_aa: false,
            integer_scale: false,
            lcd_grid: true,
            grid_intensity: 0.35,
        }
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub rom_dir: Option<PathBuf>,
    pub palette: PaletteKind,
    pub shaders: Shaders,
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
