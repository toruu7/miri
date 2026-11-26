#![windows_subsystem = "windows"]

mod egui_sw;
mod pipe_client;

use egui::{Color32, ColorImage, Context, Key, Modifiers, PointerButton, Pos2, RawInput, Rect, TextureHandle, TextureOptions, Vec2 as EguiVec2};
use egui_sw::EguiSoftwareRasterizer;
use pet_core::{
    builtin_presets, new_user_profile_id, resolve_profile, BackdropMode, CatProfile, Config,
    DesignerToOverlay, DiagnosticsSnapshot, GameHideScope, MarkingPattern, OverlayToDesigner,
    PetSim, PoseOverride, Rgb8,
};
use pet_render::{RenderOptions, Renderer};
use pipe_client::{ClientEvent, PipeClient, WM_APP_IPC_EVENT};
use std::cell::RefCell;
use std::fs;
use std::mem::size_of;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};
use windows::core::{PCWSTR, w};
use windows::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HINSTANCE, HWND, LPARAM, LRESULT, RECT,
    WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, GetDC, ReleaseDC, SRCCOPY, StretchDIBits,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForWindow, SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, VK_BACK, VK_CONTROL, VK_DELETE, VK_DOWN, VK_END, VK_ESCAPE, VK_HOME, VK_LEFT,
    VK_MENU, VK_RETURN, VK_RIGHT, VK_SHIFT, VK_TAB, VK_UP,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, FindWindowW, GetClientRect,
    GetMessageW, KillTimer, LoadCursorW, PostQuitMessage, RegisterClassExW, SetForegroundWindow,
    SetTimer, ShowWindow, TranslateMessage, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, IDC_ARROW, MSG,
    SW_RESTORE, SW_SHOW, WINDOW_EX_STYLE, WM_CHAR, WM_CLOSE, WM_DESTROY, WM_DPICHANGED, WM_KEYDOWN,
    WM_KEYUP, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_PAINT, WM_RBUTTONDOWN,
    WM_RBUTTONUP, WM_SIZE, WM_TIMER, WNDCLASSEXW, WS_OVERLAPPEDWINDOW,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DesignerTab {
    Profiles,
    Appearance,
    Peripherals,
    WindowPrivacy,
    Diagnostics,
}

#[derive(Default)]
struct WinEvents {
    egui_events: Vec<egui::Event>,
    resized: Option<(usize, usize)>,
    dpi: Option<u32>,
    ipc_ready: bool,
    timer_tick: bool,
    closed: bool,
}

thread_local! {
    static WIN_EVENTS: RefCell<WinEvents> = RefCell::new(WinEvents::default());
    static CUR_PPP: RefCell<f32> = const { RefCell::new(1.0) };
}

fn app_data_dir() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("DesktopCat")
}

fn load_offline_state() -> (Config, Vec<CatProfile>) {
    let dir = app_data_dir();
    let cfg = fs::read(dir.join("config.json"))
        .ok()
        .and_then(|b| serde_json::from_slice::<Config>(&b).ok())
        .map(|mut c| {
            c.sanitize();
            c
        })
        .unwrap_or_default();
    let mut profiles = Vec::new();
    if let Ok(entries) = fs::read_dir(dir.join("profiles")) {
        for e in entries.flatten() {
            if let Ok(b) = fs::read(e.path()) {
                if let Ok(mut p) = serde_json::from_slice::<CatProfile>(&b) {
                    p.sanitize();
                    if !p.is_builtin() {
                        profiles.push(p);
                    }
                }
            }
        }
    }
    profiles.sort_by(|a, b| a.name.cmp(&b.name));
    (cfg, profiles)
}

fn save_offline_profile(profile: &CatProfile) -> Result<(), String> {
    let dir = app_data_dir().join("profiles");
    let _ = fs::create_dir_all(&dir);
    let slug: String = profile
        .id
        .strip_prefix("user:")
        .unwrap_or(&profile.id)
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(64)
        .collect();
    let bytes = serde_json::to_vec_pretty(profile).map_err(|e| format!("{e}"))?;
    fs::write(dir.join(format!("{slug}.json")), bytes).map_err(|e| format!("{e}"))
}

fn save_offline_config(cfg: &Config) -> Result<(), String> {
    let dir = app_data_dir();
    let _ = fs::create_dir_all(&dir);
    let bytes = serde_json::to_vec_pretty(cfg).map_err(|e| format!("{e}"))?;
    fs::write(dir.join("config.json"), bytes).map_err(|e| format!("{e}"))
}

