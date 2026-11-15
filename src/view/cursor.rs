//! Mouse cursor that shows what a click would do: the HUD (`hud::ui_system`) works out the
//! action under the pointer every frame and leaves it in `CursorWant`; `apply_cursor` turns it
//! into a hardware cursor (arrow + order glyph, `icons::render_cursor`). Over menus and panels,
//! and when there is no action, the cursor egui asks for is used instead.
use super::icons;
use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::window::{CursorIcon, CustomCursor, CustomCursorImage, PrimaryWindow, SystemCursorIcon};
use bevy_egui::{EguiGlobalSettings, EguiOutput, PrimaryEguiContext};
use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum CursorKind {
    Move,
    AttackMove,
    Patrol,
    /// Attack a unit.
    Attack,
    /// Fire at the ground (or at a remembered building).
    Ground,
    Assist,
    Reclaim,
    Revive,
    Capture,
    Load,
    Unload,
    Terraform,
}

impl CursorKind {
    pub const ALL: [CursorKind; 12] = [
        CursorKind::Move,
        CursorKind::AttackMove,
        CursorKind::Patrol,
        CursorKind::Attack,
        CursorKind::Ground,
        CursorKind::Assist,
        CursorKind::Reclaim,
        CursorKind::Revive,
        CursorKind::Capture,
        CursorKind::Load,
        CursorKind::Unload,
        CursorKind::Terraform,
    ];

    /// RGBA pixels of the cursor, `size` x `size`.
    pub fn pixels(self, size: usize) -> Vec<u8> {
        let (glyph, tint) = self.look();
        icons::render_cursor(glyph, tint, size)
    }

    /// Order glyph (`icons::render_order` name) and tint.
    fn look(self) -> (&'static str, [f32; 3]) {
        match self {
            CursorKind::Move => ("move", [0.55, 1.0, 0.55]),
            CursorKind::AttackMove => ("attack_move", [1.0, 0.55, 0.35]),
            CursorKind::Patrol => ("patrol", [0.55, 0.8, 1.0]),
            CursorKind::Attack => ("ground", [1.0, 0.3, 0.25]),
            CursorKind::Ground => ("ground", [1.0, 0.7, 0.25]),
            CursorKind::Assist => ("assist", [0.45, 1.0, 0.75]),
            CursorKind::Reclaim => ("reclaim", [0.6, 1.0, 0.4]),
            CursorKind::Revive => ("revive", [0.5, 0.9, 1.0]),
            CursorKind::Capture => ("capture", [1.0, 0.9, 0.35]),
            CursorKind::Load => ("load", [0.4, 0.8, 1.0]),
            CursorKind::Unload => ("unload", [1.0, 0.75, 0.3]),
            CursorKind::Terraform => ("terraform", [0.85, 0.65, 0.35]),
        }
    }
}

/// The action under the pointer this frame; `None` = the ordinary cursor. Written by the HUD,
/// taken (reset) by `apply_cursor`, so it falls back to the ordinary cursor outside a game.
#[derive(Resource, Default)]
pub struct CursorWant(pub Option<CursorKind>);

/// Cursor images made so far, by kind and pixel size.
#[derive(Resource, Default)]
pub struct CursorImages(HashMap<(CursorKind, u32), Handle<Image>>);

/// egui would otherwise put its own cursor on the window whenever its choice changes;
/// `apply_cursor` passes egui's choice on itself.
pub fn take_over_from_egui(mut settings: ResMut<EguiGlobalSettings>) {
    settings.enable_cursor_icon_updates = false;
}

pub fn apply_cursor(
    mut commands: Commands,
    mut want: ResMut<CursorWant>,
    mut cache: ResMut<CursorImages>,
    mut images: ResMut<Assets<Image>>,
    window: Query<(Entity, &Window, Option<&CursorIcon>), With<PrimaryWindow>>,
    egui: Query<&EguiOutput, With<PrimaryEguiContext>>,
) {
    let want = want.0.take();
    let Ok((entity, win, current)) = window.single() else { return };
    let icon = match want {
        Some(kind) => {
            // About 32 px at 100 % display scaling, bigger on high-DPI screens.
            let size = (32.0 * win.scale_factor()).round().clamp(24.0, 96.0) as u32;
            let handle = cache
                .0
                .entry((kind, size))
                .or_insert_with(|| {
                    let px = kind.pixels(size as usize);
                    images.add(Image::new(
                        Extent3d { width: size, height: size, depth_or_array_layers: 1 },
                        TextureDimension::D2,
                        px,
                        TextureFormat::Rgba8UnormSrgb,
                        RenderAssetUsages::MAIN_WORLD,
                    ))
                })
                .clone();
            let hot = (icons::CURSOR_HOTSPOT * size as f32 / 32.0).round() as u16;
            CursorIcon::Custom(CustomCursor::Image(CustomCursorImage { handle, texture_atlas: None, flip_x: false, flip_y: false, rect: None, hotspot: (hot, hot) }))
        }
        None => {
            let e = egui.single().map(|o| o.platform_output.cursor_icon).unwrap_or_default();
            CursorIcon::System(bevy_egui::helpers::egui_to_winit_cursor_icon(e).unwrap_or(SystemCursorIcon::Default))
        }
    };
    if current != Some(&icon) {
        commands.entity(entity).insert(icon);
    }
}
