//! A PR's isolated working copy: an existing worktree that already has the PR's branch,
//! or a new one next to the main clone (`<clone>_pr<N>`).

use crate::git::{self, git, git_str};
use crate::github::PrSummary;
use crate::store::Repo;
use serde::Serialize;
use std::path::{Path, PathBuf};

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

/// The remote for the PR's head repository, adding one named after its owner if the
/// clone has none (as `gh pr checkout` does). Uses ssh when the base remote does.
fn head_remote(repo: &Repo, pr: &PrSummary) -> Result<String, String> {
    if let Some(remote) = git::remote_for(&repo.path, &pr.head_owner, &pr.head_repo)? {
        return Ok(remote);
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
        &pr.head_https_url
    } else {
        &pr.head_ssh_url
    };
    let name = &pr.head_owner;
    git(&repo.path, &["remote", "add", name, url])?;
    Ok(name.clone())
}

/// Fetches `branch` of `remote` into `refs/remotes/<remote>/<branch>` regardless of the
/// remote's configured refspec.
fn fetch(repo: &Path, remote: &str, branch: &str) -> Result<String, String> {
    let tracking = format!("{remote}/{branch}");
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

/// An existing linked worktree that belongs to this PR: its branch is the PR's branch,
/// `pr-<N>`, or tracks the PR's head. The main checkout is never adopted.
fn find_existing(
    repo: &Path,
    pr: &PrSummary,
    tracking: &str,
) -> Result<Option<git::Worktree>, String> {
    let pr_branch = format!("pr-{}", pr.number);
    Ok(git::worktrees(repo)?
        .into_iter()
        .filter(|w| !w.main && w.path.exists())
        .find(|w| {
            w.branch.as_deref().is_some_and(|b| {
                b == pr_branch
                    || git::upstream(repo, b).as_deref() == Some(tracking)
                    || (b == pr.head_ref && git::upstream(repo, b).is_none())
            })
        }))
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
    let dir = repo
        .path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| repo.name.clone());
    repo.path
        .parent()
        .unwrap_or(&repo.path)
        .join(format!("{dir}_pr{number}"))
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

    if let Some(w) = find_existing(&repo.path, pr, &tracking)? {
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

    // Keep the PR's branch name when possible: with the same name, a plain `git push`
    // updates the PR. Fall back to pr-<N> if a different branch already has that name.
    let reusable = git::branch_exists(&repo.path, &pr.head_ref)
        && git::upstream(&repo.path, &pr.head_ref).is_none_or(|u| u == tracking);
    let branch = if !git::branch_exists(&repo.path, &pr.head_ref) || reusable {
        pr.head_ref.clone()
    } else {
        format!("pr-{}", pr.number)
    };
    let path_arg = path.to_string_lossy().into_owned();
    let mut note = None;
    if git::branch_exists(&repo.path, &branch) {
        let checked_out = git::worktrees(&repo.path)?
            .into_iter()
            .find(|w| w.branch.as_deref() == Some(&branch));
        if let Some(w) = checked_out {
            return Err(format!(
                "branch {branch} is checked out in {}; switch that checkout to another branch first",
                w.path.display()
            ));
        }
        git(
            &repo.path,
            &["worktree", "add", "--quiet", &path_arg, &branch],
        )?;
        // A reused local branch may be behind the PR: bring it up to date if that is a
        // fast-forward, otherwise leave the user's commits alone and say so.
        if git(&path, &["merge", "--ff-only", "--quiet", &tracking]).is_err() {
            note = Some(format!(
                "The existing local branch {branch} has commits that are not in the PR ({tracking}); it was left as it is."
            ));
        }
    } else {
        git(
            &repo.path,
            &[
                "worktree", "add", "--quiet", "-b", &branch, &path_arg, &tracking,
            ],
        )?;
    }
    git(
        &repo.path,
        &["branch", "--quiet", "--set-upstream-to", &tracking, &branch],
    )?;
    Ok(Prepared {
        path,
        branch,
        adopted: false,
        note,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::tests::{commit, repo as git_repo};

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
        git(root.path(), &["clone", "-q", &up.to_string_lossy(), "proj"]).unwrap();
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
    fn creates_a_tracking_worktree_next_to_the_clone_and_finds_it_again() {
        let (root, repo, pr) = setup();

        let first = prepare(&repo, &pr).unwrap();
        assert_eq!(first.path, root.path().join("proj_pr42"));
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
        assert_eq!((again.path, again.adopted), (first.path, true));
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
            (p.path, p.branch.as_str(), p.adopted),
            (mine, "pr-42", true)
        );
    }
}
