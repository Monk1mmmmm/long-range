//! Strategic icons drawn from the JSON icon spec: a base shape per unit class, black glyphs
//! for the role ("+" direct fire, "." artillery, arc anti-air, ...) and tier pips underneath.
//! Icons are white so they can be tinted with the owner colour.
use crate::data::{IconDef, UnitDef};

pub const ICON: usize = 64;

fn seg(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let (abx, aby) = (bx - ax, by - ay);
    let t = (((px - ax) * abx + (py - ay) * aby) / (abx * abx + aby * aby).max(1e-6)).clamp(0.0, 1.0);
    ((px - ax - abx * t).powi(2) + (py - ay - aby * t).powi(2)).sqrt()
}

/// Signed distance to a convex polygon given counter-clockwise or clockwise points.
fn poly(px: f32, py: f32, pts: &[(f32, f32)]) -> f32 {
    let n = pts.len();
    let mut d = f32::MAX;
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let (a, b) = (pts[j], pts[i]);
        d = d.min(seg(px, py, a.0, a.1, b.0, b.1));
        if (b.1 > py) != (a.1 > py) && px < (a.0 - b.0) * (py - b.1) / (a.1 - b.1) + b.0 {
            inside = !inside;
        }
        j = i;
    }
    if inside {
        -d
    } else {
        d
    }
}

/// Distance field of the body shape; coordinates are in -1..1 with y down.
fn shape_sd(shape: &str, x: f32, y: f32) -> f32 {
    match shape {
        "land" => x.abs() + y.abs() - 0.74,
        "bot" => poly(x, y, &[(-0.6, -0.55), (0.6, -0.55), (0.6, 0.1), (0.0, 0.7), (-0.6, 0.1)]),
        "air" => poly(x, y, &[(0.0, -0.72), (0.78, 0.55), (-0.78, 0.55)]),
        "factory" => poly(x, y, &[(-0.8, -0.35), (-0.4, -0.7), (0.4, -0.7), (0.8, -0.35), (0.8, 0.62), (-0.8, 0.62)]),
        "commander" => (x * x + y * y).sqrt() - 0.72,
        "civilian" => (x.abs().max(y.abs()) - 0.56).abs() - 0.07,
        _ => x.abs().max(y.abs()) - 0.6,
    }
}

/// Distance to a glyph stroke/fill; smaller than 0 is inside.
fn glyph_sd(g: &str, x: f32, y: f32) -> f32 {
    match g {
        "plus" => seg(x, y, -0.3, 0.0, 0.3, 0.0).min(seg(x, y, 0.0, -0.3, 0.0, 0.3)) - 0.075,
        "dot" => (x * x + y * y).sqrt() - 0.17,
        "ring" => ((x * x + y * y).sqrt() - 0.24).abs() - 0.06,
        "arc" => {
            // Upper half of a ring: the anti-air "umbrella".
            let d = ((x * x + (y - 0.12) * (y - 0.12)).sqrt() - 0.32).abs() - 0.065;
            if y > 0.14 {
                d.max(y - 0.14)
            } else {
                d
            }
        }
        "chevron" => seg(x, y, -0.28, 0.16, 0.0, -0.2).min(seg(x, y, 0.0, -0.2, 0.28, 0.16)) - 0.07,
        "wave" => {
            let a = ((x * x + (y - 0.25) * (y - 0.25)).sqrt() - 0.22).abs() - 0.05;
            let b = ((x * x + (y - 0.25) * (y - 0.25)).sqrt() - 0.44).abs() - 0.05;
            let d = a.min(b).max(y - 0.12).max(x.abs() - (0.3 - y) * 1.1);
            d.min((x * x + (y - 0.25) * (y - 0.25)).sqrt() - 0.08)
        }
        "bar" => seg(x, y, -0.3, 0.36, 0.3, 0.36) - 0.06,
        "eng" => {
            // Wrench-like open square.
            let d = (x.abs().max(y.abs()) - 0.22).abs() - 0.06;
            d.max(-(seg(x, y, 0.0, -0.3, 0.0, -0.1) - 0.09))
        }
        "mass" => x.abs().max(y.abs()) - 0.18,
        "bolt" => seg(x, y, 0.1, -0.32, -0.12, 0.02).min(seg(x, y, -0.12, 0.02, 0.12, 0.0)).min(seg(x, y, 0.12, 0.0, -0.1, 0.32)) - 0.06,
        "frame" => (x.abs().max(y.abs()) - 0.4).abs() - 0.045,
        "land" => x.abs() + y.abs() - 0.26,
        "tri" => poly(x, y, &[(0.0, -0.28), (0.3, 0.2), (-0.3, 0.2)]),
        _ => f32::MAX,
    }
}

