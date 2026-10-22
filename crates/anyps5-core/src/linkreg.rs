//! The per-user Windows registration of the `anyps5` URL scheme and of the "Open with AnyPS5"
//! entry of folders (the same keys the installer writes, docs/DESIGN.md 9.5).
//!
//! ```text
//! HKCU\Software\Classes\anyps5                         "URL:AnyPS5 Link", "URL Protocol"
//! HKCU\Software\Classes\anyps5\shell\open\command      "<exe>" "%1"
//! HKCU\Software\Classes\Directory\shell\AnyPS5         "Open with AnyPS5"   (folders only)
//! HKCU\Software\Classes\Directory\shell\AnyPS5\command "<exe>" "%1"
//! ```
//!
//! Nothing here needs administrator rights, and nothing is written outside those keys. The scheme
//! name is a parameter so the tests can use `anyps5-test...` and leave the real registration alone.

use std::io;
use std::path::Path;

/// Who answers the links right now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Handler {
    /// Nobody: the scheme is not registered.
    Absent,
    /// The launcher at the given path.
    ThisExe,
    /// A launcher somewhere else (an installed copy while this one is portable, say).
    Other(String),
}

/// The command a registration runs: the program and the one argument Windows hands over.
pub fn command_for(exe: &Path) -> String {
    format!("\"{}\" \"%1\"", exe.display())
}

/// The program path in a registered command (`"C:\x\a.exe" "%1"`).
pub fn program_of(command: &str) -> &str {
    let command = command.trim();
    match command.strip_prefix('"') {
        Some(rest) => rest.split('"').next().unwrap_or_default(),
        None => command.split(' ').next().unwrap_or_default(),
    }
}

#[cfg(any(windows, test))]
fn same_program(registered: &str, exe: &Path) -> bool {
    let normal = |s: &str| s.replace('/', "\\").to_lowercase();
    normal(registered) == normal(&exe.display().to_string())
}

/// Only the launcher's own scheme names are ever written or deleted.
#[cfg(any(windows, test))]
fn valid_scheme(scheme: &str) -> bool {
    scheme.starts_with("anyps5")
        && scheme
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

#[cfg(windows)]
mod imp {
    use super::*;
    use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, WIN32_ERROR};
    use windows::Win32::System::Registry::{
        HKEY, HKEY_CLASSES_ROOT, HKEY_CURRENT_USER, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ,
        RRF_RT_REG_SZ, RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegGetValueW, RegSetValueExW,
    };
    use windows::core::PCWSTR;

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn check(status: WIN32_ERROR) -> io::Result<()> {
        if status == ERROR_SUCCESS {
            Ok(())
        } else {
            Err(io::Error::from_raw_os_error(status.0 as i32))
        }
    }

    /// Sets the default value (`name` empty) or a named value of `HKCU\Software\Classes\<key>`.
    fn set(key: &str, name: &str, value: &str) -> io::Result<()> {
        let subkey = wide(&format!("Software\\Classes\\{key}"));
        let mut handle = HKEY::default();
        // SAFETY: the wide strings are NUL-terminated and outlive the calls; the handle is closed.
        unsafe {
            check(RegCreateKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(subkey.as_ptr()),
                None,
                PCWSTR::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_WRITE,
                None,
                &mut handle,
                None,
            ))?;
            let data = wide(value);
            let bytes = std::slice::from_raw_parts(data.as_ptr().cast::<u8>(), data.len() * 2);
            let name = wide(name);
            let name = if name.len() == 1 {
                PCWSTR::null()
            } else {
                PCWSTR(name.as_ptr())
            };
            let result = check(RegSetValueExW(handle, name, None, REG_SZ, Some(bytes)));
            let _ = RegCloseKey(handle);
            result
        }
    }

    /// Deletes `HKCU\Software\Classes\<key>` with everything below it. A missing key is fine.
    fn delete(key: &str) -> io::Result<()> {
        let subkey = wide(&format!("Software\\Classes\\{key}"));
        // SAFETY: a NUL-terminated wide string.
        let status = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(subkey.as_ptr())) };
        if status == ERROR_FILE_NOT_FOUND {
            Ok(())
        } else {
            check(status)
        }
    }

    /// The default value of `<key>` in the merged view of the classes (user over machine).
    fn read(key: &str) -> Option<String> {
        let subkey = wide(key);
        let mut buffer = [0u16; 1024];
        let mut size = std::mem::size_of_val(&buffer) as u32;
        // SAFETY: the buffer and its size describe the same memory.
        let status = unsafe {
            RegGetValueW(
                HKEY_CLASSES_ROOT,
                PCWSTR(subkey.as_ptr()),
                PCWSTR::null(),
                RRF_RT_REG_SZ,
                None,
                Some(buffer.as_mut_ptr().cast()),
                Some(&mut size),
            )
        };
        if status != ERROR_SUCCESS {
            return None;
        }
        let len = buffer.iter().position(|c| *c == 0).unwrap_or(buffer.len());
        Some(String::from_utf16_lossy(&buffer[..len]))
    }

    pub fn status(scheme: &str, exe: &Path) -> Handler {
        if !valid_scheme(scheme) {
            return Handler::Absent;
        }
        match read(&format!("{scheme}\\shell\\open\\command")) {
            None => Handler::Absent,
            Some(command) if same_program(program_of(&command), exe) => Handler::ThisExe,
            Some(command) => Handler::Other(program_of(&command).to_owned()),
        }
    }

    pub fn register(scheme: &str, exe: &Path, folder_menu: bool) -> io::Result<()> {
        if !valid_scheme(scheme) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "not an anyps5 scheme name",
            ));
        }
        let command = command_for(exe);
        let icon = format!("\"{}\",0", exe.display());
        set(scheme, "", "URL:AnyPS5 Link")?;
        set(scheme, "URL Protocol", "")?;
        set(&format!("{scheme}\\DefaultIcon"), "", &icon)?;
        set(&format!("{scheme}\\shell\\open\\command"), "", &command)?;
        if folder_menu {
            set("Directory\\shell\\AnyPS5", "", "Open with AnyPS5")?;
            set("Directory\\shell\\AnyPS5", "Icon", &icon)?;
            set("Directory\\shell\\AnyPS5\\command", "", &command)?;
        }
        Ok(())
    }

    pub fn unregister(scheme: &str, folder_menu: bool) -> io::Result<()> {
        if !valid_scheme(scheme) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "not an anyps5 scheme name",
            ));
        }
        delete(scheme)?;
        if folder_menu {
            delete("Directory\\shell\\AnyPS5")?;
        }
        Ok(())
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;

    pub fn status(_scheme: &str, _exe: &Path) -> Handler {
        Handler::Absent
    }

    pub fn register(_scheme: &str, _exe: &Path, _folder_menu: bool) -> io::Result<()> {
        Err(io::ErrorKind::Unsupported.into())
    }

    pub fn unregister(_scheme: &str, _folder_menu: bool) -> io::Result<()> {
        Err(io::ErrorKind::Unsupported.into())
    }
}

