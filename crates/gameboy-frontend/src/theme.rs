use std::sync::Arc;

use egui::{Color32, FontData, FontDefinitions, FontFamily, FontId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PaletteKind {
    #[default]
    Green,
    Amber,
    Ice,
    Berry,
    Mono,
}

impl PaletteKind {
    pub const ALL: [PaletteKind; 5] = [
        PaletteKind::Green,
        PaletteKind::Amber,
        PaletteKind::Ice,
        PaletteKind::Berry,
        PaletteKind::Mono,
    ];

    pub fn label(self) -> &'static str {
        match self {
            PaletteKind::Green => "GREEN",
            PaletteKind::Amber => "AMBER",
            PaletteKind::Ice => "ICE",
            PaletteKind::Berry => "BERRY",
            PaletteKind::Mono => "BLACK & WHITE",
        }
    }

    pub fn palette(self) -> Palette {
        match self {
            PaletteKind::Green => Palette {
                scr: rgb(0x9b, 0xbc, 0x0f),
                scr2: rgb(0x7f, 0xa0, 0x3f),
                scr3: rgb(0x4c, 0x63, 0x20),
                acc: rgb(0xc3, 0xdf, 0x3a),
                bg: rgb(0x0a, 0x12, 0x07),
                bg2: rgb(0x0d, 0x18, 0x09),
                line: rgb(0x2e, 0x4a, 0x16),
                sw1: rgb(0x9b, 0xbc, 0x0f),
                sw2: rgb(0x0f, 0x38, 0x0f),
            },
            PaletteKind::Amber => Palette {
                scr: rgb(0xf6, 0xb7, 0x3c),
                scr2: rgb(0xc9, 0x8a, 0x2a),
                scr3: rgb(0x7a, 0x4f, 0x16),
                acc: rgb(0xff, 0xd2, 0x7a),
                bg: rgb(0x14, 0x0c, 0x02),
                bg2: rgb(0x1a, 0x0f, 0x03),
                line: rgb(0x4a, 0x2f, 0x0e),
                sw1: rgb(0xf6, 0xb7, 0x3c),
                sw2: rgb(0x3a, 0x1e, 0x02),
            },
            PaletteKind::Ice => Palette {
                scr: rgb(0x7f, 0xd4, 0xff),
                scr2: rgb(0x4f, 0x9f, 0xce),
                scr3: rgb(0x2b, 0x5f, 0x7e),
                acc: rgb(0xc2, 0xec, 0xff),
                bg: rgb(0x05, 0x0d, 0x14),
                bg2: rgb(0x08, 0x13, 0x1c),
                line: rgb(0x16, 0x3a, 0x4a),
                sw1: rgb(0x7f, 0xd4, 0xff),
                sw2: rgb(0x06, 0x22, 0x31),
            },
            PaletteKind::Berry => Palette {
                scr: rgb(0xd5, 0x9b, 0xff),
                scr2: rgb(0xa0, 0x6f, 0xce),
                scr3: rgb(0x5f, 0x3f, 0x7e),
                acc: rgb(0xec, 0xc2, 0xff),
                bg: rgb(0x0d, 0x07, 0x14),
                bg2: rgb(0x12, 0x09, 0x1c),
                line: rgb(0x3a, 0x23, 0x50),
                sw1: rgb(0xd5, 0x9b, 0xff),
                sw2: rgb(0x1e, 0x0a, 0x31),
            },
            PaletteKind::Mono => Palette {
                scr: rgb(0xea, 0xea, 0xea),
                scr2: rgb(0xa8, 0xa8, 0xa8),
                scr3: rgb(0x66, 0x66, 0x66),
                acc: rgb(0xff, 0xff, 0xff),
                bg: rgb(0x08, 0x08, 0x08),
                bg2: rgb(0x14, 0x14, 0x14),
                line: rgb(0x39, 0x39, 0x39),
                sw1: rgb(0xff, 0xff, 0xff),
                sw2: rgb(0x00, 0x00, 0x00),
            },
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub scr: Color32,
    pub scr2: Color32,
    pub scr3: Color32,
    pub acc: Color32,
    pub bg: Color32,
    pub bg2: Color32,
    pub line: Color32,
    pub sw1: Color32,
    pub sw2: Color32,
}

impl Palette {
    pub fn dot(self) -> Color32 {
        with_alpha(self.scr, 23)
    }

    pub fn tint(self, alpha: u8) -> Color32 {
        with_alpha(self.scr, alpha)
    }

    pub fn shade(self, luma: u8) -> Color32 {
        let t = luma as f32 / 255.0;
        lerp(self.sw2, self.sw1, t)
    }
}

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

fn with_alpha(c: Color32, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a)
}

fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let mix = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(mix(a.r(), b.r()), mix(a.g(), b.g()), mix(a.b(), b.b()))
}

