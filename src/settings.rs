//! Player settings (`settings.json` next to the `assets` folder), the renderer configuration
//! derived from them, and the settings window shared by the main menu and the in-game menu.
use crate::view::terrain::MainCamera;
use bevy::prelude::*;
use bevy::window::{MonitorSelection, PresentMode, PrimaryWindow, WindowMode};
use bevy_egui::egui;
use serde::{Deserialize, Serialize};

#[derive(Resource, Clone, Serialize, Deserialize, PartialEq, Debug)]
#[serde(default)]
pub struct Settings {
    /// Graphics API: "auto", "dx12", "vulkan" or "gl". Applied at start-up.
    pub backend: String,
    /// "compat" (no GPU occlusion culling; safe on old integrated GPUs) or "full". Applied at start-up.
    pub renderer: String,
    /// 0 = off, 1 = low, 2 = high.
    pub shadows: u8,
    /// "off", "fxaa" or "msaa4".
    pub antialiasing: String,
    pub vsync: bool,
    pub fullscreen: bool,
    /// "default" or "faf_alternative".
    pub hotkeys: String,
    /// Player name used in the lobby; empty = the system user name.
    pub name: String,
    /// Draw strategic icons one and a half times as large.
    pub big_icons: bool,
    /// Volume of shots and explosions, 0 (off) to 1.
    pub sfx_volume: f32,
    /// Model detail of trees and rocks: 0 = low (also no shadows), 1 = medium, 2 = high.
    pub flora_detail: u8,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { backend: "auto".into(), renderer: "compat".into(), shadows: 2, antialiasing: "off".into(), vsync: true, fullscreen: false, hotkeys: "default".into(), name: String::new(), big_icons: false, sfx_volume: 0.6, flora_detail: 1 }
    }
}

fn base_dir() -> std::path::PathBuf {
    crate::data::asset_root().parent().map(|p| p.to_path_buf()).unwrap_or_default()
}

impl Settings {
    pub fn path() -> std::path::PathBuf {
        base_dir().join("settings.json")
    }
    pub fn load() -> Settings {
        std::fs::read_to_string(Self::path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }
    pub fn save(&self) {
        let _ = std::fs::write(Self::path(), serde_json::to_string_pretty(self).unwrap_or_default());
    }
    /// The backend that will actually be requested, after environment overrides and "auto".
    pub fn effective_backend(&self) -> String {
        let b = std::env::var("LR_BACKEND").unwrap_or_else(|_| self.backend.clone()).to_lowercase();
        if b != "auto" && !b.is_empty() {
            return b;
        }
        if cfg!(windows) {
            // Under Wine, DX12 goes through vkd3d, where wgpu's format queries panic; Vulkan works there.
            let wine = ["WINELOADER", "WINEDLLDIR0", "WINEDATADIR", "WINEUSERNAME", "WINEPREFIX"].iter().any(|k| std::env::var_os(k).is_some());
            // Native Windows: DX12, because old Intel Vulkan drivers (igvk64.dll) crash at start-up.
            if wine { "vulkan" } else { "dx12" }.into()
        } else {
            "auto".into()
        }
    }
    pub fn full_renderer(&self) -> bool {
        std::env::var("LR_RENDERER").unwrap_or_else(|_| self.renderer.clone()).eq_ignore_ascii_case("full")
    }
}

/// Renderer setup shared by the game and the editor.
///
/// Old integrated GPUs (Intel HD 5xx and similar) crash or show a black view with Bevy's default
/// pipeline, so the default configuration is conservative: DirectX 12 on Windows (with the DXC
/// shader compiler built in), GPU occlusion culling off (its depth-pyramid compute passes are
/// what those drivers reject) and no MSAA. All of it can be changed in the settings window.
pub fn render_plugin(s: &Settings) -> bevy::render::RenderPlugin {
    use bevy::render::settings::{Backends, RenderCreation, WgpuFeatures, WgpuSettings};
    let mut w = WgpuSettings::default();
    match s.effective_backend().as_str() {
        "vulkan" => w.backends = Some(Backends::VULKAN),
        "dx12" => w.backends = Some(Backends::DX12),
        "gl" | "opengl" => w.backends = Some(Backends::GL),
        "metal" => w.backends = Some(Backends::METAL),
        // "auto": whatever wgpu prefers (WGPU_BACKEND is honoured by `WgpuSettings::default`).
        _ => {}
    }
    if !s.full_renderer() {
        // Without this feature Bevy reports "some GPU preprocessing is limited": it keeps the simple
        // mesh preprocessing but drops indirect/occlusion culling and with it the depth-pyramid
        // compute pipelines (read-write R32Float storage textures) that fail on these GPUs.
        // (Zeroing the compute limits instead is not an option: the atmosphere plugin then fails.)
        w.disabled_features = Some(WgpuFeatures::INDIRECT_FIRST_INSTANCE);
    }
    // DX12 shader compiler. MSVC builds have DXC linked in (Bevy selects it). Other Windows builds
    // (MinGW cross build) look for dxcompiler.dll next to the executable: Bevy only checks the
    // current directory, and falls back to FXC, with which the 3D view stays black.
    if cfg!(all(windows, not(target_env = "msvc"))) && std::env::var_os("WGPU_DX12_COMPILER").is_none() {
        use bevy::render::settings::Dx12Compiler;
        let dll = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("dxcompiler.dll"))).filter(|p| p.is_file());
        w.dx12_shader_compiler = match dll {
            Some(p) => Dx12Compiler::DynamicDxc { dxc_path: p.to_string_lossy().into_owned() },
            None => {
                eprintln!("dxcompiler.dll not found next to the executable: DirectX 12 uses the old FXC shader compiler");
                Dx12Compiler::Fxc
            }
        };
    }
    // OpenGL has one context guarded by a lock: pipelines compiled on Bevy's background threads
    // time out waiting for it ("Could not lock adapter context") and take the program down.
    let gl = w.backends == Some(Backends::GL);
    bevy::render::RenderPlugin { render_creation: RenderCreation::Automatic(Box::new(w)), synchronous_pipeline_compilation: gl, ..default() }
}

