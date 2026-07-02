//! A modern, Game Boy-inspired dark theme: deep slate surfaces with a DMG-green
//! primary accent and a GBC-purple secondary accent.

use egui::{Color32, CornerRadius, FontFamily, FontId, Margin, Stroke, TextStyle};

// Surfaces (dark base).
pub const WINDOW_BG: Color32 = Color32::from_rgb(13, 15, 20);
pub const PANEL_BG: Color32 = Color32::from_rgb(18, 21, 28);
pub const CARD: Color32 = Color32::from_rgb(25, 29, 39);
pub const CARD_HOVER: Color32 = Color32::from_rgb(34, 40, 54);
pub const STROKE: Color32 = Color32::from_rgb(38, 44, 58);

// Text.
pub const TEXT: Color32 = Color32::from_rgb(232, 235, 241);
pub const WEAK: Color32 = Color32::from_rgb(140, 148, 163);

// Accents.
pub const GREEN: Color32 = Color32::from_rgb(128, 208, 96);
pub const GREEN_DIM: Color32 = Color32::from_rgb(74, 130, 56);
/// Faint green wash used to highlight the currently-loaded ROM.
pub const GREEN_TINT: Color32 = Color32::from_rgb(28, 44, 32);
pub const PURPLE: Color32 = Color32::from_rgb(170, 140, 250);

const RADIUS: CornerRadius = CornerRadius::same(10);

/// Installs the theme onto the egui context. Call once at startup.
pub fn apply(ctx: &egui::Context) {
    ctx.set_theme(egui::ThemePreference::Dark);
    ctx.all_styles_mut(style_mut);
}

fn style_mut(style: &mut egui::Style) {
    style.text_styles = [
        (TextStyle::Heading, FontId::new(22.0, FontFamily::Proportional)),
        (TextStyle::Body, FontId::new(15.0, FontFamily::Proportional)),
        (TextStyle::Button, FontId::new(15.0, FontFamily::Proportional)),
        (TextStyle::Small, FontId::new(12.0, FontFamily::Proportional)),
        (TextStyle::Monospace, FontId::new(13.0, FontFamily::Monospace)),
    ]
    .into();

    style.spacing.item_spacing = egui::vec2(10.0, 8.0);
    style.spacing.button_padding = egui::vec2(14.0, 8.0);
    style.spacing.window_margin = Margin::same(12);
    style.spacing.menu_margin = Margin::same(10);

    let v = &mut style.visuals;
    v.dark_mode = true;
    v.override_text_color = Some(TEXT);
    v.panel_fill = PANEL_BG;
    v.window_fill = WINDOW_BG;
    v.extreme_bg_color = WINDOW_BG;
    v.faint_bg_color = CARD;
    v.hyperlink_color = GREEN;
    v.window_corner_radius = CornerRadius::same(12);
    v.window_stroke = Stroke::new(1.0, STROKE);
    v.selection.bg_fill = GREEN_DIM;
    v.selection.stroke = Stroke::new(1.0, GREEN);

    // Non-interactive frames (labels, separators).
    v.widgets.noninteractive.bg_fill = CARD;
    v.widgets.noninteractive.weak_bg_fill = CARD;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, STROKE);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
    v.widgets.noninteractive.corner_radius = RADIUS;

    // Idle interactive widgets (buttons at rest).
    v.widgets.inactive.bg_fill = CARD;
    v.widgets.inactive.weak_bg_fill = CARD;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, STROKE);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);
    v.widgets.inactive.corner_radius = RADIUS;

    // Hovered.
    v.widgets.hovered.bg_fill = CARD_HOVER;
    v.widgets.hovered.weak_bg_fill = CARD_HOVER;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, GREEN);
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, TEXT);
    v.widgets.hovered.corner_radius = RADIUS;

    // Pressed / active.
    v.widgets.active.bg_fill = GREEN_DIM;
    v.widgets.active.weak_bg_fill = GREEN_DIM;
    v.widgets.active.bg_stroke = Stroke::new(1.0, GREEN);
    v.widgets.active.fg_stroke = Stroke::new(1.0, TEXT);
    v.widgets.active.corner_radius = RADIUS;

    // Open (combo boxes, menus).
    v.widgets.open.bg_fill = CARD_HOVER;
    v.widgets.open.weak_bg_fill = CARD_HOVER;
    v.widgets.open.bg_stroke = Stroke::new(1.0, STROKE);
    v.widgets.open.fg_stroke = Stroke::new(1.0, TEXT);
    v.widgets.open.corner_radius = RADIUS;
}
