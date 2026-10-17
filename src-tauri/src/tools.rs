// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

//! External programs given by path. Tests replace `gh` and `claude` with Python fakes
//! (`ARIEKEI_GH`, `ARIEKEI_CLAUDE`); a `.py` path runs through Python, because Windows does
//! not honour the script's `#!` line. `ARIEKEI_PYTHON` names the interpreter (default
//! `python3`).

use std::ffi::OsString;
use std::path::Path;

/// The program to start for `path`, and the arguments that go before the caller's.
pub fn program(path: &Path) -> (OsString, Vec<OsString>) {
    if path.extension().is_some_and(|e| e == "py") {
        let python = std::env::var_os("ARIEKEI_PYTHON").unwrap_or_else(|| "python3".into());
        (python, vec!["-I".into(), path.into()])
    } else {
        (path.into(), vec![])
    }
}

/// A `Command` for `path` (see [`program`]).
pub fn command(path: &Path) -> std::process::Command {
    let (program, prefix) = program(path);
    let mut cmd = std::process::Command::new(program);
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
