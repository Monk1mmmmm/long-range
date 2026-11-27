//! Interface look shared by the menus, the HUD and the editor: fonts and a flat egui style.
//! Play (body text, Latin + Cyrillic) and Michroma (the game title only, Latin) are embedded from
//! `assets/fonts` (SIL OFL, licences next to them). Flat on purpose: square corners, thin 1 px lines,
//! no shadows, no gradients.

use bevy::prelude::*;
use bevy_egui::egui::{self, Color32, CornerRadius, FontData, FontFamily, Shadow, Stroke};
use std::sync::Arc;

/// Font family of the game title.
pub const TITLE: &str = "title";
/// Font family of headings and main buttons (Play Bold).
pub const BOLD: &str = "bold";

/// Panel colours, also used by the menus' own painting.
pub const PANEL: Color32 = Color32::from_rgb(17, 22, 28);
pub const LINE: Color32 = Color32::from_rgb(62, 78, 92);
pub const LINE_HI: Color32 = Color32::from_rgb(132, 164, 188);
pub const TEXT: Color32 = Color32::from_rgb(204, 214, 222);
pub const TEXT_DIM: Color32 = Color32::from_rgb(132, 146, 158);
/// Highlight (active button, accents): a dull amber.
pub const ACCENT: Color32 = Color32::from_rgb(214, 176, 92);

pub fn title_font(ctx: &egui::Context, size: f32) -> egui::FontId {
    egui::FontId::new(size, family(ctx, TITLE))
}

pub fn bold_font(ctx: &egui::Context, size: f32) -> egui::FontId {
    egui::FontId::new(size, family(ctx, BOLD))
}

/// Our named family, or the plain one on the first frame (`set_fonts` takes effect a frame later and an
/// unbound family panics).
fn family(ctx: &egui::Context, name: &str) -> FontFamily {
    let f = FontFamily::Name(name.into());
    if ctx.fonts(|x| x.definitions().families.contains_key(&f)) { f } else { FontFamily::Proportional }
}

fn fonts() -> egui::FontDefinitions {
    let mut f = egui::FontDefinitions::default();
    let add = |f: &mut egui::FontDefinitions, name: &str, bytes: &'static [u8]| {
        f.font_data.insert(name.to_owned(), Arc::new(FontData::from_static(bytes)));
    };
    add(&mut f, "play", include_bytes!("../assets/fonts/Play-Regular.ttf"));
    add(&mut f, "play-bold", include_bytes!("../assets/fonts/Play-Bold.ttf"));
    add(&mut f, "michroma", include_bytes!("../assets/fonts/Michroma-Regular.ttf"));
    // egui's own fonts stay behind ours for symbols and emoji.
    let fallback = f.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    f.families.entry(FontFamily::Proportional).or_default().insert(0, "play".into());
    let mut bold = vec!["play-bold".to_owned()];
    bold.extend(fallback.iter().cloned());
    f.families.insert(FontFamily::Name(BOLD.into()), bold);
    let mut title = vec!["michroma".to_owned(), "play-bold".to_owned()];
    title.extend(fallback);
    f.families.insert(FontFamily::Name(TITLE.into()), title);
    f
}

fn style(s: &mut egui::Style) {
    let sq = CornerRadius::ZERO;
    let v = &mut s.visuals;
    v.window_corner_radius = sq;
    v.menu_corner_radius = sq;
    v.window_shadow = Shadow::NONE;
    v.popup_shadow = Shadow::NONE;
    v.window_fill = PANEL;
    v.panel_fill = PANEL;
    v.window_stroke = Stroke::new(1.0, LINE);
    v.extreme_bg_color = Color32::from_rgb(9, 12, 16);
    v.faint_bg_color = Color32::from_rgb(22, 28, 35);
    v.code_bg_color = Color32::from_rgb(30, 38, 46);
    v.hyperlink_color = Color32::from_rgb(130, 184, 220);
    v.warn_fg_color = Color32::from_rgb(232, 160, 60);
    v.error_fg_color = Color32::from_rgb(236, 92, 80);
    v.selection.bg_fill = Color32::from_rgb(48, 78, 102);
    v.selection.stroke = Stroke::new(1.0, Color32::from_rgb(220, 232, 240));
    let w = &mut v.widgets;
    for (k, wv) in [&mut w.noninteractive, &mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open].into_iter().enumerate() {
        wv.corner_radius = sq;
        wv.expansion = 0.0;
        let (fill, line, text) = match k {
            0 => (PANEL, LINE, TEXT),
            1 => (Color32::from_rgb(30, 39, 48), LINE, TEXT),
            2 => (Color32::from_rgb(40, 54, 66), LINE_HI, Color32::WHITE),
            3 => (Color32::from_rgb(56, 48, 30), ACCENT, Color32::WHITE),
            _ => (Color32::from_rgb(36, 47, 58), LINE_HI, TEXT),
        };
        wv.bg_fill = fill;
        wv.weak_bg_fill = fill;
        wv.bg_stroke = Stroke::new(1.0, line);
        wv.fg_stroke = Stroke::new(if k == 0 { 1.0 } else { 1.5 }, text);
    }
    w.noninteractive.bg_fill = PANEL;
    w.noninteractive.weak_bg_fill = PANEL;
    s.spacing.button_padding = egui::vec2(8.0, 3.0);
}