fn main() {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }

    // Session-scoped single-instance mutex (`Local\DesktopCat_Designer_v1`) -> double-launch focuses existing window!
    let _mutex = unsafe {
        match CreateMutexW(None, false, w!("Local\\DesktopCat_Designer_v1")) {
            Ok(h) => {
                if GetLastError() == ERROR_ALREADY_EXISTS {
                    if let Ok(existing) = FindWindowW(w!("DesktopCatDesignerHost"), PCWSTR::null())
                    {
                        if !existing.0.is_null() {
                            let _ = ShowWindow(existing, SW_RESTORE);
                            let _ = SetForegroundWindow(existing);
                        }
                    }
                    let _ = CloseHandle(h);
                    return;
                }
                h
            }
            Err(_) => return,
        }
    };

    let hinst: HINSTANCE = unsafe { GetModuleHandleW(PCWSTR::null()).unwrap_or_default().into() };
    let hwnd = match create_designer_window(hinst) {
        Ok(h) => h,
        Err(_) => return,
    };

    let mut dpi = unsafe { GetDpiForWindow(hwnd).max(96) };
    let mut ppp = dpi as f32 / 96.0;
    CUR_PPP.with(|p| *p.borrow_mut() = ppp);

    let (mut win_w, mut win_h) = client_size(hwnd);
    let mut raster = EguiSoftwareRasterizer::new(win_w, win_h);
    let ctx = Context::default();

    let (mut config, mut user_profiles) = load_offline_state();
    let mut draft = resolve_profile(&config.active_profile_id, &user_profiles);
    let mut dirty_draft = false;
    let mut pose_override = PoseOverride::AwakeNeutral;
    let mut active_tab = DesignerTab::Profiles;
    let mut diagnostics = DiagnosticsSnapshot::default();
    let mut status_banner: Option<String> = None;
    let mut preview_seq: u64 = 0;
    let mut last_preview_sent = Instant::now() - Duration::from_secs(1);
    let mut preview_dirty_to_send = false;
    let mut config_dirty_to_send = false;

    let mut pipe_client = std::env::var("DESKTOP_CAT_IPC_PIPE")
        .ok()
        .and_then(|name| PipeClient::connect(&name, hwnd).ok());

    let mut preview_renderer = Renderer::new(300, 250);
    let mut preview_sim = PetSim::default();
    preview_sim.set_pose_override(pose_override);
    let mut preview_tex: Option<TextureHandle> = None;

    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOW);
        // 30 FPS gentle preview timer (only active while designer is open)
        let _ = SetTimer(hwnd, 1, 33, None);
    }

    let start_time = Instant::now();
    let mut last_tick = Instant::now();

    let mut msg = MSG::default();
    loop {
        let ok = unsafe { GetMessageW(&mut msg, None, 0, 0) };
        if !ok.as_bool() {
            break;
        }
        unsafe {
            let _ = TranslateMessage(&msg);
            let _ = DispatchMessageW(&msg);
        }

        let evs = WIN_EVENTS.with(|e| std::mem::take(&mut *e.borrow_mut()));
        if evs.closed {
            break;
        }
        if let Some(new_dpi) = evs.dpi {
            dpi = new_dpi.max(96);
            ppp = dpi as f32 / 96.0;
            CUR_PPP.with(|p| *p.borrow_mut() = ppp);
        }
        if let Some((nw, nh)) = evs.resized {
            win_w = nw.max(1);
            win_h = nh.max(1);
            raster.resize(win_w, win_h);
        }

        // Drain IPC events from `cat-overlay.exe`
        let mut disconnected_cleanly = false;
        let mut disconnected_dirty = false;
        if let Some(ref client) = pipe_client {
            for ce in client.drain() {
                match ce {
                    ClientEvent::Message(OverlayToDesigner::HelloAck {
                        config: c,
                        user_profiles: ups,
                        diagnostics: d,
                        ..
                    }) => {
                        config = c;
                        user_profiles = ups;
                        draft = resolve_profile(&config.active_profile_id, &user_profiles);
                        diagnostics = d;
                        dirty_draft = false;
                        status_banner = None;
                    }
                    ClientEvent::Message(OverlayToDesigner::CommitAck { ok, error }) => {
                        if ok {
                            dirty_draft = false;
                            status_banner = Some("Saved profile to live overlay.".to_string());
                        } else {
                            status_banner = Some(format!(
                                "Save failed: {}",
                                error.unwrap_or_else(|| "unknown error".to_string())
                            ));
                        }
                    }
                    ClientEvent::Message(OverlayToDesigner::Diagnostics(d)) => {
                        diagnostics = d;
                    }
                    ClientEvent::Message(OverlayToDesigner::OverlayShuttingDown) => {
                        if !dirty_draft {
                            disconnected_cleanly = true;
                        } else {
                            disconnected_dirty = true;
                        }
                    }
                    ClientEvent::Message(OverlayToDesigner::Pong) => {}
                    ClientEvent::Disconnected => {
                        if !dirty_draft {
                            disconnected_cleanly = true;
                        } else {
                            disconnected_dirty = true;
                        }
                    }
                }
            }
        }
        if disconnected_cleanly {
            break;
        }
        if disconnected_dirty {
            pipe_client = None;
            status_banner =
                Some("Overlay disconnected — unsaved edits preserved. Save Draft or Relaunch Overlay.".to_string());
        }

        // Step local embedded `pet-render` preview
        let dt = last_tick.elapsed().as_secs_f32().min(pet_core::MAX_SIM_DT);
        last_tick = Instant::now();
        preview_sim.set_pose_override(pose_override);
        preview_sim.step(dt);
        let snap = preview_sim.snapshot();
        preview_renderer.render(
            &draft,
            &snap,
            RenderOptions {
                backdrop: if config.capture_stage_enabled {
                    BackdropMode::Solid(config.chroma_key_color)
                } else {
                    BackdropMode::Solid(Rgb8::new(34, 38, 48))
                },
                chroma_hard_edge: config.chroma_hard_edge,
                show_keycap_legends: config.showcase_mode && draft.peripherals.show_keycap_legends,
            },
        );
        let preview_img = premul_rgba_to_color_image(
            preview_renderer.width() as usize,
            preview_renderer.height() as usize,
            preview_renderer.rgba_bytes(),
        );
        if let Some(ref mut tex) = preview_tex {
            tex.set(preview_img, TextureOptions::LINEAR);
        } else {
            preview_tex = Some(ctx.load_texture("cat_preview", preview_img, TextureOptions::LINEAR));
        }

        // Run egui frame
        let raw_input = RawInput {
            screen_rect: Some(Rect::from_min_size(
                Pos2::ZERO,
                EguiVec2::new(win_w as f32 / ppp, win_h as f32 / ppp),
            )),
            time: Some(start_time.elapsed().as_secs_f64()),
            modifiers: current_modifiers(),
            events: evs.egui_events,
            ..Default::default()
        };

        let is_connected = pipe_client.as_ref().map(|c| c.is_connected()).unwrap_or(false);
        let full_output = ctx.run(raw_input, |ui_ctx| {
            // Left panel: Live Cat Preview + Pose Selector
            egui::SidePanel::left("preview_panel")
                .exact_width(310.0)
                .show(ui_ctx, |ui| {
                    ui.add_space(6.0);
                    ui.heading("Desktop Cat Designer");
                    if is_connected {
                        ui.colored_label(Color32::from_rgb(130, 230, 140), "Connected to live overlay");
                    } else {
                        ui.colored_label(Color32::from_rgb(245, 195, 90), "Offline mode (overlay not connected)");
                        if ui.button("Launch Overlay").clicked() {
                            launch_overlay_sibling();
                        }
                    }
                    if let Some(ref b) = status_banner {
                        ui.add_space(4.0);
                        ui.label(b);
                    }

                    ui.add_space(8.0);
                    if let Some(ref tex) = preview_tex {
                        ui.image((tex.id(), EguiVec2::new(292.0, 244.0)));
                    }

                    ui.add_space(6.0);
                    ui.label("Preview Pose (privacy stays enforced on live keys):");
                    ui.horizontal_wrapped(|ui| {
                        for (pose, lbl) in [
                            (PoseOverride::AwakeNeutral, "Awake"),
                            (PoseOverride::Typing, "Typing"),
                            (PoseOverride::Sleep, "Sleep"),
                            (PoseOverride::PrivacyCurl, "Privacy"),
                            (PoseOverride::Auto, "Auto"),
                        ] {
                            if ui.selectable_label(pose_override == pose, lbl).clicked() {
                                pose_override = pose;
                                preview_dirty_to_send = true;
                            }
                        }
                    });

                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label("Profile Name:");
                        if ui.text_edit_singleline(&mut draft.name).changed() {
                            dirty_draft = true;
                        }
                    });
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        let save_lbl = if draft.is_builtin() {
                            "Clone & Save Custom Profile"
                        } else {
                            "Save Custom Profile"
                        };
                        if ui.button(save_lbl).clicked() {
                            let mut to_save = draft.clone();
                            if to_save.is_builtin() {
                                to_save.base_preset_id = Some(to_save.id.clone());
                                to_save.id = new_user_profile_id();
                                if to_save.name == draft.name {
                                    to_save.name = format!("{} Custom", draft.name);
                                }
                            }
                            to_save.sanitize();
                            draft = to_save.clone();
                            if let Some(slot) = user_profiles.iter_mut().find(|p| p.id == to_save.id) {
                                *slot = to_save.clone();
                            } else {
                                user_profiles.push(to_save.clone());
                            }
                            config.active_profile_id = to_save.id.clone();
                            if let Some(ref c) = pipe_client {
                                c.send(&DesignerToOverlay::CommitProfile {
                                    profile: to_save,
                                    make_active: true,
                                });
                            } else {
                                let _ = save_offline_profile(&draft);
                                let _ = save_offline_config(&config);
                                dirty_draft = false;
                                status_banner = Some("Saved draft offline to AppData.".to_string());
                            }
                        }
                        if dirty_draft && ui.button("Revert").clicked() {
                            draft = resolve_profile(&config.active_profile_id, &user_profiles);
                            dirty_draft = false;
                            if let Some(ref c) = pipe_client {
                                c.send(&DesignerToOverlay::CancelPreview);
                            }
                        }
                    });
                });

            // Right main panel: Tabs
            egui::CentralPanel::default().show(ui_ctx, |ui| {
                ui.horizontal(|ui| {
                    for (tab, lbl) in [
                        (DesignerTab::Profiles, "Presets & Profiles"),
                        (DesignerTab::Appearance, "Cat Coat & Eyes"),
                        (DesignerTab::Peripherals, "Keyboard & Mouse"),
                        (DesignerTab::WindowPrivacy, "Window & Privacy"),
                        (DesignerTab::Diagnostics, "Diagnostics"),
                    ] {
                        if ui.selectable_label(active_tab == tab, lbl).clicked() {
                            active_tab = tab;
                            if tab == DesignerTab::Diagnostics {
                                if let Some(ref c) = pipe_client {
                                    c.send(&DesignerToOverlay::RequestDiagnostics);
                                }
                            }
                        }
                    }
                });
                ui.separator();

                egui::ScrollArea::vertical().show(ui, |ui| match active_tab {
                    DesignerTab::Profiles => {
                        ui.label("15 Built-In Presets (read-only; edit and click Clone & Save to customize):");
                        ui.add_space(4.0);
                        ui.horizontal_wrapped(|ui| {
                            for p in builtin_presets() {
                                let selected = draft.id == p.id;
                                if swatch_pill(ui, &p.name, p.appearance.base_coat, selected).clicked() {
                                    draft = p.clone();
                                    config.active_profile_id = p.id;
                                    dirty_draft = false;
                                    config_dirty_to_send = true;
                                    preview_dirty_to_send = true;
                                }
                            }
                        });
                        ui.add_space(10.0);
                        ui.separator();
                        ui.label(format!("Saved Custom Profiles ({}):", user_profiles.len()));
                        if user_profiles.is_empty() {
                            ui.label("No custom profiles yet. Pick any preset, tweak colors, and click Clone & Save.");
                        }
                        let mut to_delete: Option<String> = None;
                        for p in &user_profiles {
                            ui.horizontal(|ui| {
                                let selected = draft.id == p.id;
                                if swatch_pill(ui, &p.name, p.appearance.base_coat, selected).clicked() {
                                    draft = p.clone();
                                    config.active_profile_id = p.id.clone();
                                    dirty_draft = false;
                                    config_dirty_to_send = true;
                                    preview_dirty_to_send = true;
                                }
                                if ui.small_button("Delete").clicked() {
                                    to_delete = Some(p.id.clone());
                                }
                            });
                        }
                        if let Some(del_id) = to_delete {
                            user_profiles.retain(|p| p.id != del_id);
                            if config.active_profile_id == del_id {
                                config.active_profile_id = "builtin:midnight".to_string();
                                draft = resolve_profile(&config.active_profile_id, &user_profiles);
                            }
                            if let Some(ref c) = pipe_client {
                                c.send(&DesignerToOverlay::DeleteProfile { profile_id: del_id });
                            }
                        }
                    }

                    DesignerTab::Appearance => {
                        let a = &mut draft.appearance;
                        let mut changed = false;
                        changed |= rgb_row(ui, "Base Coat", &mut a.base_coat);
                        ui.horizontal(|ui| {
                            ui.label("Marking Pattern:");
                            ui.horizontal_wrapped(|ui| {
                                for &pat in MarkingPattern::ALL {
                                    if ui.selectable_label(a.marking_pattern == pat, pat.display_name()).clicked() {
                                        a.marking_pattern = pat;
                                        changed = true;
                                    }
                                }
                            });
                        });
                        changed |= rgb_row(ui, "Primary Marking Color", &mut a.marking_color);
                        changed |= rgb_row(ui, "Secondary Marking (Calico/Tortie)", &mut a.secondary_marking_color);
                        changed |= rgb_row(ui, "Inner Ear Color", &mut a.inner_ear_color);
                        ui.separator();
                        ui.label("Heterochromia (Independent Eye Colors):");
                        changed |= rgb_row(ui, "Left Eye Color", &mut a.left_eye_color);
                        changed |= rgb_row(ui, "Right Eye Color", &mut a.right_eye_color);
                        if ui.button("Match Both Eyes").clicked() {
                            a.right_eye_color = a.left_eye_color;
                            changed = true;
                        }
                        ui.separator();
                        changed |= rgb_row(ui, "Whiskers", &mut a.whisker_color);
                        changed |= rgb_row(ui, "Nose", &mut a.nose_color);
                        if changed {
                            dirty_draft = true;
                            preview_dirty_to_send = true;
                        }
                    }

                    DesignerTab::Peripherals => {
                        let p = &mut draft.peripherals;
                        let mut changed = false;
                        changed |= rgb_row(ui, "Keyboard Case", &mut p.keyboard_case);
                        changed |= rgb_row(ui, "Keycap Base", &mut p.keycap_base);
                        changed |= rgb_row(ui, "Accent Keys (Esc & Enter)", &mut p.keycap_accent);
                        changed |= rgb_row(ui, "Active Key Highlight", &mut p.keycap_highlight);
                        changed |= rgb_row(ui, "Keycap Legend Color", &mut p.keycap_legend_color);
                        if ui.checkbox(&mut p.show_keycap_legends, "Show Printed Legends on 3D Keycaps (Showcase Mode)").changed() {
                            changed = true;
                        }
                        ui.separator();
                        changed |= rgb_row(ui, "Mouse Body", &mut p.mouse_body);
                        changed |= rgb_row(ui, "Mouse Scroll Wheel", &mut p.mouse_wheel);
                        if changed {
                            dirty_draft = true;
                            preview_dirty_to_send = true;
                        }
                    }

                    DesignerTab::WindowPrivacy => {
                        let mut c_changed = false;
                        ui.label("Window Scaling:");
                        ui.horizontal(|ui| {
                            c_changed |= ui.add(egui::Slider::new(&mut config.scale, 0.4..=2.4).text("Scale")).changed();
                            for (lbl, val) in [("Mini", 0.6), ("Small", 0.85), ("Normal", 1.0), ("Large", 1.5)] {
                                if ui.small_button(lbl).clicked() {
                                    config.scale = val;
                                    c_changed = true;
                                }
                            }
                        });
                        c_changed |= ui.add(egui::Slider::new(&mut config.opacity, 0.15..=1.0).text("Opacity")).changed();
                        c_changed |= ui.checkbox(&mut config.click_through_locked, "Click-Through Lock (disables dragging & head patting)").changed();
                        c_changed |= ui.checkbox(&mut config.auto_start, "Auto-Start on Windows Login").changed();

                        ui.separator();
                        ui.label("Privacy & Typing Display:");
                        c_changed |= ui.checkbox(&mut config.privacy_heuristics_enabled, "Auto-Privacy on Credential Prompts & Password Managers").changed();
                        c_changed |= ui.checkbox(&mut config.showcase_mode, "Showcase Mode (Exact 1:1 key lighting — OFF = Zone-Anonymized default)").changed();
                        if config.showcase_mode {
                            ui.colored_label(
                                Color32::from_rgb(245, 195, 90),
                                "Note: Exact 1:1 key lighting can reveal typed keys on screen shares. Use Ctrl+Alt+P when typing passwords in browsers.",
                            );
                        }
                        c_changed |= ui.checkbox(&mut config.show_floating_popups, "Show Floating White Keycap Pop-Ups (requires Showcase Mode)").changed();

                        ui.separator();
                        ui.label("Gaming & OBS Capture Stage:");
                        ui.horizontal(|ui| {
                            ui.label("Fullscreen Game Hide:");
                            for (scope, lbl) in [
                                (GameHideScope::SameMonitor, "Same Monitor"),
                                (GameHideScope::AnyMonitor, "Any Monitor"),
                                (GameHideScope::Disabled, "Disabled"),
                            ] {
                                if ui.selectable_label(config.game_hide_scope == scope, lbl).clicked() {
                                    config.game_hide_scope = scope;
                                    c_changed = true;
                                }
                            }
                        });
                        c_changed |= ui.checkbox(&mut config.capture_stage_enabled, "Open OBS Capture Stage Window (Green Screen, non-topmost)").changed();
                        c_changed |= rgb_row(ui, "Chroma Key Backdrop", &mut config.chroma_key_color);
                        ui.horizontal(|ui| {
                            for (lbl, hex) in [("Green", 0x00B140), ("Blue", 0x0047BB), ("Magenta", 0xFF00FF)] {
                                if ui.small_button(lbl).clicked() {
                                    config.chroma_key_color = Rgb8::from_hex(hex);
                                    c_changed = true;
                                }
                            }
                        });
                        c_changed |= ui.checkbox(&mut config.chroma_hard_edge, "Hard-Edge Chroma Anti-Spill Mode").changed();

                        if c_changed {
                            config_dirty_to_send = true;
                        }
                    }

                    DesignerTab::Diagnostics => {
                        if ui.button("Refresh Live Diagnostics").clicked() {
                            if let Some(ref c) = pipe_client {
                                c.send(&DesignerToOverlay::RequestDiagnostics);
                            }
                        }
                        ui.add_space(6.0);
                        ui.label(format!("App Version: {}", diagnostics.app_version));
                        ui.label(format!("Pacing Stage: {} ({} FPS, battery={})", diagnostics.pacing_stage, diagnostics.target_fps, diagnostics.on_battery));
                        ui.label(format!("Canvas: {}x{} px @ {} DPI", diagnostics.canvas_w_px, diagnostics.canvas_h_px, diagnostics.dpi));
                        ui.label(format!("Overlay Working Set: {:.1} MB | GDI Objects: {}", diagnostics.working_set_kb as f32 / 1024.0, diagnostics.gdi_objects));
                        ui.label(format!("Render Avg: {} us | Dirty Rect Avg: {}% | Cache Rebuilds: {} | Spring Resets: {}", diagnostics.render_avg_us, diagnostics.dirty_rect_avg_pct, diagnostics.sprite_cache_rebuilds, diagnostics.spring_nan_resets));
                        ui.label(format!("Raw Input Registered: {} ({} ev/s) | UIPI Stale Releases: {}", diagnostics.raw_input_registered, diagnostics.raw_input_events_sec, diagnostics.uipi_stale_releases));
                        ui.label(format!("Privacy Source: {} | Fullscreen Suppressed: {} ({})", diagnostics.privacy_source, diagnostics.fullscreen_suppressed, diagnostics.fullscreen_reason));
                        if !diagnostics.hotkey_conflicts.is_empty() {
                            ui.colored_label(Color32::from_rgb(245, 130, 120), format!("Hotkey Conflicts: {:?}", diagnostics.hotkey_conflicts));
                        }
                        if let Some(ref e) = diagnostics.last_config_error {
                            ui.colored_label(Color32::from_rgb(245, 130, 120), format!("Last Config Error: {e}"));
                        }
                    }
                });
            });
        });

        // Throttled <= 30 Hz live IPC preview & config sync
        if preview_dirty_to_send && last_preview_sent.elapsed() >= Duration::from_millis(33) {
            preview_dirty_to_send = false;
            last_preview_sent = Instant::now();
            preview_seq += 1;
            if let Some(ref c) = pipe_client {
                c.send(&DesignerToOverlay::PreviewPatch {
                    seq: preview_seq,
                    profile_draft: draft.clone(),
                    pose_override,
                });
            }
        }
        if config_dirty_to_send {
            config_dirty_to_send = false;
            if let Some(ref c) = pipe_client {
                c.send(&DesignerToOverlay::UpdateConfig {
                    config: config.clone(),
                });
            } else {
                let _ = save_offline_config(&config);
            }
        }

        // Rasterize egui meshes on CPU and blit to Win32 window
        raster.apply_textures_delta(&full_output.textures_delta);
        let clipped = ctx.tessellate(full_output.shapes, ppp);
        raster.paint(&clipped, ppp);
        blit_bgra(hwnd, raster.width(), raster.height(), raster.bgra_bytes());
    }

    if let Some(ref c) = pipe_client {
        c.send(&DesignerToOverlay::CancelPreview);
    }
    unsafe {
        let _ = KillTimer(hwnd, 1);
        let _ = DestroyWindow(hwnd);
    }
}

