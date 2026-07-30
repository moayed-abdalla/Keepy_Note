<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { WebviewWindow } from '@tauri-apps/api/webviewWindow';

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

  let tab = $state<'create' | 'existing'>('create');
  let title = $state('');
  let query = $state('');
  let lists = $state<TaskList[]>([]);
  let selectedId = $state<string | null>(null);
  let busy = $state(false);
  let pinningTitle = $state('');
  let error = $state('');

  let filtered = $derived(
    (() => {
      const q = query.trim().toLowerCase();
      if (!q) return lists;
      return lists.filter((l) => l.title.toLowerCase().includes(q));
    })()
  );

  let selected = $derived(filtered.find((l) => l.id === selectedId) ?? null);

  async function openStickyWindow(sticky: StickyNoteState) {
    const label = `sticky-${sticky.id}`;
    const existing = await WebviewWindow.getByLabel(label);
    if (existing) {
      await existing.setFocus();
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
    } catch (e) {
      // Frontend window creation can fail on Windows; fall back to main-thread Rust open.
      console.warn('openStickyWindow failed, falling back to open_sticky', e);
      await invoke('open_sticky', { id: sticky.id });
    }
  }

  async function loadLists() {
    busy = true;
    error = '';
    pinningTitle = '';
    try {
      lists = await invoke<TaskList[]>('list_task_lists');
      if (selectedId && !lists.some((l) => l.id === selectedId)) {
        selectedId = null;
      }
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  async function createList() {
    busy = true;
    error = '';
    pinningTitle = title.trim() || 'Untitled list';
    try {
      const sticky = await invoke<StickyNoteState>('create_list_and_pin', {
        title: title.trim() || 'Untitled list'
      });
      await openStickyWindow(sticky);
      await getCurrentWindow().close();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
      pinningTitle = '';
    }
  }

  function selectList(list: TaskList) {
    if (busy) return;
    error = '';
    selectedId = list.id;
  }

  async function pinSelected() {
    if (!selected) {
      error = 'Select a list to pin.';
      return;
    }
    await pinList(selected);
  }

  async function pinList(list: TaskList) {
    if (busy) return;
    if (!list?.id) {
      error = 'That list is missing an id from Google Tasks.';
      return;
    }
    selectedId = list.id;
    busy = true;
    error = '';
    pinningTitle = list.title || 'Untitled list';
    try {
      const sticky = await invoke<StickyNoteState>('pin_list', {
        taskListId: list.id,
        title: list.title || 'Untitled list'
      });
      await openStickyWindow(sticky);
      await getCurrentWindow().close();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
      pinningTitle = '';
    }
  }

  function switchToExisting() {
    tab = 'existing';
    error = '';
    loadLists();
  }
</script>

<div class="app-shell">
  <div class="panel stack">
    <h1>Add Sticky List</h1>
    <p class="muted">Pin a Google Tasks list as a sticky checklist.</p>

    {#if error}
      <div class="error" role="alert">{error}</div>
    {/if}

    <div class="tabs">
      <button class="tab" class:active={tab === 'create'} disabled={busy} onclick={() => (tab = 'create')}>
        Create list
      </button>
      <button class="tab" class:active={tab === 'existing'} disabled={busy} onclick={switchToExisting}>
        Existing lists
      </button>
    </div>

    {#if tab === 'create'}
      <label class="field">
        <span>List name</span>
        <input
          bind:value={title}
          placeholder="e.g. Groceries"
          disabled={busy}
          onkeydown={(e) => e.key === 'Enter' && createList()}
        />
      </label>
      <button class="btn primary" disabled={busy} onclick={createList}>
        {busy ? 'Creating…' : 'Create & pin'}
      </button>
    {:else}
      <label class="field">
        <span>Filter lists</span>
        <div class="row">
          <input
            style="flex:1"
            bind:value={query}
            placeholder="Search by name"
            disabled={busy}
          />
          <button class="btn" disabled={busy} onclick={loadLists}>Refresh</button>
        </div>
      </label>

      <div class="list" aria-busy={busy}>
        {#if busy && lists.length === 0}
          <div class="muted" style="padding:12px;">Loading…</div>
        {:else if filtered.length === 0}
          <div class="muted" style="padding:12px;">No lists found.</div>
        {:else}
          {#each filtered as list (list.id)}
            <button
              type="button"
              class="list-item"
              class:selected={selectedId === list.id}
              disabled={busy}
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
        {#if busy && pinningTitle}
          Pinning “{pinningTitle}”…
        {:else if selected}
          Selected “{selected.title || 'Untitled list'}”. Double-click or press Pin.
        {:else}
          Select a list, then pin it as a sticky.
        {/if}
      </p>

      <button class="btn primary" disabled={busy || !selected} onclick={pinSelected}>
        {busy && pinningTitle ? 'Pinning…' : 'Pin selected list'}
      </button>
    {/if}
  </div>
</div>

<style>
  .hint {
    margin: 0;
    min-height: 1.2em;
  }
</style>
