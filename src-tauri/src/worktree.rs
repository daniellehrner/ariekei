//! A work item's isolated working copy: an existing worktree that already has its branch,
//! or a new one next to the main clone (`<clone>_pr<N>`, `<clone>_issue<N>`, `<clone>_task<n>`).

use crate::git::{self, git, git_str};
use crate::github::PrSummary;
use crate::store::Repo;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

/// One fetch at a time per clone: two at once (e.g. opening two PRs in quick succession)
/// update the same remote-tracking refs and one fails with "cannot lock ref".
fn fetching(repo: &Path) -> Arc<Mutex<()>> {
    static LOCKS: Mutex<Option<HashMap<PathBuf, Arc<Mutex<()>>>>> = Mutex::new(None);
    let key = repo.canonicalize().unwrap_or_else(|_| repo.to_path_buf());
    LOCKS
        .lock()
        .unwrap()
        .get_or_insert_with(HashMap::new)
        .entry(key)
        .or_default()
        .clone()
}

fn hold(lock: &Mutex<()>) -> MutexGuard<'_, ()> {
    lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Prepared {
    pub path: PathBuf,
    pub branch: String,
    /// True when an existing worktree was taken over instead of creating one.
    pub adopted: bool,
    /// Something the user should know, e.g. a reused branch that differs from the PR.
    pub note: Option<String>,
}

/// The remote for the PR's head repository, adding one if the clone has none.
fn head_remote(repo: &Repo, pr: &PrSummary) -> Result<String, String> {
    remote_for_repo(
        repo,
        &pr.head_owner,
        &pr.head_repo,
        &pr.head_ssh_url,
        &pr.head_https_url,
    )
    .map(|(remote, _)| remote)
}

/// The remote for GitHub's `owner/name`. If the clone has none, adds one named after the
/// owner as `gh pr checkout` does (`<owner>-<name>` if that name is taken), using ssh when
/// the base remote does. Returns the remote and whether it was added.
fn remote_for_repo(
    repo: &Repo,
    owner: &str,
    name: &str,
    ssh_url: &str,
    https_url: &str,
) -> Result<(String, bool), String> {
    if let Some(remote) = git::remote_for(&repo.path, owner, name)? {
        return Ok((remote, false));
    }
    let base_url = git_str(
        &repo.path,
        &[
            "config",
            "--get",
            &format!("remote.{}.url", repo.base_remote),
        ],
    )
    .unwrap_or_default();
    let url = if base_url.starts_with("https://") {
        https_url
    } else {
        ssh_url
    };
    let taken = git::remotes(&repo.path)?
        .into_iter()
        .any(|(r, _)| r == owner);
    let remote = if taken {
        format!("{owner}-{name}")
    } else {
        owner.to_owned()
    };
    git(&repo.path, &["remote", "add", &remote, url])?;
    Ok((remote, true))
}

/// Fetches `branch` of `remote` into `refs/remotes/<remote>/<branch>` regardless of the
/// remote's configured refspec.
fn fetch(repo: &Path, remote: &str, branch: &str) -> Result<String, String> {
    let tracking = format!("{remote}/{branch}");
    let lock = fetching(repo);
    let _one_at_a_time = hold(&lock);
    git(
        repo,
        &[
            "fetch",
            "--quiet",
            remote,
            &format!("+refs/heads/{branch}:refs/remotes/{tracking}"),
        ],
    )?;
    Ok(tracking)
}

/// An existing linked worktree for the branch `tracking`: its branch tracks it, is `own`
/// (the item's own name, e.g. `pr-<N>`), or is `plain` (the remote's branch name) with no
/// upstream. The main checkout is never adopted.
fn find_existing(
    repo: &Path,
    own: &str,
    plain: &str,
    tracking: &str,
) -> Result<Option<git::Worktree>, String> {
    Ok(git::worktrees(repo)?
        .into_iter()
        .filter(|w| !w.main && w.path.exists())
        .find(|w| {
            w.branch.as_deref().is_some_and(|b| {
                b == own
                    || git::upstream(repo, b).as_deref() == Some(tracking)
                    || (b == plain && git::upstream(repo, b).is_none())
            })
        }))
}

