//! Player settings (`settings.json` next to the `assets` folder), the renderer configuration
//! derived from them, and the settings window shared by the main menu and the in-game menu.
use crate::view::terrain::MainCamera;
use bevy::prelude::*;
use bevy::window::{MonitorSelection, PresentMode, PrimaryWindow, WindowMode};
use crate::i18n::tr;
use bevy_egui::egui;
use serde::{Deserialize, Serialize};

#[derive(Resource, Clone, Serialize, Deserialize, PartialEq, Debug)]
#[serde(default)]
pub struct Settings {
    /// Graphics API: "auto", "dx12", "vulkan" or "gl". Applied at start-up.
    pub backend: String,
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
    /// Size in points of the strategic icon in the corner of a build menu button.
    pub menu_icon: f32,
    /// Size of the strategic icons on the map, as a factor of `icons::icon_size`.
    pub icon_scale: f32,
    /// Camera distance from which units are drawn as strategic icons instead of models.
    pub icon_dist: f32,
    /// Level of detail for unit models: where a unit shows as its strategic icon (from `icon_dist`,
    /// or too small on screen), its model is not drawn (`view::model_lod`). Off: models stay under the icons.
    pub model_lod: bool,
    /// Volume of shots and explosions, 0 (off) to 1.
    pub sfx_volume: f32,
    /// Model detail of trees and rocks: 0 = low (also no shadows), 1 = medium, 2 = high.
    pub flora_detail: u8,
    /// Smallest on-screen size of a projectile in pixels; far away they become yellow squares this big.
    pub projectile_px: f32,
    /// Effects and animations (`EFFECTS`): 0 = low (debris appears where it lands, plain pieces
    /// without shadows, fewer explosion puffs, no flash lights), 1 = medium (debris falls and
    /// tumbles), 2 = high (also dust where debris lands, debris shadows).
    pub effects: u8,
    /// Buildings seen earlier and now out of sight keep their last-seen models (`view::ghost_visuals`);
    /// off: only their icons.
    pub ghost_models: bool,
    /// Draw strategic icons of neutral map buildings (houses, civic, industrial). Off: their
    /// models only, at any zoom; a building a player garrisons takes that player's icon again.
    pub neutral_icons: bool,
    /// Interface language (`assets/lang/<code>.json`, "en" = English); empty = from the system (`i18n::system_language`).
    pub language: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { backend: "auto".into(), shadows: 2, antialiasing: "off".into(), vsync: true, fullscreen: false, hotkeys: "default".into(), name: String::new(), menu_icon: 17.0, icon_scale: 1.0, icon_dist: 150.0, model_lod: true, sfx_volume: 0.6, flora_detail: 1, projectile_px: 5.0, effects: 2, ghost_models: true, neutral_icons: true, language: String::new() }
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
        } else if wsl_gpu() {
            // WSL: Vulkan there is llvmpipe (software), OpenGL reaches the GPU (see `linux_gl`).
            "gl".into()
        } else {
            "auto".into()
        }
    }
}

