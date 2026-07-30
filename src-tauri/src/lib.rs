mod credentials;
mod desktop_embed;
mod google_auth;
mod google_tasks;
mod store;
mod sync;

use credentials::GoogleCredentials;
use google_auth::{AuthState, AuthStatus};
use google_tasks::{tasks_fingerprint, TaskList, TasksClient};
use parking_lot::Mutex;
use std::sync::Arc;
use store::{store_path, AppSettings, AppStore, PinMode, StickyNoteState, StickyTaskItem};
use sync::SyncEngine;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, State, WebviewUrl, WebviewWindowBuilder,
};
use tauri_plugin_autostart::MacosLauncher;
use uuid::Uuid;

pub struct AppState {
    pub auth: Arc<AuthState>,
    pub tasks: Arc<TasksClient>,
    pub store: Arc<Mutex<AppStore>>,
    pub store_path: std::path::PathBuf,
    pub sync: Arc<SyncEngine>,
}

#[tauri::command]
async fn auth_status(state: State<'_, AppState>) -> Result<AuthStatus, String> {
    Ok(state.auth.status())
}

#[tauri::command]
async fn login(state: State<'_, AppState>) -> Result<AuthStatus, String> {
    state.auth.login().await?;
    Ok(state.auth.status())
}

#[tauri::command]
async fn logout(state: State<'_, AppState>) -> Result<AuthStatus, String> {
    state.auth.sign_out()?;
    Ok(state.auth.status())
}

#[tauri::command]
async fn get_settings(state: State<'_, AppState>) -> Result<AppSettings, String> {
    Ok(state.store.lock().settings.clone())
}

#[tauri::command]
async fn save_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: AppSettings,
) -> Result<AppSettings, String> {
    {
        let mut store = state.store.lock();
        store.settings = settings.clone();
        store.save(&state.store_path)?;
    }
    use tauri_plugin_autostart::ManagerExt;
    let autostart = app.autolaunch();
    if settings.autostart {
        let _ = autostart.enable();
    } else {
        let _ = autostart.disable();
    }
    Ok(settings)
}

#[tauri::command]
async fn list_stickies(state: State<'_, AppState>) -> Result<Vec<StickyNoteState>, String> {
    Ok(state.store.lock().stickies.clone())
}

#[tauri::command]
async fn get_sticky(state: State<'_, AppState>, id: String) -> Result<StickyNoteState, String> {
    state
        .store
        .lock()
        .stickies
        .iter()
        .find(|s| s.id == id)
        .cloned()
        .ok_or_else(|| "Sticky not found".into())
}

#[tauri::command]
async fn list_task_lists(state: State<'_, AppState>) -> Result<Vec<TaskList>, String> {
    if !state.auth.is_signed_in() {
        return Err("Not signed in".into());
    }
    state.tasks.list_task_lists().await
}

#[tauri::command]
async fn create_list_and_pin(
    state: State<'_, AppState>,
    title: String,
) -> Result<StickyNoteState, String> {
    if !state.auth.is_signed_in() {
        return Err("Not signed in".into());
    }
    let name = title.trim();
    let name = if name.is_empty() { "Untitled list" } else { name };
    let list = state.tasks.create_task_list(name).await?;
    // Window is opened by the frontend to avoid a WebView2 IPC deadlock on Windows.
    pin_list_inner(&state, list.id, list.title, Vec::new(), None)
}