/// A branch on GitHub to work on: `branch` of `owner/repo` (the base repo or a fork).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BranchRef {
    pub owner: String,
    pub repo: String,
    pub branch: String,
}

/// What the user typed for an existing branch: `owner:branch` (a fork with the base repo's
/// name, as in GitHub's `head` notation), `owner/repo:branch`, a GitHub branch URL
/// (`https://github.com/owner/repo/tree/<branch>`), or a branch of the base repo.
pub fn parse_branch_ref(
    input: &str,
    base_owner: &str,
    base_name: &str,
) -> Result<BranchRef, String> {
    let input = input.trim();
    let bad = || {
        format!(
            "{input:?} is not a branch: use owner:branch, owner/repo:branch, a GitHub branch URL or a branch name"
        )
    };
    let url = ["https://github.com/", "http://github.com/", "github.com/"]
        .iter()
        .find_map(|prefix| input.strip_prefix(prefix));
    let (owner, repo, branch) = if let Some(rest) = url {
        let mut parts = rest.splitn(4, '/');
        match (parts.next(), parts.next(), parts.next(), parts.next()) {
            (Some(o), Some(r), Some("tree"), Some(b)) => (o, r, b.trim_end_matches('/')),
            _ => return Err(bad()),
        }
    } else if let Some((head, branch)) = input.split_once(':') {
        match head.split_once('/') {
            Some((o, r)) => (o, r, branch),
            None => (head, base_name, branch),
        }
    } else {
        (base_owner, base_name, input)
    };
    let name_ok = |s: &str| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
    };
    // Roughly `git check-ref-format`, and never an option (a leading `-`) for git.
    let branch_ok = !branch.is_empty()
        && !branch.starts_with(['-', '/'])
        && !branch.ends_with(['/', '.'])
        && !branch.contains("..")
        && !branch.contains("//")
        && !branch.ends_with(".lock")
        && !branch
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || ":~^?*[\\".contains(c));
    if !name_ok(owner) || !name_ok(repo) || !branch_ok {
        return Err(bad());
    }
    Ok(BranchRef {
        owner: owner.to_owned(),
        repo: repo.to_owned(),
        branch: branch.to_owned(),
    })
}

/// Branch name for an issue: `issue-<N>-<words of the title>`.
pub fn issue_branch(number: i64, title: &str) -> String {
    branch_name(&format!("issue-{number}"), title)
}

/// Branch name for task `T<n>`: `task-<n>-<words of the title>`.
pub fn task_branch(n: i64, title: &str) -> String {
    branch_name(&format!("task-{n}"), title)
}

fn branch_name(prefix: &str, title: &str) -> String {
    let mut slug = String::new();
    for word in title
        .to_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
    {
        if slug.len() + word.len() + 1 > 40 {
            break;
        }
        if !slug.is_empty() {
            slug.push('-');
        }
        slug.push_str(word);
    }
    if slug.is_empty() {
        prefix.to_owned()
    } else {
        format!("{prefix}-{slug}")
    }
}

/// Where pushes of new branches go: the viewer's fork if the clone has a remote for it,
/// else the base remote.
pub fn push_remote(repo: &Repo) -> Result<String, String> {
    Ok(git::remote_for(&repo.path, &repo.viewer, &repo.name)?
        .unwrap_or_else(|| repo.base_remote.clone()))
}

/// A worktree for working on issue `number`: a new branch from the base remote's
/// `default_branch`, set up so that a plain `git push` creates it on the push remote.
/// Safe to call again (finds the worktree it created before).
pub fn prepare_issue(
    repo: &Repo,
    number: i64,
    title: &str,
    default_branch: &str,
) -> Result<Prepared, String> {
    new_branch_worktree(
        repo,
        issue_branch(number, title),
        &format!("issue{number}"),
        default_branch,
        "start working on the issue",
    )
}

