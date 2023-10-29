// Copyright 2021 System76 <info@system76.com>
// SPDX-License-Identifier: MPL-2.0

use crate::exec::error::ExecError;
use crate::DesktopEntry;
use std::convert::TryFrom;
use std::path::PathBuf;
use std::process::Command;

pub mod error;

impl DesktopEntry<'_> {
    /// Launch the given desktop entry action.
    pub fn launch_action(&self, action: &str, uris: &[&str]) -> Result<(), ExecError> {
        let has_action = self
            .actions()
            .map_or(false,
                |actions|
                actions
                    .split(';')
                    .any(|act| act == action)
            );
        if !has_action {
            return Err(ExecError::ActionNotFound { action: action.to_string(), desktop_entry: self.path });
        }
        self.shell_launch(uris, Some(action.to_string()))
    }

    /// Launch the given desktop entry.
    pub fn launch(&self, uris: &[&str]) -> Result<(), ExecError> {
        self.shell_launch(uris, None)
    }

    fn shell_launch(&self, uris: &[&str], action: Option<String>) -> Result<(), ExecError> {
        let exec = if let Some(action) = action {
            self.action_exec(&action)
                .ok_or(ExecError::ActionExecKeyNotFound { action, desktop_entry: self.path })
        } else {
            self.exec()
                .ok_or(ExecError::MissingExecKey(self.path))
        }?;

        let exec_args =
            exec.split_ascii_whitespace()
                .map(ArgOrFieldCode::try_from)
                .collect::<Result<Vec<ArgOrFieldCode>, _>>()?;

        let mut exec_args = self.get_args(uris, exec_args);

        if exec_args.is_empty() {
            return Err(ExecError::EmptyExecString);
        }

        let exec; // trick to keep terminal.to_string_lossy() in scope
        let (exec, args) = if self.terminal() {
            let (terminal, separator) = detect_terminal();
            exec_args.insert(0, separator.to_owned());
            exec = terminal.to_string_lossy().to_string();
            (&exec, &exec_args[..])
        } else {
            (&exec_args[0], &exec_args[1..])
        };

        let mut cmd = Command::new(exec);

        if let Some(ref dir) = self.path() {
            cmd.current_dir(dir.as_ref());
        }
        cmd.args(args).spawn().map(|_| ()).map_err(ExecError::IoError)
    }

    // Replace field code with their values and ignore deprecated and unknown field codes
    fn get_args(&self, uris: &[&str], exec_args: Vec<ArgOrFieldCode>) -> Vec<String> {
        exec_args
            .iter()
            .filter_map(|arg| match arg {
                ArgOrFieldCode::SingleFileName | ArgOrFieldCode::SingleUrl => {
                    uris.first().map(|filename| filename.to_string())
                }
                ArgOrFieldCode::FileList | ArgOrFieldCode::UrlList => {
                    if !uris.is_empty() {
                        Some(uris.join(" "))
                    } else {
                        None
                    }
                }
                ArgOrFieldCode::IconKey => self.icon().map(ToString::to_string),
                ArgOrFieldCode::TranslatedName => {
                    let locale = std::env::var("LANG").ok();
                    if let Some(locale) = locale {
                        let locale = locale.split_once('.').map(|(locale, _)| locale);
                        self.name(locale).map(|locale| locale.to_string())
                    } else {
                        None
                    }
                }
                ArgOrFieldCode::DesktopFileLocation => Some(self.path.to_string_lossy().to_string()),
                ArgOrFieldCode::Arg(arg) => Some(arg.to_string()),
            })
            .collect()
    }
}

// either a command line argument or a field-code as described
// in https://specifications.freedesktop.org/desktop-entry-spec/desktop-entry-spec-latest.html#exec-variables
enum ArgOrFieldCode<'a> {
    SingleFileName,
    FileList,
    SingleUrl,
    UrlList,
    IconKey,
    TranslatedName,
    DesktopFileLocation,
    Arg(&'a str),
}