/// Log configuration shared by the game and the editor.
///
/// wgpu's OpenGL backend reports the same harmless texture-view "heuristics" message several times
/// per frame (thousands of lines a minute); that module is silenced, its shader errors are kept.
pub fn log_plugin() -> bevy::log::LogPlugin {
    let filter = format!("{},wgpu_hal::gles=off,wgpu_hal::gles::device=error", bevy::log::DEFAULT_FILTER);
    bevy::log::LogPlugin { filter, ..default() }
}

/// Render errors seen so far (also appended to `render-errors.log`), for the settings window.
#[derive(Resource, Default)]
pub struct RenderErrors(pub Vec<String>);

/// Keeps the app running after wgpu validation errors and records them.
///
/// Bevy 0.20 quits on any render error, which on GPUs with partial feature support turns one
/// failed pipeline into "black window, then the program closes". Validation and internal (driver, shader compiler) errors are
/// logged, written to `render-errors.log` and shown in Settings; device loss and out-of-memory still quit.
pub fn tolerate_render_errors(app: &mut App) {
    use bevy::render::error_handler::{ErrorType, RenderErrorHandler, RenderErrorPolicy};
    app.init_resource::<RenderErrors>();
    app.insert_resource(RenderErrorHandler(|error, main_world, _| {
        let text = error.description.trim().to_string();
        let mut log = main_world.get_resource_or_insert_with(RenderErrors::default);
        let key: String = text.chars().take(160).collect();
        if log.0.len() < 40 && !log.0.iter().any(|e| e.starts_with(&key)) {
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(base_dir().join("render-errors.log")) {
                let _ = writeln!(f, "[{:?}] {}\n", error.ty, text);
            }
            log.0.push(text);
        }
        // Internal = a driver refused something, typically one shader (the OpenGL backend cannot compile
        // Bevy's ambient-occlusion shader, which the game never uses). Only that pipeline is lost.
        if matches!(error.ty, ErrorType::Validation | ErrorType::Internal) {
            RenderErrorPolicy::Ignore
        } else {
            error!("Quitting because of a fatal render error: {:?}", error.ty);
            main_world.write_message(AppExit::error());
            RenderErrorPolicy::StopRendering
        }
    }));
}

/// Shows the program icon in the window's title bar and the taskbar (Windows).
///
/// `build.rs` puts the icon into the exe as resource 1, which Explorer shows, but winit registers its
/// window class without an icon, so the running window would show the generic one. Other systems
/// have nothing to do here (Linux desktops take the icon from a .desktop file, Wayland ignores it).
pub fn add_window_icon(app: &mut App) {
    #[cfg(windows)]
    app.add_systems(Update, set_window_icon);
    #[cfg(not(windows))]
    let _ = app;
}

