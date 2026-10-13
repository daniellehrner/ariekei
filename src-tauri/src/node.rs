//! Finds a Node.js runtime for the ACP adapter (which requires Node 22+).
//!
//! Order: `ARIEKEI_NODE`, then `node` on PATH, then the newest install from a
//! common version manager. GUI apps often start with a minimal PATH (macOS in
//! particular), so the version-manager fallback matters in practice.

use std::path::{Path, PathBuf};
use std::process::Command;

pub const MIN_MAJOR: u32 = 22;

const EXE: &str = if cfg!(windows) { "node.exe" } else { "node" };

/// Parses `v22.23.3` / `22.23.3` (as in `node --version` or install dir names).
fn parse_version(s: &str) -> Option<(u32, u32, u32)> {
    let mut parts = s.trim().trim_start_matches('v').split('.');
    let mut next = || parts.next()?.parse().ok();
    Some((next()?, next()?, next()?))
}

fn version_of(node: &Path) -> Option<(u32, u32, u32)> {
    let out = Command::new(node).arg("--version").output().ok()?;
    out.status
        .success()
        .then(|| parse_version(&String::from_utf8_lossy(&out.stdout)))?
}

/// `<root>/<version dir>/<bin path>` for every version directory under `root`.
fn versioned(root: &Path, bin: &[&str]) -> Vec<((u32, u32, u32), PathBuf)> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return vec![];
    };
    entries
        .flatten()
        .filter_map(|e| {
            let version = parse_version(&e.file_name().to_string_lossy())?;
            let path = bin.iter().fold(e.path(), |p, part| p.join(part));
            path.is_file().then_some((version, path))
        })
        .collect()
}

fn bin_path() -> &'static [&'static str] {
    if cfg!(windows) { &[EXE] } else { &["bin", EXE] }
}

/// Node installs under the home directory managed by fnm, nvm, mise and asdf.
fn home_installs(home: &Path) -> Vec<((u32, u32, u32), PathBuf)> {
    let fnm_bin = [&["installation"], bin_path()].concat();
    let roots: [(PathBuf, &[&str]); 6] = [
        (home.join(".local/share/fnm/node-versions"), &fnm_bin),
        (
            home.join("Library/Application Support/fnm/node-versions"),
            &fnm_bin,
        ),
        (home.join(".fnm/node-versions"), &fnm_bin),
        (home.join(".nvm/versions/node"), bin_path()),
        (home.join(".local/share/mise/installs/node"), bin_path()),
        (home.join(".asdf/installs/nodejs"), bin_path()),
    ];
    roots
        .iter()
        .flat_map(|(root, bin)| versioned(root, bin))
        .collect()
}

/// All version-manager installs: home-relative ones, those pointed to by the
/// managers' env vars (incl. nvm-windows), and fixed locations (Volta, Homebrew).
fn managed_installs(home: &Path) -> Vec<((u32, u32, u32), PathBuf)> {
    let mut found = home_installs(home);
    let env_dir = |var: &str| std::env::var_os(var).map(PathBuf::from);
    if let Some(dir) = env_dir("FNM_DIR") {
        found.extend(versioned(
            &dir.join("node-versions"),
            &[&["installation"], bin_path()].concat(),
        ));
    }
    if let Some(dir) = env_dir("NVM_DIR") {
        found.extend(versioned(&dir.join("versions/node"), bin_path()));
    }
    if let Some(dir) = env_dir("APPDATA") {
        found.extend(versioned(
            &dir.join("fnm/node-versions"),
            &["installation", EXE],
        ));
        found.extend(versioned(&dir.join("nvm"), &[EXE]));
    }
    if let Some(dir) = env_dir("NVM_HOME") {
        found.extend(versioned(&dir, &[EXE]));
    }
    // Unversioned locations: ask the binary itself.
    for fixed in [
        home.join(".volta/bin").join(EXE),
        "/opt/homebrew/bin/node".into(),
        "/usr/local/bin/node".into(),
    ] {
        if fixed.is_file()
            && let Some(v) = version_of(&fixed)
        {
            found.push((v, fixed));
        }
    }
    found
}

fn on_path() -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(EXE))
        .find(|p| p.is_file())
}