#[tauri::command]
async fn pin_list(
    state: State<'_, AppState>,
    task_list_id: String,
    title: Option<String>,
) -> Result<StickyNoteState, String> {
    eprintln!("pin_list: start id={task_list_id}");
    if !state.auth.is_signed_in() {
        return Err("Not signed in".into());
    }
    if task_list_id.trim().is_empty() {
        return Err("Missing task list id".into());
    }

    // Prefer the title from the picker; only hit get_task_list if needed.
    let list_title = {
        let from_picker = title
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty());
        if let Some(t) = from_picker {
            t
        } else {
            eprintln!("pin_list: fetching list metadata");
            state.tasks.get_task_list(&task_list_id).await?.title
        }
    };

    eprintln!("pin_list: fetching tasks");
    let tasks = match tokio::time::timeout(
        std::time::Duration::from_secs(20),
        state.tasks.list_tasks(&task_list_id),
    )
    .await
    {
        Ok(Ok(t)) => t,
        Ok(Err(e)) => {
            // Still pin an empty checklist so a transient network blip doesn't block pinning.
            eprintln!("pin_list: list_tasks error ({e}); pinning empty checklist");
            Vec::new()
        }
        Err(_) => {
            eprintln!("pin_list: list_tasks timed out; pinning empty checklist");
            Vec::new()
        }
    };

    let fingerprint = format!("{}::{}", list_title, tasks_fingerprint(&tasks));
    let items: Vec<StickyTaskItem> = tasks.iter().map(|t| t.to_sticky_item()).collect();
    eprintln!("pin_list: creating sticky ({} items)", items.len());
    let _ = std::io::Write::flush(&mut std::io::stderr());

    // parking_lot locks are blocking; run store writes off the async worker threads.
    let store = state.store.clone();
    let store_path = state.store_path.clone();
    let sticky = match tokio::time::timeout(
        std::time::Duration::from_secs(8),
        tokio::task::spawn_blocking(move || {
            pin_list_inner_owned(
                store,
                store_path,
                task_list_id,
                list_title,
                items,
                Some(fingerprint),
            )
        }),
    )
    .await
    {
        Ok(Ok(result)) => result?,
        Ok(Err(e)) => return Err(format!("pin_list join error: {e}")),
        Err(_) => {
            return Err(
                "Timed out while saving the sticky. Try closing other Keepy Note instances and pin again."
                    .into(),
            )
        }
    };

    eprintln!("pin_list: done id={}", sticky.id);
    let _ = std::io::Write::flush(&mut std::io::stderr());
    Ok(sticky)
}

fn pin_list_inner(
    state: &AppState,
    task_list_id: String,
    title: String,
    items: Vec<StickyTaskItem>,
    remote_updated: Option<String>,
) -> Result<StickyNoteState, String> {
    pin_list_inner_owned(
        state.store.clone(),
        state.store_path.clone(),
        task_list_id,
        title,
        items,
        remote_updated,
    )
}

fn pin_list_inner_owned(
    store: Arc<Mutex<AppStore>>,
    store_path: std::path::PathBuf,
    task_list_id: String,
    title: String,
    items: Vec<StickyTaskItem>,
    remote_updated: Option<String>,
) -> Result<StickyNoteState, String> {
    use std::io::Write;
    eprintln!("pin_list_inner: check duplicate");
    let _ = std::io::stderr().flush();

    let count = {
        let guard = store.try_lock().ok_or_else(|| {
            "App store is busy (another operation is in progress). Try again in a moment.".to_string()
        })?;
        // Re-pinning an already-saved list should just re-open its sticky.
        if let Some(existing) = guard.stickies.iter().find(|s| s.task_list_id == task_list_id) {
            eprintln!("pin_list_inner: already pinned id={}", existing.id);
            return Ok(existing.clone());
        }
        guard.stickies.len() as f64
    };

    eprintln!("pin_list_inner: build struct");
    let _ = std::io::stderr().flush();
    let sticky = StickyNoteState {
        id: Uuid::new_v4().to_string(),
        task_list_id,
        title,
        items,
        color: "navy".into(),
        x: 80.0 + (count * 24.0),
        y: 80.0 + (count * 24.0),
        width: 280.0,
        height: 360.0,
        pin_mode: PinMode::AlwaysOnTop,
        remote_updated,
    };

    eprintln!("pin_list_inner: saving {}", sticky.id);
    let _ = std::io::stderr().flush();
    {
        let mut guard = store.try_lock().ok_or_else(|| {
            "App store is busy while saving. Try again in a moment.".to_string()
        })?;
        guard.stickies.push(sticky.clone());
        guard.save(&store_path)?;
    }
    eprintln!("pin_list_inner: saved");
    let _ = std::io::stderr().flush();

    Ok(sticky)
}

