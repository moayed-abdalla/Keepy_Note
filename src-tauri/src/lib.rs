mod credentials;
mod desktop_embed;
mod google_auth;
mod google_tasks;
mod sticky_tray;
mod store;
mod sync;

use credentials::GoogleCredentials;
use google_auth::{AuthState, AuthStatus};
use google_tasks::{tasks_fingerprint, TaskList, TasksClient};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;
use sticky_tray::StickyTrayMap;
use store::{store_path, AppSettings, AppStore, PinMode, StickyNoteState, StickyTaskItem};
use sync::SyncEngine;
use tauri::{AppHandle, Manager, State, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_autostart::MacosLauncher;
use uuid::Uuid;

pub struct AppState {
    pub auth: Arc<AuthState>,
    pub tasks: Arc<TasksClient>,
    pub store: Arc<Mutex<AppStore>>,
    pub store_path: std::path::PathBuf,
    pub sync: Arc<SyncEngine>,
    pub sticky_trays: StickyTrayMap,
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
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    color: String,
) -> Result<(), String> {
    let sticky = {
        let mut store = state.store.lock();
        let sticky = store
            .stickies
            .iter_mut()
            .find(|s| s.id == id)
            .ok_or_else(|| "Sticky not found".to_string())?;
        sticky.color = color;
        let cloned = sticky.clone();
        store.save(&state.store_path)?;
        cloned
    };

    let trays = state.sticky_trays.clone();
    let app2 = app.clone();
    app.run_on_main_thread(move || {
        if let Err(e) = sticky_tray::create_or_update(&app2, &trays, &sticky) {
            eprintln!("update sticky tray color: {e}");
        }
    })
    .map_err(|e| e.to_string())?;
    Ok(())
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
    sticky_tray::remove(&state.sticky_trays, &id);
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
    let trays = state.sticky_trays.clone();
    app.run_on_main_thread(move || {
        if let Err(e) = open_sticky_window(&app2, &sticky) {
            eprintln!("open_sticky: {e}");
        }
        if let Err(e) = sticky_tray::create_or_update(&app2, &trays, &sticky) {
            eprintln!("open_sticky tray: {e}");
        }
    })
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn ensure_sticky_tray(
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
    let trays = state.sticky_trays.clone();
    let app2 = app.clone();
    app.run_on_main_thread(move || {
        if let Err(e) = sticky_tray::create_or_update(&app2, &trays, &sticky) {
            eprintln!("ensure_sticky_tray: {e}");
        }
    })
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn open_main(app: AppHandle) -> Result<(), String> {
    open_main_window(&app)
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

/// Public for sticky_tray::show_sticky re-open path.
pub(crate) fn open_sticky_window_for_tray(
    app: &AppHandle,
    sticky: &StickyNoteState,
) -> Result<(), String> {
    open_sticky_window(app, sticky)
}

pub(crate) fn open_main_window(app: &AppHandle) -> Result<(), String> {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.show();
        let _ = win.set_focus();
        return Ok(());
    }
    WebviewWindowBuilder::new(app, "main", WebviewUrl::App("/".into()))
        .title("Keepy Note")
        .inner_size(440.0, 720.0)
        .resizable(true)
        .build()
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn restore_stickies(app: &AppHandle, state: &AppState) {
    let stickies = state.store.lock().stickies.clone();
    for sticky in stickies {
        let _ = open_sticky_window(app, &sticky);
        if let Err(e) = sticky_tray::create_or_update(app, &state.sticky_trays, &sticky) {
            eprintln!("restore sticky tray: {e}");
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            let _ = open_main_window(app);
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec!["--autostart".into()]),
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
            let sticky_trays: StickyTrayMap = Arc::new(Mutex::new(HashMap::new()));
            let sync = Arc::new(SyncEngine::new(
                tasks.clone(),
                store.clone(),
                path.clone(),
                sticky_trays.clone(),
            ));

            let state = AppState {
                auth: auth.clone(),
                tasks,
                store,
                store_path: path,
                sync: sync.clone(),
                sticky_trays,
            };

            // Lists restore on every launch; main hub only on interactive (non-boot) launch.
            restore_stickies(app.handle(), &state);
            sync.start(app.handle().clone());

            // Apply autostart preference (re-enable so registry picks up --autostart args).
            {
                use tauri_plugin_autostart::ManagerExt;
                let autostart = app.autolaunch();
                if state.store.lock().settings.autostart {
                    let _ = autostart.enable();
                }
            }

            app.manage(state);

            let from_autostart = std::env::args().any(|a| a == "--autostart");
            if !from_autostart {
                let _ = open_main_window(app.handle());
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
            ensure_sticky_tray,
            open_main,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Keepy Note")
        .run(|_app_handle, event| {
            // Stay alive when windows close (list trays keep the process); still allow Quit.
            if let tauri::RunEvent::ExitRequested { api, code, .. } = &event {
                if code.is_none() {
                    api.prevent_exit();
                }
            }
        });
}
