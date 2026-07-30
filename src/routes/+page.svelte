<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';

  onMount(async () => {
    try {
      await invoke('open_settings');
    } catch {
      // Running in browser preview without Tauri
    }
  });
</script>

<div class="app-shell">
  <div class="panel stack">
    <h1>Keepy Note</h1>
    <p class="muted">
      Running in the system tray. Use the tray icon to pin a task list or open Settings.
    </p>
  </div>
</div>
