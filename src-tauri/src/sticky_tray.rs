use crate::store::StickyNoteState;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;
use tauri::{
    image::Image,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager,
};

/// Theme colors matching `src/routes/sticky/+page.svelte` CSS.
fn theme_rgb(color: &str) -> ([u8; 3], [u8; 3]) {
    match color {
        "red" => ([0x3d, 0x1c, 0x22], [0xef, 0x8a, 0x96]),
        "orange" => ([0x3d, 0x29, 0x18], [0xf0, 0xa8, 0x6a]),
        "yellow" => ([0x36, 0x30, 0x18], [0xe8, 0xd4, 0x6a]),
        "green" => ([0x1c, 0x33, 0x27], [0x8f, 0xd9, 0xab]),
        "navy" => ([0x1c, 0x2b, 0x45], [0x8f, 0xb4, 0xef]),
        "indigo" => ([0x22, 0x22, 0x40], [0x9a, 0x9a, 0xef]),
        "purple" => ([0x2e, 0x20, 0x40], [0xc4, 0xa6, 0xf5]),
        "graphite" => ([0x2a, 0x2d, 0x31], [0xb8, 0xc0, 0xc9]),
        _ => ([0x1c, 0x2b, 0x45], [0x8f, 0xb4, 0xef]), // default navy
    }
}

/// Build a 32x32 RGBA circle icon tinted to the sticky's theme.
pub fn build_icon(color: &str) -> Image<'static> {
    const SIZE: u32 = 32;
    let (fill, accent) = theme_rgb(color);
    let mut rgba = vec![0u8; (SIZE * SIZE * 4) as usize];
    let cx = (SIZE as f32 - 1.0) / 2.0;
    let cy = cx;
    let outer_r = SIZE as f32 / 2.0 - 1.0;
    let ring_inner = outer_r - 3.0;

    for y in 0..SIZE {
        for x in 0..SIZE {
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            let dist = (dx * dx + dy * dy).sqrt();
            let i = ((y * SIZE + x) * 4) as usize;
            if dist <= ring_inner {
                rgba[i] = fill[0];
                rgba[i + 1] = fill[1];
                rgba[i + 2] = fill[2];
                rgba[i + 3] = 255;
            } else if dist <= outer_r {
                rgba[i] = accent[0];
                rgba[i + 1] = accent[1];
                rgba[i + 2] = accent[2];
                rgba[i + 3] = 255;
            }
        }
    }

    Image::new_owned(rgba, SIZE, SIZE)
}

pub type StickyTrayMap = Arc<Mutex<HashMap<String, TrayIcon>>>;

pub fn remove(trays: &StickyTrayMap, id: &str) {
    let mut map = trays.lock();
    if let Some(tray) = map.remove(id) {
        let _ = tray.set_visible(false);
        drop(tray);
    }
}

/// Create or refresh a per-sticky tray icon (colored to the note's theme).
pub fn create_or_update(
    app: &AppHandle,
    trays: &StickyTrayMap,
    sticky: &StickyNoteState,
) -> Result<(), String> {
    let icon = build_icon(&sticky.color);
    let title = if sticky.title.trim().is_empty() {
        "Sticky".to_string()
    } else {
        sticky.title.clone()
    };

    {
        let mut map = trays.lock();
        if let Some(existing) = map.get_mut(&sticky.id) {
            let _ = existing.set_icon(Some(icon));
            let _ = existing.set_tooltip(Some(&title));
            return Ok(());
        }
    }

    let sticky_id = sticky.id.clone();
    let sticky_id_menu = sticky_id.clone();
    let sticky_id_click = sticky_id.clone();

    let show = MenuItem::with_id(
        app,
        format!("show-{sticky_id}"),
        "Show",
        true,
        None::<&str>,
    )
    .map_err(|e| e.to_string())?;
    let open_main = MenuItem::with_id(
        app,
        format!("open-main-{sticky_id}"),
        "Open Keepy Note",
        true,
        None::<&str>,
    )
    .map_err(|e| e.to_string())?;
    let close = MenuItem::with_id(
        app,
        format!("close-{sticky_id}"),
        "Close",
        true,
        None::<&str>,
    )
    .map_err(|e| e.to_string())?;
    let quit = MenuItem::with_id(
        app,
        format!("quit-{sticky_id}"),
        "Quit",
        true,
        None::<&str>,
    )
    .map_err(|e| e.to_string())?;
    let menu = Menu::with_items(app, &[&show, &open_main, &close, &quit]).map_err(|e| e.to_string())?;

    let tray_id = format!("sticky-tray-{sticky_id}");
    let tray = TrayIconBuilder::with_id(&tray_id)
        .icon(icon)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip(&title)
        .on_menu_event(move |app, event| {
            let id_str = event.id.as_ref();
            if id_str == format!("show-{sticky_id_menu}") {
                show_sticky(app, &sticky_id_menu);
            } else if id_str == format!("open-main-{sticky_id_menu}") {
                let _ = crate::open_main_window(app);
            } else if id_str == format!("close-{sticky_id_menu}") {
                close_sticky(app, &sticky_id_menu);
            } else if id_str == format!("quit-{sticky_id_menu}") {
                app.exit(0);
            }
        })
        .on_tray_icon_event(move |tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_sticky(tray.app_handle(), &sticky_id_click);
            }
        })
        .build(app)
        .map_err(|e| e.to_string())?;

    trays.lock().insert(sticky_id, tray);
    Ok(())
}

fn show_sticky(app: &AppHandle, id: &str) {
    let label = format!("sticky-{id}");
    if let Some(win) = app.get_webview_window(&label) {
        let _ = win.show();
        let _ = win.set_focus();
        return;
    }
    // Window missing — re-open via the open_sticky path on the main thread.
    let app2 = app.clone();
    let id = id.to_string();
    let _ = app.run_on_main_thread(move || {
        if let Some(state) = app2.try_state::<crate::AppState>() {
            let sticky = {
                let store = state.store.lock();
                store.stickies.iter().find(|s| s.id == id).cloned()
            };
            if let Some(sticky) = sticky {
                let _ = crate::open_sticky_window_for_tray(&app2, &sticky);
            }
        }
    });
}

fn close_sticky(app: &AppHandle, id: &str) {
    if let Some(state) = app.try_state::<crate::AppState>() {
        {
            let mut store = state.store.lock();
            store.stickies.retain(|s| s.id != id);
            let _ = store.save(&state.store_path);
        }
        remove(&state.sticky_trays, id);
    }
    if let Some(win) = app.get_webview_window(&format!("sticky-{id}")) {
        let _ = win.close();
    }
}