/// Renderer setup shared by the game and the editor.
///
/// Bevy's full pipeline, GPU occlusion culling included. Old integrated GPUs (Intel HD 5xx and
/// similar) crash at start-up with Vulkan, so Windows defaults to DirectX 12 (with the DXC shader
/// compiler built in), and MSAA is off by default. Both can be changed in the settings window.
pub fn render_plugin(s: &Settings) -> bevy::render::RenderPlugin {
    use bevy::render::settings::{Backends, RenderCreation, WgpuSettings};
    let mut w = WgpuSettings::default();
    match s.effective_backend().as_str() {
        "vulkan" => w.backends = Some(Backends::VULKAN),
        "dx12" => w.backends = Some(Backends::DX12),
        "gl" | "opengl" => w.backends = Some(Backends::GL),
        "metal" => w.backends = Some(Backends::METAL),
        // "auto": whatever wgpu prefers (WGPU_BACKEND is honoured by `WgpuSettings::default`).
        _ => {}
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
    #[cfg(target_os = "linux")]
    if gl {
        if let Some(manual) = linux_gl::render_creation(&w) {
            return bevy::render::RenderPlugin { render_creation: manual, synchronous_pipeline_compilation: true, ..default() };
        }
    }
    bevy::render::RenderPlugin { render_creation: RenderCreation::Automatic(Box::new(w)), synchronous_pipeline_compilation: gl, ..default() }
}

/// WSL with GPU support (WSLg): Mesa's d3d12 driver draws through the Windows GPU driver.
fn wsl_gpu() -> bool {
    cfg!(target_os = "linux") && std::path::Path::new("/usr/lib/wsl/lib/libd3d12.so").exists()
}

/// OpenGL on Linux. Bevy 0.20 creates the wgpu instance without a display handle, so wgpu's EGL
/// falls back to the "surfaceless" platform, whose surfaces cannot present: Bevy then panics with
/// "failed to choose present mode". Here the renderer is made by hand (as `initialize_renderer`
/// does) on an X11 display opened for it, and the window is put on X11 (Xwayland under Wayland):
/// an X window can be drawn through any connection to the same server, a Wayland surface only
/// through the window's own connection, which does not exist yet at this point.
/// This is the path to a real GPU under WSL (Mesa's d3d12 driver; its Vulkan there is llvmpipe).
#[cfg(target_os = "linux")]
mod linux_gl {
    use bevy::render::renderer::{RenderAdapter, RenderAdapterInfo, RenderDevice, RenderInstance, RenderQueue};
    use bevy::render::settings::{RenderCreation, WgpuSettings, WgpuSettingsPriority};
    use wgpu::rwh;

    #[derive(Debug)]
    struct XDisplay(std::ptr::NonNull<std::ffi::c_void>, i32);
    // The connection is opened once, initialised for threads, and never closed.
    unsafe impl Send for XDisplay {}
    unsafe impl Sync for XDisplay {}
    impl rwh::HasDisplayHandle for XDisplay {
        fn display_handle(&self) -> Result<rwh::DisplayHandle<'_>, rwh::HandleError> {
            let raw = rwh::RawDisplayHandle::Xlib(rwh::XlibDisplayHandle::new(Some(self.0), self.1));
            Ok(unsafe { rwh::DisplayHandle::borrow_raw(raw) })
        }
    }

    /// The connection the renderer was made with (set by `render_creation`).
    static DISPLAY: std::sync::OnceLock<XDisplay> = std::sync::OnceLock::new();

    /// wgpu only creates surfaces for the display its instance was made with, and winit's windows
    /// carry winit's own connection: point them at ours before the render world makes their surface.
    pub fn use_our_display(mut windows: Query<&mut RawHandleWrapper>) {
        let Some(d) = DISPLAY.get() else { return };
        let ours = rwh::RawDisplayHandle::Xlib(rwh::XlibDisplayHandle::new(Some(d.0), d.1));
        for mut w in &mut windows {
            if w.get_display_handle() != ours {
                // SAFETY: the X window works through any connection to the same server, and ours is never closed.
                unsafe {
                    w.set_display_handle(ours);
                }
            }
        }
    }

    fn open_x() -> Option<XDisplay> {
        let xlib = Box::leak(Box::new(x11_dl::xlib::Xlib::open().ok()?));
        unsafe {
            (xlib.XInitThreads)();
            let d = (xlib.XOpenDisplay)(std::ptr::null());
            if d.is_null() {
                return None;
            }
            let screen = (xlib.XDefaultScreen)(d);
            Some(XDisplay(std::ptr::NonNull::new_unchecked(d.cast()), screen))
        }
    }

    pub fn render_creation(w: &WgpuSettings) -> Option<RenderCreation> {
        let Some(display) = open_x() else {
            eprintln!("OpenGL: no X11 display (DISPLAY unset?); trying Bevy's default set-up");
            return None;
        };
        // winit prefers Wayland whenever WAYLAND_DISPLAY is set. Nothing runs on other threads yet,
        // and Mesa reads its variables when the instance below loads the driver.
        if std::env::var_os("WAYLAND_DISPLAY").is_some() {
            std::env::remove_var("WAYLAND_DISPLAY");
        }
        if super::wsl_gpu() && std::env::var_os("GALLIUM_DRIVER").is_none() {
            // Mesa picks llvmpipe under WSL unless told otherwise.
            std::env::set_var("GALLIUM_DRIVER", "d3d12");
        }
        if std::env::var("GALLIUM_DRIVER").as_deref() == Ok("d3d12") {
            // The d3d12 driver does not report GL_EXT_texture_shadow_lod, which Bevy's PBR shaders
            // need for shadows (without it every mesh shader fails: black screen). Claiming it works.
            let ext = std::env::var("MESA_EXTENSION_OVERRIDE").unwrap_or_default();
            if !ext.contains("GL_EXT_texture_shadow_lod") {
                std::env::set_var("MESA_EXTENSION_OVERRIDE", format!("{ext} +GL_EXT_texture_shadow_lod").trim());
            }
        }
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::GL,
            flags: w.instance_flags,
            memory_budget_thresholds: w.instance_memory_budget_thresholds,
            display: Some(Box::new(XDisplay(display.0, display.1))),
            backend_options: wgpu::BackendOptions {
                gl: wgpu::GlBackendOptions { gles_minor_version: w.gles3_minor_version, fence_behavior: wgpu::GlFenceBehavior::Normal, debug_fns: wgpu::GlDebugFns::Auto },
                ..Default::default()
            },
        });
        let options = wgpu::RequestAdapterOptions { power_preference: w.power_preference, compatible_surface: None, force_fallback_adapter: false, apply_limit_buckets: false };
        let adapter = match bevy::tasks::block_on(instance.request_adapter(&options)) {
            Ok(a) => a,
            Err(e) => {
                eprintln!("OpenGL: no adapter ({e}); trying Bevy's default set-up");
                return None;
            }
        };
        let info = adapter.get_info();
        // (the logger is not set up yet when the plugins are made)
        eprintln!("OpenGL renderer: {} ({})", info.name, info.driver_info);
        // Same feature and limit choice as Bevy's `initialize_renderer`.
        let (mut features, mut limits) = match w.priority {
            WgpuSettingsPriority::Functionality => (adapter.features(), adapter.limits()),
            _ => (wgpu::Features::empty(), w.limits.clone()),
        };
        if let Some(off) = w.disabled_features {
            features.remove(off);
        }
        features |= w.features;
        if let Some(c) = w.constrained_limits.as_ref() {
            limits = limits.or_worse_values_from(c);
        }
        let desc = wgpu::DeviceDescriptor {
            label: w.device_label.as_ref().map(AsRef::as_ref),
            required_features: features,
            required_limits: limits,
            experimental_features: unsafe { wgpu::ExperimentalFeatures::enabled() },
            memory_hints: w.memory_hints.clone(),
            trace: wgpu::Trace::Off,
        };
        let (device, queue) = match bevy::tasks::block_on(adapter.request_device(&desc)) {
            Ok(dq) => dq,
            Err(e) => {
                eprintln!("OpenGL: no device ({e}); trying Bevy's default set-up");
                return None;
            }
        };
        let _ = DISPLAY.set(display);
        Some(RenderCreation::manual(RenderDevice::from(device), RenderQueue::new(queue), RenderAdapterInfo::new(info), RenderAdapter::new(adapter), RenderInstance::new(instance)))
    }
    use bevy::prelude::Query;
    use bevy::window::RawHandleWrapper;
}

