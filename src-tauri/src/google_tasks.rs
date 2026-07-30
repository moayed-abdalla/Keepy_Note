use crate::google_auth::AuthState;
use crate::store::StickyTaskItem;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use url::Url;

const TASKS_BASE: &str = "https://tasks.googleapis.com/tasks/v1";

fn append_path_segment(base: &str, segment: &str) -> Result<String, String> {
    let mut url = Url::parse(base).map_err(|e| e.to_string())?;
    {
        let mut segments = url
            .path_segments_mut()
            .map_err(|_| "tasks API base URL cannot be a base".to_string())?;
        segments.pop_if_empty();
        segments.push(segment);
    }
    Ok(url.into())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskList {
    pub id: String,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: String,
    pub title: Option<String>,
    pub notes: Option<String>,
    pub status: Option<String>,
    pub updated: Option<String>,
    pub parent: Option<String>,
    pub position: Option<String>,
}

impl Task {
    pub fn to_sticky_item(&self) -> StickyTaskItem {
        StickyTaskItem {
            id: self.id.clone(),
            title: self.title.clone().unwrap_or_default(),
            status: self
                .status
                .clone()
                .unwrap_or_else(|| "needsAction".into()),
            position: self.position.clone(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct TaskListResponse {
    items: Option<Vec<TaskList>>,
}

#[derive(Debug, Deserialize)]
struct TaskResponse {
    items: Option<Vec<Task>>,
}

#[derive(Debug, Serialize)]
struct CreateTaskListBody {
    title: String,
}

#[derive(Debug, Serialize)]
struct PatchTaskListBody {
    title: String,
}

#[derive(Debug, Serialize)]
struct CreateTaskBody {
    title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    notes: Option<String>,
}

#[derive(Debug, Serialize)]
struct PatchTaskBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    notes: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<String>,
}

pub struct TasksClient {
    auth: Arc<AuthState>,
    http: reqwest::Client,
}

impl TasksClient {
    pub fn new(auth: Arc<AuthState>) -> Self {
        Self {
            auth,
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(20))
                .connect_timeout(Duration::from_secs(10))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
        }
    }

    async fn authorized_get(&self, url: &str) -> Result<reqwest::Response, String> {
        let token = self.auth.access_token().await?;
        self.http
            .get(url)
            .bearer_auth(token)
            .send()
            .await
            .map_err(|e| e.to_string())
    }

    async fn authorized_post<T: Serialize>(
        &self,
        url: &str,
        body: &T,
    ) -> Result<reqwest::Response, String> {
        let token = self.auth.access_token().await?;
        self.http
            .post(url)
            .bearer_auth(token)
            .json(body)
            .send()
            .await
            .map_err(|e| e.to_string())
    }

    async fn authorized_patch<T: Serialize>(
        &self,
        url: &str,
        body: &T,
    ) -> Result<reqwest::Response, String> {
        let token = self.auth.access_token().await?;
        self.http
            .patch(url)
            .bearer_auth(token)
            .json(body)
            .send()
            .await
            .map_err(|e| e.to_string())
    }

    async fn authorized_delete(&self, url: &str) -> Result<reqwest::Response, String> {
        let token = self.auth.access_token().await?;
        self.http
            .delete(url)
            .bearer_auth(token)
            .send()
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn list_task_lists(&self) -> Result<Vec<TaskList>, String> {
        let resp = self
            .authorized_get(&format!("{TASKS_BASE}/users/@me/lists"))
            .await?;
        if !resp.status().is_success() {
            return Err(format!(
                "list task lists failed: {}",
                resp.text().await.unwrap_or_default()
            ));
        }
        let parsed: TaskListResponse = resp.json().await.map_err(|e| e.to_string())?;
        Ok(parsed.items.unwrap_or_default())
    }

    pub async fn get_task_list(&self, task_list_id: &str) -> Result<TaskList, String> {
        let url = append_path_segment(&format!("{TASKS_BASE}/users/@me/lists/"), task_list_id)?;
        let resp = self.authorized_get(&url).await?;
        if !resp.status().is_success() {
            return Err(format!(
                "get task list failed: {}",
                resp.text().await.unwrap_or_default()
            ));
        }
        resp.json().await.map_err(|e| e.to_string())
    }

    pub async fn create_task_list(&self, title: &str) -> Result<TaskList, String> {
        let resp = self
            .authorized_post(
                &format!("{TASKS_BASE}/users/@me/lists"),
                &CreateTaskListBody {
                    title: title.to_string(),
                },
            )
            .await?;
        if !resp.status().is_success() {
            return Err(format!(
                "create task list failed: {}",
                resp.text().await.unwrap_or_default()
            ));
        }
        resp.json().await.map_err(|e| e.to_string())
    }

    pub async fn update_task_list(
        &self,
        task_list_id: &str,
        title: &str,
    ) -> Result<TaskList, String> {
        let url = append_path_segment(&format!("{TASKS_BASE}/users/@me/lists/"), task_list_id)?;
        let resp = self
            .authorized_patch(
                &url,
                &PatchTaskListBody {
                    title: title.to_string(),
                },
            )
            .await?;
        if !resp.status().is_success() {
            return Err(format!(
                "update task list failed: {}",
                resp.text().await.unwrap_or_default()
            ));
        }
        resp.json().await.map_err(|e| e.to_string())
    }

    /// Top-level tasks only, sorted by Google position.
    pub async fn list_tasks(&self, task_list_id: &str) -> Result<Vec<Task>, String> {
        let mut url = append_path_segment(&format!("{TASKS_BASE}/lists/"), task_list_id)?;
        url.push_str("/tasks");
        let mut parsed = Url::parse(&url).map_err(|e| e.to_string())?;
        parsed
            .query_pairs_mut()
            .append_pair("showCompleted", "true")
            .append_pair("showHidden", "false")
            .append_pair("maxResults", "100");
        let resp = self.authorized_get(parsed.as_str()).await?;
        if !resp.status().is_success() {
            return Err(format!(
                "list tasks failed: {}",
                resp.text().await.unwrap_or_default()
            ));
        }
        let parsed: TaskResponse = resp.json().await.map_err(|e| e.to_string())?;
        let mut items = parsed.items.unwrap_or_default();
        items.retain(|t| t.parent.is_none());
        items.sort_by(|a, b| a.position.cmp(&b.position));
        Ok(items)
    }

    pub async fn create_task(
        &self,
        task_list_id: &str,
        title: &str,
        notes: Option<&str>,
    ) -> Result<Task, String> {
        let body = CreateTaskBody {
            title: title.to_string(),
            notes: notes.map(|s| s.to_string()),
        };
        let mut url = append_path_segment(&format!("{TASKS_BASE}/lists/"), task_list_id)?;
        url.push_str("/tasks");
        let resp = self.authorized_post(&url, &body).await?;
        if !resp.status().is_success() {
            return Err(format!(
                "create task failed: {}",
                resp.text().await.unwrap_or_default()
            ));
        }
        resp.json().await.map_err(|e| e.to_string())
    }

    pub async fn update_task(
        &self,
        task_list_id: &str,
        task_id: &str,
        title: Option<String>,
        notes: Option<String>,
        status: Option<String>,
    ) -> Result<Task, String> {
        let body = PatchTaskBody {
            title,
            notes,
            status,
        };
        let mut url = append_path_segment(&format!("{TASKS_BASE}/lists/"), task_list_id)?;
        url.push_str("/tasks/");
        let url = append_path_segment(&url, task_id)?;
        let resp = self.authorized_patch(&url, &body).await?;
        if !resp.status().is_success() {
            return Err(format!(
                "update task failed: {}",
                resp.text().await.unwrap_or_default()
            ));
        }
        resp.json().await.map_err(|e| e.to_string())
    }

    /// Move a task after `previous` (None = first in list).
    pub async fn move_task(
        &self,
        task_list_id: &str,
        task_id: &str,
        previous: Option<&str>,
    ) -> Result<Task, String> {
        let token = self.auth.access_token().await?;
        let mut url = append_path_segment(&format!("{TASKS_BASE}/lists/"), task_list_id)?;
        url.push_str("/tasks/");
        let mut url = append_path_segment(&url, task_id)?;
        url.push_str("/move");
        let mut req = self.http.post(&url).bearer_auth(token);
        if let Some(prev) = previous {
            req = req.query(&[("previous", prev)]);
        }
        let resp = req.send().await.map_err(|e| e.to_string())?;
        if !resp.status().is_success() {
            return Err(format!(
                "move task failed: {}",
                resp.text().await.unwrap_or_default()
            ));
        }
        resp.json().await.map_err(|e| e.to_string())
    }

    pub async fn delete_task(&self, task_list_id: &str, task_id: &str) -> Result<(), String> {
        let mut url = append_path_segment(&format!("{TASKS_BASE}/lists/"), task_list_id)?;
        url.push_str("/tasks/");
        let url = append_path_segment(&url, task_id)?;
        let resp = self.authorized_delete(&url).await?;
        if !resp.status().is_success() {
            return Err(format!(
                "delete task failed: {}",
                resp.text().await.unwrap_or_default()
            ));
        }
        Ok(())
    }
}

/// Build a fingerprint from task updated timestamps for change detection.
pub fn tasks_fingerprint(tasks: &[Task]) -> String {
    tasks
        .iter()
        .map(|t| {
            format!(
                "{}:{}:{}:{}",
                t.id,
                t.title.as_deref().unwrap_or(""),
                t.status.as_deref().unwrap_or(""),
                t.updated.as_deref().unwrap_or("")
            )
        })
        .collect::<Vec<_>>()
        .join("|")
}