impl<'a> TryFrom<&'a str> for ArgOrFieldCode<'a> {
    type Error = ExecError<'a>;

    fn try_from(value: &'a str) -> Result<Self, Self::Error> {
        match value {
            "%f" => Ok(ArgOrFieldCode::SingleFileName),
            "%F" => Ok(ArgOrFieldCode::FileList),
            "%u" => Ok(ArgOrFieldCode::SingleUrl),
            "%U" => Ok(ArgOrFieldCode::UrlList),
            "%i" => Ok(ArgOrFieldCode::IconKey),
            "%c" => Ok(ArgOrFieldCode::TranslatedName),
            "%k" => Ok(ArgOrFieldCode::DesktopFileLocation),
            "%d" | "%D" | "%n" | "%N" | "%v" | "%m" => Err(ExecError::DeprecatedFieldCode(value.to_string())),
            other if other.starts_with('%') => Err(ExecError::UnknownFieldCode(other.to_string())),
            other => Ok(ArgOrFieldCode::Arg(other)),
        }
    }
}

// Returns the default terminal emulator linked to `/usr/bin/x-terminal-emulator`
// or fallback to gnome terminal, then konsole
fn detect_terminal() -> (PathBuf, &'static str) {
    use std::fs::read_link;

    const SYMLINK: &str = "/usr/bin/x-terminal-emulator";

    if let Ok(found) = read_link(SYMLINK) {
        let arg = if found.to_string_lossy().contains("gnome-terminal") { "--" } else { "-e" };

        return (read_link(&found).unwrap_or(found), arg);
    }

    let gnome_terminal = PathBuf::from("/usr/bin/gnome-terminal");
    if gnome_terminal.exists() {
        (gnome_terminal, "--")
    } else {
        (PathBuf::from("/usr/bin/konsole"), "-e")
    }
}

#[cfg(test)]
mod test {
    use crate::exec::error::ExecError;
    use crate::DesktopEntry;
    use speculoos::prelude::*;
    use std::fs;
    use std::path::{Path, PathBuf};

    #[test]
    fn should_fail_if_exec_string_is_empty() {
        let path = PathBuf::from("tests/entries/empty-exec.desktop");
        let input = fs::read_to_string(&path).unwrap();
        let de = DesktopEntry::decode(Path::new(path.as_path()), &input).unwrap();
        let result = de.launch(&[]);

        assert_that!(result).is_err().matches(|err| matches!(err, ExecError::EmptyExecString));
    }

    #[test]
    #[ignore = "Needs a desktop environment and alacritty installed, run locally only"]
    fn should_exec_simple_command() {
        let path = PathBuf::from("tests/entries/alacritty-simple.desktop");
        let input = fs::read_to_string(&path).unwrap();
        let de = DesktopEntry::decode(path.as_path(), &input).unwrap();
        let result = de.launch(&[]);

        assert_that!(result).is_ok();
    }

    #[test]
    #[ignore = "Needs a desktop environment and alacritty and mesa-utils installed, run locally only"]
    fn should_exec_complex_command() {
        let path = PathBuf::from("tests/entries/non-terminal-cmd.desktop");
        let input = fs::read_to_string(&path).unwrap();
        let de = DesktopEntry::decode(path.as_path(), &input).unwrap();
        let result = de.launch(&[]);

        assert_that!(result).is_ok();
    }

    #[test]
    #[ignore = "Needs a desktop environment and alacritty and mesa-utils installed, run locally only"]
    fn should_exec_terminal_command() {
        let path = PathBuf::from("tests/entries/non-terminal-cmd.desktop");
        let input = fs::read_to_string(&path).unwrap();
        let de = DesktopEntry::decode(path.as_path(), &input).unwrap();
        let result = de.launch(&[]);

        assert_that!(result).is_ok();
    }

    #[test]
    #[ignore = "Needs a desktop environment with nvim installed, run locally only"]
    fn should_launch_with_field_codes() {
        let path = PathBuf::from("/usr/share/applications/nvim.desktop");
        let input = fs::read_to_string(&path).unwrap();
        let de = DesktopEntry::decode(path.as_path(), &input).unwrap();
        let result = de.launch(&["src/lib.rs"]);

        assert_that!(result).is_ok();
    }

    #[test]
    #[ignore = "Needs a desktop environment with alacritty installed, run locally only"]
    fn should_launch_action() {
        let path = PathBuf::from("/usr/share/applications/Alacritty.desktop");
        let input = fs::read_to_string(&path).unwrap();
        let de = DesktopEntry::decode(path.as_path(), &input).unwrap();
        let result = de.launch_action("New", &[]);

        assert_that!(result).is_ok();
    }
}
