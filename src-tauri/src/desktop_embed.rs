#[cfg(windows)]
mod win {
    use windows::Win32::Foundation::{BOOL, COLORREF, HWND, LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, FindWindowA, FindWindowExA, GetWindowLongPtrW, SendMessageTimeoutA,
        SetLayeredWindowAttributes, SetParent, SetWindowLongPtrW, SetWindowPos, GWL_EXSTYLE,
        GWL_STYLE, HWND_TOP, LWA_ALPHA, SMTO_NORMAL, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE,
        SWP_NOSIZE, SWP_SHOWWINDOW, WINDOW_EX_STYLE, WINDOW_STYLE, WS_CHILD, WS_EX_LAYERED,
        WS_EX_TOOLWINDOW, WS_POPUP,
    };

    #[derive(Default)]
    struct DesktopHosts {
        progman: HWND,
        def_view: HWND,
        worker_w: HWND,
    }

    unsafe extern "system" fn enum_desktop_hosts(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let hosts = &mut *(lparam.0 as *mut DesktopHosts);
        let defview =
            FindWindowExA(hwnd, HWND::default(), windows::core::s!("SHELLDLL_DefView"), None)
                .unwrap_or_default();
        if defview.is_invalid() {
            return BOOL(1);
        }

        hosts.def_view = defview;

        // Legacy layout: WorkerW is the next top-level sibling after the DefView host.
        let sibling =
            FindWindowExA(HWND::default(), hwnd, windows::core::s!("WorkerW"), None)
                .unwrap_or_default();
        if !sibling.is_invalid() {
            hosts.worker_w = sibling;
        }

        BOOL(0)
    }

    fn find_desktop_hosts() -> Option<DesktopHosts> {
        unsafe {
            let progman = FindWindowA(windows::core::s!("Progman"), None).ok()?;
            if progman.is_invalid() {
                return None;
            }

            // Request the "raised desktop" so wallpaper / WorkerW layers exist.
            let mut result: usize = 0;
            let _ = SendMessageTimeoutA(
                progman,
                0x052C,
                WPARAM(0),
                LPARAM(0),
                SMTO_NORMAL,
                1000,
                Some(&mut result),
            );
            // Win11 variants sometimes need the alternate spawn parameters.
            let _ = SendMessageTimeoutA(
                progman,
                0x052C,
                WPARAM(0xD),
                LPARAM(0),
                SMTO_NORMAL,
                1000,
                Some(&mut result),
            );
            let _ = SendMessageTimeoutA(
                progman,
                0x052C,
                WPARAM(0xD),
                LPARAM(1),
                SMTO_NORMAL,
                1000,
                Some(&mut result),
            );

            let mut hosts = DesktopHosts {
                progman,
                ..Default::default()
            };
            let _ = EnumWindows(
                Some(enum_desktop_hosts),
                LPARAM(&mut hosts as *mut _ as isize),
            );

            // Win11 24H2+: WorkerW is a child of Progman, not a top-level sibling.
            if hosts.worker_w.is_invalid() {
                let mut child = HWND::default();
                loop {
                    child = FindWindowExA(progman, child, windows::core::s!("WorkerW"), None)
                        .unwrap_or_default();
                    if child.is_invalid() {
                        break;
                    }
                    hosts.worker_w = child;
                }
            }

            if hosts.def_view.is_invalid() {
                hosts.def_view = FindWindowExA(
                    progman,
                    HWND::default(),
                    windows::core::s!("SHELLDLL_DefView"),
                    None,
                )
                .unwrap_or_default();
            }

            if hosts.def_view.is_invalid() && hosts.worker_w.is_invalid() {
                None
            } else {
                Some(hosts)
            }
        }
    }

    pub fn embed_window(child_hwnd: isize) -> Result<(), String> {
        let hosts =
            find_desktop_hosts().ok_or_else(|| "Could not locate desktop shell layer".to_string())?;
        unsafe {
            let child = HWND(child_hwnd as *mut core::ffi::c_void);

            let style = GetWindowLongPtrW(child, GWL_STYLE) as u32;
            let new_style = WINDOW_STYLE((style & !WS_POPUP.0) | WS_CHILD.0);
            SetWindowLongPtrW(child, GWL_STYLE, new_style.0 as isize);

            let ex = GetWindowLongPtrW(child, GWL_EXSTYLE) as u32;
            let new_ex = WINDOW_EX_STYLE(ex | WS_EX_TOOLWINDOW.0 | WS_EX_LAYERED.0);
            SetWindowLongPtrW(child, GWL_EXSTYLE, new_ex.0 as isize);
            let _ = SetLayeredWindowAttributes(child, COLORREF(0), 255, LWA_ALPHA);

            // Prefer Progman parenting (Win11). Fall back to WorkerW (Win10 / older Win11).
            let parent = if !hosts.progman.is_invalid() && !hosts.def_view.is_invalid() {
                hosts.progman
            } else if !hosts.worker_w.is_invalid() {
                hosts.worker_w
            } else {
                return Err("Could not locate Progman or WorkerW for desktop embed".into());
            };

            SetParent(child, parent).map_err(|e| e.to_string())?;

            if !hosts.def_view.is_invalid() && parent == hosts.progman {
                // Sit under desktop icons (DefView) and above wallpaper WorkerW.
                let _ = SetWindowPos(
                    child,
                    hosts.def_view,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW | SWP_FRAMECHANGED,
                );
                if !hosts.worker_w.is_invalid() {
                    let _ = SetWindowPos(
                        hosts.worker_w,
                        child,
                        0,
                        0,
                        0,
                        0,
                        SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
                    );
                }
            } else {
                let _ = SetWindowPos(
                    child,
                    HWND_TOP,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW | SWP_FRAMECHANGED,
                );
            }
        }
        Ok(())
    }

    pub fn unembed_window(child_hwnd: isize) -> Result<(), String> {
        unsafe {
            let child = HWND(child_hwnd as *mut core::ffi::c_void);
            SetParent(child, HWND::default()).map_err(|e| e.to_string())?;
            let style = GetWindowLongPtrW(child, GWL_STYLE) as u32;
            let new_style = WINDOW_STYLE((style & !WS_CHILD.0) | WS_POPUP.0);
            SetWindowLongPtrW(child, GWL_STYLE, new_style.0 as isize);
            let ex = GetWindowLongPtrW(child, GWL_EXSTYLE) as u32;
            let new_ex = WINDOW_EX_STYLE(ex & !WS_EX_LAYERED.0);
            SetWindowLongPtrW(child, GWL_EXSTYLE, new_ex.0 as isize);
            let _ = SetWindowPos(
                child,
                HWND_TOP,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW | SWP_FRAMECHANGED,
            );
        }
        Ok(())
    }
}

#[cfg(windows)]
pub use win::{embed_window, unembed_window};

#[cfg(not(windows))]
pub fn embed_window(_child_hwnd: isize) -> Result<(), String> {
    Err("Desktop embed is only supported on Windows".into())
}

#[cfg(not(windows))]
pub fn unembed_window(_child_hwnd: isize) -> Result<(), String> {
    Ok(())
}
