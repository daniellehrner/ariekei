//! The order files are reviewed in. Not alphabetical: what a file is decides when it is
//! read — source first (grouped by directory), then tests, then config and build files,
//! and generated, lock and vendored files last (collapsed in the UI). Files shown late get
//! less attention (Fregnan et al. 2022), so the code that matters most comes first.

use crate::git::git;
use serde::Serialize;
use std::collections::HashSet;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Group {
    Source,
    Tests,
    Config,
    Generated,
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn is_test(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    let name = file_name(&lower);
    let stem = name.split('.').next().unwrap_or(name);
    lower.contains("/test/")
        || lower.contains("/tests/")
        || lower.starts_with("test/")
        || lower.starts_with("tests/")
        || lower.contains("/spec/")
        || lower.contains("/__tests__/")
        || lower.contains("src/test")
        || lower.contains("src/integration-test")
        || lower.contains("src/testfixtures")
        || stem.ends_with("test")
        || stem.ends_with("tests")
        || stem.ends_with("_test")
        || stem.starts_with("test_")
        || name.contains(".test.")
        || name.contains(".spec.")
}

fn is_generated(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    let name = file_name(&lower);
    const LOCKS: [&str; 9] = [
        "package-lock.json",
        "pnpm-lock.yaml",
        "yarn.lock",
        "cargo.lock",
        "poetry.lock",
        "gemfile.lock",
        "composer.lock",
        "go.sum",
        "gradle.lockfile",
    ];
    LOCKS.contains(&name)
        || name.ends_with(".lock")
        || name.contains(".min.")
        || name.ends_with(".snap")
        || lower.starts_with("vendor/")
        || lower.contains("/vendor/")
        || lower.contains("/generated/")
        || lower.starts_with("dist/")
        || lower.contains("/node_modules/")
        || lower.ends_with("gradle/verification-metadata.xml")
}

fn is_config(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    let name = file_name(&lower);
    const NAMES: [&str; 8] = [
        "pom.xml",
        "makefile",
        "dockerfile",
        "package.json",
        "tsconfig.json",
        "cmakelists.txt",
        ".gitignore",
        ".gitattributes",
    ];
    const EXTS: [&str; 12] = [
        ".gradle",
        ".gradle.kts",
        ".toml",
        ".yml",
        ".yaml",
        ".properties",
        ".json",
        ".ini",
        ".cfg",
        ".conf",
        ".xml",
        ".lock",
    ];
    NAMES.contains(&name)
        || EXTS.iter().any(|e| name.ends_with(e))
        || lower.starts_with(".github/")
        || lower.starts_with(".gitlab")
        || lower.starts_with("gradle/")
}

pub fn group_of(path: &str, attributed_generated: bool) -> Group {
    if attributed_generated || is_generated(path) {
        Group::Generated
    } else if is_test(path) {
        Group::Tests
    } else if is_config(path) {
        Group::Config
    } else {
        Group::Source
    }
}

/// Files marked `linguist-generated` or `linguist-vendored` in `.gitattributes` (GitHub's
/// convention), among `paths`.
pub fn attributed_generated(cwd: &Path, paths: &[String]) -> HashSet<String> {
    if paths.is_empty() {
        return HashSet::new();
    }
    let mut args = vec![
        "check-attr",
        "linguist-generated",
        "linguist-vendored",
        "--",
    ];
    args.extend(paths.iter().map(String::as_str));
    let Ok(out) = git(cwd, &args) else {
        return HashSet::new();
    };
    String::from_utf8_lossy(&out)
        .lines()
        .filter_map(|l| {
            // "<path>: linguist-generated: set|true|unspecified"
            let (path, rest) = l.split_once(": ")?;
            let value = rest.rsplit(": ").next()?;
            matches!(value, "set" | "true").then(|| path.to_owned())
        })
        .collect()
}

/// Sort key: group, then directory (so related files stay together), then name.
pub fn sort_key(path: &str, group: Group) -> (Group, String, String) {
    let (dir, name) = path.rsplit_once('/').unwrap_or(("", path));
    (group, dir.to_owned(), name.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_files_by_what_they_are() {
        let cases = [
            (
                "ethereum/core/src/main/java/org/x/Block.java",
                Group::Source,
            ),
            (
                "ethereum/core/src/test/java/org/x/BlockTest.java",
                Group::Tests,
            ),
            (
                "ethereum/core/src/integration-test/java/org/x/SyncIT.java",
                Group::Tests,
            ),
            ("web/src/app.test.ts", Group::Tests),
            ("pkg/sync_test.go", Group::Tests),
            ("tests/test_parser.py", Group::Tests),
            ("build.gradle", Group::Config),
            ("app/build.gradle.kts", Group::Config),
            (".github/workflows/ci.yml", Group::Config),
            ("pom.xml", Group::Config),
            ("package-lock.json", Group::Generated),
            ("Cargo.lock", Group::Generated),
            ("gradle/verification-metadata.xml", Group::Generated),
            ("vendor/lib/x.c", Group::Generated),
            ("web/dist/app.min.js", Group::Generated),
            ("src/lib.rs", Group::Source),
            ("README.md", Group::Source),
        ];
        for (path, group) in cases {
            assert_eq!(group_of(path, false), group, "{path}");
        }
        assert_eq!(
            group_of("src/Gen.java", true),
            Group::Generated,
            ".gitattributes wins"
        );
    }

    #[test]
    fn source_before_tests_before_config_before_generated_and_directories_together() {
        let mut files = vec![
            "Cargo.lock",
            "src/b/z.rs",
            "build.gradle",
            "src/a/y.rs",
            "src/test/AT.java",
            "src/a/x.rs",
        ];
        files.sort_by_key(|p| sort_key(p, group_of(p, false)));
        assert_eq!(
            files,
            [
                "src/a/x.rs",
                "src/a/y.rs",
                "src/b/z.rs",
                "src/test/AT.java",
                "build.gradle",
                "Cargo.lock"
            ]
        );
    }

    #[test]
    fn reads_linguist_attributes() {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-q"]).unwrap();
        std::fs::write(
            dir.path().join(".gitattributes"),
            "schema/*.java linguist-generated\nthird_party/** linguist-vendored\n",
        )
        .unwrap();
        let paths = vec![
            "schema/Api.java".to_owned(),
            "third_party/x/y.c".to_owned(),
            "src/Main.java".to_owned(),
        ];
        let marked = attributed_generated(dir.path(), &paths);
        assert_eq!(
            marked,
            HashSet::from(["schema/Api.java".to_owned(), "third_party/x/y.c".to_owned()])
        );
    }
}
