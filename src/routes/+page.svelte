<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { WebviewWindow } from '@tauri-apps/api/webviewWindow';

  type AuthStatus = { signed_in: boolean };
  type AppSettings = { poll_interval_secs: number; autostart: boolean };

  type TaskList = {
    id: string;
    title: string;
  };

  type StickyNoteState = {
    id: string;
    title: string;
    x: number;
    y: number;
    width: number;
    height: number;
    pin_mode: 'always_on_top' | 'desktop_embed';
  };

  let status: AuthStatus = $state({ signed_in: false });
  let settings: AppSettings = $state({ poll_interval_secs: 60, autostart: true });
  let profileOpen = $state(false);
  let authBusy = $state(false);
  let message = $state('');
  let authError = $state('');

  let tab = $state<'create' | 'existing'>('create');
  let title = $state('');
  let query = $state('');
  let lists = $state<TaskList[]>([]);
  let selectedId = $state<string | null>(null);
  let pinBusy = $state(false);
  let pinningTitle = $state('');
  let pinError = $state('');

  let filtered = $derived(
    (() => {
      const q = query.trim().toLowerCase();
      if (!q) return lists;
      return lists.filter((l) => l.title.toLowerCase().includes(q));
    })()
  );

  let selected = $derived(filtered.find((l) => l.id === selectedId) ?? null);

  async function refreshAuth() {
    status = await invoke<AuthStatus>('auth_status');
    settings = await invoke<AppSettings>('get_settings');
  }

  async function signIn() {
    authBusy = true;
    authError = '';
    message = '';
    try {
      status = await invoke<AuthStatus>('login');
      message = 'Signed in successfully.';
    } catch (e) {
      authError = String(e);
    } finally {
      authBusy = false;
    }
  }

  async function signOut() {
    authBusy = true;
    authError = '';
    try {
      status = await invoke<AuthStatus>('logout');
      message = 'Signed out.';
      profileOpen = false;
    } catch (e) {
      authError = String(e);
    } finally {
      authBusy = false;
    }
  }

  async function save() {
    authBusy = true;
    authError = '';
    try {
      settings = await invoke<AppSettings>('save_settings', { settings });
      message = 'Settings saved.';
    } catch (e) {
      authError = String(e);
    } finally {
      authBusy = false;
    }
  }

  async function syncNow() {
    authBusy = true;
    authError = '';
    try {
      await invoke('sync_now');
      message = 'Synced.';
    } catch (e) {
      authError = String(e);
    } finally {
      authBusy = false;
    }
  }

  async function openStickyWindow(sticky: StickyNoteState) {
    const label = `sticky-${sticky.id}`;
    const existing = await WebviewWindow.getByLabel(label);
    if (existing) {
      await existing.setFocus();
      await invoke('ensure_sticky_tray', { id: sticky.id }).catch(() => {});
      return;
    }

    try {
      const win = new WebviewWindow(label, {
        url: `/sticky?id=${encodeURIComponent(sticky.id)}`,
        title: sticky.title || 'Sticky',
        width: sticky.width || 280,
        height: sticky.height || 360,
        x: sticky.x ?? 80,
        y: sticky.y ?? 80,
        decorations: false,
        transparent: true,
        shadow: false,
        alwaysOnTop: sticky.pin_mode !== 'desktop_embed',
        skipTaskbar: true,
        resizable: true,
        visible: true
      });

      await new Promise<void>((resolve, reject) => {
        const timer = setTimeout(() => reject(new Error('Sticky window timed out')), 15000);
        win.once('tauri://created', () => {
          clearTimeout(timer);
          resolve();
        });
        win.once('tauri://error', (event) => {
          clearTimeout(timer);
          reject(new Error(String(event.payload ?? 'Failed to open sticky window')));
        });
      });
      await invoke('ensure_sticky_tray', { id: sticky.id });
    } catch (e) {
      console.warn('openStickyWindow failed, falling back to open_sticky', e);
      await invoke('open_sticky', { id: sticky.id });
    }
  }

  async function loadLists() {
    pinBusy = true;
    pinError = '';
    pinningTitle = '';
    try {
      lists = await invoke<TaskList[]>('list_task_lists');
      if (selectedId && !lists.some((l) => l.id === selectedId)) {
        selectedId = null;
      }
    } catch (e) {
      pinError = String(e);
    } finally {
      pinBusy = false;
    }
  }

  async function createList() {
    if (!status.signed_in) {
      pinError = 'Sign in with Google to create a sticky list.';
      return;
    }
    pinBusy = true;
    pinError = '';
    pinningTitle = title.trim() || 'Untitled list';
    try {
      const sticky = await invoke<StickyNoteState>('create_list_and_pin', {
        title: title.trim() || 'Untitled list'
      });
      await openStickyWindow(sticky);
      title = '';
    } catch (e) {
      pinError = String(e);
    } finally {
      pinBusy = false;
      pinningTitle = '';
    }
  }

  function selectList(list: TaskList) {
    if (pinBusy) return;
    pinError = '';
    selectedId = list.id;
  }

  async function pinSelected() {
    if (!selected) {
      pinError = 'Select a list to pin.';
      return;
    }
    await pinList(selected);
  }

  async function pinList(list: TaskList) {
    if (pinBusy) return;
    if (!list?.id) {
      pinError = 'That list is missing an id from Google Tasks.';
      return;
    }
    selectedId = list.id;
    pinBusy = true;
    pinError = '';
    pinningTitle = list.title || 'Untitled list';
    try {
      const sticky = await invoke<StickyNoteState>('pin_list', {
        taskListId: list.id,
        title: list.title || 'Untitled list'
      });
      await openStickyWindow(sticky);
    } catch (e) {
      pinError = String(e);
    } finally {
      pinBusy = false;
      pinningTitle = '';
    }
  }

  function switchToExisting() {
    tab = 'existing';
    pinError = '';
    if (status.signed_in) loadLists();
  }

  function toggleProfile() {
    profileOpen = !profileOpen;
    message = '';
    authError = '';
  }

  onMount(() => {
    refreshAuth().catch((e) => (authError = String(e)));
  });