/// A worktree for task `T<n>`, like an issue's: branch `task-<n>-…`, path `<clone>_task<n>`.
pub fn prepare_task(
    repo: &Repo,
    n: i64,
    title: &str,
    default_branch: &str,
) -> Result<Prepared, String> {
    new_branch_worktree(
        repo,
        task_branch(n, title),
        &format!("task{n}"),
        default_branch,
        "start the task",
    )
}

/// A worktree for task `T<n>` on an existing branch (e.g. on a fork): adds a remote for its
/// repository if the clone has none, fetches the branch and checks it out in
/// `<clone>_task<n>`, tracking it, so a plain `git push` updates it.
pub fn prepare_task_on(
    repo: &Repo,
    n: i64,
    title: &str,
    head: &BranchRef,
    default_branch: &str,
) -> Result<Prepared, String> {
    let BranchRef {
        owner,
        repo: name,
        branch,
    } = head;
    let (remote, added) = remote_for_repo(
        repo,
        owner,
        name,
        &format!("git@github.com:{owner}/{name}.git"),
        &format!("https://github.com/{owner}/{name}.git"),
    )?;
    let tracking = match fetch(&repo.path, &remote, branch) {
        Ok(tracking) => tracking,
        Err(e) => {
            // E.g. a typo in the branch or the fork: leave no remote behind.
            if added {
                let _ = git(&repo.path, &["remote", "remove", &remote]);
            }
            return Err(format!("could not fetch {branch} from {owner}/{name}: {e}"));
        }
    };
    // The review compares with the default branch; not fatal if it can't be fetched.
    if let Err(e) = fetch(&repo.path, &repo.base_remote, default_branch) {
        tracing::warn!("could not fetch {default_branch}: {e}");
    }
    let own = task_branch(n, title);
    if let Some(w) = find_existing(&repo.path, &own, branch, &tracking)? {
        return Ok(Prepared {
            path: w.path,
            branch: w.branch.unwrap_or_default(),
            adopted: true,
            note: None,
        });
    }
    let path = sibling(repo, &format!("task{n}"));
    if path.exists() {
        return Err(format!(
            "{} already exists; move it away to start the task",
            path.display()
        ));
    }
    let (branch, note) = track_worktree(&repo.path, &path, branch, own, &tracking, &tracking)?;
    Ok(Prepared {
        path,
        branch,
        adopted: false,
        note,
    })
}

/// A new `branch` from the base remote's `default_branch` in the worktree
/// `<clone>_<suffix>`, or the linked worktree that already has the branch.
fn new_branch_worktree(
    repo: &Repo,
    branch: String,
    suffix: &str,
    default_branch: &str,
    to: &str,
) -> Result<Prepared, String> {
    let base = fetch(&repo.path, &repo.base_remote, default_branch)?;
    if let Some(w) = git::worktrees(&repo.path)?
        .into_iter()
        .find(|w| !w.main && w.branch.as_deref() == Some(&branch))
    {
        return Ok(Prepared {
            path: w.path,
            branch,
            adopted: true,
            note: None,
        });
    }
    let path = sibling(repo, suffix);
    if path.exists() {
        return Err(format!(
            "{} already exists; move it away to {to}",
            path.display()
        ));
    }
    let path_arg = path.to_string_lossy().into_owned();
    if git::branch_exists(&repo.path, &branch) {
        git(
            &repo.path,
            &["worktree", "add", "--quiet", &path_arg, &branch],
        )?;
    } else {
        git(
            &repo.path,
            &[
                "worktree",
                "add",
                "--quiet",
                "--no-track",
                "-b",
                &branch,
                &path_arg,
                &base,
            ],
        )?;
    }
    let remote = push_remote(repo)?;
    git(
        &repo.path,
        &["config", &format!("branch.{branch}.remote"), &remote],
    )?;
    git(
        &repo.path,
        &[
            "config",
            &format!("branch.{branch}.merge"),
            &format!("refs/heads/{branch}"),
        ],
    )?;
    Ok(Prepared {
        path,
        branch,
        adopted: false,
        note: None,
    })
}

