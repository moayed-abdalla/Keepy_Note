# Google Cloud Setup for Keepy Note

Complete these steps once before running the app. No billing account is required.

## 1. Create a Google Cloud project

1. Open [Google Cloud Console](https://console.cloud.google.com/)
2. Create a new project (e.g. `Keepy Note`)

## 2. Enable the Google Tasks API

1. Go to **APIs & Services → Library**
2. Search for **Google Tasks API**
3. Click **Enable**

## 3. Configure the OAuth consent screen

1. Go to **APIs & Services → OAuth consent screen**
2. Choose **External**
3. Fill in App name (`Keepy Note`), your email, and developer contact
4. Add scope: `https://www.googleapis.com/auth/tasks`
5. Under **Test users**, add your Gmail address
6. Leave the app in **Testing** status (personal use — no Google verification needed)

## 4. Create Desktop OAuth credentials

1. Go to **APIs & Services → Credentials**
2. **Create Credentials → OAuth client ID**
3. Application type: **Desktop app**
4. Name: `Keepy Note Desktop`
5. Download the JSON (or copy Client ID and Client Secret)

## How to use the app

Keepy Note runs from the **system tray** (the icons near the Windows clock). After launch:

1. A **Settings** window opens so you can sign in.
2. Click **Add sticky list** (or left-click the tray icon) to create a new Google Tasks list or pin an existing one.
3. Each sticky is a checklist for that list — check items off and drag to reorder; changes sync to Google Tasks.
4. Right-click the tray icon for Sync / Settings / Quit.

If the tray icon is missing, click the **^** chevron in the notification area to show hidden icons.

## 5. Configure Keepy Note

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

When you build the installer (`npm run tauri build`), that file is bundled into the app. It is also copied to:

`%APPDATA%\com.moaye.keepy-note\google_credentials.json`

on first successful launch.

## Scope

- `https://www.googleapis.com/auth/tasks` — full read/write access to your Google Tasks
