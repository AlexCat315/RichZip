//! "Paper & Ink": a warm, quiet palette for the archive workspace.
//!
//! The window is a sheet of warm paper, the file list a clean white card
//! lifted by a hairline instead of a shadow, and a single deep pine green
//! carries every interactive emphasis. Nothing is tinted blue, nothing
//! glows, and no region is separated by more than one step of fill.

use cove_ui::compose::theme::{RadiusScale, TextStyleToken, Theme, Typography};
use cove_ui::core::{Color, Font, FontWeight};

fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::from_rgba8(r, g, b, 255)
}

fn alpha(r: u8, g: u8, b: u8, a: u8) -> Color {
    Color::from_rgba8(r, g, b, a)
}

pub fn paper() -> Theme {
    let mut theme = Theme::default_light();
    let c = &mut theme.colors;

    // Warm canvas, white card, one faint step between them.
    c.background = rgb(0xFB, 0xFA, 0xF7);
    c.surface = rgb(0xFF, 0xFF, 0xFE);
    c.surface_variant = rgb(0xF0, 0xEE, 0xE8);
    c.surface_pressed = rgb(0xE6, 0xE3, 0xD9);
    c.surface_sunken = rgb(0xF7, 0xF5, 0xF0);
    c.surface_raised = rgb(0xFF, 0xFF, 0xFE);
    c.surface_floating = rgb(0xFF, 0xFF, 0xFE);
    c.surface_overlay = rgb(0xFF, 0xFF, 0xFE);

    c.on_background = rgb(0x1A, 0x19, 0x17);
    c.on_surface = rgb(0x1A, 0x19, 0x17);
    c.on_surface_variant = rgb(0x4E, 0x4B, 0x44);
    c.on_surface_muted = rgb(0x76, 0x73, 0x6A);

    // Deep pine: ink that happens to be green, never a "success" green.
    c.accent = rgb(0x1F, 0x51, 0x45);
    c.accent_hover = rgb(0x18, 0x40, 0x37);
    c.accent_pressed = rgb(0x12, 0x31, 0x2A);
    c.on_accent = rgb(0xFF, 0xFF, 0xFF);
    c.accent_muted = alpha(0x1F, 0x51, 0x45, 0x14);
    c.accent_secondary = rgb(0x8A, 0x6A, 0x3F);

    // Selected rows are a quiet tint, never an accent band.
    c.selection = alpha(0x1F, 0x51, 0x45, 0x1F);

    c.state_layer_hover = alpha(0x00, 0x00, 0x00, 0x11);
    c.state_layer_pressed = alpha(0x00, 0x00, 0x00, 0x12);
    c.state_layer_selected = alpha(0x1F, 0x51, 0x45, 0x33);

    c.success = rgb(0x2C, 0x7A, 0x4B);
    c.warning = rgb(0x9A, 0x5B, 0x00);
    c.error = rgb(0xB3, 0x26, 0x1E);
    c.info = rgb(0x1F, 0x51, 0x45);

    c.border = rgb(0xE2, 0xDF, 0xD5);
    c.border_strong = rgb(0xC7, 0xC3, 0xB6);
    c.divider = rgb(0xE9, 0xE6, 0xDC);
    c.focus_ring = alpha(0x1F, 0x51, 0x45, 0x59);
    c.selection = alpha(0x1F, 0x51, 0x45, 0x3D);
    c.overlay = alpha(0x00, 0x00, 0x00, 0x4D);

    // Tight radii: checks 4, controls 6, groups 10, sheets 14.
    theme.radius = RadiusScale::new(0.0, 4.0, 6.0, 10.0, 14.0, 9999.0);
    theme.typography = type_scale();
    theme
}

/// One step denser than the stock scale: the list is the app, so its body
/// text stays at 14 while captions drop to 11.5 and metadata breathes.
fn type_scale() -> Typography {
    Typography::new(
        None,
        TextStyleToken::new(Font::system(32.0, FontWeight::Bold), 38.0, -0.8),
        TextStyleToken::new(Font::system(22.0, FontWeight::Bold), 28.0, -0.4),
        TextStyleToken::new(Font::system(15.5, FontWeight::Semibold), 21.0, -0.2),
        TextStyleToken::new(Font::system(13.0, FontWeight::Regular), 19.0, 0.0),
        TextStyleToken::new(Font::system(13.0, FontWeight::Semibold), 19.0, 0.0),
        TextStyleToken::new(Font::system(11.5, FontWeight::Regular), 15.0, 0.1),
        TextStyleToken::new(Font::system(12.0, FontWeight::Medium), 17.0, 0.2),
        TextStyleToken::new(Font::monospaced(12.5, FontWeight::Regular), 18.0, 0.0),
    )
}