fn swatch_pill(ui: &mut egui::Ui, label: &str, rgb: Rgb8, selected: bool) -> egui::Response {
    let fill = if selected {
        Color32::from_rgb(66, 84, 124)
    } else {
        Color32::from_rgb(42, 46, 56)
    };
    let btn = egui::Button::new(format!("●  {label}"))
        .fill(fill)
        .stroke(egui::Stroke::new(
            if selected { 1.5_f32 } else { 1.0_f32 },
            Color32::from_rgb(rgb.r, rgb.g, rgb.b),
        ));
    ui.add(btn)
}

fn rgb_row(ui: &mut egui::Ui, label: &str, rgb: &mut Rgb8) -> bool {
    let mut arr = [rgb.r, rgb.g, rgb.b];
    let mut changed = false;
    ui.horizontal(|ui| {
        if ui.color_edit_button_srgb(&mut arr).changed() {
            rgb.r = arr[0];
            rgb.g = arr[1];
            rgb.b = arr[2];
            changed = true;
        }
        ui.label(format!("{label} ({})", rgb.to_hex_string()));
    });
    changed
}

fn premul_rgba_to_color_image(w: usize, h: usize, rgba: &[u8]) -> ColorImage {
    let mut pixels = Vec::with_capacity(w * h);
    for px in rgba.as_chunks::<4>().0 {
        pixels.push(Color32::from_rgba_premultiplied(px[0], px[1], px[2], px[3]));
    }
    ColorImage {
        size: [w, h],
        pixels,
    }
}