/// Where a PR's head is fetched for reviewing without a worktree.
pub fn pull_ref(number: i64) -> String {
    format!("refs/ariekei/pull/{number}/head")
}

/// Fetches GitHub's `refs/pull/<N>/head` (works for PRs from forks too) and the base
/// branch from `remote` into the clone; returns the head commit.
pub fn fetch_pr_head(
    repo: &Path,
    remote: &str,
    number: i64,
    base_ref: &str,
) -> Result<String, String> {
    let lock = fetching(repo);
    let _one_at_a_time = hold(&lock);
    git(
        repo,
        &[
            "fetch",
            "--quiet",
            remote,
            &format!("+refs/pull/{number}/head:{}", pull_ref(number)),
            &format!("+refs/heads/{base_ref}:refs/remotes/{remote}/{base_ref}"),
        ],
    )?;
    git_str(repo, &["rev-parse", &pull_ref(number)])
}

pub fn worktree_path(repo: &Repo, number: i64) -> PathBuf {
    sibling(repo, &format!("pr{number}"))
}

/// `<clone>_<suffix>`, next to the main clone.
fn sibling(repo: &Repo, suffix: &str) -> PathBuf {
    let dir = repo
        .path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| repo.name.clone());
    repo.path
        .parent()
        .unwrap_or(&repo.path)
        .join(format!("{dir}_{suffix}"))
}

/// Makes the PR's worktree ready: fetches the head and base branches, then adopts or
/// creates the worktree. Safe to call again (it finds what it created before).
pub fn prepare(repo: &Repo, pr: &PrSummary) -> Result<Prepared, String> {
    let remote = head_remote(repo, pr)?;
    let tracking = fetch(&repo.path, &remote, &pr.head_ref)?;
    // The base is needed for the default review marker (merge-base); not fatal if it fails.
    if let Err(e) = fetch(&repo.path, &repo.base_remote, &pr.base_ref) {
        tracing::warn!("could not fetch base {}: {e}", pr.base_ref);
    }

    if let Some(w) = find_existing(
        &repo.path,
        &format!("pr-{}", pr.number),
        &pr.head_ref,
        &tracking,
    )? {
        return Ok(Prepared {
            path: w.path,
            branch: w.branch.unwrap_or_default(),
            adopted: true,
            note: None,
        });
    }

    let path = worktree_path(repo, pr.number);
    if path.exists() {
        return Err(format!(
            "{} already exists but is not a worktree of this PR's branch; move it away or check out the PR there",
            path.display()
        ));
    }

    let (branch, note) = track_worktree(
        &repo.path,
        &path,
        &pr.head_ref,
        format!("pr-{}", pr.number),
        &tracking,
        &format!("the PR ({tracking})"),
    )?;
    Ok(Prepared {
        path,
        branch,
        adopted: false,
        note,
    })
}

