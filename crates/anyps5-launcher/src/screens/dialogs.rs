//! Native Windows file dialogs through `rfd` (docs/DESIGN.md 9.2). Every function blocks until the
//! dialog closes; the shell's input is blocked meanwhile and released after, so keys and pad
//! presses made behind the dialog do not fire later.

use std::path::{Path, PathBuf};

use crate::ui::Shell;

fn blocked<T>(shell: &mut Shell, run: impl FnOnce() -> T) -> T {
    shell.input.blocked = true;
    let result = run();
    shell.input.blocked = false;
    result
}

/// A file name that is safe on Windows: no path separators or reserved characters.
pub fn safe_file_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control() {
                '-'
            } else {
                c
            }
        })
        .collect();
    cleaned.trim().trim_end_matches('.').to_owned()
}

/// Picks a folder, starting in `start`.
pub fn pick_folder(shell: &mut Shell, title: &str, start: Option<&Path>) -> Option<PathBuf> {
    blocked(shell, || {
        let mut dialog = rfd::FileDialog::new().set_title(title);
        if let Some(dir) = start.filter(|d| d.is_dir()) {
            dialog = dialog.set_directory(dir);
        }
        dialog.pick_folder()
    })
}

/// Picks one file with the given extensions (`("JSON", &["json"])`).
pub fn pick_file(
    shell: &mut Shell,
    title: &str,
    filter: (&str, &[&str]),
    start: Option<&Path>,
) -> Option<PathBuf> {
    blocked(shell, || {
        let mut dialog = rfd::FileDialog::new()
            .set_title(title)
            .add_filter(filter.0, filter.1);
        if let Some(dir) = start.filter(|d| d.is_dir()) {
            dialog = dialog.set_directory(dir);
        }
        dialog.pick_file()
    })
}

/// Asks where to save a file, suggesting `file_name`.
pub fn save_file(
    shell: &mut Shell,
    title: &str,
    file_name: &str,
    filter: (&str, &[&str]),
    start: Option<&Path>,
) -> Option<PathBuf> {
    blocked(shell, || {
        let mut dialog = rfd::FileDialog::new()
            .set_title(title)
            .set_file_name(safe_file_name(file_name))
            .add_filter(filter.0, filter.1);
        if let Some(dir) = start.filter(|d| d.is_dir()) {
            dialog = dialog.set_directory(dir);
        }
        dialog.save_file()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_lose_reserved_characters() {
        assert_eq!(
            safe_file_name("Stray - AnyPlay.json"),
            "Stray - AnyPlay.json"
        );
        assert_eq!(safe_file_name("Dungeons: Part 2/3?"), "Dungeons- Part 2-3-");
        assert_eq!(safe_file_name("  name. "), "name");
    }
}
