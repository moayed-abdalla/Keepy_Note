use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoogleCredentials {
    pub client_id: String,
    pub client_secret: String,
}

impl GoogleCredentials {
    pub fn load_with_dirs(
        app_data: &Path,
        resource_dir: Option<&PathBuf>,
    ) -> Result<Self, String> {
        let mut candidates: Vec<PathBuf> = vec![app_data.join("google_credentials.json")];

        if let Some(res) = resource_dir {
            candidates.push(res.join("google_credentials.json"));
            // NSIS sometimes nests resources one level deeper
            candidates.push(res.join("resources").join("google_credentials.json"));
        }

        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                candidates.push(dir.join("google_credentials.json"));
                candidates.push(dir.join("resources").join("google_credentials.json"));
            }
        }

        if let Ok(cwd) = std::env::current_dir() {
            candidates.push(cwd.join("google_credentials.json"));
            candidates.push(cwd.join("src-tauri").join("google_credentials.json"));
        }

        for path in candidates {
            if let Some(creds) = try_load_file(&path) {
                return Ok(creds);
            }
        }

        Err(
            "Missing Google OAuth credentials. Place google_credentials.json next to the app or in %APPDATA%\\com.moaye.keepy-note\\. See GOOGLE_SETUP.md."
                .into(),
        )
    }
}

fn try_load_file(path: &Path) -> Option<GoogleCredentials> {
    let raw = fs::read_to_string(path).ok()?;
    let creds: GoogleCredentials = serde_json::from_str(&raw).ok()?;
    if creds.client_id.contains("YOUR_CLIENT_ID")
        || creds.client_secret.contains("YOUR_CLIENT_SECRET")
        || creds.client_id.is_empty()
        || creds.client_secret.is_empty()
    {
        return None;
    }
    Some(creds)
}
