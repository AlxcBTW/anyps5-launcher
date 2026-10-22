//! Window chrome on Windows (DESIGN.md 9.2): the native title bar stays (snap layouts, resize
//! borders, accessibility) and is recoloured through DWM so it merges with the black backdrop.
//!
//! * `DWMWA_USE_IMMERSIVE_DARK_MODE`: dark caption buttons and menus (Windows 10 1809 and later;
//!   attribute 19 on builds before 20H1).
//! * `DWMWA_CAPTION_COLOR` black, `DWMWA_TEXT_COLOR` white, `DWMWA_BORDER_COLOR` black
//!   (Windows 11 only; older builds refuse the attributes and keep the dark caption).
//! * `DWMWA_WINDOW_CORNER_PREFERENCE` round (Windows 11; the window is not maximised by default
//!   on every PC, so the corners are asked for explicitly).
//!
//! Every call is best effort: a refused attribute leaves the system look in place. Other platforms
//! compile an empty [`apply`].

/// What was accepted, for the log and the tests.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Applied {
    pub dark_mode: bool,
    pub caption_colors: bool,
    pub round_corners: bool,
}

/// `COLORREF` (0x00BBGGRR) of an opaque colour.
pub const fn colorref(r: u8, g: u8, b: u8) -> u32 {
    (r as u32) | ((g as u32) << 8) | ((b as u32) << 16)
}

#[cfg(windows)]
mod imp {
    use super::{Applied, colorref};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows_sys::Win32::Graphics::Dwm::{
        DWMWA_BORDER_COLOR, DWMWA_CAPTION_COLOR, DWMWA_TEXT_COLOR, DWMWA_USE_IMMERSIVE_DARK_MODE,
        DWMWA_WINDOW_CORNER_PREFERENCE, DwmSetWindowAttribute,
    };

    /// `DWMWA_USE_IMMERSIVE_DARK_MODE` before Windows 10 20H1.
    const DARK_MODE_BEFORE_20H1: i32 = 19;
    /// `DWMWCP_ROUND`
    const CORNER_ROUND: u32 = 2;

    fn set<T>(hwnd: isize, attribute: i32, value: &T) -> bool {
        // SAFETY: `hwnd` is the window handle of this process; the pointer and size describe `value`.
        let result = unsafe {
            DwmSetWindowAttribute(
                hwnd as *mut core::ffi::c_void,
                attribute as u32,
                std::ptr::from_ref(value).cast(),
                std::mem::size_of::<T>() as u32,
            )
        };
        result == 0
    }

    fn high_contrast() -> bool {
        use windows_sys::Win32::UI::Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW};
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            SPI_GETHIGHCONTRAST, SystemParametersInfoW,
        };
        // SAFETY: HIGHCONTRASTW is plain data; zero is a valid value.
        let mut info: HIGHCONTRASTW = unsafe { std::mem::zeroed() };
        info.cbSize = std::mem::size_of::<HIGHCONTRASTW>() as u32;
        // SAFETY: info is a HIGHCONTRASTW with its size set, as SPI_GETHIGHCONTRAST requires.
        let ok = unsafe {
            SystemParametersInfoW(
                SPI_GETHIGHCONTRAST,
                info.cbSize,
                std::ptr::from_mut(&mut info).cast(),
                0,
            )
        };
        ok != 0 && info.dwFlags & HCF_HIGHCONTRASTON != 0
    }

    pub fn apply(window: &impl HasWindowHandle) -> Applied {
        let Ok(handle) = window.window_handle() else {
            return Applied::default();
        };
        let RawWindowHandle::Win32(handle) = handle.as_raw() else {
            return Applied::default();
        };
        let hwnd = handle.hwnd.get();
        let on: u32 = 1;
        let dark_mode =
            set(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, &on) || set(hwnd, DARK_MODE_BEFORE_20H1, &on);
        // Windows high contrast keeps the system caption colours.
        let colors = !high_contrast();
        let black = colorref(0, 0, 0);
        let white = colorref(255, 255, 255);
        let caption = colors && set(hwnd, DWMWA_CAPTION_COLOR, &black);
        let text = colors && set(hwnd, DWMWA_TEXT_COLOR, &white);
        let border = colors && set(hwnd, DWMWA_BORDER_COLOR, &black);
        let round_corners = set(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, &CORNER_ROUND);
        let applied = Applied {
            dark_mode,
            caption_colors: caption && text && border,
            round_corners,
        };
        tracing::debug!("window chrome: {applied:?}");
        applied
    }
}

/// Recolours the title bar of `window` (once, after the window exists).
#[cfg(windows)]
pub fn apply(window: &impl raw_window_handle::HasWindowHandle) -> Applied {
    imp::apply(window)
}

/// Other platforms keep their own title bar.
#[cfg(not(windows))]
pub fn apply<W>(_window: &W) -> Applied {
    Applied::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colorref_is_blue_green_red() {
        assert_eq!(colorref(0, 0, 0), 0);
        assert_eq!(colorref(255, 255, 255), 0x00FF_FFFF);
        assert_eq!(colorref(0x11, 0x22, 0x33), 0x0033_2211);
    }
}