#[tauri::command]
async fn rename_list(
    state: State<'_, AppState>,
    id: String,
    title: String,
) -> Result<(), String> {
    if !state.auth.is_signed_in() {
        return Err("Not signed in".into());
    }
    let name = title.trim().to_string();
    if name.is_empty() {
        return Err("List title cannot be empty".into());
    }

    let list_id = {
        let store = state.store.lock();
        store
            .stickies
            .iter()
            .find(|s| s.id == id)
            .map(|s| s.task_list_id.clone())
            .ok_or_else(|| "Sticky not found".to_string())?
    };

    let updated = state.tasks.update_task_list(&list_id, &name).await?;

    {
        let mut store = state.store.lock();
        if let Some(s) = store.stickies.iter_mut().find(|s| s.id == id) {
            s.title = updated.title;
            // Invalidate fingerprint so next poll refreshes cleanly
            s.remote_updated = None;
        }
        store.save(&state.store_path)?;
    }
    Ok(())
}

#[tauri::command]
async fn add_task(
    state: State<'_, AppState>,
    sticky_id: String,
    title: String,
) -> Result<StickyTaskItem, String> {
    if !state.auth.is_signed_in() {
        return Err("Not signed in".into());
    }
    let name = title.trim().to_string();
    if name.is_empty() {
        return Err("Task title cannot be empty".into());
    }

    let list_id = {
        let store = state.store.lock();
        store
            .stickies
            .iter()
            .find(|s| s.id == sticky_id)
            .map(|s| s.task_list_id.clone())
            .ok_or_else(|| "Sticky not found".to_string())?
    };

    let task = state.tasks.create_task(&list_id, &name, None).await?;
    let item = task.to_sticky_item();

    {
        let mut store = state.store.lock();
        if let Some(s) = store.stickies.iter_mut().find(|s| s.id == sticky_id) {
            s.items.push(item.clone());
            s.remote_updated = None;
        }
        store.save(&state.store_path)?;
    }
    Ok(item)
}

#[tauri::command]
async fn update_task_title(
    state: State<'_, AppState>,
    sticky_id: String,
    task_id: String,
    title: String,
) -> Result<StickyTaskItem, String> {
    if !state.auth.is_signed_in() {
        return Err("Not signed in".into());
    }
    let name = title.trim().to_string();

    let list_id = {
        let store = state.store.lock();
        store
            .stickies
            .iter()
            .find(|s| s.id == sticky_id)
            .map(|s| s.task_list_id.clone())
            .ok_or_else(|| "Sticky not found".to_string())?
    };

    let task = state
        .tasks
        .update_task(&list_id, &task_id, Some(name), None, None)
        .await?;
    let item = task.to_sticky_item();

    {
        let mut store = state.store.lock();
        if let Some(s) = store.stickies.iter_mut().find(|s| s.id == sticky_id) {
            if let Some(existing) = s.items.iter_mut().find(|i| i.id == task_id) {
                *existing = item.clone();
            }
            s.remote_updated = None;
        }
        store.save(&state.store_path)?;
    }
    Ok(item)
}

#[tauri::command]
async fn set_task_status(
    state: State<'_, AppState>,
    sticky_id: String,
    task_id: String,
    completed: bool,
) -> Result<StickyTaskItem, String> {
    if !state.auth.is_signed_in() {
        return Err("Not signed in".into());
    }

    let list_id = {
        let store = state.store.lock();
        store
            .stickies
            .iter()
            .find(|s| s.id == sticky_id)
            .map(|s| s.task_list_id.clone())
            .ok_or_else(|| "Sticky not found".to_string())?
    };

    let status = if completed {
        "completed".to_string()
    } else {
        "needsAction".to_string()
    };

    let task = state
        .tasks
        .update_task(&list_id, &task_id, None, None, Some(status))
        .await?;
    let item = task.to_sticky_item();

    {
        let mut store = state.store.lock();
        if let Some(s) = store.stickies.iter_mut().find(|s| s.id == sticky_id) {
            if let Some(existing) = s.items.iter_mut().find(|i| i.id == task_id) {
                *existing = item.clone();
            }
            s.remote_updated = None;
        }
        store.save(&state.store_path)?;
    }
    Ok(item)
}