#[cfg(windows)]
fn set_window_icon(mut done: Local<bool>, window: Query<Entity, With<PrimaryWindow>>, _main_thread: bevy::ecs::system::NonSendMarker) {
    use winit::dpi::PhysicalSize;
    use winit::platform::windows::{IconExtWindows, WindowExtWindows};
    use winit::window::Icon;
    if *done {
        return;
    }
    let Ok(entity) = window.single() else { return };
    bevy::winit::WINIT_WINDOWS.with_borrow(|windows| {
        // The window is made a frame or two after start-up.
        let Some(w) = windows.get_window(entity) else { return };
        *done = true;
        // Small icon: title bar; big icon: taskbar and Alt+Tab. Windows picks the nearest size in the file.
        match (Icon::from_resource(1, Some(PhysicalSize::new(16, 16))), Icon::from_resource(1, Some(PhysicalSize::new(32, 32)))) {
            (Ok(small), Ok(big)) => {
                w.set_window_icon(Some(small));
                w.set_taskbar_icon(Some(big));
            }
            (Err(e), _) | (_, Err(e)) => warn!("program icon not found in the executable: {e}"),
        }
    });
}

/// Applies the live settings (anti-aliasing, vsync, fullscreen, shadow resolution).
pub fn apply_settings(
    settings: Res<Settings>,
    mut commands: Commands,
    cameras: Query<(Entity, &Msaa, Has<bevy::anti_alias::fxaa::Fxaa>), With<MainCamera>>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    mut shadow_map: ResMut<bevy::light::DirectionalLightShadowMap>,
) {
    let want_msaa = if settings.antialiasing == "msaa4" { Msaa::Sample4 } else { Msaa::Off };
    let want_fxaa = settings.antialiasing == "fxaa";
    for (e, msaa, fxaa) in &cameras {
        if *msaa != want_msaa {
            commands.entity(e).insert(want_msaa);
        }
        if want_fxaa && !fxaa {
            commands.entity(e).insert(bevy::anti_alias::fxaa::Fxaa::default());
        } else if !want_fxaa && fxaa {
            commands.entity(e).remove::<bevy::anti_alias::fxaa::Fxaa>();
        }
    }
    for mut w in &mut windows {
        let pm = if settings.vsync { PresentMode::AutoVsync } else { PresentMode::AutoNoVsync };
        if w.present_mode != pm {
            w.present_mode = pm;
        }
        let windowed = matches!(w.mode, WindowMode::Windowed);
        if settings.fullscreen == windowed {
            w.mode = if settings.fullscreen { WindowMode::BorderlessFullscreen(MonitorSelection::Current) } else { WindowMode::Windowed };
        }
    }
    let size = if settings.shadows >= 2 { 2048 } else { 1024 };
    if shadow_map.size != size {
        shadow_map.size = size;
    }
}

/// Restarts the program (used after changing the graphics API or renderer).
pub fn restart() {
    if let Ok(exe) = std::env::current_exe() {
        let args: Vec<String> = std::env::args().skip(1).filter(|a| a != "--skirmish").collect();
        let _ = std::process::Command::new(exe).args(args).spawn();
    }
    std::process::exit(0);
}

/// Names of the tree and rock detail levels (`Settings::flora_detail`).
pub const FLORA_DETAIL: [&str; 3] = ["Low (no shadows)", "Medium", "High"];

