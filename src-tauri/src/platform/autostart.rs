//! Windows per-user autostart registry reconciliation (ADR-26).

use std::io;
use std::path::Path;
use winreg::RegKey;
use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE};

const RUN_KEY: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReconcileAction {
    None,
    Write,
    Remove,
}

pub fn expected_command(executable: &Path) -> String {
    format!(r#""{}""#, executable.display())
}

pub fn required_action(
    desired: bool,
    current_enabled: bool,
    current_command: Option<&str>,
    expected: &str,
) -> ReconcileAction {
    if desired {
        if current_enabled && current_command == Some(expected) {
            ReconcileAction::None
        } else {
            ReconcileAction::Write
        }
    } else if current_command.is_some() {
        ReconcileAction::Remove
    } else {
        ReconcileAction::None
    }
}

pub fn read_command(app_name: &str) -> io::Result<Option<String>> {
    let run = match RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(RUN_KEY, KEY_READ) {
        Ok(run) => run,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    match run.get_value(app_name) {
        Ok(command) => Ok(Some(command)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

pub fn ensure_run_key() -> io::Result<()> {
    RegKey::predef(HKEY_CURRENT_USER)
        .create_subkey(RUN_KEY)
        .map(|_| ())
}

pub fn write_command(app_name: &str, command: &str) -> io::Result<()> {
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags(RUN_KEY, KEY_SET_VALUE)?
        .set_value(app_name, &command)
}

pub fn remove_command(app_name: &str) -> io::Result<()> {
    let run = match RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(RUN_KEY, KEY_SET_VALUE)
    {
        Ok(run) => run,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    match run.delete_value(app_name) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_with_spaces_is_always_quoted() {
        assert_eq!(
            expected_command(Path::new(r"C:\Program Files\Typex\typex.exe")),
            r#""C:\Program Files\Typex\typex.exe""#
        );
    }

    #[test]
    fn stale_debug_path_is_repaired() {
        let expected = r#""C:\Users\me\AppData\Local\Programs\Typex\typex.exe""#;
        assert_eq!(
            required_action(
                true,
                true,
                Some(r"C:\work\Typex\src-tauri\target\debug\typex.exe "),
                expected,
            ),
            ReconcileAction::Write
        );
    }

    #[test]
    fn matching_enabled_command_is_not_rewritten() {
        let expected = r#""C:\Users\me\AppData\Local\Programs\Typex\typex.exe""#;
        assert_eq!(
            required_action(true, true, Some(expected), expected),
            ReconcileAction::None
        );
    }

    #[test]
    fn disabled_setting_removes_a_residual_entry() {
        assert_eq!(
            required_action(
                false,
                false,
                Some(r#""C:\old-install\typex.exe""#),
                r#""C:\current\typex.exe""#,
            ),
            ReconcileAction::Remove
        );
        assert_eq!(
            required_action(false, false, None, r#""C:\current\typex.exe""#),
            ReconcileAction::None
        );
    }
}