#[tauri::command]
async fn delete_task(
    state: State<'_, AppState>,
    sticky_id: String,
    task_id: String,
) -> Result<(), String> {
    if !state.auth.is_signed_in() {
        return Err("Not signed in".into());
    }

    let list_id = {
        let store = state.store.lock();
        store
            .stickies
            .iter()
            .find(|s| s.id == sticky_id)
            .map(|s| s.task_list_id.clone())
            .ok_or_else(|| "Sticky not found".to_string())?
    };

    state.tasks.delete_task(&list_id, &task_id).await?;

    {
        let mut store = state.store.lock();
        if let Some(s) = store.stickies.iter_mut().find(|s| s.id == sticky_id) {
            s.items.retain(|i| i.id != task_id);
            s.remote_updated = None;
        }
        store.save(&state.store_path)?;
    }
    Ok(())
}

#[tauri::command]
async fn reorder_task(
    state: State<'_, AppState>,
    sticky_id: String,
    task_id: String,
    previous_task_id: Option<String>,
) -> Result<Vec<StickyTaskItem>, String> {
    if !state.auth.is_signed_in() {
        return Err("Not signed in".into());
    }

    let list_id = {
        let store = state.store.lock();
        store
            .stickies
            .iter()
            .find(|s| s.id == sticky_id)
            .map(|s| s.task_list_id.clone())
            .ok_or_else(|| "Sticky not found".to_string())?
    };

    state
        .tasks
        .move_task(&list_id, &task_id, previous_task_id.as_deref())
        .await?;

    let tasks = state.tasks.list_tasks(&list_id).await?;
    let items: Vec<StickyTaskItem> = tasks.iter().map(|t| t.to_sticky_item()).collect();
    let fingerprint = {
        let title = {
            let store = state.store.lock();
            store
                .stickies
                .iter()
                .find(|s| s.id == sticky_id)
                .map(|s| s.title.clone())
                .unwrap_or_default()
        };
        format!("{}::{}", title, tasks_fingerprint(&tasks))
    };

    {
        let mut store = state.store.lock();
        if let Some(s) = store.stickies.iter_mut().find(|s| s.id == sticky_id) {
            s.items = items.clone();
            s.remote_updated = Some(fingerprint);
        }
        store.save(&state.store_path)?;
    }
    Ok(items)
}

#[tauri::command]
async fn set_sticky_editing(
    state: State<'_, AppState>,
    id: String,
    editing: bool,
) -> Result<(), String> {
    state.sync.set_editing(&id, editing);
    Ok(())
}