/// Draws the settings widgets. Returns true when the window should close.
pub fn settings_ui(ui: &mut egui::Ui, s: &mut Settings, started_with: &Settings, adapter: &str, errors: &[String], in_game: bool) -> bool {
    let before = s.clone();
    let mut close = false;
    ui.heading("Settings");
    ui.add_space(6.0);
    egui::Grid::new("settings_grid").num_columns(2).spacing([16.0, 8.0]).show(ui, |ui| {
        ui.label("Graphics API");
        let name = |b: &str| match b {
            "dx12" => "DirectX 12",
            "vulkan" => "Vulkan",
            "gl" => "OpenGL",
            _ => "Automatic",
        };
        egui::ComboBox::from_id_salt("backend").selected_text(name(&s.backend)).show_ui(ui, |ui| {
            let mut options = vec!["auto", "vulkan", "gl"];
            if cfg!(windows) {
                options.insert(1, "dx12");
            }
            for b in options {
                ui.selectable_value(&mut s.backend, b.to_string(), name(b));
            }
        });
        ui.end_row();
        ui.label("Renderer");
        egui::ComboBox::from_id_salt("renderer").selected_text(if s.renderer == "full" { "Full" } else { "Compatible" }).show_ui(ui, |ui| {
            ui.selectable_value(&mut s.renderer, "compat".to_string(), "Compatible (old integrated GPUs)");
            ui.selectable_value(&mut s.renderer, "full".to_string(), "Full (GPU occlusion culling)");
        });
        ui.end_row();
        ui.label("Shadows");
        egui::ComboBox::from_id_salt("shadows").selected_text(["Off", "Low", "High"][s.shadows.min(2) as usize]).show_ui(ui, |ui| {
            for (k, l) in ["Off", "Low", "High"].iter().enumerate() {
                ui.selectable_value(&mut s.shadows, k as u8, *l);
            }
        });
        ui.end_row();
        ui.label("Trees and rocks");
        egui::ComboBox::from_id_salt("flora").selected_text(FLORA_DETAIL[s.flora_detail.min(2) as usize]).show_ui(ui, |ui| {
            for (k, l) in FLORA_DETAIL.iter().enumerate() {
                ui.selectable_value(&mut s.flora_detail, k as u8, *l);
            }
        });
        ui.end_row();
        ui.label("Anti-aliasing");
        let aa = |a: &str| match a {
            "fxaa" => "FXAA",
            "msaa4" => "MSAA 4x",
            _ => "Off",
        };
        egui::ComboBox::from_id_salt("aa").selected_text(aa(&s.antialiasing)).show_ui(ui, |ui| {
            for a in ["off", "fxaa", "msaa4"] {
                ui.selectable_value(&mut s.antialiasing, a.to_string(), aa(a));
            }
        });
        ui.end_row();
        ui.label("V-sync");
        ui.checkbox(&mut s.vsync, "");
        ui.end_row();
        ui.label("Fullscreen");
        ui.checkbox(&mut s.fullscreen, "(F11)");
        ui.end_row();
        ui.label("Sound effects");
        if crate::view::sound::AVAILABLE {
            ui.add(egui::Slider::new(&mut s.sfx_volume, 0.0..=1.0).show_value(false));
        } else {
            ui.label(egui::RichText::new("this build has no sound").weak());
        }
        ui.end_row();
        ui.label("Strategic icons");
        ui.checkbox(&mut s.big_icons, "Bigger");
        ui.end_row();
        ui.label("Hotkeys");
        egui::ComboBox::from_id_salt("hotkeys").selected_text(if s.hotkeys == "faf_alternative" { "FAF alternative" } else { "Default" }).show_ui(ui, |ui| {
            ui.selectable_value(&mut s.hotkeys, "default".to_string(), "Default");
            ui.selectable_value(&mut s.hotkeys, "faf_alternative".to_string(), "FAF alternative");
        });
        ui.end_row();
    });
    ui.add_space(6.0);
    ui.label(egui::RichText::new(format!("Running on: {adapter}")).weak());
    let needs_restart = s.backend != started_with.backend || s.renderer != started_with.renderer;
    if needs_restart {
        ui.colored_label(egui::Color32::from_rgb(255, 210, 100), "Graphics API and renderer change after a restart.");
    }
    if !errors.is_empty() {
        ui.add_space(4.0);
        ui.colored_label(egui::Color32::from_rgb(255, 120, 105), format!("{} graphics error(s) so far - details in render-errors.log. First one:", errors.len()));
        let first: String = errors[0].chars().take(260).collect();
        ui.label(egui::RichText::new(first).small());
        ui.label(egui::RichText::new("If the 3D view is black or broken, try another graphics API above.").small());
    }
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        if ui.button("Close").clicked() {
            close = true;
        }
        if needs_restart && ui.button(if in_game { "Restart now (ends this game)" } else { "Restart now" }).clicked() {
            s.save();
            restart();
        }
    });
    if *s != before {
        s.save();
    }
    close
}

/// One-line description of the GPU and API in use.
pub fn adapter_line(world: &World) -> String {
    world
        .get_resource::<bevy::render::renderer::RenderAdapterInfo>()
        .map(|a| format!("{} ({:?})", a.name, a.backend))
        .unwrap_or_else(|| "unknown".into())
}

/// The settings the program was started with (to tell whether a restart is needed).
#[derive(Resource, Clone)]
pub struct StartSettings(pub Settings);