/// Outline of `r` with its top-left and bottom-right corners cut off by `c` (the menus' plate shape).
pub fn chamfer(r: egui::Rect, c: f32) -> Vec<egui::Pos2> {
    use egui::pos2;
    vec![pos2(r.min.x + c, r.min.y), pos2(r.max.x, r.min.y), pos2(r.max.x, r.max.y - c), pos2(r.max.x - c, r.max.y), pos2(r.min.x, r.max.y), pos2(r.min.x, r.min.y + c)]
}

/// A flat plate: fill and a 1 px outline, corners cut.
pub fn plate(p: &egui::Painter, r: egui::Rect, c: f32, fill: Color32, line: Color32) {
    p.add(plate_shape(r, c, fill, line));
}

pub fn plate_shape(r: egui::Rect, c: f32, fill: Color32, line: Color32) -> egui::Shape {
    let pts = chamfer(r, c);
    egui::Shape::Vec(vec![egui::Shape::convex_polygon(pts.clone(), fill, Stroke::NONE), egui::Shape::closed_line(pts, Stroke::new(1.0, line))])
}

/// The game title, upper-case in the title font, widely spaced, with a thin rule under it.
pub fn title(ui: &mut egui::Ui, text: &str, size: f32) {
    let mut job = egui::text::LayoutJob::default();
    job.append(&text.to_uppercase(), 0.0, egui::TextFormat { font_id: title_font(ui.ctx(), size), color: Color32::from_rgb(226, 230, 232), extra_letter_spacing: size * 0.08, ..Default::default() });
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    let w = galley.size().x;
    let (r, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), galley.size().y + 14.0), egui::Sense::hover());
    let x0 = r.center().x - w * 0.5;
    let p = ui.painter();
    p.galley(egui::pos2(x0, r.min.y), galley, Color32::WHITE);
    // A rule as wide as the word with a short amber piece at its left end.
    let y = r.max.y - 2.0;
    p.hline(x0..=x0 + w, y, Stroke::new(1.0, LINE_HI));
    p.rect_filled(egui::Rect::from_min_max(egui::pos2(x0, y - 1.0), egui::pos2(x0 + 48.0, y + 2.0)), 0.0, ACCENT);
}

/// A main-menu button: an upper-case label on a cut-corner plate, a bar on its left edge while hovered.
pub fn menu_button(ui: &mut egui::Ui, text: &str, size: egui::Vec2, primary: bool) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    if ui.is_rect_visible(r) {
        let p = ui.painter();
        let down = resp.is_pointer_button_down_on();
        let (fill, line, col) = if down {
            (Color32::from_rgba_unmultiplied(64, 54, 30, 235), ACCENT, Color32::WHITE)
        } else if resp.hovered() {
            (Color32::from_rgba_unmultiplied(38, 52, 64, 235), LINE_HI, Color32::WHITE)
        } else {
            (Color32::from_rgba_unmultiplied(20, 27, 34, 225), LINE, TEXT)
        };
        plate(p, r, 8.0, fill, line);
        if resp.hovered() || down {
            let bar = egui::Rect::from_min_max(egui::pos2(r.min.x + 1.0, r.min.y + 9.0), egui::pos2(r.min.x + 4.0, r.max.y - 1.0));
            p.rect_filled(bar, 0.0, ACCENT);
        }
        let font = bold_font(ui.ctx(), if primary { 19.0 } else { 15.5 });
        p.text(egui::pos2(r.min.x + 18.0, r.center().y), egui::Align2::LEFT_CENTER, text.to_uppercase(), font, col);
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// The menu backdrop picture scaled to cover the screen, darkened by `dim` (0-255) for busy screens.
pub fn backdrop(ctx: &egui::Context, tex: Option<&egui::TextureHandle>, dim: u8) {
    let screen = ctx.content_rect();
    let p = ctx.layer_painter(egui::LayerId::background());
    p.rect_filled(screen, 0.0, Color32::from_rgb(13, 16, 20));
    let Some(tex) = tex else { return };
    let [w, h] = tex.size().map(|x| x as f32);
    let (sa, ia) = (screen.width() / screen.height(), w / h);
    // Crop the picture's sides or top and bottom so it fills the screen undistorted.
    let uv = if ia > sa {
        let k = sa / ia;
        egui::Rect::from_min_max(egui::pos2(0.5 - k * 0.5, 0.0), egui::pos2(0.5 + k * 0.5, 1.0))
    } else {
        let k = ia / sa;
        egui::Rect::from_min_max(egui::pos2(0.0, 0.5 - k * 0.5), egui::pos2(1.0, 0.5 + k * 0.5))
    };
    p.image(tex.id(), screen, uv, Color32::WHITE);
    if dim > 0 {
        p.rect_filled(screen, 0.0, Color32::from_rgba_unmultiplied(13, 16, 20, dim));
    }
}

/// Loads `assets/ui/menu_bg.png` as an egui texture (None when the file is missing).
pub fn load_backdrop(ctx: &egui::Context) -> Option<egui::TextureHandle> {
    let img = image::open(crate::data::asset_root().join("ui/menu_bg.png")).ok()?.into_rgba8();
    let ci = egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
    Some(ctx.load_texture("menu_bg", ci, egui::TextureOptions::LINEAR))
}

/// Puts the fonts and the style on the window's egui context once (game and editor).
pub fn apply_theme(mut contexts: bevy_egui::EguiContexts, mut done: Local<bool>) -> Result {
    if *done {
        return Ok(());
    }
    let ctx = contexts.ctx_mut()?;
    ctx.set_theme(egui::Theme::Dark);
    ctx.set_fonts(fonts());
    ctx.all_styles_mut(style);
    *done = true;
    Ok(())
}