#[tauri::command]
async fn update_sticky_geometry(
    state: State<'_, AppState>,
    id: String,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> Result<(), String> {
    let mut store = state.store.lock();
    let sticky = store
        .stickies
        .iter_mut()
        .find(|s| s.id == id)
        .ok_or_else(|| "Sticky not found".to_string())?;
    sticky.x = x;
    sticky.y = y;
    sticky.width = width;
    sticky.height = height;
    store.save(&state.store_path)
}

#[tauri::command]
async fn update_sticky_color(
    state: State<'_, AppState>,
    id: String,
    color: String,
) -> Result<(), String> {
    let mut store = state.store.lock();
    let sticky = store
        .stickies
        .iter_mut()
        .find(|s| s.id == id)
        .ok_or_else(|| "Sticky not found".to_string())?;
    sticky.color = color;
    store.save(&state.store_path)
}

#[tauri::command]
async fn set_pin_mode(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    mode: PinMode,
) -> Result<PinMode, String> {
    {
        let mut store = state.store.lock();
        let sticky = store
            .stickies
            .iter_mut()
            .find(|s| s.id == id)
            .ok_or_else(|| "Sticky not found".to_string())?;
        sticky.pin_mode = mode.clone();
        store.save(&state.store_path)?;
    }

    let label = format!("sticky-{id}");
    if let Some(win) = app.get_webview_window(&label) {
        match mode {
            PinMode::AlwaysOnTop => {
                #[cfg(windows)]
                {
                    if let Ok(hwnd) = win.hwnd() {
                        let _ = desktop_embed::unembed_window(hwnd.0 as isize);
                    }
                }
                let _ = win.set_always_on_top(true);
                let _ = win.set_skip_taskbar(true);
            }
            PinMode::DesktopEmbed => {
                let _ = win.set_always_on_top(false);
                #[cfg(windows)]
                {
                    match win.hwnd() {
                        Ok(hwnd) => {
                            if let Err(e) = desktop_embed::embed_window(hwnd.0 as isize) {
                                eprintln!("desktop mode apply failed: {e}");
                            }
                        }
                        Err(e) => return Err(e.to_string()),
                    }
                }
                #[cfg(not(windows))]
                {
                    // Still a useful "not always on top" mode elsewhere.
                }
            }
        }
    }
    Ok(mode)
}

#[tauri::command]
async fn unpin_sticky(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<(), String> {
    {
        let mut store = state.store.lock();
        store.stickies.retain(|s| s.id != id);
        store.save(&state.store_path)?;
    }
    if let Some(win) = app.get_webview_window(&format!("sticky-{id}")) {
        let _ = win.close();
    }
    Ok(())
}

#[tauri::command]
async fn sync_now(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    state.sync.sync_now(&app).await
}

#[tauri::command]
async fn open_sticky(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    let sticky = {
        let store = state.store.lock();
        store
            .stickies
            .iter()
            .find(|s| s.id == id)
            .cloned()
            .ok_or_else(|| "Sticky not found".to_string())?
    };

    // Creating WebView windows from an async command can deadlock on Windows.
    // Schedule on the main thread instead.
    let app2 = app.clone();
    app.run_on_main_thread(move || {
        if let Err(e) = open_sticky_window(&app2, &sticky) {
            eprintln!("open_sticky: {e}");
        }
    })
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn open_picker(app: AppHandle) -> Result<(), String> {
    open_picker_window(&app)
}

#[tauri::command]
async fn open_settings(app: AppHandle) -> Result<(), String> {
    open_settings_window(&app)
}

fn open_sticky_window(app: &AppHandle, sticky: &StickyNoteState) -> Result<(), String> {
    let label = format!("sticky-{}", sticky.id);
    if let Some(win) = app.get_webview_window(&label) {
        let _ = win.set_focus();
        return Ok(());
    }

    let url = WebviewUrl::App(format!("/sticky?id={}", sticky.id).into());
    let win = WebviewWindowBuilder::new(app, &label, url)
        .title(&sticky.title)
        .inner_size(sticky.width, sticky.height)
        .min_inner_size(200.0, 160.0)
        .position(sticky.x, sticky.y)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(matches!(sticky.pin_mode, PinMode::AlwaysOnTop))
        .skip_taskbar(true)
        .resizable(true)
        .build()
        .map_err(|e| e.to_string())?;

    if matches!(sticky.pin_mode, PinMode::DesktopEmbed) {
        #[cfg(windows)]
        {
            if let Ok(hwnd) = win.hwnd() {
                if desktop_embed::embed_window(hwnd.0 as isize).is_err() {
                    let _ = win.set_always_on_top(true);
                }
            }
        }
    }
    let _ = win.set_focus();
    Ok(())
}

fn open_picker_window(app: &AppHandle) -> Result<(), String> {
    if let Some(win) = app.get_webview_window("picker") {
        // Reload so a previous hung/busy pin attempt can't leave the UI unresponsive.
        let _ = win.eval("window.location.replace('/picker')");
        let _ = win.show();
        let _ = win.set_focus();
        return Ok(());
    }
    WebviewWindowBuilder::new(app, "picker", WebviewUrl::App("/picker".into()))
        .title("Add Sticky List")
        .inner_size(420.0, 560.0)
        .resizable(true)
        .always_on_top(true)
        .build()
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn open_settings_window(app: &AppHandle) -> Result<(), String> {
    if let Some(win) = app.get_webview_window("settings") {
        let _ = win.set_focus();
        return Ok(());
    }
    WebviewWindowBuilder::new(app, "settings", WebviewUrl::App("/settings".into()))
        .title("Keepy Note")
        .inner_size(440.0, 560.0)
        .resizable(true)
        .build()
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn restore_stickies(app: &AppHandle, state: &AppState) {
    let stickies = state.store.lock().stickies.clone();
    for sticky in stickies {
        let _ = open_sticky_window(app, &sticky);
    }
}

fn setup_tray(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let add = MenuItem::with_id(app, "add", "Add Sticky List", true, None::<&str>)?;
    let sync = MenuItem::with_id(app, "sync", "Sync Now", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&add, &sync, &settings, &quit])?;

    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or_else(|| "Missing app icon for system tray".to_string())?;

    let tray = TrayIconBuilder::new()
        .icon(icon)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip("Keepy Note — right-click for menu")
        .on_menu_event(|app, event| match event.id.as_ref() {
            "add" => {
                let _ = open_picker_window(app);
            }
            "sync" => {
                let app2 = app.clone();
                tauri::async_runtime::spawn(async move {
                    if let Some(state) = app2.try_state::<AppState>() {
                        let _ = state.sync.sync_now(&app2).await;
                    }
                });
            }
            "settings" => {
                let _ = open_settings_window(app);
            }
            "quit" => {
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                let _ = open_picker_window(app);
            }
        })
        .build(app)?;

    // Keep the tray alive for the whole app lifetime (dropping it removes the icon
    // and can make a tray-only app look like it never started).
    app.manage(tray);
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec![]),
        ))
        .setup(|app| {
            let data_dir = app
                .path()
                .app_data_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from("."));
            let resource_dir = app.path().resource_dir().ok();

            let creds = match GoogleCredentials::load_with_dirs(&data_dir, resource_dir.as_ref()) {
                Ok(c) => {
                    // Persist a copy under AppData so the installed app always finds them.
                    let dest = data_dir.join("google_credentials.json");
                    if !dest.exists() {
                        let _ = serde_json::to_string_pretty(&c)
                            .ok()
                            .and_then(|raw| std::fs::write(&dest, raw).ok());
                    }
                    c
                }
                Err(e) => {
                    eprintln!("Keepy Note credentials warning: {e}");
                    GoogleCredentials {
                        client_id: String::new(),
                        client_secret: String::new(),
                    }
                }
            };

            let auth = Arc::new(AuthState::new(creds));
            let tasks = Arc::new(TasksClient::new(auth.clone()));

            let path = store_path(data_dir);
            let loaded = AppStore::load(&path);
            let store = Arc::new(Mutex::new(loaded));
            let sync = Arc::new(SyncEngine::new(tasks.clone(), store.clone(), path.clone()));

            let state = AppState {
                auth: auth.clone(),
                tasks,
                store,
                store_path: path,
                sync: sync.clone(),
            };

            // Hide the default main window — tray-only presence
            if let Some(main) = app.get_webview_window("main") {
                let _ = main.hide();
            }

            setup_tray(app.handle())?;
            restore_stickies(app.handle(), &state);
            sync.start(app.handle().clone());

            // Apply autostart preference
            {
                use tauri_plugin_autostart::ManagerExt;
                let autostart = app.autolaunch();
                if state.store.lock().settings.autostart {
                    let _ = autostart.enable();
                }
            }

            let signed_in = state.auth.is_signed_in();
            let has_stickies = !state.store.lock().stickies.is_empty();
            app.manage(state);

            // Always show a visible window on launch so Start Menu launches aren't "invisible".
            let _ = open_settings_window(app.handle());
            if signed_in && !has_stickies {
                let _ = open_picker_window(app.handle());
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            auth_status,
            login,
            logout,
            get_settings,
            save_settings,
            list_stickies,
            get_sticky,
            list_task_lists,
            create_list_and_pin,
            pin_list,
            rename_list,
            add_task,
            update_task_title,
            set_task_status,
            delete_task,
            reorder_task,
            set_sticky_editing,
            update_sticky_geometry,
            update_sticky_color,
            set_pin_mode,
            unpin_sticky,
            sync_now,
            open_sticky,
            open_picker,
            open_settings,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Keepy Note")
        .run(|_app_handle, event| {
            // Stay alive in the tray when windows close; still allow Quit.
            if let tauri::RunEvent::ExitRequested { api, code, .. } = &event {
                if code.is_none() {
                    api.prevent_exit();
                }
            }
        });
}