/// Picks `preferred` if it is new enough, else the newest qualifying candidate.
fn choose(
    preferred: Option<((u32, u32, u32), PathBuf)>,
    mut candidates: Vec<((u32, u32, u32), PathBuf)>,
) -> Option<PathBuf> {
    if let Some((v, p)) = preferred
        && v.0 >= MIN_MAJOR
    {
        return Some(p);
    }
    candidates.retain(|(v, _)| v.0 >= MIN_MAJOR);
    candidates
        .into_iter()
        .max_by_key(|(v, _)| *v)
        .map(|(_, p)| p)
}

/// Locates Node 22+, or explains what was found instead.
pub fn find() -> Result<PathBuf, String> {
    if let Some(explicit) = std::env::var_os("ARIEKEI_NODE").map(PathBuf::from) {
        return match version_of(&explicit) {
            Some(v) if v.0 >= MIN_MAJOR => Ok(explicit),
            Some(v) => Err(format!(
                "ARIEKEI_NODE={} is Node {}.{}.{}; need {MIN_MAJOR}+",
                explicit.display(),
                v.0,
                v.1,
                v.2
            )),
            None => Err(format!(
                "ARIEKEI_NODE={} is not a working node binary",
                explicit.display()
            )),
        };
    }
    let path_node = on_path().and_then(|p| Some((version_of(&p)?, p)));
    let home = std::env::home_dir().unwrap_or_default();
    let managed = managed_installs(&home);
    if let Some(node) = choose(path_node.clone(), managed.clone()) {
        // Directory names can lie (broken installs); confirm the final pick runs.
        if version_of(&node).is_some_and(|v| v.0 >= MIN_MAJOR) {
            return Ok(node);
        }
    }
    let seen: Vec<String> = path_node
        .iter()
        .chain(&managed)
        .map(|(v, p)| format!("{} ({}.{}.{})", p.display(), v.0, v.1, v.2))
        .collect();
    Err(format!(
        "Node.js {MIN_MAJOR}+ is required for the Claude agent but was not found{}. \
         Install it (e.g. with fnm or nvm) or set ARIEKEI_NODE.",
        if seen.is_empty() {
            String::new()
        } else {
            format!("; found {}", seen.join(", "))
        }
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_node_versions() {
        assert_eq!(parse_version("v22.23.3\n"), Some((22, 23, 3)));
        assert_eq!(parse_version("18.19.1"), Some((18, 19, 1)));
        assert_eq!(parse_version("system"), None);
    }

    #[test]
    fn prefers_path_node_when_new_enough() {
        let path = Some(((22, 1, 0), PathBuf::from("/usr/bin/node")));
        let managed = vec![((24, 0, 0), PathBuf::from("/fnm/24/node"))];
        assert_eq!(choose(path, managed), Some(PathBuf::from("/usr/bin/node")));
    }

    #[test]
    fn falls_back_to_newest_managed_install() {
        let path = Some(((18, 19, 1), PathBuf::from("/usr/bin/node")));
        let managed = vec![
            ((22, 23, 3), PathBuf::from("/fnm/22/node")),
            ((24, 1, 0), PathBuf::from("/nvm/24/node")),
            ((20, 0, 0), PathBuf::from("/nvm/20/node")),
        ];
        assert_eq!(choose(path, managed), Some(PathBuf::from("/nvm/24/node")));
        assert_eq!(choose(None, vec![((20, 0, 0), "/old".into())]), None);
    }

    #[test]
    fn finds_versioned_installs_in_manager_layouts() {
        let home = tempfile::tempdir().unwrap();
        let bin = if cfg!(windows) { vec![] } else { vec!["bin"] };
        for (root, version) in [
            (".nvm/versions/node", "v22.5.0"),
            (".nvm/versions/node", "v20.1.0"),
        ] {
            let dir = bin
                .iter()
                .fold(home.path().join(root).join(version), |p, b| p.join(b));
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(EXE), "").unwrap();
        }
        let mut found: Vec<_> = home_installs(home.path())
            .into_iter()
            .map(|(v, _)| v)
            .collect();
        found.sort();
        assert_eq!(found, [(20, 1, 0), (22, 5, 0)]);
    }
}