const PIXEL: &str = "pixel";
const SILK: &str = "silk";

pub fn pixel(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(PIXEL.into()))
}

pub fn silk(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(SILK.into()))
}

pub fn mono(size: f32) -> FontId {
    FontId::new(size, FontFamily::Monospace)
}

pub fn apply(ctx: &egui::Context) {
    install_fonts(ctx);
    ctx.set_theme(egui::ThemePreference::Dark);
}

pub fn style(ctx: &egui::Context, pal: Palette) {
    use egui::{CornerRadius, Stroke, TextStyle};

    ctx.all_styles_mut(|style| {
        style.text_styles = [
            (TextStyle::Heading, pixel(18.0)),
            (TextStyle::Body, mono(14.5)),
            (TextStyle::Button, mono(13.5)),
            (TextStyle::Small, mono(12.0)),
            (TextStyle::Monospace, mono(13.0)),
        ]
        .into();

        style.spacing.item_spacing = egui::vec2(10.0, 8.0);
        style.spacing.button_padding = egui::vec2(12.0, 7.0);

        let radius = CornerRadius::same(3);
        let card = pal.tint(14);
        let card_hover = pal.tint(26);

        let v = &mut style.visuals;
        v.dark_mode = true;
        v.override_text_color = Some(pal.scr);
        v.panel_fill = pal.bg;
        v.window_fill = pal.bg2;
        v.extreme_bg_color = pal.bg;
        v.faint_bg_color = card;
        v.hyperlink_color = pal.acc;
        v.window_corner_radius = CornerRadius::same(6);
        v.window_stroke = Stroke::new(1.0, pal.line);
        v.selection.bg_fill = pal.tint(48);
        v.selection.stroke = Stroke::new(1.0, pal.scr);

        for (w, fill) in [
            (&mut v.widgets.noninteractive, card),
            (&mut v.widgets.inactive, card),
            (&mut v.widgets.open, card),
        ] {
            w.bg_fill = fill;
            w.weak_bg_fill = fill;
            w.bg_stroke = Stroke::new(1.0, pal.line);
            w.fg_stroke = Stroke::new(1.0, pal.scr);
            w.corner_radius = radius;
        }
        v.widgets.hovered.bg_fill = card_hover;
        v.widgets.hovered.weak_bg_fill = card_hover;
        v.widgets.hovered.bg_stroke = Stroke::new(1.0, pal.scr);
        v.widgets.hovered.fg_stroke = Stroke::new(1.0, pal.acc);
        v.widgets.hovered.corner_radius = radius;
        v.widgets.active.bg_fill = pal.tint(48);
        v.widgets.active.weak_bg_fill = pal.tint(48);
        v.widgets.active.bg_stroke = Stroke::new(1.0, pal.scr);
        v.widgets.active.fg_stroke = Stroke::new(1.0, pal.scr);
        v.widgets.active.corner_radius = radius;
    });
}

fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();

    fonts.font_data.insert(
        PIXEL.to_owned(),
        Arc::new(FontData::from_static(include_bytes!(
            "../assets/PressStart2P-Regular.ttf"
        ))),
    );
    fonts.font_data.insert(
        SILK.to_owned(),
        Arc::new(FontData::from_static(include_bytes!(
            "../assets/Silkscreen-Regular.ttf"
        ))),
    );

    let fallbacks = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();

    for (name, family) in [(PIXEL, PIXEL), (SILK, SILK)] {
        let mut chain = vec![name.to_owned()];
        chain.extend(fallbacks.iter().cloned());
        fonts
            .families
            .insert(FontFamily::Name(family.into()), chain);
    }

    ctx.set_fonts(fonts);
}
