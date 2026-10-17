// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

//! Git access via the `git` CLI (respects the user's config, credentials and worktrees).

use serde::Serialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn git(cwd: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .output()
        .map_err(|e| format!("failed to run git: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(out.stdout)
}

/// Like `git`, as trimmed text.
pub fn git_str(cwd: &Path, args: &[&str]) -> Result<String, String> {
    git(cwd, args).map(|o| String::from_utf8_lossy(&o).trim().to_owned())
}

// --- repository layout ------------------------------------------------------------

/// `owner/name` of a GitHub remote URL (ssh, scp-style or https).
pub fn github_slug(url: &str) -> Option<(String, String)> {
    let rest = url
        .strip_prefix("git@github.com:")
        .or_else(|| url.strip_prefix("ssh://git@github.com/"))
        .or_else(|| url.strip_prefix("https://github.com/"))
        .or_else(|| url.strip_prefix("http://github.com/"))?;
    let rest = rest.trim_end_matches('/');
    let rest = rest.strip_suffix(".git").unwrap_or(rest);
    let (owner, name) = rest.split_once('/')?;
    (!owner.is_empty() && !name.is_empty() && !name.contains('/'))
        .then(|| (owner.to_owned(), name.to_owned()))
}

/// `(name, url)` of every remote, as configured (`insteadOf` rewrites not applied, so a
/// GitHub URL stays recognizable).
pub fn remotes(repo: &Path) -> Result<Vec<(String, String)>, String> {
    let names = git_str(repo, &["remote"])?;
    names
        .lines()
        .map(|name| {
            Ok((
                name.to_owned(),
                git_str(repo, &["config", "--get", &format!("remote.{name}.url")])?,
            ))
        })
        .collect()
}

/// The remote whose URL points at GitHub's `owner/name` (case-insensitive).
pub fn remote_for(repo: &Path, owner: &str, name: &str) -> Result<Option<String>, String> {
    Ok(remotes(repo)?.into_iter().find_map(|(remote, url)| {
        let (o, n) = github_slug(&url)?;
        (o.eq_ignore_ascii_case(owner) && n.eq_ignore_ascii_case(name)).then_some(remote)
    }))
}

/// The main worktree of the repository containing `path` (which may be a linked worktree).
pub fn main_worktree(path: &Path) -> Result<PathBuf, String> {
    let common = git_str(
        path,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?;
    let common = PathBuf::from(common);
    // For a normal repo the common dir is `<main>/.git`.
    match common.file_name() {
        Some(n) if n == ".git" => Ok(common.parent().unwrap_or(&common).to_owned()),
        _ => Err(format!(
            "{} is a bare repository; Ariekei needs a checkout",
            common.display()
        )),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worktree {
    pub path: PathBuf,
    pub branch: Option<String>,
    pub main: bool,
}

pub fn parse_worktrees(porcelain: &str) -> Vec<Worktree> {
    porcelain
        .split("\n\n")
        .filter_map(|block| {
            let mut path = None;
            let mut branch = None;
            for line in block.lines() {
                if let Some(p) = line.strip_prefix("worktree ") {
                    path = Some(PathBuf::from(p));
                } else if let Some(b) = line.strip_prefix("branch refs/heads/") {
                    branch = Some(b.to_owned());
                }
            }
            Some(Worktree {
                path: path?,
                branch,
                main: false,
            })
        })
        .enumerate()
        .map(|(i, w)| Worktree { main: i == 0, ..w })
        .collect()
}

pub fn worktrees(repo: &Path) -> Result<Vec<Worktree>, String> {
    Ok(parse_worktrees(&git_str(
        repo,
        &["worktree", "list", "--porcelain"],
    )?))
}

/// The upstream of a local branch as `remote/branch`, if set.
pub fn upstream(repo: &Path, branch: &str) -> Option<String> {
    let up = git_str(
        repo,
        &[
            "for-each-ref",
            "--format=%(upstream:short)",
            &format!("refs/heads/{branch}"),
        ],
    )
    .ok()?;
    (!up.is_empty()).then_some(up)
}

pub fn branch_exists(repo: &Path, branch: &str) -> bool {
    git(
        repo,
        &[
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ],
    )
    .is_ok()
}

pub fn commit_exists(repo: &Path, rev: &str) -> bool {
    git(repo, &["cat-file", "-e", &format!("{rev}^{{commit}}")]).is_ok()
}

pub fn head(cwd: &Path) -> Result<String, String> {
    git_str(cwd, &["rev-parse", "HEAD"])
}

pub fn merge_base(cwd: &Path, a: &str, b: &str) -> Result<String, String> {
    git_str(cwd, &["merge-base", a, b])
}

/// Uncommitted changes, including untracked files.
pub fn is_dirty(cwd: &Path) -> Result<bool, String> {
    Ok(!git(cwd, &["status", "--porcelain"])?.is_empty())
}

// --- diffs ------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChangedFile {
    pub path: String,
    /// Git status letter: A, M, D, … ("?" for untracked).
    pub status: String,
}

/// Files that differ between `from` and `to` (a revision, or the working tree incl.
/// untracked files when `to` is `None`).
pub fn changed_files(cwd: &Path, from: &str, to: Option<&str>) -> Result<Vec<ChangedFile>, String> {
    let mut args = vec!["diff", "--name-status", "--no-renames", "-z", from];
    args.extend(to);
    args.push("--");
    let out = git(cwd, &args)?;
    let fields: Vec<&[u8]> = out.split(|b| *b == 0).filter(|f| !f.is_empty()).collect();
    let mut files: Vec<ChangedFile> = fields
        .chunks(2)
        .filter_map(|c| match c {
            [status, path] => Some(ChangedFile {
                status: String::from_utf8_lossy(status).into_owned(),
                path: String::from_utf8_lossy(path).into_owned(),
            }),
            _ => None,
        })
        .collect();

    if to.is_none() {
        let untracked = git(cwd, &["ls-files", "--others", "--exclude-standard", "-z"])?;
        files.extend(
            untracked
                .split(|b| *b == 0)
                .filter(|p| !p.is_empty())
                .map(|p| ChangedFile {
                    path: String::from_utf8_lossy(p).into_owned(),
                    status: "?".into(),
                }),
        );
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

/// A changed region in git's terms: 1-based start lines and line counts. A count of 0
/// means an insertion/deletion point *after* `start`.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Hunk {
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
}

pub fn parse_hunks(diff: &str) -> Vec<Hunk> {
    let range = |s: &str| -> Option<(u32, u32)> {
        let (start, len) = s.split_once(',').unwrap_or((s, "1"));
        Some((start.parse().ok()?, len.parse().ok()?))
    };
    diff.lines()
        .filter_map(|l| {
            let rest = l.strip_prefix("@@ -")?;
            let (old, rest) = rest.split_once(" +")?;
            let (new, _) = rest.split_once(" @@")?;
            let ((old_start, old_lines), (new_start, new_lines)) = (range(old)?, range(new)?);
            Some(Hunk {
                old_start,
                old_lines,
                new_start,
                new_lines,
            })
        })
        .collect()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileDiff {
    pub old: String,
    pub new: String,
    pub binary: bool,
    pub hunks: Vec<Hunk>,
}

/// Content of `path` at `from` and at `to` (working tree when `None`), plus git's hunks.
pub fn file_diff(
    cwd: &Path,
    from: &str,
    to: Option<&str>,
    path: &str,
    ignore_ws: bool,
) -> Result<FileDiff, String> {
    let old = git(cwd, &["show", &format!("{from}:{path}")]).unwrap_or_default();
    let new = match to {
        Some(rev) => git(cwd, &["show", &format!("{rev}:{path}")]).unwrap_or_default(),
        None => std::fs::read(cwd.join(path)).unwrap_or_default(),
    };
    if old.contains(&0) || new.contains(&0) {
        return Ok(FileDiff {
            old: String::new(),
            new: String::new(),
            binary: true,
            hunks: vec![],
        });
    }
    let (old, new) = (
        String::from_utf8_lossy(&old).into_owned(),
        String::from_utf8_lossy(&new).into_owned(),
    );
    let tracked = to.is_some() || git(cwd, &["ls-files", "--error-unmatch", "--", path]).is_ok();
    let hunks = if tracked {
        let mut args = vec![
            "diff",
            "-U0",
            "--no-color",
            "--no-ext-diff",
            "--no-renames",
            from,
        ];
        if ignore_ws {
            args.push("-w");
        }
        args.extend(to);
        args.extend(["--", path]);
        parse_hunks(&String::from_utf8_lossy(&git(cwd, &args)?))
    } else {
        // Untracked: the whole file is new.
        vec![Hunk {
            old_start: 0,
            old_lines: 0,
            new_start: 1,
            new_lines: new.lines().count() as u32,
        }]
    };
    Ok(FileDiff {
        old,
        new,
        binary: false,
        hunks,
    })
}

/// Files whose changes from `from` to `to` (working tree if `None`) are whitespace only:
/// listed by a normal diff, but absent from one that ignores whitespace.
pub fn whitespace_only_files(
    cwd: &Path,
    from: &str,
    to: Option<&str>,
) -> Result<HashSet<String>, String> {
    let paths = |ignore_ws: bool| -> Result<HashSet<String>, String> {
        let mut args = vec!["diff", "--numstat", "--no-renames", "-z", from];
        if ignore_ws {
            args.push("-w");
        }
        args.extend(to);
        args.push("--");
        let out = git(cwd, &args)?;
        Ok(out
            .split(|b| *b == 0)
            .filter_map(|entry| {
                String::from_utf8_lossy(entry)
                    .splitn(3, '\t')
                    .nth(2)
                    .map(str::to_owned)
            })
            .filter(|p| !p.is_empty())
            .collect())
    };
    let ignoring = paths(true)?;
    Ok(paths(false)?
        .into_iter()
        .filter(|p| !ignoring.contains(p))
        .collect())
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Commit {
    pub sha: String,
    pub subject: String,
    pub author: String,
    pub date: String,
    /// How many files it changes.
    pub files: usize,
}

/// Commits reachable from `to` but not from `from`, newest first.
pub fn commits_between(cwd: &Path, from: &str, to: &str) -> Result<Vec<Commit>, String> {
    let out = git_str(
        cwd,
        &[
            "log",
            "--name-only",
            "--format=%x1e%H%x1f%s%x1f%an%x1f%aI",
            &format!("{from}..{to}"),
        ],
    )?;
    Ok(out
        .split('\u{1e}')
        .filter_map(|entry| {
            let mut lines = entry.lines();
            let mut f = lines.next()?.split('\u{1f}');
            Some(Commit {
                sha: f.next()?.to_owned(),
                subject: f.next()?.to_owned(),
                author: f.next()?.to_owned(),
                date: f.next()?.to_owned(),
                files: lines.filter(|l| !l.trim().is_empty()).count(),
            })
        })
        .collect())
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub fn commit(p: &Path, msg: &str) {
        git(p, &["add", "-A"]).unwrap();
        git(
            p,
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-qm",
                msg,
            ],
        )
        .unwrap();
    }

    pub fn repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        git(p, &["init", "-q", "-b", "main"]).unwrap();
        std::fs::write(p.join("a.txt"), "one\n").unwrap();
        std::fs::write(p.join("gone.txt"), "bye\n").unwrap();
        commit(p, "init");
        dir
    }

    #[test]
    fn changed_files_include_modified_deleted_and_untracked() {
        let dir = repo();
        let p = dir.path();
        std::fs::write(p.join("a.txt"), "two\n").unwrap();
        std::fs::remove_file(p.join("gone.txt")).unwrap();
        std::fs::write(p.join("new.txt"), "hi\n").unwrap();

        let files = changed_files(p, "HEAD", None).unwrap();
        let summary: Vec<_> = files
            .iter()
            .map(|f| (f.path.as_str(), f.status.as_str()))
            .collect();
        assert_eq!(
            summary,
            [("a.txt", "M"), ("gone.txt", "D"), ("new.txt", "?")]
        );
    }

    #[test]
    fn diffs_between_commits_and_against_the_worktree() {
        let dir = repo();
        let p = dir.path();
        let first = head(p).unwrap();
        std::fs::write(p.join("a.txt"), "one\ntwo\nthree\n").unwrap();
        commit(p, "grow a");
        std::fs::write(p.join("new.txt"), "x\ny\n").unwrap();

        let range = changed_files(p, &first, Some("HEAD")).unwrap();
        assert_eq!(
            range.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(),
            ["a.txt"]
        );

        let d = file_diff(p, &first, Some("HEAD"), "a.txt", false).unwrap();
        assert_eq!(
            (d.old.as_str(), d.new.as_str()),
            ("one\n", "one\ntwo\nthree\n")
        );
        assert_eq!(
            d.hunks,
            [Hunk {
                old_start: 1,
                old_lines: 0,
                new_start: 2,
                new_lines: 2
            }]
        );

        let untracked = file_diff(p, &first, None, "new.txt", false).unwrap();
        assert_eq!(
            untracked.hunks,
            [Hunk {
                old_start: 0,
                old_lines: 0,
                new_start: 1,
                new_lines: 2
            }]
        );

        let commits = commits_between(p, &first, "HEAD").unwrap();
        assert_eq!(
            commits
                .iter()
                .map(|c| (c.subject.as_str(), c.files))
                .collect::<Vec<_>>(),
            [("grow a", 1)]
        );
        assert!(is_dirty(p).unwrap());
    }

    #[test]
    fn whitespace_only_changes_are_found_and_hidden() {
        let dir = repo();
        let p = dir.path();
        let base = head(p).unwrap();
        // A change to a.txt, then a whitespace-only change to gone.txt.
        std::fs::write(p.join("a.txt"), "one\nfeature\n").unwrap();
        commit(p, "feature");
        std::fs::write(p.join("gone.txt"), "bye  \n").unwrap();
        commit(p, "spacing");
        let now = head(p).unwrap();
        assert_eq!(
            whitespace_only_files(p, &base, Some(&now)).unwrap(),
            HashSet::from(["gone.txt".to_owned()])
        );
        let d = file_diff(p, &base, Some(&now), "gone.txt", true).unwrap();
        assert!(d.hunks.is_empty(), "-w hides whitespace-only hunks");
    }

    #[test]
    fn parses_hunk_headers() {
        let diff = "diff --git a/x b/x\n@@ -3 +3 @@ fn\n-a\n+b\n@@ -10,0 +11,2 @@\n+c\n+d\n@@ -20,3 +22,0 @@\n";
        assert_eq!(
            parse_hunks(diff),
            [
                Hunk {
                    old_start: 3,
                    old_lines: 1,
                    new_start: 3,
                    new_lines: 1
                },
                Hunk {
                    old_start: 10,
                    old_lines: 0,
                    new_start: 11,
                    new_lines: 2
                },
                Hunk {
                    old_start: 20,
                    old_lines: 3,
                    new_start: 22,
                    new_lines: 0
                },
            ]
        );
    }

    #[test]
    fn parses_github_remote_urls() {
        let o = |a: &str, b: &str| Some((a.to_owned(), b.to_owned()));
        assert_eq!(
            github_slug("git@github.com:besu-eth/besu.git"),
            o("besu-eth", "besu")
        );
        assert_eq!(
            github_slug("https://github.com/daniellehrner/besu"),
            o("daniellehrner", "besu")
        );
        assert_eq!(github_slug("ssh://git@github.com/a/b.git/"), o("a", "b"));
        assert_eq!(github_slug("git@gitlab.com:a/b.git"), None);
        assert_eq!(github_slug("https://github.com/a"), None);
    }

    #[test]
    fn parses_worktree_list() {
        let porcelain = "worktree /src/besu\nHEAD 1\nbranch refs/heads/main\n\nworktree /src/besu_pr1\nHEAD 2\nbranch refs/heads/fix/x\n\nworktree /src/det\nHEAD 3\ndetached\n";
        let w = parse_worktrees(porcelain);
        assert_eq!(w.len(), 3);
        assert!(w[0].main && !w[1].main);
        assert_eq!(w[1].branch.as_deref(), Some("fix/x"));
        assert_eq!(w[2].branch, None);
    }
}
