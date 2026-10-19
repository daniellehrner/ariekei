// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

//! External programs given by path. Tests replace `gh` and `claude` with Python fakes
//! (`ARIEKEI_GH`, `ARIEKEI_CLAUDE`); a `.py` path runs through Python, because Windows does
//! not honour the script's `#!` line. `ARIEKEI_PYTHON` names the interpreter (default
//! `python3`).

use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::process::Command;

/// The program to start for `path`, and the arguments that go before the caller's.
pub fn program(path: &Path) -> (OsString, Vec<OsString>) {
    if path.extension().is_some_and(|e| e == "py") {
        let python = std::env::var_os("ARIEKEI_PYTHON").unwrap_or_else(|| "python3".into());
        (python, vec!["-I".into(), path.into()])
    } else {
        (path.into(), vec![])
    }
}

/// A `Command` that opens no window: on Windows a console program (git, gh, Python) started
/// by Ariekei, a GUI app, would get a console window of its own, which takes the focus too.
pub fn quiet(program: impl AsRef<OsStr>) -> Command {
    #[allow(unused_mut)] // changed on Windows only
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// A `Command` for `path` (see [`program`]), with no window (see [`quiet`]).
pub fn command(path: &Path) -> Command {
    let (program, prefix) = program(path);
    let mut cmd = quiet(program);
    cmd.args(prefix);
    cmd
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn python_scripts_run_through_python() {
        let (program, args) = program(Path::new("/x/fake_gh.py"));
        assert!(program.to_string_lossy().contains("python"));
        assert_eq!(
            args,
            [OsString::from("-I"), OsString::from("/x/fake_gh.py")]
        );
        assert_eq!(
            super::program(Path::new("/usr/bin/gh")),
            ("/usr/bin/gh".into(), vec![])
        );
    }
}
