# Keepy Note setup

## For end users

1. Run [`Keepy-Note-setup.exe`](Keepy-Note-setup.exe) from the repo root and finish the installer.
2. Keepy Note runs from the **system tray** (icons near the Windows clock). A **Settings** window opens so you can sign in.
3. Click **Sign in with Google**, then use **Add sticky list** (or left-click the tray icon) to pin a Google Tasks list.
4. Right-click the tray icon for Sync / Settings / Quit.

If the tray icon is missing, click the **^** chevron in the notification area to show hidden icons.

You do **not** need to create Google Cloud credentials or edit any JSON files.

## For developers (building / releasing)

Complete these steps once before building. No billing account is required. OAuth credentials are baked into the installer at build time so end users only install and sign in.

### 1. Create a Google Cloud project

1. Open [Google Cloud Console](https://console.cloud.google.com/)
2. Create a new project (e.g. `Keepy Note`)

### 2. Enable the Google Tasks API

1. Go to **APIs & Services → Library**
2. Search for **Google Tasks API**
3. Click **Enable**

### 3. Configure the OAuth consent screen

1. Go to **APIs & Services → OAuth consent screen**
2. Choose **External**
3. Fill in App name (`Keepy Note`), your email, and developer contact
4. Add scope: `https://www.googleapis.com/auth/tasks`
5. Under **Test users**, add every Gmail address that should be able to sign in while the app is in Testing
6. Leave the app in **Testing** status unless you complete Google verification

### 4. Create Desktop OAuth credentials

1. Go to **APIs & Services → Credentials**
2. **Create Credentials → OAuth client ID**
3. Application type: **Desktop app**
4. Name: `Keepy Note Desktop`
5. Download the JSON (or copy Client ID and Client Secret)

### 5. Configure Keepy Note and build

Copy the example file and fill in your values:

```powershell
Copy-Item src-tauri\google_credentials.example.json src-tauri\google_credentials.json
```

Edit `src-tauri/google_credentials.json`:

```json
{
  "client_id": "YOUR_CLIENT_ID.apps.googleusercontent.com",
  "client_secret": "YOUR_CLIENT_SECRET"
}
```

`google_credentials.json` is gitignored and never committed.

Build the installer (credentials are bundled into the app):

```powershell
npm run tauri:build
```

That produces `Keepy-Note-setup.exe` at the repo root. Share that file with users.

On first successful launch, credentials are also copied to:

`%APPDATA%\com.moaye.keepy-note\google_credentials.json`

### Scope

- `https://www.googleapis.com/auth/tasks` — full read/write access to Google Tasks
