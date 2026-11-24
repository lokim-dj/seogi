//! Whether the place that has the keyboard takes text, asked just before a
//! dictation's words are typed. When it plainly does not (a button, a check
//! box or a menu has the keyboard), the words are not typed into nowhere: the
//! indicator offers to copy them instead.
//!
//! On macOS this asks the system's accessibility interface, which Seogi may
//! use once the user allowed it to type. Anything it cannot tell is left to
//! the paste, as before. That includes a whole window or app, nothing
//! reported as focused, and tables: Electron apps (chat, editors) and
//! spreadsheets report those while still taking a paste. Windows is not
//! asked (the paste is tried there).

/// `Some(true)` the focused element takes text, `Some(false)` it plainly does
/// not, `None` it cannot be told.
pub fn takes_text() -> Option<bool> {
    #[cfg(target_os = "macos")]
    {
        mac::takes_text()
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

/// What a focused element's role and abilities say: `None` when they do not
/// settle it. Only controls that never hold text count as plainly not.
pub(crate) fn judge(role: Option<&str>, has_text_range: bool) -> Option<bool> {
    const TEXT: &[&str] = &["AXTextField", "AXTextArea", "AXComboBox", "AXSearchField"];
    const NOT_TEXT: &[&str] = &[
        "AXButton",
        "AXCheckBox",
        "AXRadioButton",
        "AXPopUpButton",
        "AXMenuButton",
        "AXMenuItem",
        "AXMenuBarItem",
        "AXSlider",
    ];
    if has_text_range || role.is_some_and(|role| TEXT.contains(&role)) {
        return Some(true);
    }
    if role.is_some_and(|role| NOT_TEXT.contains(&role)) {
        return Some(false);
    }
    None
}

#[cfg(target_os = "macos")]
mod mac {
    use std::ffi::{c_char, c_void, CString};

    type CFTypeRef = *const c_void;
    type CFStringRef = *const c_void;
    type AXUIElementRef = *const c_void;

    const UTF8: u32 = 0x0800_0100;
    const AX_SUCCESS: i32 = 0;

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXUIElementCreateSystemWide() -> AXUIElementRef;
        fn AXUIElementCopyAttributeValue(
            element: AXUIElementRef,
            attribute: CFStringRef,
            value: *mut CFTypeRef,
        ) -> i32;
        fn AXIsProcessTrusted() -> bool;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFStringCreateWithCString(
            allocator: *const c_void,
            text: *const c_char,
            encoding: u32,
        ) -> CFStringRef;
        fn CFStringGetCString(
            text: CFStringRef,
            buffer: *mut c_char,
            size: isize,
            encoding: u32,
        ) -> bool;
        fn CFGetTypeID(value: CFTypeRef) -> usize;
        fn CFStringGetTypeID() -> usize;
        fn CFRelease(value: CFTypeRef);
    }

    /// A Core Foundation object released when dropped.
    struct Owned(CFTypeRef);
    impl Drop for Owned {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: an object this module created or copied, once.
                unsafe { CFRelease(self.0) }
            }
        }
    }

    fn cf_string(text: &str) -> Option<Owned> {
        let text = CString::new(text).ok()?;
        // SAFETY: a valid C string; the result is owned and released.
        let made = unsafe { CFStringCreateWithCString(std::ptr::null(), text.as_ptr(), UTF8) };
        (!made.is_null()).then_some(Owned(made))
    }

    /// An attribute of `element`, and the AX error when there is none.
    fn attribute(element: AXUIElementRef, name: &str) -> Result<Owned, i32> {
        let name = cf_string(name).ok_or(-1)?;
        let mut value: CFTypeRef = std::ptr::null();
        // SAFETY: `element` is a live AX element, `name` a CFString, and the
        // copied value is owned and released.
        let error = unsafe { AXUIElementCopyAttributeValue(element, name.0, &mut value) };
        if error == AX_SUCCESS && !value.is_null() {
            Ok(Owned(value))
        } else {
            Err(error)
        }
    }

    fn text_of(value: &Owned) -> Option<String> {
        // SAFETY: type checks on a live CF object.
        if unsafe { CFGetTypeID(value.0) != CFStringGetTypeID() } {
            return None;
        }
        let mut buffer = vec![0 as c_char; 256];
        // SAFETY: the buffer is as long as said.
        let ok = unsafe {
            CFStringGetCString(value.0, buffer.as_mut_ptr(), buffer.len() as isize, UTF8)
        };
        if !ok {
            return None;
        }
        // SAFETY: CFStringGetCString wrote a terminated string.
        let text = unsafe { std::ffi::CStr::from_ptr(buffer.as_ptr()) };
        Some(text.to_string_lossy().into_owned())
    }

    pub fn takes_text() -> Option<bool> {
        // SAFETY: a plain query.
        if !unsafe { AXIsProcessTrusted() } {
            return None;
        }
        // SAFETY: returns an owned element, released by `Owned`.
        let system = Owned(unsafe { AXUIElementCreateSystemWide() });
        if system.0.is_null() {
            return None;
        }
        // Nothing reported as focused is not "nowhere": some apps (Electron
        // ones among them) do not report their focus. The paste decides.
        let focused = attribute(system.0, "AXFocusedUIElement").ok()?;
        let role = attribute(focused.0, "AXRole")
            .ok()
            .and_then(|r| text_of(&r));
        let has_range = attribute(focused.0, "AXSelectedTextRange").is_ok();
        let verdict = super::judge(role.as_deref(), has_range);
        log::debug!(
            "The keyboard is with a {} ({}text range): {:?}",
            role.as_deref().unwrap_or("?"),
            if has_range { "" } else { "no " },
            verdict
        );
        verdict
    }
}

#[cfg(test)]
mod tests {
    use super::judge;

    /// Asks the real system; run by hand: `cargo test -- --ignored keyboard_now`.
    #[test]
    #[ignore]
    fn keyboard_now() {
        println!("takes text: {:?}", super::takes_text());
    }

    #[test]
    fn text_places_take_text_and_plain_controls_do_not() {
        assert_eq!(judge(Some("AXTextArea"), false), Some(true));
        // A web editor that is only a group, but has a caret.
        assert_eq!(judge(Some("AXGroup"), true), Some(true));
        assert_eq!(judge(Some("AXButton"), false), Some(false));
        assert_eq!(judge(Some("AXCheckBox"), false), Some(false));
        assert_eq!(judge(Some("AXMenuItem"), false), Some(false));
        // Anything else is left to the paste.
        assert_eq!(judge(Some("AXGroup"), false), None);
        assert_eq!(judge(None, false), None);
    }

    #[test]
    fn windows_apps_and_tables_are_left_to_the_paste() {
        // Electron apps report the window or the app; spreadsheets a cell.
        for role in [
            "AXWindow",
            "AXApplication",
            "AXWebArea",
            "AXTable",
            "AXRow",
            "AXCell",
            "AXList",
            "AXOutline",
        ] {
            assert_eq!(judge(Some(role), false), None, "{role}");
        }
    }
}