fn launch_overlay_sibling() {
    if let Ok(exe) = std::env::current_exe() {
        let ov = exe.with_file_name("cat-overlay.exe");
        if ov.exists() {
            let _ = Command::new(ov).spawn();
        }
    }
}

fn create_designer_window(hinst: HINSTANCE) -> Result<HWND, String> {
    unsafe {
        let class_name = w!("DesktopCatDesignerHost");
        let wc = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(designer_wndproc),
            hInstance: hinst,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            lpszClassName: class_name,
            ..Default::default()
        };
        let _ = RegisterClassExW(&wc);
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class_name,
            w!("Desktop Cat Designer"),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            920,
            620,
            None,
            None,
            hinst,
            None,
        )
        .map_err(|e| format!("{e}"))
    }
}

fn client_size(hwnd: HWND) -> (usize, usize) {
    unsafe {
        let mut rc = RECT::default();
        let _ = GetClientRect(hwnd, &mut rc);
        ((rc.right - rc.left).max(1) as usize, (rc.bottom - rc.top).max(1) as usize)
    }
}

fn blit_bgra(hwnd: HWND, w: usize, h: usize, bgra: &[u8]) {
    unsafe {
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w as i32,
                biHeight: -(h as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let hdc = GetDC(hwnd);
        if !hdc.is_invalid() {
            let _ = StretchDIBits(
                hdc,
                0,
                0,
                w as i32,
                h as i32,
                0,
                0,
                w as i32,
                h as i32,
                Some(bgra.as_ptr() as *const _),
                &bmi,
                DIB_RGB_COLORS,
                SRCCOPY,
            );
            let _ = ReleaseDC(hwnd, hdc);
        }
    }
}

fn current_modifiers() -> Modifiers {
    unsafe {
        let down = |vk: i32| (GetKeyState(vk) as u16 & 0x8000) != 0;
        let ctrl = down(VK_CONTROL.0 as i32);
        Modifiers {
            alt: down(VK_MENU.0 as i32),
            ctrl,
            shift: down(VK_SHIFT.0 as i32),
            mac_cmd: false,
            command: ctrl,
        }
    }
}

fn vk_to_egui_key(vk: u16) -> Option<Key> {
    match vk {
        x if x == VK_BACK.0 => Some(Key::Backspace),
        x if x == VK_DELETE.0 => Some(Key::Delete),
        x if x == VK_RETURN.0 => Some(Key::Enter),
        x if x == VK_TAB.0 => Some(Key::Tab),
        x if x == VK_ESCAPE.0 => Some(Key::Escape),
        x if x == VK_LEFT.0 => Some(Key::ArrowLeft),
        x if x == VK_RIGHT.0 => Some(Key::ArrowRight),
        x if x == VK_UP.0 => Some(Key::ArrowUp),
        x if x == VK_DOWN.0 => Some(Key::ArrowDown),
        x if x == VK_HOME.0 => Some(Key::Home),
        x if x == VK_END.0 => Some(Key::End),
        _ => None,
    }
}

fn lparam_pos_points(lparam: LPARAM) -> Pos2 {
    let ppp = CUR_PPP.with(|p| *p.borrow()).max(0.5);
    let x = (lparam.0 & 0xFFFF) as i16 as f32 / ppp;
    let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32 / ppp;
    Pos2::new(x, y)
}

unsafe extern "system" fn designer_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_MOUSEMOVE => {
            let pos = lparam_pos_points(lparam);
            WIN_EVENTS.with(|e| e.borrow_mut().egui_events.push(egui::Event::PointerMoved(pos)));
            LRESULT(0)
        }
        WM_LBUTTONDOWN | WM_LBUTTONUP | WM_RBUTTONDOWN | WM_RBUTTONUP => {
            let pos = lparam_pos_points(lparam);
            let btn = if msg == WM_LBUTTONDOWN || msg == WM_LBUTTONUP {
                PointerButton::Primary
            } else {
                PointerButton::Secondary
            };
            let pressed = msg == WM_LBUTTONDOWN || msg == WM_RBUTTONDOWN;
            WIN_EVENTS.with(|e| {
                e.borrow_mut().egui_events.push(egui::Event::PointerButton {
                    pos,
                    button: btn,
                    pressed,
                    modifiers: current_modifiers(),
                })
            });
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            let delta = ((wparam.0 >> 16) as i16) as f32 / 120.0 * 32.0;
            WIN_EVENTS.with(|e| {
                e.borrow_mut().egui_events.push(egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: EguiVec2::new(0.0, delta),
                    modifiers: current_modifiers(),
                })
            });
            LRESULT(0)
        }
        WM_CHAR => {
            if let Some(ch) = char::from_u32(wparam.0 as u32) {
                if !ch.is_control() {
                    WIN_EVENTS.with(|e| {
                        e.borrow_mut()
                            .egui_events
                            .push(egui::Event::Text(ch.to_string()))
                    });
                }
            }
            LRESULT(0)
        }
        WM_KEYDOWN | WM_KEYUP => {
            if let Some(key) = vk_to_egui_key((wparam.0 & 0xFFFF) as u16) {
                WIN_EVENTS.with(|e| {
                    e.borrow_mut().egui_events.push(egui::Event::Key {
                        key,
                        physical_key: None,
                        pressed: msg == WM_KEYDOWN,
                        repeat: false,
                        modifiers: current_modifiers(),
                    })
                });
            }
            LRESULT(0)
        }
        WM_SIZE => {
            let w = (lparam.0 & 0xFFFF) as usize;
            let h = ((lparam.0 >> 16) & 0xFFFF) as usize;
            if w > 0 && h > 0 {
                WIN_EVENTS.with(|e| e.borrow_mut().resized = Some((w, h)));
            }
            LRESULT(0)
        }
        WM_DPICHANGED => {
            let new_dpi = (wparam.0 & 0xFFFF) as u32;
            WIN_EVENTS.with(|e| e.borrow_mut().dpi = Some(new_dpi));
            LRESULT(0)
        }
        WM_APP_IPC_EVENT => {
            WIN_EVENTS.with(|e| e.borrow_mut().ipc_ready = true);
            LRESULT(0)
        }
        WM_TIMER => {
            WIN_EVENTS.with(|e| e.borrow_mut().timer_tick = true);
            LRESULT(0)
        }
        WM_PAINT => DefWindowProcW(hwnd, msg, wparam, lparam),
        WM_CLOSE => {
            WIN_EVENTS.with(|e| e.borrow_mut().closed = true);
            PostQuitMessage(0);
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