</script>

<div class="app-shell">
  <div class="panel stack">
    <div class="header-row">
      <h1>Keepy Note</h1>
      {#if status.signed_in}
        <div class="profile-wrap">
          <button
            type="button"
            class="avatar-btn"
            aria-label="Account menu"
            aria-expanded={profileOpen}
            onclick={toggleProfile}
            title="Account & settings"
          >
            <svg viewBox="0 0 24 24" width="22" height="22" aria-hidden="true">
              <circle cx="12" cy="8" r="4" fill="currentColor" opacity="0.9" />
              <path
                d="M4 20c0-3.5 3.6-6 8-6s8 2.5 8 6"
                fill="none"
                stroke="currentColor"
                stroke-width="2"
                stroke-linecap="round"
              />
            </svg>
          </button>
          {#if profileOpen}
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <div
              class="profile-backdrop"
              onclick={() => (profileOpen = false)}
              onkeydown={(e) => e.key === 'Escape' && (profileOpen = false)}
            ></div>
            <div class="profile-dropdown stack" role="menu">
              <div class="muted">Signed in with Google</div>

              <label class="field">
                <span>Sync interval (seconds)</span>
                <input type="number" min="15" max="600" bind:value={settings.poll_interval_secs} />
              </label>

              <label class="row">
                <input type="checkbox" bind:checked={settings.autostart} />
                <span>Start Keepy Note when Windows starts</span>
              </label>

              <div class="row">
                <button class="btn primary" disabled={authBusy} onclick={save}>Save</button>
                <button class="btn" disabled={authBusy} onclick={syncNow}>Sync now</button>
              </div>

              <button class="btn danger" disabled={authBusy} onclick={signOut}>Sign out</button>

              {#if message}
                <div class="muted">{message}</div>
              {/if}
              {#if authError}
                <div class="error">{authError}</div>
              {/if}
            </div>
          {/if}
        </div>
      {/if}
    </div>

    {#if !status.signed_in}
      <div class="signin-banner stack">
        <div class="row" style="justify-content: space-between;">
          <div>
            <strong>Google account</strong>
            <div class="muted">Sign in to create and sync sticky lists</div>
          </div>
          <button class="btn primary" disabled={authBusy} onclick={signIn}>Sign in with Google</button>
        </div>
        {#if authError}
          <div class="error">{authError}</div>
        {/if}
        {#if message}
          <div class="muted">{message}</div>
        {/if}
      </div>
    {/if}

    <p class="muted">
      Pin a Google Tasks list as a sticky checklist. Stickies stay open when you close this window —
      use each list’s colored tray icon to show, close, or quit. Relaunch Keepy Note to open this window again.
    </p>

    {#if pinError}
      <div class="error" role="alert">{pinError}</div>
    {/if}

    <div class="tabs">
      <button
        class="tab"
        class:active={tab === 'create'}
        disabled={pinBusy}
        onclick={() => (tab = 'create')}
      >
        Create list
      </button>
      <button
        class="tab"
        class:active={tab === 'existing'}
        disabled={pinBusy}
        onclick={switchToExisting}
      >
        Existing lists
      </button>
    </div>

    {#if tab === 'create'}
      <label class="field">
        <span>List name</span>
        <input
          bind:value={title}
          placeholder="e.g. Groceries"
          disabled={pinBusy || !status.signed_in}
          onkeydown={(e) => e.key === 'Enter' && createList()}
        />
      </label>
      <button class="btn primary" disabled={pinBusy || !status.signed_in} onclick={createList}>
        {pinBusy ? 'Creating…' : 'Create & pin'}
      </button>
    {:else}
      <label class="field">
        <span>Filter lists</span>
        <div class="row">
          <input
            style="flex:1"
            bind:value={query}
            placeholder="Search by name"
            disabled={pinBusy || !status.signed_in}
          />
          <button class="btn" disabled={pinBusy || !status.signed_in} onclick={loadLists}>
            Refresh
          </button>
        </div>
      </label>

      <div class="list" aria-busy={pinBusy}>
        {#if !status.signed_in}
          <div class="muted" style="padding:12px;">Sign in to see your Google Tasks lists.</div>
        {:else if pinBusy && lists.length === 0}
          <div class="muted" style="padding:12px;">Loading…</div>
        {:else if filtered.length === 0}
          <div class="muted" style="padding:12px;">No lists found.</div>
        {:else}
          {#each filtered as list (list.id)}
            <button
              type="button"
              class="list-item"
              class:selected={selectedId === list.id}
              disabled={pinBusy}
              aria-pressed={selectedId === list.id}
              onclick={() => selectList(list)}
              ondblclick={() => pinList(list)}
            >
              <strong>{list.title || 'Untitled list'}</strong>
            </button>
          {/each}
        {/if}
      </div>

      <p class="muted hint">
        {#if pinBusy && pinningTitle}
          Pinning “{pinningTitle}”…
        {:else if selected}
          Selected “{selected.title || 'Untitled list'}”. Double-click or press Pin.
        {:else}
          Select a list, then pin it as a sticky.
        {/if}
      </p>

      <button
        class="btn primary"
        disabled={pinBusy || !selected || !status.signed_in}
        onclick={pinSelected}
      >
        {pinBusy && pinningTitle ? 'Pinning…' : 'Pin selected list'}
      </button>
    {/if}
  </div>
</div>

<style>
  .header-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }

  .header-row h1 {
    margin: 0;
  }

  .profile-wrap {
    position: relative;
  }

  .avatar-btn {
    width: 36px;
    height: 36px;
    border-radius: 50%;
    border: 1px solid var(--border);
    background: var(--panel-raised);
    color: var(--accent);
    display: grid;
    place-items: center;
    padding: 0;
  }

  .avatar-btn:hover {
    background: var(--hover);
  }

  .profile-backdrop {
    position: fixed;
    inset: 0;
    z-index: 40;
  }

  .profile-dropdown {
    position: absolute;
    top: calc(100% + 8px);
    right: 0;
    width: 280px;
    padding: 12px;
    border-radius: 12px;
    border: 1px solid var(--border);
    background: var(--panel);
    box-shadow: 0 12px 28px rgba(0, 0, 0, 0.45);
    z-index: 50;
  }

  .signin-banner {
    padding: 12px;
    border-radius: 10px;
    border: 1px solid var(--border);
    background: var(--bg);
  }

  .hint {
    margin: 0;
    min-height: 1.2em;
  }
</style>