/// Renders an icon into RGBA pixels (ICON x ICON).
pub fn render(icon: &IconDef, tier: u8) -> Vec<u8> {
    let mut out = vec![0u8; ICON * ICON * 4];
    let pips = tier.min(3) as i32;
    // Leave room for the pips below the body.
    let (scale, oy) = if pips > 0 { (0.82, -0.14) } else { (0.95, 0.0) };
    let aa = 2.0 / ICON as f32 / scale;
    for py in 0..ICON {
        for px in 0..ICON {
            let (fx, fy) = ((px as f32 + 0.5) / ICON as f32 * 2.0 - 1.0, (py as f32 + 0.5) / ICON as f32 * 2.0 - 1.0);
            let (x, y) = (fx / scale, (fy - oy) / scale);
            let mut sd = shape_sd(&icon.shape, x, y);
            if icon.shape == "commander" {
                // A ring around the commander disc.
                sd = sd.min(((x * x + y * y).sqrt() - 0.95).abs() - 0.05);
            }
            // Pips: small squares in a row.
            let mut pip = f32::MAX;
            for k in 0..pips {
                let cx = (k as f32 - (pips as f32 - 1.0) * 0.5) * 0.36;
                pip = pip.min(((fx - cx).abs()).max((fy - 0.8).abs()) - 0.11);
            }
            let body = sd.min(pip / scale);
            let outline = 0.085;
            let fill = (0.5 - body / aa).clamp(0.0, 1.0);
            let edge = (0.5 - (body - outline) / aa).clamp(0.0, 1.0);
            let mut glyph = f32::MAX;
            for g in &icon.glyphs {
                glyph = glyph.min(glyph_sd(g, x, y));
            }
            if icon.shape == "commander" {
                glyph = glyph.min((x * x + y * y).sqrt() - 0.25);
            }
            let ink = (0.5 - glyph / aa).clamp(0.0, 1.0) * fill;
            let white = fill * (1.0 - ink);
            let i = (py * ICON + px) * 4;
            let v = (white * 255.0) as u8;
            out[i] = v;
            out[i + 1] = v;
            out[i + 2] = v;
            out[i + 3] = (edge.max(fill) * 255.0) as u8;
        }
    }
    out
}

/// A generic radar blip (unknown contact).
pub fn render_blip() -> Vec<u8> {
    let mut out = vec![0u8; ICON * ICON * 4];
    for py in 0..ICON {
        for px in 0..ICON {
            let (x, y) = ((px as f32 + 0.5) / ICON as f32 * 2.0 - 1.0, (py as f32 + 0.5) / ICON as f32 * 2.0 - 1.0);
            let d = ((x.abs() + y.abs()) - 0.6).abs() - 0.1;
            let core = (x * x + y * y).sqrt() - 0.16;
            let a = (0.5 - d.min(core) * ICON as f32 * 0.5).clamp(0.0, 1.0);
            let i = (py * ICON + px) * 4;
            out[i] = 255;
            out[i + 1] = 255;
            out[i + 2] = 255;
            out[i + 3] = (a * 255.0) as u8;
        }
    }
    out
}

/// On-screen size of a unit's icon in points.
pub fn icon_size(d: &UnitDef) -> f32 {
    let base = if d.is_factory() {
        26.0
    } else if d.icon.shape == "commander" {
        28.0
    } else if d.is_structure() {
        19.0
    } else {
        20.0
    };
    base + d.tier.min(3) as f32 * 1.5
}
