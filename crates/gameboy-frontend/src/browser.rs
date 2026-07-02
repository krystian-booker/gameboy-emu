use std::{
    fs,
    path::{Path, PathBuf},
};

pub struct DirBrowser {
    current: PathBuf,
    error: Option<String>,
}

impl DirBrowser {
    pub fn new(start: Option<PathBuf>) -> Self {
        let current = start
            .filter(|path| path.is_dir())
            .or_else(home_dir)
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("/"));
        Self {
            current,
            error: None,
        }
    }

    fn navigate_to(&mut self, path: PathBuf) {
        self.current = path;
        self.error = None;
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) -> Option<PathBuf> {
        let mut chosen = None;

        ui.horizontal(|ui| {
            let has_parent = self.current.parent().is_some();
            if ui
                .add_enabled(has_parent, egui::Button::new("⬆ Up"))
                .clicked()
            {
                if let Some(parent) = self.current.parent() {
                    self.navigate_to(parent.to_path_buf());
                }
            }
            ui.add_space(8.0);
            ui.monospace(self.current.display().to_string());
        });

        ui.add_space(6.0);

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .max_height(ui.available_height() - 48.0)
            .show(ui, |ui| match subdirectories(&self.current) {
                Ok(dirs) => {
                    if dirs.is_empty() {
                        ui.weak("(no subfolders)");
                    }
                    for dir in dirs {
                        let name = dir
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        if ui
                            .add_sized(
                                [ui.available_width(), 24.0],
                                egui::Button::new(format!("📁 {name}"))
                                    .fill(egui::Color32::TRANSPARENT),
                            )
                            .clicked()
                        {
                            self.navigate_to(dir);
                        }
                    }
                }
                Err(err) => {
                    self.error = Some(err.to_string());
                }
            });

        if let Some(error) = &self.error {
            ui.colored_label(egui::Color32::from_rgb(220, 80, 80), error);
        }

        ui.add_space(8.0);
        ui.separator();
        if ui
            .add(egui::Button::new("Use this folder").min_size(egui::vec2(160.0, 30.0)))
            .clicked()
        {
            chosen = Some(self.current.clone());
        }

        chosen
    }
}

fn subdirectories(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut dirs: Vec<PathBuf> = fs::read_dir(dir)?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .map(|entry| entry.path())
        .filter(|path| {
            !path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with('.'))
        })
        .collect();
    dirs.sort_by_key(|path| {
        path.file_name()
            .map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_default()
    });
    Ok(dirs)
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .filter(|path| path.is_dir())
}
