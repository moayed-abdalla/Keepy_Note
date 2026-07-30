//! Desktop pin mode keeps stickies as normal top-level windows (clickable),
//! just not always-on-top and lowered in the z-order.
//!
//! True Progman/WorkerW embedding fights live wallpaper engines (Wallpaper Engine,
//! Lively, etc.): those apps own the same shell layer and typically steal mouse input.

#[cfg(windows)]
mod win {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetParent, GetWindowLongPtrW, SetParent, SetWindowLongPtrW, SetWindowPos, GWL_EXSTYLE,
        GWL_STYLE, HWND_BOTTOM, HWND_TOP, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
        SWP_SHOWWINDOW, WINDOW_EX_STYLE, WINDOW_STYLE, WS_CHILD, WS_EX_LAYERED, WS_EX_TOOLWINDOW,
        WS_POPUP,
    };

    /// Soft desktop mode: stay a normal window, sit behind other apps.
    pub fn embed_window(child_hwnd: isize) -> Result<(), String> {
        unsafe {
            let child = HWND(child_hwnd as *mut core::ffi::c_void);
            // Recover from any previous hard shell-parent embed.
            detach_from_shell(child);
            let _ = SetWindowPos(
                child,
                HWND_BOTTOM,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW | SWP_FRAMECHANGED,
            );
        }
        Ok(())
    }

    pub fn unembed_window(child_hwnd: isize) -> Result<(), String> {
        unsafe {
            let child = HWND(child_hwnd as *mut core::ffi::c_void);
            detach_from_shell(child);
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

    unsafe fn detach_from_shell(child: HWND) {
        let parent = GetParent(child).unwrap_or_default();
        if !parent.is_invalid() {
            let _ = SetParent(child, HWND::default());
        }

        let style = GetWindowLongPtrW(child, GWL_STYLE) as u32;
        if style & WS_CHILD.0 != 0 {
            let new_style = WINDOW_STYLE((style & !WS_CHILD.0) | WS_POPUP.0);
            SetWindowLongPtrW(child, GWL_STYLE, new_style.0 as isize);
        }

        let ex = GetWindowLongPtrW(child, GWL_EXSTYLE) as u32;
        // Clear styles that hard-embed used; keep TOOLWINDOW so stickies stay out of the taskbar.
        let cleared = ex & !WS_EX_LAYERED.0;
        let new_ex = WINDOW_EX_STYLE(cleared | WS_EX_TOOLWINDOW.0);
        SetWindowLongPtrW(child, GWL_EXSTYLE, new_ex.0 as isize);
    }
}

#[cfg(windows)]
pub use win::{embed_window, unembed_window};

#[cfg(not(windows))]
pub fn embed_window(_child_hwnd: isize) -> Result<(), String> {
    Err("Desktop mode is only supported on Windows".into())
}

#[cfg(not(windows))]
pub fn unembed_window(_child_hwnd: isize) -> Result<(), String> {
    Ok(())
}
