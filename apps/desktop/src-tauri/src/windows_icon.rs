//! Sharp window and taskbar icons on Windows.
//!
//! Tauri builds the window icon from one image of `icon.ico` and hands that
//! single bitmap to Windows, which then stretches it to the title-bar and
//! taskbar sizes, so the taskbar button looks blocky. The executable already
//! embeds the full multi-size icon, so this loads it from the exe resources
//! at exactly the sizes Windows asks for at the window's DPI (Windows picks
//! the closest hand-made entry) and sets those as the window's small and big
//! icons. Re-applied when the window moves to a monitor with another scale.
#![allow(unsafe_code)] // Win32 FFI: resource lookup and WM_SETICON.

use std::ffi::c_void;
use std::ptr::null;

use windows_sys::Win32::Foundation::{BOOL, HMODULE};
use windows_sys::Win32::System::LibraryLoader::{EnumResourceNamesW, GetModuleHandleW};
use windows_sys::Win32::UI::HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DestroyIcon, LoadImageW, SendMessageW, ICON_BIG, ICON_SMALL, IMAGE_ICON, LR_DEFAULTCOLOR,
    SM_CXICON, SM_CXSMICON, WM_SETICON,
};

/// `RT_GROUP_ICON` (MAKEINTRESOURCE(14)).
const RT_GROUP_ICON: usize = 14;
/// Icon resource id tauri-build gives the app icon.
const TAURI_ICON_ID: usize = 32512;

/// A resource name: an integer id or a string.
enum ResourceName {
    Id(usize),
    Text(Vec<u16>),
}

impl ResourceName {
    fn as_ptr(&self) -> *const u16 {
        match self {
            ResourceName::Id(id) => *id as *const u16,
            ResourceName::Text(text) => text.as_ptr(),
        }
    }
}

unsafe extern "system" fn first_group_icon(
    _module: HMODULE,
    _kind: *const u16,
    name: *const u16,
    found: isize,
) -> BOOL {
    // SAFETY: `found` is the `Option<ResourceName>` passed by `app_icon_name`,
    // alive for the duration of EnumResourceNamesW.
    let found = unsafe { &mut *(found as *mut Option<ResourceName>) };
    let raw = name as usize;
    *found = Some(if raw >> 16 == 0 {
        ResourceName::Id(raw)
    } else {
        // SAFETY: a non-integer name is a NUL-terminated UTF-16 string that
        // is valid during this callback; copy it out.
        let mut len = 0;
        while unsafe { *name.add(len) } != 0 {
            len += 1;
        }
        let mut text = unsafe { std::slice::from_raw_parts(name, len) }.to_vec();
        text.push(0);
        ResourceName::Text(text)
    });
    0 // stop after the first icon group
}

/// The exe's app icon: tauri-build's id when present, else the first icon
/// group in the resources.
fn app_icon_name(module: HMODULE) -> Option<ResourceName> {
    // SAFETY: probing a resource id in our own module; a missing id returns null.
    let probe = unsafe {
        LoadImageW(
            module,
            TAURI_ICON_ID as *const u16,
            IMAGE_ICON,
            16,
            16,
            LR_DEFAULTCOLOR,
        )
    };
    if !probe.is_null() {
        // SAFETY: `probe` is an icon we just loaded and own.
        unsafe { DestroyIcon(probe) };
        return Some(ResourceName::Id(TAURI_ICON_ID));
    }
    let mut found: Option<ResourceName> = None;
    // SAFETY: the callback only writes through the pointer to `found`, which
    // outlives the call.
    unsafe {
        EnumResourceNamesW(
            module,
            RT_GROUP_ICON as *const u16,
            Some(first_group_icon),
            &mut found as *mut Option<ResourceName> as isize,
        );
    }
    found
}

/// Sets the window's small (title bar) and big (taskbar, Alt+Tab) icons from
/// the exe's multi-size icon at the window's current DPI. Does nothing if the
/// icon cannot be loaded, leaving Tauri's icon in place.
pub(crate) fn apply(hwnd: *mut c_void) {
    if hwnd.is_null() {
        return;
    }
    // SAFETY: plain Win32 calls on our own window and module; every returned
    // handle is checked before use.
    unsafe {
        let module = GetModuleHandleW(null());
        let Some(name) = app_icon_name(module) else {
            tracing::warn!("app icon resource not found; keeping the default window icon");
            return;
        };
        let dpi = match GetDpiForWindow(hwnd) {
            0 => 96,
            dpi => dpi,
        };
        for (kind, metric) in [(ICON_BIG, SM_CXICON), (ICON_SMALL, SM_CXSMICON)] {
            let size = GetSystemMetricsForDpi(metric, dpi);
            let icon = LoadImageW(
                module,
                name.as_ptr(),
                IMAGE_ICON,
                size,
                size,
                LR_DEFAULTCOLOR,
            );
            if !icon.is_null() {
                SendMessageW(hwnd, WM_SETICON, kind as usize, icon as isize);
            }
        }
    }
}