/// Who answers `<scheme>://` links now, compared with `exe`.
pub fn status(scheme: &str, exe: &Path) -> Handler {
    imp::status(scheme, exe)
}

/// Makes `exe` answer `<scheme>://` links, and adds "Open with AnyPS5" to folders when asked.
pub fn register(scheme: &str, exe: &Path, folder_menu: bool) -> io::Result<()> {
    imp::register(scheme, exe, folder_menu)
}

/// Removes the registration again (and the folder entry when asked). Missing keys are fine.
pub fn unregister(scheme: &str, folder_menu: bool) -> io::Result<()> {
    imp::unregister(scheme, folder_menu)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_program_of_a_command() {
        assert_eq!(program_of(r#""C:\a b\x.exe" "%1""#), r"C:\a b\x.exe");
        assert_eq!(program_of(r"C:\x.exe %1"), r"C:\x.exe");
        assert_eq!(program_of(""), "");
    }

    #[test]
    fn a_command_round_trips() {
        let exe = Path::new(r"C:\Program Files\AnyPS5 Launcher\AnyPS5-Launcher.exe");
        assert_eq!(program_of(&command_for(exe)), exe.to_str().unwrap());
        assert!(same_program(
            r"c:/program files/anyps5 launcher/anyps5-launcher.EXE",
            exe
        ));
        assert!(!same_program(r"C:\Other\AnyPS5-Launcher.exe", exe));
    }

    #[test]
    fn only_our_scheme_names_pass() {
        assert!(valid_scheme("anyps5"));
        assert!(valid_scheme("anyps5-test-1234"));
        for bad in [
            "",
            "http",
            "anyps5\\..\\x",
            "anyps5/x",
            "Anyps5",
            "*",
            "anyps5 x",
        ] {
            assert!(!valid_scheme(bad), "{bad:?}");
        }
        assert!(register("http", Path::new("x"), false).is_err());
        assert!(unregister("*", true).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn registers_reads_and_removes_a_test_scheme() {
        let scheme = format!("anyps5-test-unit-{}", std::process::id());
        let exe = Path::new(r"C:\Test Dir\launcher.exe");
        assert_eq!(status(&scheme, exe), Handler::Absent);
        register(&scheme, exe, false).unwrap();
        assert_eq!(status(&scheme, exe), Handler::ThisExe);
        assert_eq!(
            status(&scheme, Path::new(r"D:\else\x.exe")),
            Handler::Other(r"C:\Test Dir\launcher.exe".to_owned())
        );
        // Registering again with another program replaces the command.
        register(&scheme, Path::new(r"D:\else\x.exe"), false).unwrap();
        assert_eq!(
            status(&scheme, Path::new(r"D:\else\x.exe")),
            Handler::ThisExe
        );
        unregister(&scheme, false).unwrap();
        assert_eq!(status(&scheme, exe), Handler::Absent);
        // Removing what is not there is fine.
        unregister(&scheme, false).unwrap();
    }
}
