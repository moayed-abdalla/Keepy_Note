use crate::google_tasks::{tasks_fingerprint, TasksClient};
use crate::sticky_tray::{self, StickyTrayMap};
use crate::store::{AppStore, StickyNoteState};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

#[derive(Clone)]
pub struct SyncEngine {
    tasks: Arc<TasksClient>,
    store: Arc<Mutex<AppStore>>,
    store_path: std::path::PathBuf,
    editing: Arc<Mutex<HashMap<String, bool>>>,
    sticky_trays: StickyTrayMap,
}

impl SyncEngine {
    pub fn new(
        tasks: Arc<TasksClient>,
        store: Arc<Mutex<AppStore>>,
        store_path: std::path::PathBuf,
        sticky_trays: StickyTrayMap,
    ) -> Self {
        Self {
            tasks,
            store,
            store_path,
            editing: Arc::new(Mutex::new(HashMap::new())),
            sticky_trays,
        }
    }

    pub fn start(&self, app: AppHandle) {
        let engine = self.clone();
        let app_poll = app.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                let interval = {
                    let s = engine.store.lock();
                    s.settings.poll_interval_secs.max(15)
                };
                tokio::time::sleep(Duration::from_secs(interval)).await;
                if let Err(e) = engine.poll_remote(&app_poll).await {
                    let _ = app_poll.emit("sync-error", e);
                }
            }
        });
    }

    pub fn set_editing(&self, sticky_id: &str, editing: bool) {
        self.editing.lock().insert(sticky_id.to_string(), editing);
    }

    pub async fn poll_remote(&self, app: &AppHandle) -> Result<(), String> {
        let stickies: Vec<StickyNoteState> = self.store.lock().stickies.clone();
        for sticky in stickies {
            if self
                .editing
                .lock()
                .get(&sticky.id)
                .copied()
                .unwrap_or(false)
            {
                continue;
            }

            let list = match self.tasks.get_task_list(&sticky.task_list_id).await {
                Ok(list) => list,
                Err(e) => {
                    if e.contains("404") || e.to_lowercase().contains("not found") {
                        let mut store = self.store.lock();
                        store.stickies.retain(|s| s.id != sticky.id);
                        store.save(&self.store_path)?;
                        sticky_tray::remove(&self.sticky_trays, &sticky.id);
                        if let Some(w) = app.get_webview_window(&format!("sticky-{}", sticky.id)) {
                            let _ = w.close();
                        }
                    }
                    continue;
                }
            };

            let tasks = match self.tasks.list_tasks(&sticky.task_list_id).await {
                Ok(t) => t,
                Err(e) => {
                    let _ = app.emit("sync-error", e);
                    continue;
                }
            };

            let fingerprint = format!("{}::{}", list.title, tasks_fingerprint(&tasks));
            if sticky.remote_updated.as_deref() == Some(fingerprint.as_str())
                && sticky.title == list.title
            {
                continue;
            }

            let items: Vec<_> = tasks.iter().map(|t| t.to_sticky_item()).collect();
            {
                let mut store = self.store.lock();
                if let Some(s) = store.stickies.iter_mut().find(|s| s.id == sticky.id) {
                    s.title = list.title.clone();
                    s.items = items.clone();
                    s.remote_updated = Some(fingerprint);
                }
                store.save(&self.store_path)?;
            }
            let _ = app.emit(
                "sticky-updated",
                serde_json::json!({
                    "id": sticky.id,
                    "title": list.title,
                    "items": items,
                }),
            );
        }
        Ok(())
    }

    pub async fn sync_now(&self, app: &AppHandle) -> Result<(), String> {
        self.poll_remote(app).await
    }
}
