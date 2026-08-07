<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { flip } from 'svelte/animate';
  import { dndzone } from 'svelte-dnd-action';
  import { page } from '$app/stores';
  import { invoke } from '@tauri-apps/api/core';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';

  type PinMode = 'always_on_top' | 'desktop_embed';
  type ThemeKey =
    | 'red'
    | 'orange'
    | 'yellow'
    | 'green'
    | 'navy'
    | 'indigo'
    | 'purple'
    | 'graphite';

  type StickyTaskItem = {
    id: string;
    title: string;
    status: string;
    position?: string | null;
  };

  type StickyNoteState = {
    id: string;
    title: string;
    items: StickyTaskItem[];
    color: string;
    pin_mode: PinMode;
  };

  // Rainbow order + grey. Keys `navy` / `purple` / `graphite` kept for saved notes.
  const THEME_KEYS: ThemeKey[] = [
    'red',
    'orange',
    'yellow',
    'green',
    'navy',
    'indigo',
    'purple',
    'graphite'
  ];
  const THEMES: Record<ThemeKey, { label: string; preview: string }> = {
    red: { label: 'Red', preview: 'linear-gradient(180deg, #3D1C22, #2A1418)' },
    orange: { label: 'Orange', preview: 'linear-gradient(180deg, #3D2918, #2A1C12)' },
    yellow: { label: 'Yellow', preview: 'linear-gradient(180deg, #363018, #262210)' },
    green: { label: 'Green', preview: 'linear-gradient(180deg, #1C3327, #14261C)' },
    navy: { label: 'Blue', preview: 'linear-gradient(180deg, #1C2B45, #141F33)' },
    indigo: { label: 'Indigo', preview: 'linear-gradient(180deg, #222240, #18182E)' },
    purple: { label: 'Violet', preview: 'linear-gradient(180deg, #2E2040, #241832)' },
    graphite: { label: 'Grey', preview: 'linear-gradient(180deg, #2A2D31, #1E2023)' }
  };
  const DEFAULT_THEME: ThemeKey = 'navy';
  const flipDurationMs = 180;

  function normalizeTheme(value: string | null | undefined): ThemeKey {
    if (value && THEME_KEYS.includes(value as ThemeKey)) return value as ThemeKey;
    return 'graphite';
  }

  let sticky = $state<StickyNoteState | null>(null);
  let title = $state('');
  let items = $state<StickyTaskItem[]>([]);
  let theme = $state<ThemeKey>(DEFAULT_THEME);
  let pinMode = $state<PinMode>('always_on_top');
  let newItem = $state('');
  let error = $state('');
  let showColors = $state(false);
  let unlisten: UnlistenFn | null = null;
  let titleTimer: ReturnType<typeof setTimeout> | null = null;
  let dragDisabled = $state(false);

  let id = $derived($page.url.searchParams.get('id') ?? '');

  async function load() {
    if (!id) return;
    sticky = await invoke<StickyNoteState>('get_sticky', { id });
    title = sticky.title;
    items = sticky.items ?? [];
    theme = normalizeTheme(sticky.color);
    pinMode = sticky.pin_mode;
  }

  function queueRename() {
    if (!id) return;
    if (titleTimer) clearTimeout(titleTimer);
    titleTimer = setTimeout(async () => {
      try {
        await invoke('rename_list', { id, title });
      } catch (e) {
        error = String(e);
      }
    }, 500);
  }

  async function onFocus() {
    if (id) await invoke('set_sticky_editing', { id, editing: true });
  }

  async function onBlur() {
    if (id) await invoke('set_sticky_editing', { id, editing: false });
    queueRename();
  }

  async function setTheme(next: ThemeKey) {
    theme = next;
    showColors = false;
    if (id) await invoke('update_sticky_color', { id, color: next });
  }

  async function togglePin() {
    const next: PinMode = pinMode === 'always_on_top' ? 'desktop_embed' : 'always_on_top';
    try {
      pinMode = await invoke<PinMode>('set_pin_mode', { id, mode: next });
    } catch (e) {
      error = String(e);
      try {
        const refreshed = await invoke<StickyNoteState>('get_sticky', { id });
        pinMode = refreshed.pin_mode;
      } catch {
        /* ignore */
      }
    }
  }

  async function closeSticky() {
    if (!id) return;
    await invoke('unpin_sticky', { id });
  }

  type ResizeDirection =
    | 'East'
    | 'North'
    | 'NorthEast'
    | 'NorthWest'
    | 'South'
    | 'SouthEast'
    | 'SouthWest'
    | 'West';

  async function startDrag(e: MouseEvent) {
    const t = e.target as HTMLElement;
    if (
      t.closest(
        'textarea, input, button, .color-pop, .item, .dnd-handle, .checklist, .resize-handle'
      )
    )
      return;
    await getCurrentWindow().startDragging();
  }

  async function startResize(e: MouseEvent, direction: ResizeDirection) {
    e.preventDefault();
    e.stopPropagation();
    await getCurrentWindow().startResizeDragging(direction);
  }

  /** Drag the sticky by the list name; click (no move) focuses to rename. */
  function onTitleMouseDown(e: MouseEvent) {
    if (e.button !== 0) return;
    const input = e.currentTarget as HTMLInputElement;
    // Already editing — allow normal text selection.
    if (document.activeElement === input) return;

    e.preventDefault();
    const startX = e.clientX;
    const startY = e.clientY;
    let dragged = false;
    const threshold = 4;

    const onMove = (ev: MouseEvent) => {
      if (
        Math.abs(ev.clientX - startX) > threshold ||
        Math.abs(ev.clientY - startY) > threshold
      ) {
        dragged = true;
        cleanup();
        getCurrentWindow().startDragging();
      }
    };

    const onUp = () => {
      cleanup();
      if (!dragged) {
        input.focus();
        input.select();
      }
    };

    const cleanup = () => {
      window.removeEventListener('mousemove', onMove);
      window.removeEventListener('mouseup', onUp);
    };

    window.addEventListener('mousemove', onMove);
    window.addEventListener('mouseup', onUp);
  }

  function isCompleted(item: StickyTaskItem) {
    return item.status === 'completed';
  }

  async function toggleStatus(item: StickyTaskItem) {
    if (!id) return;
    const completed = !isCompleted(item);
    try {
      const updated = await invoke<StickyTaskItem>('set_task_status', {
        stickyId: id,
        taskId: item.id,
        completed
      });
      items = items.map((i) => (i.id === item.id ? updated : i));
    } catch (e) {
      error = String(e);
    }
  }

  async function saveItemTitle(item: StickyTaskItem, value: string) {
    if (!id) return;
    const trimmed = value.trim();
    if (trimmed === item.title) return;
    try {
      const updated = await invoke<StickyTaskItem>('update_task_title', {
        stickyId: id,
        taskId: item.id,
        title: trimmed
      });
      items = items.map((i) => (i.id === item.id ? updated : i));
    } catch (e) {
      error = String(e);
    }
  }

  async function removeItem(item: StickyTaskItem) {
    if (!id) return;
    try {
      await invoke('delete_task', { stickyId: id, taskId: item.id });
      items = items.filter((i) => i.id !== item.id);
    } catch (e) {
      error = String(e);
    }
  }

  async function addItem() {
    if (!id) return;
    const text = newItem.trim();
    if (!text) return;
    try {
      const created = await invoke<StickyTaskItem>('add_task', {
        stickyId: id,
        title: text
      });
      items = [...items, created];
      newItem = '';
    } catch (e) {
      error = String(e);
    }
  }

  function handleConsider(e: CustomEvent<{ items: StickyTaskItem[] }>) {
    items = e.detail.items;
  }

  async function handleFinalize(
    e: CustomEvent<{ items: StickyTaskItem[]; info: { id: string } }>
  ) {
    if (!id) return;
    const next = e.detail.items;
    items = next;
    const draggedId = String(e.detail.info.id);
    const idx = next.findIndex((i) => i.id === draggedId);
    if (idx < 0) return;
    const previousTaskId = idx > 0 ? next[idx - 1].id : null;
    try {
      items = await invoke<StickyTaskItem[]>('reorder_task', {
        stickyId: id,
        taskId: draggedId,
        previousTaskId
      });
    } catch (err) {
      error = String(err);
      try {
        await load();
      } catch {
        /* ignore */
      }
    }
  }

  onMount(async () => {
    try {
      await load();
      unlisten = await listen<{
        id: string;
        title: string;
        items: StickyTaskItem[];
      }>('sticky-updated', (ev) => {
        if (ev.payload.id === id) {
          title = ev.payload.title;
          items = ev.payload.items ?? [];
        }
      });

      const win = getCurrentWindow();
      const persistGeometry = async () => {
        if (!id) return;
        const pos = await win.outerPosition();
        const size = await win.innerSize();
        const factor = await win.scaleFactor();
        await invoke('update_sticky_geometry', {
          id,
          x: pos.x / factor,
          y: pos.y / factor,
          width: size.width / factor,
          height: size.height / factor
        });
      };
      await win.onMoved(() => {
        persistGeometry().catch(() => undefined);
      });
      await win.onResized(() => {
        persistGeometry().catch(() => undefined);
      });
    } catch (e) {
      error = String(e);
    }
  });

  onDestroy(() => {
    if (unlisten) unlisten();
    if (titleTimer) clearTimeout(titleTimer);
  });
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="sticky"
  class:chrome-locked={showColors}
  data-theme={theme}
  onmousedown={startDrag}