/// Log configuration shared by the game and the editor.
///
/// wgpu's OpenGL backend reports the same harmless texture-view "heuristics" message several times
/// per frame (thousands of lines a minute); that module is silenced, its shader errors are kept.
///
/// Everything logged also goes to `<program>.log` next to `settings.json` (the one of the run before
/// is kept as `<program>-previous.log`), and a panic writes its message and backtrace there, so a
/// crash leaves a record even when the game was not started from a console.
pub fn log_plugin() -> bevy::log::LogPlugin {
    let filter = format!("{},wgpu_hal::gles=off,wgpu_hal::gles::device=error", bevy::log::DEFAULT_FILTER);
    bevy::log::LogPlugin { filter, custom_layer: log_file_layer, ..default() }
}

/// `<program>.log` next to `settings.json`.
pub fn log_path() -> std::path::PathBuf {
    let stem = std::env::current_exe().ok().and_then(|e| e.file_stem().map(|s| s.to_string_lossy().into_owned())).unwrap_or_else(|| "longrange".into());
    base_dir().join(format!("{stem}.log"))
}

fn log_file_layer(_: &mut App) -> Option<bevy::log::BoxedLayer> {
    use bevy::log::tracing_subscriber::Layer;
    let path = log_path();
    let previous = path.with_file_name(format!("{}-previous.log", path.file_stem().map_or("longrange".into(), |s| s.to_string_lossy())));
    let _ = std::fs::rename(&path, previous);
    let file = std::fs::File::create(&path).ok()?;
    // Panics (also those on the render thread) end up in the file with a backtrace.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open(log_path()) {
            let _ = writeln!(f, "\nPANIC: {info}\n{}", std::backtrace::Backtrace::force_capture());
        }
        default_hook(info);
    }));
    Some(bevy::log::tracing_subscriber::fmt::layer().with_ansi(false).with_writer(std::sync::Mutex::new(file)).boxed())
}