/// Checks out a branch tracking `tracking` in a new worktree at `path`, and returns it with a
/// note for the user, if any. The branch keeps the remote's name `wanted` when possible
/// (with the same name, a plain `git push` goes back there), else is `fallback` (a local
/// `wanted` tracks something else). A reused local branch is fast-forwarded to `tracking`;
/// if it has commits of its own it is left as it is and the note says so (`upstream` names
/// `tracking` in it).
fn track_worktree(
    clone: &Path,
    path: &Path,
    wanted: &str,
    fallback: String,
    tracking: &str,
    upstream: &str,
) -> Result<(String, Option<String>), String> {
    let reusable = git::branch_exists(clone, wanted)
        && git::upstream(clone, wanted).is_none_or(|u| u == tracking);
    let branch = if !git::branch_exists(clone, wanted) || reusable {
        wanted.to_owned()
    } else {
        fallback
    };
    let path_arg = path.to_string_lossy().into_owned();
    let mut note = None;
    if git::branch_exists(clone, &branch) {
        let checked_out = git::worktrees(clone)?
            .into_iter()
            .find(|w| w.branch.as_deref() == Some(&branch));
        if let Some(w) = checked_out {
            return Err(format!(
                "branch {branch} is checked out in {}; switch that checkout to another branch first",
                w.path.display()
            ));
        }
        git(clone, &["worktree", "add", "--quiet", &path_arg, &branch])?;
        if git(path, &["merge", "--ff-only", "--quiet", tracking]).is_err() {
            note = Some(format!(
                "Your local branch {branch} has commits that are not in {upstream}, so it was left as it is."
            ));
        }
    } else {
        git(
            clone,
            &[
                "worktree", "add", "--quiet", "-b", &branch, &path_arg, tracking,
            ],
        )?;
    }
    git(
        clone,
        &["branch", "--quiet", "--set-upstream-to", tracking, &branch],
    )?;
    Ok((branch, note))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::tests::{commit, repo as git_repo};

    /// `git worktree list` spells paths its own way (`/private/var/…` on macOS,
    /// `C:/Users/RUNNER~1/…` on Windows); compare where they point.
    fn canon(p: &Path) -> PathBuf {
        std::fs::canonicalize(p).unwrap()
    }

    /// A "GitHub" repo with a PR branch, and a clone of it whose origin points there.
    fn setup() -> (tempfile::TempDir, Repo, PrSummary) {
        let root = tempfile::tempdir().unwrap();
        let upstream = git_repo();
        let up = upstream.path();
        git(up, &["checkout", "-q", "-b", "feature/x"]).unwrap();
        std::fs::write(up.join("a.txt"), "feature\n").unwrap();
        commit(up, "feature work");
        git(up, &["checkout", "-q", "main"]).unwrap();

        let clone = root.path().join("proj");
        // Keep LF in checkouts so file contents compare the same on Windows runners.
        git(
            root.path(),
            &[
                "clone",
                "-q",
                "-c",
                "core.autocrlf=false",
                &up.to_string_lossy(),
                "proj",
            ],
        )
        .unwrap();
        // origin looks like GitHub's owner/proj; insteadOf maps it to the local upstream.
        let github = "git@github.com:owner/proj.git";
        git(
            &clone,
            &["config", &format!("url.{}.insteadOf", up.display()), github],
        )
        .unwrap();
        git(&clone, &["remote", "set-url", "origin", github]).unwrap();
        std::mem::forget(upstream); // keep the "GitHub" repo alive for the test

        let repo = Repo {
            id: 1,
            path: clone,
            owner: "owner".into(),
            name: "proj".into(),
            base_remote: "origin".into(),
            viewer: "me".into(),
        };
        let pr = PrSummary {
            number: 42,
            title: "t".into(),
            author: "me".into(),
            state: "OPEN".into(),
            url: String::new(),
            is_draft: false,
            updated_at: String::new(),
            head_ref: "feature/x".into(),
            head_sha: String::new(),
            head_owner: "owner".into(),
            head_repo: "proj".into(),
            head_ssh_url: String::new(),
            head_https_url: String::new(),
            base_ref: "main".into(),
            review_decision: None,
            ci: None,
            failing_checks: vec![],
            unresolved_threads: 0,
            latest_activity: None,
            reviews: vec![],
        };
        (root, repo, pr)
    }

    #[test]
    fn parses_the_ways_to_name_an_existing_branch() {
        let parse = |s: &str| parse_branch_ref(s, "hyperledger", "besu");
        let r = |o: &str, n: &str, b: &str| {
            Ok(BranchRef {
                owner: o.into(),
                repo: n.into(),
                branch: b.into(),
            })
        };
        assert_eq!(
            parse("daniellehrner:feat/evm-v2-all-opcodes"),
            r("daniellehrner", "besu", "feat/evm-v2-all-opcodes")
        );
        assert_eq!(
            parse("friend/besu-fork:fix"),
            r("friend", "besu-fork", "fix")
        );
        assert_eq!(
            parse("https://github.com/friend/besu/tree/feat/x/"),
            r("friend", "besu", "feat/x")
        );
        assert_eq!(
            parse(" release-25.1 "),
            r("hyperledger", "besu", "release-25.1")
        );
        for bad in [
            "",
            "friend:",
            ":x",
            "a b",
            "friend:-x",
            "friend:a..b",
            "https://github.com/friend/besu/pull/3",
            "friend:x:y",
        ] {
            assert!(parse(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn creates_a_tracking_worktree_next_to_the_clone_and_finds_it_again() {
        let (root, repo, pr) = setup();

        let first = prepare(&repo, &pr).unwrap();
        assert_eq!(canon(&first.path), canon(&root.path().join("proj_pr42")));
        assert_eq!((first.branch.as_str(), first.adopted), ("feature/x", false));
        assert_eq!(
            std::fs::read_to_string(first.path.join("a.txt")).unwrap(),
            "feature\n"
        );
        assert_eq!(
            git::upstream(&repo.path, "feature/x").as_deref(),
            Some("origin/feature/x")
        );

        let again = prepare(&repo, &pr).unwrap();
        assert_eq!(
            (canon(&again.path), again.adopted),
            (canon(&first.path), true)
        );
    }

    #[test]
    fn fast_forwards_a_stale_local_branch_and_keeps_a_diverged_one() {
        let (_root, repo, pr) = setup();
        // A local branch with the PR's name, behind the PR head.
        git(&repo.path, &["branch", "feature/x", "main"]).unwrap();
        let p = prepare(&repo, &pr).unwrap();
        assert_eq!(p.note, None);
        assert_eq!(
            std::fs::read_to_string(p.path.join("a.txt")).unwrap(),
            "feature\n"
        );

        let (_root2, repo2, pr2) = setup();
        git(&repo2.path, &["checkout", "-q", "-b", "feature/x"]).unwrap();
        std::fs::write(repo2.path.join("local.txt"), "mine\n").unwrap();
        commit(&repo2.path, "local work");
        git(&repo2.path, &["checkout", "-q", "main"]).unwrap();
        let p2 = prepare(&repo2, &pr2).unwrap();
        assert!(
            p2.note
                .as_deref()
                .is_some_and(|n| n.contains("left as it is")),
            "{:?}",
            p2.note
        );
        assert!(p2.path.join("local.txt").exists());
    }

    #[test]
    fn fetches_a_pull_ref_for_review_without_a_worktree() {
        let (_root, repo, _pr) = setup();
        // GitHub keeps every PR's head at refs/pull/<N>/head of the base repo.
        let up = git_str(
            &repo.path,
            &["config", "--get-regexp", "^url\\..*\\.insteadof$"],
        )
        .unwrap();
        let upstream = PathBuf::from(
            up.split_whitespace()
                .next()
                .unwrap()
                .trim_start_matches("url.")
                .trim_end_matches(".insteadof"),
        );
        git(&upstream, &["update-ref", "refs/pull/7/head", "feature/x"]).unwrap();

        let head = fetch_pr_head(&repo.path, "origin", 7, "main").unwrap();
        assert_eq!(
            head,
            git_str(&upstream, &["rev-parse", "feature/x"]).unwrap()
        );
        let base = git::merge_base(&repo.path, &head, "origin/main").unwrap();
        let files = git::changed_files(&repo.path, &base, Some(&head)).unwrap();
        assert_eq!(
            files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(),
            ["a.txt"]
        );
        assert!(
            git::worktrees(&repo.path).unwrap().len() == 1,
            "no worktree was created"
        );

        // Fetches running at once in the same clone all succeed (they take turns).
        std::thread::scope(|scope| {
            let fetches: Vec<_> = (0..6)
                .map(|_| scope.spawn(|| fetch_pr_head(&repo.path, "origin", 7, "main")))
                .collect();
            for f in fetches {
                assert_eq!(f.join().unwrap().as_deref(), Ok(head.as_str()));
            }
        });
    }

    #[test]
    fn issue_branches_have_a_short_slug() {
        assert_eq!(
            issue_branch(7, "Crash on start-up (Windows)!"),
            "issue-7-crash-on-start-up-windows"
        );
        assert_eq!(issue_branch(8, "???"), "issue-8");
        let long = issue_branch(
            9,
            "Make the snap sync pipeline survive a lost wakeup under heavy load",
        );
        assert!(long.len() <= "issue-9-".len() + 40, "{long}");
        assert_eq!(long, "issue-9-make-the-snap-sync-pipeline-survive-a");
    }

    #[test]
    fn issue_worktree_branches_from_the_default_branch_and_pushes_to_the_fork() {
        let (root, repo, _pr) = setup();
        // A fork remote of the viewer ("me"), also mapped to the local upstream.
        let up = git_str(
            &repo.path,
            &["config", "--get-regexp", "^url\\..*\\.insteadof$"],
        )
        .unwrap();
        let upstream = up
            .split_whitespace()
            .next()
            .unwrap()
            .trim_start_matches("url.")
            .trim_end_matches(".insteadof")
            .to_owned();
        git(
            &repo.path,
            // --add: keep the existing mapping of origin (a second plain set would replace it).
            &[
                "config",
                "--add",
                &format!("url.{upstream}.insteadOf"),
                "git@github.com:me/proj.git",
            ],
        )
        .unwrap();
        git(
            &repo.path,
            &["remote", "add", "mine", "git@github.com:me/proj.git"],
        )
        .unwrap();

        let p = prepare_issue(&repo, 7, "Crash on start", "main").unwrap();
        assert_eq!(canon(&p.path), canon(&root.path().join("proj_issue7")));
        assert_eq!(
            (p.branch.as_str(), p.adopted),
            ("issue-7-crash-on-start", false)
        );
        assert_eq!(
            git::head(&p.path).unwrap(),
            git_str(&repo.path, &["rev-parse", "origin/main"]).unwrap()
        );
        assert_eq!(
            git_str(
                &repo.path,
                &["config", "branch.issue-7-crash-on-start.remote"]
            )
            .unwrap(),
            "mine"
        );
        assert!(
            prepare_issue(&repo, 7, "Crash on start", "main")
                .unwrap()
                .adopted
        );
    }

    #[test]
    fn task_worktree_is_a_new_branch_from_the_default_branch() {
        let (root, repo, _pr) = setup();
        let p = prepare_task(&repo, 2, "Why does sync stall?", "main").unwrap();
        assert_eq!(canon(&p.path), canon(&root.path().join("proj_task2")));
        assert_eq!(
            (p.branch.as_str(), p.adopted),
            ("task-2-why-does-sync-stall", false)
        );
        assert_eq!(
            git::head(&p.path).unwrap(),
            git_str(&repo.path, &["rev-parse", "origin/main"]).unwrap()
        );
        assert_eq!(
            git_str(
                &repo.path,
                &["config", "branch.task-2-why-does-sync-stall.merge"]
            )
            .unwrap(),
            "refs/heads/task-2-why-does-sync-stall"
        );
        assert_eq!(task_branch(3, "!"), "task-3");
    }

    #[test]
    fn adopts_an_existing_pr_worktree() {
        let (root, repo, pr) = setup();
        let mine = root.path().join("my-checkout");
        git(
            &repo.path,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "pr-42",
                &mine.to_string_lossy(),
                "main",
            ],
        )
        .unwrap();

        let p = prepare(&repo, &pr).unwrap();
        assert_eq!(
            (canon(&p.path), p.branch.as_str(), p.adopted),
            (canon(&mine), "pr-42", true)
        );
    }
}