>
  <div class="toolbar" data-tauri-drag-region>
    <input
      class="title"
      bind:value={title}
      onmousedown={onTitleMouseDown}
      oninput={queueRename}
      onfocus={onFocus}
      onblur={onBlur}
      placeholder="List name"
    />
    <div class="actions">
      <button
        class="icon chrome"
        title={pinMode === 'always_on_top'
          ? 'Always on top (click for desktop mode)'
          : 'Desktop mode — behind other windows (click for always on top)'}
        onclick={togglePin}
      >
        {#if pinMode === 'always_on_top'}
          <!-- thumbtack -->
          <svg class="ico" viewBox="0 0 16 16" aria-hidden="true"
            ><path
              d="M6.2 3.2h3.6c.7 0 1.2.6 1.1 1.3l-.4 2.5H5.5l-.4-2.5c-.1-.7.4-1.3 1.1-1.3Z"
            /><path d="M5.2 7h5.6M8 7v6.2" /></svg
          >
        {:else}
          <!-- desktop -->
          <svg class="ico" viewBox="0 0 16 16" aria-hidden="true"
            ><rect x="2" y="2.5" width="12" height="8.5" rx="1.2" /><path
              d="M5.5 14h5M8 11v3"
            /></svg
          >
        {/if}
      </button>
      <button class="icon chrome" title="Color" onclick={() => (showColors = !showColors)}>
        <!-- palette -->
        <svg class="ico" viewBox="0 0 16 16" aria-hidden="true"
          ><path
            d="M8 2a6 6 0 1 0 0 12c.9 0 1.4-.7 1.4-1.4 0-.4-.2-.7-.4-1-.3-.3-.5-.6-.5-1 0-.8.7-1.5 1.5-1.5H11a3 3 0 0 0 0-6H8Z"
          /><circle cx="5.5" cy="6" r="0.9" fill="currentColor" stroke="none" /><circle
            cx="8"
            cy="4.5"
            r="0.9"
            fill="currentColor"
            stroke="none"
          /><circle cx="10.5" cy="6" r="0.9" fill="currentColor" stroke="none" /></svg
        >
      </button>
      <button class="icon chrome" title="Close / unpin" onclick={closeSticky}>
        <svg class="ico" viewBox="0 0 16 16" aria-hidden="true"
          ><path d="M4.5 4.5 11.5 11.5M11.5 4.5 4.5 11.5" /></svg
        >
      </button>
    </div>
  </div>

  {#if showColors}
    <div class="color-pop">
      {#each THEME_KEYS as key}
        <button
          class="swatch"
          class:active={theme === key}
          style={`background:${THEMES[key].preview}`}
          onclick={() => setTheme(key)}
          aria-label={THEMES[key].label}
          title={THEMES[key].label}
        ></button>
      {/each}
    </div>
  {/if}

  <div
    class="checklist"
    use:dndzone={{
      items,
      flipDurationMs,
      dragDisabled,
      dropTargetStyle: {},
      morphDisabled: true
    }}
    onconsider={handleConsider}
    onfinalize={handleFinalize}
  >
    {#each items as item (item.id)}
      <div class="item" class:done={isCompleted(item)} animate:flip={{ duration: flipDurationMs }}>
        <span class="dnd-handle chrome" title="Drag to reorder" aria-hidden="true">
          <svg class="ico grip" viewBox="0 0 16 16"
            ><circle cx="5.5" cy="4" r="1" fill="currentColor" stroke="none" /><circle
              cx="10.5"
              cy="4"
              r="1"
              fill="currentColor"
              stroke="none"
            /><circle cx="5.5" cy="8" r="1" fill="currentColor" stroke="none" /><circle
              cx="10.5"
              cy="8"
              r="1"
              fill="currentColor"
              stroke="none"
            /><circle cx="5.5" cy="12" r="1" fill="currentColor" stroke="none" /><circle
              cx="10.5"
              cy="12"
              r="1"
              fill="currentColor"
              stroke="none"
            /></svg
          >
        </span>
        <input
          class="check"
          type="checkbox"
          checked={isCompleted(item)}
          onchange={() => toggleStatus(item)}
          onfocus={() => (dragDisabled = true)}
          onblur={() => (dragDisabled = false)}
          aria-label="Complete"
        />
        <input
          class="item-title"
          value={item.title}
          onfocus={async () => {
            dragDisabled = true;
            await onFocus();
          }}
          onblur={async (e) => {
            dragDisabled = false;
            await saveItemTitle(item, (e.currentTarget as HTMLInputElement).value);
            await onBlur();
          }}
          onkeydown={(e) => {
            if (e.key === 'Enter') (e.currentTarget as HTMLInputElement).blur();
          }}
        />
        <button class="icon del chrome" title="Delete" onclick={() => removeItem(item)}>
          <svg class="ico" viewBox="0 0 16 16" aria-hidden="true"
            ><path d="M4.5 4.5 11.5 11.5M11.5 4.5 4.5 11.5" /></svg
          >
        </button>
      </div>
    {/each}
  </div>

  <div class="add-row">
    <input
      class="add-input"
      bind:value={newItem}
      placeholder="Add item…"
      onfocus={onFocus}
      onblur={onBlur}
      onkeydown={(e) => e.key === 'Enter' && addItem()}
    />
    <button class="icon add chrome" title="Add" onclick={addItem}>
      <svg class="ico" viewBox="0 0 16 16" aria-hidden="true"
        ><path d="M8 3.5v9M3.5 8h9" /></svg
      >
    </button>
  </div>

  {#if error}
    <div class="sticky-error">{error}</div>
  {/if}

  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="resize-handle n" onmousedown={(e) => startResize(e, 'North')}></div>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="resize-handle s" onmousedown={(e) => startResize(e, 'South')}></div>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="resize-handle e" onmousedown={(e) => startResize(e, 'East')}></div>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="resize-handle w" onmousedown={(e) => startResize(e, 'West')}></div>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="resize-handle ne" onmousedown={(e) => startResize(e, 'NorthEast')}></div>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="resize-handle nw" onmousedown={(e) => startResize(e, 'NorthWest')}></div>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="resize-handle se" onmousedown={(e) => startResize(e, 'SouthEast')}></div>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="resize-handle sw" onmousedown={(e) => startResize(e, 'SouthWest')}></div>
</div>

<style>
  :global(html),
  :global(body) {
    background: transparent !important;
    overflow: hidden;
  }

  .sticky[data-theme='red'] {
    --note-bg: #2a1418;
    --note-bg-raised: #3d1c22;
    --note-text: #f0b8c0;
    --note-muted: #bc8088;
    --note-line: rgba(255, 255, 255, 0.1);
    --note-accent: #ef8a96;
    --note-hover: rgba(255, 255, 255, 0.06);
  }

  .sticky[data-theme='orange'] {
    --note-bg: #2a1c12;
    --note-bg-raised: #3d2918;
    --note-text: #f0c8a0;
    --note-muted: #bc9870;
    --note-line: rgba(255, 255, 255, 0.1);
    --note-accent: #f0a86a;
    --note-hover: rgba(255, 255, 255, 0.06);
  }

  .sticky[data-theme='yellow'] {
    --note-bg: #262210;
    --note-bg-raised: #363018;
    --note-text: #e8dc9a;
    --note-muted: #b0a468;
    --note-line: rgba(255, 255, 255, 0.1);
    --note-accent: #e8d46a;
    --note-hover: rgba(255, 255, 255, 0.06);
  }

  .sticky[data-theme='green'] {
    --note-bg: #14261c;
    --note-bg-raised: #1c3327;
    --note-text: #aee0c0;
    --note-muted: #7faa8f;
    --note-line: rgba(255, 255, 255, 0.1);
    --note-accent: #8fd9ab;
    --note-hover: rgba(255, 255, 255, 0.06);
  }

  .sticky[data-theme='navy'] {
    --note-bg: #141f33;
    --note-bg-raised: #1c2b45;
    --note-text: #b0c9ee;
    --note-muted: #7f97bc;
    --note-line: rgba(255, 255, 255, 0.1);
    --note-accent: #8fb4ef;
    --note-hover: rgba(255, 255, 255, 0.06);
  }

  .sticky[data-theme='indigo'] {
    --note-bg: #18182e;
    --note-bg-raised: #222240;
    --note-text: #b8b8e8;
    --note-muted: #8484b8;
    --note-line: rgba(255, 255, 255, 0.1);
    --note-accent: #9a9aef;
    --note-hover: rgba(255, 255, 255, 0.06);
  }

  .sticky[data-theme='purple'] {
    --note-bg: #241832;
    --note-bg-raised: #2e2040;
    --note-text: #d3bef0;
    --note-muted: #9c86bc;
    --note-line: rgba(255, 255, 255, 0.1);
    --note-accent: #c4a6f5;
    --note-hover: rgba(255, 255, 255, 0.06);
  }

  .sticky[data-theme='graphite'] {
    --note-bg: #1e2023;
    --note-bg-raised: #2a2d31;
    --note-text: #cbd0d6;
    --note-muted: #8e959d;
    --note-line: rgba(255, 255, 255, 0.1);
    --note-accent: #b8c0c9;
    --note-hover: rgba(255, 255, 255, 0.06);
  }

  .sticky {
    height: 100vh;
    width: 100vw;
    border-radius: 14px;
    border: 1px solid var(--note-line);
    background: linear-gradient(180deg, var(--note-bg-raised), var(--note-bg));
    box-shadow:
      inset 0 1px 0 rgba(255, 255, 255, 0.06),
      0 14px 36px rgba(0, 0, 0, 0.45);
    color: var(--note-text);
    display: flex;
    flex-direction: column;
    overflow: hidden;
    position: relative;
  }

  .toolbar {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 8px 8px 4px 10px;
    cursor: grab;
  }

  .title {
    flex: 1;
    border: none;
    background: transparent;
    color: var(--note-text);
    font-weight: 700;
    font-size: 0.95rem;
    outline: none;
    min-width: 0;
    cursor: grab;
  }

  .title:focus {
    cursor: text;
  }

  .title::placeholder {
    color: var(--note-muted);
  }

  .actions {
    display: flex;
    gap: 2px;
  }

  .icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border: none;
    background: transparent;
    color: var(--note-text);
    border-radius: 6px;
    width: 28px;
    height: 28px;
    line-height: 1;
    flex-shrink: 0;
    padding: 0;
  }

  .icon:hover {
    background: var(--note-hover);
  }

  .icon.del {
    width: 24px;
    height: 24px;
  }

  .ico {
    width: 15px;
    height: 15px;
    display: block;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.4;
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  .ico.grip {
    width: 12px;
    height: 14px;
  }

  .chrome {
    opacity: 0;
    pointer-events: none;
    transition: opacity 0.15s ease;
  }

  .sticky:hover .chrome,
  .sticky:focus-within .chrome,
  .sticky.chrome-locked .chrome {
    opacity: 0.85;
    pointer-events: auto;
  }

  .sticky:hover .icon.del,
  .sticky:focus-within .icon.del,
  .sticky.chrome-locked .icon.del {
    opacity: 0.7;
  }

  .color-pop {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    padding: 0 10px 10px;
  }

  .swatch {
    width: 22px;
    height: 22px;
    border-radius: 50%;
    border: 2px solid rgba(255, 255, 255, 0.18);
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.08);
    padding: 0;
  }

  .swatch.active {
    border-color: var(--note-accent);
    box-shadow:
      0 0 0 2px color-mix(in srgb, var(--note-accent) 35%, transparent),
      inset 0 1px 0 rgba(255, 255, 255, 0.08);
  }

  .checklist {
    flex: 1;
    overflow: auto;
    padding: 2px 6px 4px;
    min-height: 0;
  }

  .item {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 4px 4px;
    border-radius: 6px;
  }

  .item:hover {
    background: var(--note-hover);
  }

  .item.done .item-title {
    text-decoration: line-through;
    opacity: 0.55;
  }

  .dnd-handle {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    cursor: grab;
    color: var(--note-muted);
    user-select: none;
    padding: 0 2px;
    flex-shrink: 0;
  }

  .check {
    width: 16px;
    height: 16px;
    flex-shrink: 0;
    accent-color: var(--note-accent);
  }

  .item-title {
    flex: 1;
    border: none;
    background: transparent;
    color: var(--note-text);
    outline: none;
    font-size: 0.88rem;
    min-width: 0;
    padding: 2px 0;
  }

  .add-row {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 0 10px;
    max-height: 0;
    opacity: 0;
    overflow: hidden;
    border-top: 1px solid transparent;
    transition:
      max-height 0.18s ease,
      opacity 0.15s ease,
      padding 0.18s ease,
      border-color 0.15s ease;
  }

  .sticky:hover .add-row,
  .sticky:focus-within .add-row,
  .sticky.chrome-locked .add-row {
    max-height: 48px;
    opacity: 1;
    padding: 6px 10px 10px;
    border-top-color: var(--note-line);
  }

  .add-input {
    flex: 1;
    border: none;
    background: transparent;
    color: var(--note-text);
    outline: none;
    font-size: 0.88rem;
    min-width: 0;
  }

  .add-input::placeholder {
    color: var(--note-muted);
  }

  .sticky-error {
    position: absolute;
    left: 8px;
    right: 8px;
    bottom: 8px;
    background: rgba(198, 40, 40, 0.92);
    color: #fff;
    border-radius: 6px;
    padding: 6px 8px;
    font-size: 0.75rem;
  }

  .resize-handle {
    position: absolute;
    z-index: 5;
  }

  .resize-handle.n {
    top: 0;
    left: 10px;
    right: 10px;
    height: 6px;
    cursor: n-resize;
  }

  .resize-handle.s {
    bottom: 0;
    left: 10px;
    right: 10px;
    height: 6px;
    cursor: s-resize;
  }

  .resize-handle.e {
    top: 10px;
    right: 0;
    bottom: 10px;
    width: 6px;
    cursor: e-resize;
  }

  .resize-handle.w {
    top: 10px;
    left: 0;
    bottom: 10px;
    width: 6px;
    cursor: w-resize;
  }

  .resize-handle.ne {
    top: 0;
    right: 0;
    width: 12px;
    height: 12px;
    cursor: ne-resize;
  }

  .resize-handle.nw {
    top: 0;
    left: 0;
    width: 12px;
    height: 12px;
    cursor: nw-resize;
  }

  .resize-handle.se {
    bottom: 0;
    right: 0;
    width: 12px;
    height: 12px;
    cursor: se-resize;
  }

  .resize-handle.sw {
    bottom: 0;
    left: 0;
    width: 12px;
    height: 12px;
    cursor: sw-resize;
  }
</style>
