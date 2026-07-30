use crate::credentials::GoogleCredentials;
use keyring::Entry;
use oauth2::basic::BasicClient;
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, PkceCodeChallenge,
    RedirectUrl, RefreshToken, Scope, TokenResponse, TokenUrl,
};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::{Duration, Instant};

const KEYRING_SERVICE: &str = "com.moaye.keepy-note";
const KEYRING_USER: &str = "google_refresh_token";
const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const SCOPE: &str = "https://www.googleapis.com/auth/tasks";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthStatus {
    pub signed_in: bool,
}

struct TokenCache {
    access_token: String,
    expires_at: Instant,
}

pub struct AuthState {
    credentials: GoogleCredentials,
    cache: Mutex<Option<TokenCache>>,
}

impl AuthState {
    pub fn new(credentials: GoogleCredentials) -> Self {
        Self {
            credentials,
            cache: Mutex::new(None),
        }
    }

    pub fn is_signed_in(&self) -> bool {
        load_refresh_token().ok().flatten().is_some()
    }

    pub fn status(&self) -> AuthStatus {
        AuthStatus {
            signed_in: self.is_signed_in(),
        }
    }

    pub fn sign_out(&self) -> Result<(), String> {
        *self.cache.lock() = None;
        delete_refresh_token()
    }

    pub async fn login(&self) -> Result<(), String> {
        if self.credentials.client_id.is_empty() || self.credentials.client_secret.is_empty() {
            return Err(
                "Google OAuth credentials are not configured. Copy google_credentials.example.json to src-tauri/google_credentials.json (or %APPDATA%\\com.moaye.keepy-note\\google_credentials.json) and fill in your Client ID/secret. See GOOGLE_SETUP.md."
                    .into(),
            );
        }

        let client = build_oauth_client(&self.credentials)?;

        let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        let redirect = format!("http://127.0.0.1:{port}");
        let client = client
            .set_redirect_uri(RedirectUrl::new(redirect.clone()).map_err(|e| e.to_string())?);

        let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();
        let (auth_url, csrf_token) = client
            .authorize_url(CsrfToken::new_random)
            .add_scope(Scope::new(SCOPE.to_string()))
            .add_extra_param("access_type", "offline")
            .add_extra_param("prompt", "consent")
            .set_pkce_challenge(pkce_challenge)
            .url();

        open::that(auth_url.as_str()).map_err(|e| format!("Failed to open browser: {e}"))?;

        let (code, state) = wait_for_redirect(listener)?;
        if &state != csrf_token.secret() {
            return Err("OAuth CSRF state mismatch".into());
        }

        let token = client
            .exchange_code(AuthorizationCode::new(code))
            .set_pkce_verifier(pkce_verifier)
            .request_async(oauth2::reqwest::async_http_client)
            .await
            .map_err(|e| format!("Token exchange failed: {e}"))?;

        let refresh = token.refresh_token().ok_or_else(|| {
            "No refresh token returned. Revoke app access in Google Account and try again."
                .to_string()
        })?;
        save_refresh_token(refresh.secret())?;

        let expires_in = token.expires_in().unwrap_or(Duration::from_secs(3500));
        *self.cache.lock() = Some(TokenCache {
            access_token: token.access_token().secret().clone(),
            expires_at: Instant::now() + expires_in - Duration::from_secs(60),
        });

        Ok(())
    }

    pub async fn access_token(&self) -> Result<String, String> {
        {
            let cache = self.cache.lock();
            if let Some(c) = cache.as_ref() {
                if Instant::now() < c.expires_at {
                    return Ok(c.access_token.clone());
                }
            }
        }
        self.refresh_access_token().await
    }

    async fn refresh_access_token(&self) -> Result<String, String> {
        let refresh = load_refresh_token()?
            .ok_or_else(|| "Not signed in. Please sign in from Settings.".to_string())?;

        let client = build_oauth_client(&self.credentials)?;
        let token = client
            .exchange_refresh_token(&RefreshToken::new(refresh))
            .request_async(oauth2::reqwest::async_http_client)
            .await
            .map_err(|e| format!("Token refresh failed: {e}"))?;

        if let Some(new_refresh) = token.refresh_token() {
            save_refresh_token(new_refresh.secret())?;
        }

        let expires_in = token.expires_in().unwrap_or(Duration::from_secs(3500));
        let access = token.access_token().secret().clone();
        *self.cache.lock() = Some(TokenCache {
            access_token: access.clone(),
            expires_at: Instant::now() + expires_in - Duration::from_secs(60),
        });
        Ok(access)
    }
}

fn build_oauth_client(creds: &GoogleCredentials) -> Result<BasicClient, String> {
    Ok(BasicClient::new(
        ClientId::new(creds.client_id.clone()),
        Some(ClientSecret::new(creds.client_secret.clone())),
        AuthUrl::new(AUTH_URL.to_string()).map_err(|e| e.to_string())?,
        Some(TokenUrl::new(TOKEN_URL.to_string()).map_err(|e| e.to_string())?),
    ))
}

fn wait_for_redirect(listener: TcpListener) -> Result<(String, String), String> {
    listener
        .set_nonblocking(false)
        .map_err(|e| e.to_string())?;
    let (mut stream, _) = listener
        .accept()
        .map_err(|e| format!("Waiting for OAuth redirect failed: {e}"))?;

    let mut buf = [0u8; 4096];
    let n = stream.read(&mut buf).map_err(|e| e.to_string())?;
    let req = String::from_utf8_lossy(&buf[..n]);
    let first_line = req.lines().next().unwrap_or("");
    let path = first_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| "Malformed OAuth redirect request".to_string())?;
    let url = url::Url::parse(&format!("http://localhost{path}")).map_err(|e| e.to_string())?;
    let mut code = None;
    let mut state = None;
    for (k, v) in url.query_pairs() {
        match k.as_ref() {
            "code" => code = Some(v.to_string()),
            "state" => state = Some(v.to_string()),
            "error" => return Err(format!("OAuth error: {v}")),
            _ => {}
        }
    }
    let body = b"HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nConnection: close\r\n\r\n<html><body><h2>Keepy Note signed in.</h2><p>You can close this tab and return to the app.</p></body></html>";
    let _ = stream.write_all(body);
    Ok((
        code.ok_or_else(|| "Missing code in OAuth redirect".to_string())?,
        state.ok_or_else(|| "Missing state in OAuth redirect".to_string())?,
    ))
}

fn keyring_entry() -> Result<Entry, String> {
    Entry::new(KEYRING_SERVICE, KEYRING_USER).map_err(|e| e.to_string())
}

fn save_refresh_token(token: &str) -> Result<(), String> {
    keyring_entry()?
        .set_password(token)
        .map_err(|e| format!("Failed to store refresh token: {e}"))
}

fn load_refresh_token() -> Result<Option<String>, String> {
    match keyring_entry()?.get_password() {
        Ok(p) => Ok(Some(p)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(format!("Failed to read refresh token: {e}")),
    }
}

fn delete_refresh_token() -> Result<(), String> {
    match keyring_entry()?.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("Failed to delete refresh token: {e}")),
    }
}