/// Render errors seen so far (also appended to `render-errors.log`), for the settings window.
#[derive(Resource, Default)]
pub struct RenderErrors(pub Vec<String>);

/// Keeps the app running after wgpu validation errors and records them.
///
/// Bevy 0.20 quits on any render error, which on GPUs with partial feature support turns one
/// failed pipeline into "black window, then the program closes". Validation and internal (driver, shader compiler) errors are
/// logged, written to `render-errors.log` and shown in Settings; device loss and out-of-memory still quit.
///
/// It also keeps the window's swap chain alive across Alt+Tab (see `keep_surface`).
pub fn tolerate_render_errors(app: &mut App) {
    use bevy::render::error_handler::{ErrorType, RenderErrorHandler, RenderErrorPolicy};
    app.init_resource::<RenderErrors>();
    if let Some(render_app) = app.get_sub_app_mut(bevy::render::RenderApp) {
        render_app.add_systems(bevy::render::Render, keep_surface.before(bevy::render::view::window::create_surfaces));
    }
    app.insert_resource(RenderErrorHandler(|error, main_world, render_world| {
        let text = error.description.trim().to_string();
        // A swap chain that could not be (re)configured is gone until it is configured again,
        // which Bevy only does when the window size changes: `keep_surface` retries it.
        if text.contains("Surface::configure") || text.contains("not configured for presentation") {
            render_world.insert_resource(ReconfigureSurface(0));
        }
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

/// A window surface must be configured again (after a failed attempt); the number counts frames since.
#[derive(Resource)]
struct ReconfigureSurface(u32);

/// Render world, before Bevy (re)configures window surfaces.
///
/// Alt+Tab out of the game minimises the window, which Bevy turns into a resize of the swap chain
/// to 1 x 1. On DirectX 12 that `ResizeBuffers` call can fail ("window is in use"); the swap chain
/// is then lost and the game stopped drawing or went down. A minimised window shows nothing, so its
/// swap chain simply keeps the size it had (restoring the window is then no resize either).
/// If configuring fails anyway, it is tried again about once a second.
fn keep_surface(mut commands: Commands, mut windows: Query<(Entity, &mut bevy::render::view::window::ExtractedWindow)>, mut last: Local<std::collections::HashMap<Entity, (u32, u32)>>, retry: Option<ResMut<ReconfigureSurface>>) {
    let again = match retry {
        Some(mut r) => {
            r.0 += 1;
            if r.0 >= 60 {
                commands.remove_resource::<ReconfigureSurface>();
                true
            } else {
                false
            }
        }
        None => false,
    };
    for (e, mut w) in &mut windows {
        if w.physical_width <= 1 || w.physical_height <= 1 {
            if let Some(&(width, height)) = last.get(&e) {
                w.physical_width = width;
                w.physical_height = height;
                w.size_changed = false;
            }
        } else {
            last.insert(e, (w.physical_width, w.physical_height));
        }
        if again {
            w.size_changed = true;
        }
    }
}

/// Shows the program icon in the window's title bar and the taskbar (Windows).
///
/// `build.rs` puts the icon into the exe as resource 1, which Explorer shows, but winit registers its
/// window class without an icon, so the running window would show the generic one. Other systems
/// have nothing to do here (Linux desktops take the icon from a .desktop file, Wayland ignores it).
/// Linux OpenGL only (see `linux_gl`): windows are drawn through the renderer's X11 connection.
pub fn gl_window_display(app: &mut App) {
    #[cfg(target_os = "linux")]
    app.add_systems(Last, linux_gl::use_our_display);
    #[cfg(not(target_os = "linux"))]
    let _ = app;
}

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

/// Restarts the program (used after changing the graphics API).
pub fn restart() {
    if let Ok(exe) = std::env::current_exe() {
        let args: Vec<String> = std::env::args().skip(1).filter(|a| a != "--skirmish").collect();
        let _ = std::process::Command::new(exe).args(args).spawn();
    }
    std::process::exit(0);
}

/// Names of the tree and rock detail levels (`Settings::flora_detail`).
pub const FLORA_DETAIL: [&str; 3] = [tx!("Low (no shadows)"), tx!("Medium"), tx!("High")];

/// Names of the effect levels (`Settings::effects`).
pub const EFFECTS: [&str; 3] = [tx!("Low (fewest animations)"), tx!("Medium"), tx!("High")];

/// Draws the settings widgets. Returns true when the window should close.
pub fn settings_ui(ui: &mut egui::Ui, s: &mut Settings, started_with: &Settings, adapter: &str, errors: &[String], in_game: bool) -> bool {
    let before = s.clone();
    let mut close = false;
    ui.heading(tr!("Settings"));
    ui.add_space(6.0);
    egui::Grid::new("settings_grid").num_columns(2).spacing([16.0, 8.0]).show(ui, |ui| {
        ui.label(tr!("Language"));
        let langs = crate::i18n::languages();
        let current = crate::i18n::language();
        let shown = langs.iter().find(|(c, _)| *c == current).map_or(current.clone(), |(_, n)| n.clone());
        egui::ComboBox::from_id_salt("language").selected_text(shown).show_ui(ui, |ui| {
            for (code, name) in &langs {
                if ui.selectable_label(*code == current, name).clicked() {
                    s.language = code.clone();
                    crate::i18n::set_language(code);
                }
            }
        });
        ui.end_row();
        ui.label(tr!("Graphics API"));
        let name = |b: &str| match b {
            "dx12" => "DirectX 12",
            "vulkan" => "Vulkan",
            "gl" => "OpenGL",
            _ => tr!("Automatic"),
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
        ui.label(tr!("Shadows"));
        egui::ComboBox::from_id_salt("shadows").selected_text([tr!("Off"), tr!("Low"), tr!("High")][s.shadows.min(2) as usize]).show_ui(ui, |ui| {
            for (k, l) in [tr!("Off"), tr!("Low"), tr!("High")].iter().enumerate() {
                ui.selectable_value(&mut s.shadows, k as u8, *l);
            }
        });
        ui.end_row();
        ui.label(tr!("Trees and rocks"));
        egui::ComboBox::from_id_salt("flora").selected_text(tr(FLORA_DETAIL[s.flora_detail.min(2) as usize])).show_ui(ui, |ui| {
            for (k, l) in FLORA_DETAIL.iter().enumerate() {
                ui.selectable_value(&mut s.flora_detail, k as u8, tr(l));
            }
        });
        ui.end_row();
        ui.label(tr!("Effects")).on_hover_text(tr!("Low: debris appears where it lands instead of falling, explosions are smaller and give no light. For slow graphics cards."));
        egui::ComboBox::from_id_salt("effects").selected_text(tr(EFFECTS[s.effects.min(2) as usize])).show_ui(ui, |ui| {
            for (k, l) in EFFECTS.iter().enumerate() {
                ui.selectable_value(&mut s.effects, k as u8, tr(l));
            }
        });
        ui.end_row();
        ui.label(tr!("Remembered buildings")).on_hover_text(tr!("Buildings (also neutral ones) you have seen and lost sight of stay on the map. On: drawn with their models as last seen. Off: only their icons."));
        ui.checkbox(&mut s.ghost_models, tr!("Show models in the fog"));
        ui.end_row();
        ui.label(tr!("Neutral buildings")).on_hover_text(tr!("Strategic icons of neutral map buildings (houses, civic and industrial buildings). Off: only their models, at any zoom. A building someone garrisons shows that player's icon."));
        ui.checkbox(&mut s.neutral_icons, tr!("Show icons"));
        ui.end_row();
        ui.label(tr!("Anti-aliasing"));
        let aa = |a: &str| match a {
            "fxaa" => "FXAA",
            "msaa4" => "MSAA 4x",
            _ => tr!("Off"),
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
        ui.label(tr!("Fullscreen"));
        ui.checkbox(&mut s.fullscreen, "(F11)");
        ui.end_row();
        ui.label(tr!("Sound effects"));
        if crate::view::sound::AVAILABLE {
            ui.add(egui::Slider::new(&mut s.sfx_volume, 0.0..=1.0).show_value(false));
        } else {
            ui.label(egui::RichText::new(tr!("this build has no sound")).weak());
        }
        ui.end_row();
        ui.label(tr!("Projectile size"));
        ui.add(egui::Slider::new(&mut s.projectile_px, 2.0..=20.0).step_by(0.5).suffix(tr!(" px")));
        ui.end_row();
        ui.label(tr!("Map icons from zoom"));
        ui.add(egui::Slider::new(&mut s.icon_dist, 100.0..=2500.0).logarithmic(true).step_by(100.0).suffix(tr!(" camera distance")));
        ui.end_row();
        ui.label(tr!("Units under icons")).on_hover_text(tr!("Where a unit shows as its strategic icon (zoomed out, or too small to make out), its model is not drawn. Faster with big armies. Off: models stay under the icons."));
        ui.checkbox(&mut s.model_lod, tr!("Hide models"));
        ui.end_row();
        ui.label(tr!("Map icon size"));
        ui.add(egui::Slider::new(&mut s.icon_scale, 0.5..=3.0).step_by(0.05).suffix("x"));
        ui.end_row();
        ui.label(tr!("Build menu icon size"));
        ui.add(egui::Slider::new(&mut s.menu_icon, 10.0..=40.0).step_by(1.0).suffix(tr!(" pt")));
        ui.end_row();
        ui.label(tr!("Hotkeys"));
        egui::ComboBox::from_id_salt("hotkeys").selected_text(if s.hotkeys == "faf_alternative" { tr!("FAF alternative") } else { tr!("Default") }).show_ui(ui, |ui| {
            ui.selectable_value(&mut s.hotkeys, "default".to_string(), tr!("Default"));
            ui.selectable_value(&mut s.hotkeys, "faf_alternative".to_string(), tr!("FAF alternative"));
        });
        ui.end_row();
    });
    ui.add_space(6.0);
    ui.label(egui::RichText::new(tr!("Running on: {}", adapter)).weak());
    let needs_restart = s.backend != started_with.backend;
    if needs_restart {
        ui.colored_label(egui::Color32::from_rgb(255, 210, 100), tr!("The graphics API changes after a restart."));
    }
    if !errors.is_empty() {
        ui.add_space(4.0);
        ui.colored_label(egui::Color32::from_rgb(255, 120, 105), tr!("{} graphics error(s) so far - details in render-errors.log. First one:", errors.len()));
        let first: String = errors[0].chars().take(260).collect();
        ui.label(egui::RichText::new(first).small());
        ui.label(egui::RichText::new(tr!("If the 3D view is black or broken, try another graphics API above.")).small());
    }
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        if ui.button(tr!("Close")).clicked() {
            close = true;
        }
        if needs_restart && ui.button(if in_game { tr!("Restart now (ends this game)") } else { tr!("Restart now") }).clicked() {
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
