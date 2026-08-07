/**
 * Static Ralph checks for main vs list tray split (VALIDATION.md #3–7, evidence for #8–14).
 * Exit 0 only if all assertions pass.
 */
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const lib = fs.readFileSync(path.join(root, 'src-tauri/src/lib.rs'), 'utf8');
const stickyTray = fs.readFileSync(path.join(root, 'src-tauri/src/sticky_tray.rs'), 'utf8');
const page = fs.readFileSync(path.join(root, 'src/routes/+page.svelte'), 'utf8');

const checks = [];

function assert(id, ok, detail) {
  checks.push({ id, ok, detail });
  console.log(`${ok ? 'PASS' : 'FAIL'} ${id}: ${detail}`);
}

assert(
  3,
  !lib.includes('fn setup_tray') && !lib.includes('TrayIconBuilder::new()'),
  'no setup_tray / global TrayIconBuilder::new in lib.rs',
);

assert(
  4,
  lib.includes('"--autostart".into()') || lib.includes('"--autostart"'),
  'autostart init includes --autostart',
);

assert(
  5,
  /let from_autostart = std::env::args\(\)\.any\(\|a\| a == "--autostart"\);[\s\S]*if !from_autostart \{[\s\S]*open_main_window/.test(
    lib,
  ),
  'open_main_window gated behind !from_autostart',
);

assert(
  6,
  stickyTray.includes('open-main-{sticky_id}') && stickyTray.includes('quit-{sticky_id}'),
  'sticky tray menu IDs include open-main + quit',
);

assert(
  7,
  lib.includes('tauri_plugin_single_instance::init') &&
    /single_instance::init\(\|app[^|]*\|[^|]*\|[^|]*\| \{[\s\S]*open_main_window/.test(lib),
  'single-instance plugin opens main',
);

assert(
  8,
  lib.includes('restore_stickies') &&
    !lib.includes('fn setup_tray') &&
    !page.includes('Quit from the Keepy Note tray menu'),
  'interactive path opens main; app tray removed; copy updated',
);

assert(
  9,
  lib.includes('ExitRequested') && lib.includes('prevent_exit'),
  'closing windows does not exit while lists may remain',
);

assert(
  10,
  stickyTray.includes('show_sticky') && stickyTray.includes('close_sticky') && stickyTray.includes('Show'),
  'list tray Show/Close wired',
);

assert(
  11,
  stickyTray.includes('Open Keepy Note') && stickyTray.includes('crate::open_main_window'),
  'list tray Open Keepy Note opens main',
);

assert(
  12,
  lib.includes('restore_stickies(app.handle(), &state)') &&
    /if !from_autostart \{[\s\S]*open_main_window/.test(lib),
  'boot path restores stickies without opening main when --autostart',
);

assert(
  13,
  lib.includes('tauri_plugin_single_instance::init(|app, _args, _cwd|') &&
    lib.includes('open_main_window(app)'),
  'second launch focuses/opens main',
);

assert(
  14,
  stickyTray.includes('Quit') && stickyTray.includes('app.exit(0)'),
  'Quit from list tray exits process',
);

const failed = checks.filter((c) => !c.ok);
if (failed.length) {
  console.error(`\n${failed.length} check(s) failed`);
  process.exit(1);
}
console.log(`\nAll ${checks.length} static/evidence checks passed`);
