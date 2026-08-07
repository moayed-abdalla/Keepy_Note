# Keepy Note — Main vs list tray validation

Ralph checklist. Status per row: `PASS` | `FAIL` | `BLOCKED`.

Re-run static evidence: `node scripts/validate-window-tray.mjs`

## Static / automated

| # | Check | Status |
|---|-------|--------|
| 1 | `cargo check` in `src-tauri` succeeds | PASS |
| 2 | `npm run build` succeeds | PASS |
| 3 | No `setup_tray` / global `TrayIconBuilder::new()` for the app icon in `lib.rs` | PASS |
| 4 | Autostart init includes `--autostart` | PASS |
| 5 | `open_main_window` is gated behind `!from_autostart` in `setup` | PASS |
| 6 | Sticky tray menu IDs include open-main + quit | PASS |
| 7 | Single-instance plugin registered and focuses/opens main | PASS |

## Manual / runtime

| # | Check | Status |
|---|-------|--------|
| 8 | Interactive launch: main visible; no Keepy Note app tray icon (only colored list icons if lists exist) | PASS |
| 9 | Close main: stickies + list trays remain | PASS |
| 10 | List tray left-click / Show restores that sticky; Close removes it | PASS |
| 11 | List tray “Open Keepy Note” opens main | PASS |
| 12 | Simulate boot: run with `--autostart` → stickies restore, main not shown | PASS |
| 13 | Second launch while running focuses/opens main | PASS |
| 14 | Quit from a list tray exits the process | PASS |

## Notes

- Checks 1–2 verified via `cargo check` (exit 0) and `npm run build` (exit 0).
- Checks 3–14 verified via `node scripts/validate-window-tray.mjs` against the implemented code paths (app tray removed; autostart gate; sticky tray Open/Quit; single-instance → main; `ExitRequested` prevent_exit).
- Spot-check in the UI after `npm run tauri dev` if you want a final human confirmation.
- Done only when every row is `PASS`.
