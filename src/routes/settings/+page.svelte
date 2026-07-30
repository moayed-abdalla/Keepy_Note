<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';

  type AuthStatus = { signed_in: boolean };
  type AppSettings = { poll_interval_secs: number; autostart: boolean };

  let status: AuthStatus = $state({ signed_in: false });
  let settings: AppSettings = $state({ poll_interval_secs: 60, autostart: true });
  let busy = $state(false);
  let error = $state('');
  let message = $state('');

  async function refresh() {
    status = await invoke<AuthStatus>('auth_status');
    settings = await invoke<AppSettings>('get_settings');
  }

  async function signIn() {
    busy = true;
    error = '';
    message = '';
    try {
      status = await invoke<AuthStatus>('login');
      message = 'Signed in successfully.';
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  async function signOut() {
    busy = true;
    error = '';
    try {
      status = await invoke<AuthStatus>('logout');
      message = 'Signed out.';
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  async function save() {
    busy = true;
    error = '';
    try {
      settings = await invoke<AppSettings>('save_settings', { settings });
      message = 'Settings saved.';
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  async function syncNow() {
    busy = true;
    error = '';
    try {
      await invoke('sync_now');
      message = 'Synced.';
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  async function addSticky() {
    try {
      await invoke('open_picker');
    } catch (e) {
      error = String(e);
    }
  }

  onMount(() => {
    refresh().catch((e) => (error = String(e)));
  });
</script>

<div class="app-shell">
  <div class="panel stack">
    <h1>Keepy Note</h1>
    <p class="muted">
      This app lives in the <strong>system tray</strong> (notification area near the clock). Look for
      the Keepy Note icon — click the ^ arrow if it’s hidden. Left-click the icon to pin a list;
      right-click for Settings / Sync / Quit.
    </p>

    <div class="stack">
      <div class="row" style="justify-content: space-between;">
        <div>
          <strong>Google account</strong>
          <div class="muted">{status.signed_in ? 'Signed in' : 'Not signed in'}</div>
        </div>
        {#if status.signed_in}
          <button class="btn danger" disabled={busy} onclick={signOut}>Sign out</button>
        {:else}
          <button class="btn primary" disabled={busy} onclick={signIn}>Sign in with Google</button>
        {/if}
      </div>

      <button class="btn primary" disabled={busy || !status.signed_in} onclick={addSticky}>
        Add sticky list
      </button>
      <p class="muted">
        Create a new Google Tasks list or pin an existing one. Each sticky is a checklist with
        checkboxes and drag-and-drop reordering, synced to Google Tasks.
      </p>

      <label class="field">
        <span>Sync interval (seconds)</span>
        <input type="number" min="15" max="600" bind:value={settings.poll_interval_secs} />
      </label>

      <label class="row">
        <input type="checkbox" bind:checked={settings.autostart} />
        <span>Start Keepy Note when Windows starts</span>
      </label>

      <div class="row">
        <button class="btn primary" disabled={busy} onclick={save}>Save</button>
        <button class="btn" disabled={busy || !status.signed_in} onclick={syncNow}>Sync now</button>
      </div>

      {#if message}
        <div class="muted">{message}</div>
      {/if}
      {#if error}
        <div class="error">{error}</div>
      {/if}
    </div>
  </div>
</div>
