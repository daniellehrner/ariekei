// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

//! Private mirrors: another GitHub repository in a connected clone, e.g. a private copy of
//! the repository used for security fixes. Its items are listed in the primary's Inbox, and
//! what is made for them (branches, pushes, PRs, issues) goes only to the mirror:
//!
//! - its branches are set up to push there (`mark`: `branch.<b>.pushRemote`, which beats a
//!   `remote.pushDefault` pointing at a fork);
//! - a pre-push hook in the clone (`install_guard`) refuses to push its branches and
//!   worktrees anywhere else, whoever pushes;
//! - its agents get `GH_REPO`, a note saying so (`note`), and their `gh` commands that would
//!   write elsewhere are refused (`refusal`, from a PreToolUse hook).

use crate::git::{self, git, git_str};
use crate::store::Repo;
use std::path::{Path, PathBuf};

/// The line that marks the pre-push hook as Ariekei's (it may replace it).
pub const GUARD_MARK: &str = "# ariekei-mirror-guard";

/// What a worktree's own git dir holds when it is a mirror's: the `owner/name` it pushes to.
pub const WORKTREE_MARK: &str = "ariekei-mirror";

/// The git config of a mirror's branch naming the `owner/name` it pushes to.
const BRANCH_MARK: &str = "ariekeiMirror";

/// The pre-push hook (POSIX sh: Git for Windows runs it too).
const GUARD: &str = r#"#!/bin/sh
# ariekei-mirror-guard
#
# Written by Ariekei for its private mirrors: refuses to push the work of a private
# repository anywhere else. That work is a worktree whose git dir has an `ariekei-mirror`
# file, or a branch with `branch.<name>.ariekeiMirror` set; each names the repository
# (owner/name) it may go to. Everything else is let through. After the disclosure,
# `git push --no-verify` pushes it anyway.

slug() {
    printf '%s\n' "$1" | tr 'ABCDEFGHIJKLMNOPQRSTUVWXYZ' 'abcdefghijklmnopqrstuvwxyz' |
        sed -e 's#/*$##' -e 's#\.git$##' -e 's#^.*[:/]\([^:/]*/[^:/]*\)$#\1#'
}

url=$(git config --get "remote.$1.pushurl" 2>/dev/null ||
    git config --get "remote.$1.url" 2>/dev/null || printf '%s' "$2")
dest=$(slug "$url")
refused=0
check() {
    [ -n "$2" ] || return 0
    [ "$(slug "$2")" = "$dest" ] && return 0
    echo "ariekei: $1 belongs to the private $2: not pushing it to $url." >&2
    refused=1
}

marker="$(git rev-parse --git-dir)/ariekei-mirror"
[ -f "$marker" ] && check "this worktree" "$(cat "$marker")"
while read -r ref _ _ _; do
    case "$ref" in
    refs/heads/*) branch=${ref#refs/heads/} ;;
    HEAD) branch=$(git symbolic-ref --quiet --short HEAD) ;;
    *) branch= ;;
    esac
    [ -n "$branch" ] && check "$branch" "$(git config --get "branch.$branch.ariekeiMirror")"
done
[ $refused = 0 ] || echo "ariekei: after the disclosure, push with --no-verify." >&2
exit $refused
"#;

/// `owner/name` from what the user typed: `owner/name` or a GitHub URL of the repository.
pub fn parse(input: &str) -> Result<(String, String), String> {
    let input = input.trim();
    let bad = || format!("{input:?} is not a repository: use owner/name or its GitHub URL");
    let rest = ["https://github.com/", "http://github.com/", "github.com/"]
        .iter()
        .find_map(|p| input.strip_prefix(p))
        .map(|r| r.trim_end_matches('/').trim_end_matches(".git"))
        .unwrap_or(input);
    let mut parts = rest.split('/');
    let (Some(owner), Some(name)) = (parts.next(), parts.next()) else {
        return Err(bad());
    };
    let ok = |s: &str| {
        !s.is_empty()
            && !s.starts_with(['-', '.'])
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
    };
    if !ok(owner) || !ok(name) || (parts.next().is_some() && !input.contains("github.com")) {
        return Err(bad());
    }
    Ok((owner.to_owned(), name.to_owned()))
}

/// The clone's remote for `owner/name`, added (named `name`, else `owner-name`, with ssh or
/// https as the primary's base remote has it) if it has none.
pub fn remote(primary: &Repo, owner: &str, name: &str) -> Result<String, String> {
    if let Some(remote) = git::remote_for(&primary.path, owner, name)? {
        return Ok(remote);
    }
    let base_url = git_str(
        &primary.path,
        &[
            "config",
            "--get",
            &format!("remote.{}.url", primary.base_remote),
        ],
    )
    .unwrap_or_default();
    let url = if base_url.starts_with("https://") || base_url.starts_with("http://") {
        format!("https://github.com/{owner}/{name}.git")
    } else {
        format!("git@github.com:{owner}/{name}.git")
    };
    let taken = git::remotes(&primary.path)?;
    let remote = [name.to_owned(), format!("{owner}-{name}")]
        .into_iter()
        .find(|r| !taken.iter().any(|(t, _)| t == r))
        .ok_or_else(|| format!("the clone has remotes named {name} and {owner}-{name} already"))?;
    git(&primary.path, &["remote", "add", &remote, &url])?;
    Ok(remote)
}

/// The clone's hooks folder: `core.hooksPath` if set, else the common git dir's `hooks`.
fn hooks_dir(clone: &Path) -> Result<PathBuf, String> {
    if let Ok(path) = git_str(clone, &["config", "--get", "core.hooksPath"])
        && !path.is_empty()
    {
        return Ok(clone.join(path));
    }
    let common = git_str(
        clone,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?;
    Ok(PathBuf::from(common).join("hooks"))
}

/// Installs the pre-push guard (`GUARD`) in `clone`, or updates Ariekei's. A pre-push hook of
/// someone else's is left as it is: the guard is saved next to it, and the returned note says
/// how to call it from there.
pub fn install_guard(clone: &Path) -> Result<Option<String>, String> {
    let dir = hooks_dir(clone)?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let hook = dir.join("pre-push");
    let existing = std::fs::read_to_string(&hook).ok();
    let theirs = existing.as_deref().is_some_and(|h| !h.contains(GUARD_MARK));
    let target = if theirs {
        dir.join("ariekei-mirror-guard")
    } else {
        hook.clone()
    };
    if std::fs::read_to_string(&target).ok().as_deref() != Some(GUARD) {
        std::fs::write(&target, GUARD).map_err(|e| format!("{}: {e}", target.display()))?;
        executable(&target)?;
    }
    Ok(theirs.then(|| {
        format!(
            "{} is not Ariekei's, so it was left as it is. To keep pushes of the mirror's work \
             off other repositories, have it run {} with the same arguments and input.",
            hook.display(),
            target.display()
        )
    }))
}

#[cfg(unix)]
fn executable(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
        .map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(not(unix))]
fn executable(_: &Path) -> Result<(), String> {
    Ok(())
}

/// Sets up a mirror item's checkout `dir` and its `branch` (none: a detached HEAD) so that
/// what is pushed goes to `allowed` (`owner/name`: the mirror, or a PR's head repository) and
/// nowhere else: the branch pushes to the clone's remote for it (`pushRemote`, which beats
/// `remote.pushDefault`) and tracks its branch of the same name there unless it tracks one on
/// that remote already, and both are marked for the pre-push guard. The clone's main checkout
/// itself is never marked, only its branch.
pub fn mark(repo: &Repo, dir: &Path, branch: Option<&str>, allowed: &str) -> Result<(), String> {
    let clone = &repo.path;
    if let Some(branch) = branch.filter(|b| !b.is_empty()) {
        let remote = allowed
            .split_once('/')
            .and_then(|(o, n)| git::remote_for(clone, o, n).ok().flatten())
            .unwrap_or_else(|| repo.base_remote.clone());
        let key = |k: &str| format!("branch.{branch}.{k}");
        // Its upstream is there too (e.g. a task moved to the mirror, whose branch tracked the
        // primary): what Ariekei reads as pushed, and the PR it finds for it, are the mirror's.
        if git_str(clone, &["config", "--get", &key("remote")])
            .ok()
            .as_deref()
            != Some(&remote)
        {
            git(clone, &["config", &key("remote"), &remote])?;
            git(
                clone,
                &["config", &key("merge"), &format!("refs/heads/{branch}")],
            )?;
        }
        git(clone, &["config", &key("pushRemote"), &remote])?;
        git(clone, &["config", &key(BRANCH_MARK), allowed])?;
    }
    let own = git_str(dir, &["rev-parse", "--absolute-git-dir"])?;
    let common = git_str(
        dir,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?;
    let same =
        |a: &str, b: &str| Path::new(a).canonicalize().ok() == Path::new(b).canonicalize().ok();
    if !same(&own, &common) {
        let file = Path::new(&own).join(WORKTREE_MARK);
        std::fs::write(&file, format!("{allowed}\n"))
            .map_err(|e| format!("{}: {e}", file.display()))?;
    }
    Ok(())
}

/// What a mirror item's agent is told, at every start (also when its conversation resumes).
pub fn note(mirror: &Repo, primary: Option<&Repo>) -> String {
    let slug = mirror.slug();
    let public = primary.map_or("the public repository".to_owned(), |p| p.slug());
    format!(
        "\n## Private repository\n\n\
         This work belongs to {slug}, a private mirror of {public} (e.g. for a security fix \
         that must not be public yet). Keep everything about it there:\n\
         - Push only to the `{remote}` remote ({slug}). A plain `git push` is set up for that; \
         for a branch you make yourself, `git push -u {remote} <branch>`.\n\
         - Open pull requests and issues, and comment, only on {slug}. `gh` is set to it \
         (`GH_REPO`); don't point it elsewhere.\n\
         - Never push this work to {public} or a fork of it, and never describe it in an issue, \
         pull request, comment or commit there.\n\
         - Reading {public} (code, issues, PRs) is fine.\n\
         - A pre-push hook in the clone refuses pushes of this work elsewhere: don't get around \
         it (no `--no-verify`, no other hooks path). If a push is refused, stop and tell the \
         user.\n",
        remote = mirror.base_remote,
    )
}

/// The `owner/name` a GitHub URL or `gh -R` value (`[HOST/]OWNER/REPO`) names, lowercase.
fn slug_of(s: &str) -> Option<String> {
    if let Some(at) = s.find("github.com/") {
        let mut parts = s[at + "github.com/".len()..].split('/');
        let (o, n) = (parts.next()?, parts.next()?);
        let n = n.trim_end_matches(".git");
        return (!o.is_empty() && !n.is_empty()).then(|| format!("{o}/{n}").to_lowercase());
    }
    let parts: Vec<&str> = s.trim_matches('/').split('/').collect();
    match parts.as_slice() {
        [.., o, n] if !o.is_empty() && !n.is_empty() && parts.len() <= 3 => {
            Some(format!("{o}/{n}").to_lowercase())
        }
        _ => None,
    }
}

/// The simple commands in a shell command line, as words, quotes removed: roughly as a shell
/// splits them at `;`, `&`, `|`, newlines, parentheses, backticks and `$(`. What is inside
/// quotes stays one word (e.g. a PR body).
fn commands(line: &str) -> Vec<Vec<String>> {
    let mut out = vec![];
    let mut words: Vec<String> = vec![];
    let mut word = String::new();
    let mut in_word = false;
    let mut quote: Option<char> = None;
    let mut chars = line.chars().peekable();
    let end_word = |word: &mut String, in_word: &mut bool, words: &mut Vec<String>| {
        if *in_word {
            words.push(std::mem::take(word));
            *in_word = false;
        }
    };
    while let Some(c) = chars.next() {
        if let Some(q) = quote {
            if c == q {
                quote = None;
            } else if c == '\\' && q == '"' {
                if let Some(n) = chars.next() {
                    word.push(n);
                }
            } else {
                word.push(c);
            }
            continue;
        }
        match c {
            '\'' | '"' => {
                quote = Some(c);
                in_word = true;
            }
            '\\' => {
                if let Some(n) = chars.next().filter(|n| *n != '\n') {
                    word.push(n);
                    in_word = true;
                }
            }
            ';' | '&' | '|' | '\n' | '(' | ')' | '`' | '{' | '}' => {
                end_word(&mut word, &mut in_word, &mut words);
                if !words.is_empty() {
                    out.push(std::mem::take(&mut words));
                }
            }
            '$' if chars.peek() == Some(&'(') => {
                chars.next();
                end_word(&mut word, &mut in_word, &mut words);
                if !words.is_empty() {
                    out.push(std::mem::take(&mut words));
                }
            }
            c if c.is_whitespace() => end_word(&mut word, &mut in_word, &mut words),
            c => {
                word.push(c);
                in_word = true;
            }
        }
    }
    end_word(&mut word, &mut in_word, &mut words);
    if !words.is_empty() {
        out.push(words);
    }
    out
}

/// Why a mirror item's agent may not run the shell `command`, if it may not: it would write
/// to another repository than `mirror` (`owner/name`) with `gh`, make a repository or gist
/// of the work, or get around the pre-push guard. Best effort (a shell is not parsed fully):
/// the guard and `GH_REPO` do the rest.
pub fn refusal(command: &str, mirror: &str) -> Option<String> {
    let mirror = mirror.to_lowercase();
    let refuse = |what: &str| {
        Some(format!(
            "Refused by Ariekei: this work belongs to the private repository {mirror}; {what}. \
             Push only to {mirror} and open PRs and issues only there (gh is set to it). If you \
             think this is needed, ask the user to do it."
        ))
    };
    let mut gh_repo: Option<Option<String>> = None;
    for words in commands(command) {
        let mut w: &[String] = &words;
        // Leading assignments and wrappers: `GH_REPO=x gh …`, `env -u GH_REPO gh …`, `sudo git …`.
        loop {
            match w.first().map(String::as_str) {
                Some(a) if a.contains('=') && !a.starts_with('-') && !a.starts_with('=') => {
                    if let Some(v) = a.strip_prefix("GH_REPO=") {
                        gh_repo = Some(Some(v.to_owned()));
                    }
                    w = &w[1..];
                }
                Some("env" | "command" | "exec" | "time" | "nohup" | "sudo" | "export") => {
                    if w.get(1).is_some_and(|a| a == "-u")
                        && w.get(2).is_some_and(|a| a == "GH_REPO")
                    {
                        gh_repo = Some(None);
                    }
                    w = &w[1..];
                }
                Some("-u") if w.get(1).is_some_and(|a| a == "GH_REPO") => w = &w[2..],
                Some(a) if a.starts_with('-') && a != "-" => w = &w[1..],
                _ => break,
            }
        }
        let Some(program) = w.first() else { continue };
        let program = program.rsplit(['/', '\\']).next().unwrap_or(program);
        let args = &w[1..];
        match program {
            "unset" if args.iter().any(|a| a == "GH_REPO") => gh_repo = Some(None),
            "git" => {
                let mut i = 0;
                while let Some(a) = args.get(i) {
                    let lower = a.to_lowercase();
                    if a == "-c" || a == "--config-env" {
                        if args
                            .get(i + 1)
                            .is_some_and(|v| v.to_lowercase().starts_with("core.hookspath"))
                        {
                            return refuse(
                                "changing git's hooks path would skip the pre-push guard",
                            );
                        }
                        i += 2;
                    } else if lower.starts_with("--config-env=core.hookspath") {
                        return refuse("changing git's hooks path would skip the pre-push guard");
                    } else if ["-C", "--git-dir", "--work-tree", "--namespace"]
                        .contains(&a.as_str())
                    {
                        i += 2;
                    } else if a.starts_with('-') {
                        i += 1;
                    } else {
                        break;
                    }
                }
                let sub = args.get(i).map(String::as_str);
                let rest = args.get(i + 1..).unwrap_or_default();
                if sub == Some("push") && rest.iter().any(|a| a == "--no-verify") {
                    return refuse("`git push --no-verify` skips the guard that keeps it there");
                }
                if sub == Some("config")
                    && rest
                        .iter()
                        .any(|a| a.to_lowercase().starts_with("core.hookspath"))
                {
                    return refuse("changing git's hooks path would skip the pre-push guard");
                }
            }
            "gh" => {
                let (group, verb, target, url, method, fields) = gh_parts(args);
                if group == "repo" && ["create", "fork"].contains(&verb) {
                    return refuse(&format!("`gh repo {verb}` would make a repository of it"));
                }
                if group == "gist" && !["list", "view"].contains(&verb) {
                    return refuse("a gist of it would be readable outside the repository");
                }
                if group == "api" {
                    // `verb` is the endpoint.
                    let path = verb.trim_start_matches('/');
                    let write = method.as_deref().map_or(fields, |m| m != "GET");
                    if path == "graphql" {
                        if args.iter().any(|a| a.contains("mutation")) {
                            return refuse(
                                "Ariekei can't tell which repository a GraphQL mutation writes \
                                 to; use gh pr / gh issue commands",
                            );
                        }
                    } else if write
                        && path
                            .strip_prefix("repos/")
                            .and_then(slug_of_path)
                            .as_deref()
                            != Some(mirror.as_str())
                    {
                        return refuse(&format!("`gh api {path}` would write outside it"));
                    }
                    continue;
                }
                let writes = match group {
                    "pr" => [
                        "create", "comment", "edit", "merge", "review", "close", "reopen", "ready",
                        "lock", "unlock",
                    ]
                    .contains(&verb),
                    "issue" => [
                        "create", "comment", "edit", "close", "reopen", "transfer", "lock",
                        "unlock", "pin", "unpin", "delete", "develop",
                    ]
                    .contains(&verb),
                    "release" | "label" | "secret" | "variable" | "workflow" | "cache"
                    | "ruleset" => !["list", "view", "download"].contains(&verb),
                    "repo" => ["edit", "delete", "rename", "archive", "sync"].contains(&verb),
                    _ => false,
                };
                if !writes {
                    continue;
                }
                // Where it goes: -R, else the PR or issue URL it names, else GH_REPO as the
                // command sets it, else (None) GH_REPO as Ariekei set it: the mirror.
                let aimed = target
                    .map(|t| slug_of(&t))
                    .or_else(|| url.map(|u| slug_of(&u)))
                    .or_else(|| gh_repo.clone().map(|r| r.as_deref().and_then(slug_of)));
                match aimed {
                    None => {}
                    Some(Some(s)) if s == mirror => {}
                    Some(Some(s)) => {
                        return refuse(&format!("`gh {group} {verb}` would write to {s}"));
                    }
                    Some(None) => {
                        return refuse(&format!(
                            "`gh {group} {verb}` without GH_REPO would write wherever gh's \
                             default is"
                        ));
                    }
                }
            }
            _ => {}
        }
    }
    None
}

/// Options of `gh` (its commands') that take a value: the value is not an argument.
const GH_VALUE_OPTIONS: &[&str] = &[
    "-R",
    "--repo",
    "-X",
    "--method",
    "-f",
    "-F",
    "--field",
    "--raw-field",
    "-H",
    "--header",
    "--input",
    "-q",
    "--jq",
    "-t",
    "--template",
    "--title",
    "-b",
    "--body",
    "--body-file",
    "-B",
    "--base",
    "--head",
    "-a",
    "--assignee",
    "-l",
    "--label",
    "-m",
    "--milestone",
    "-r",
    "--reviewer",
    "-p",
    "--project",
    "--hostname",
    "--cache",
    "-T",
    "--add-label",
    "--remove-label",
    "--add-assignee",
    "--remove-assignee",
    "--add-reviewer",
    "--remove-reviewer",
    "--subject",
];

/// What a `gh` command line (its arguments) does: its command group and verb (for `api`,
/// the endpoint), the repository `-R` names, a GitHub URL given as its argument, the HTTP
/// method asked for, and whether it sends fields.
#[allow(clippy::type_complexity)]
fn gh_parts(
    args: &[String],
) -> (
    &str,
    &str,
    Option<String>,
    Option<String>,
    Option<String>,
    bool,
) {
    let (mut target, mut method, mut fields) = (None, None, false);
    let mut positional: Vec<&str> = vec![];
    let mut i = 0;
    while let Some(a) = args.get(i) {
        let value = args.get(i + 1).cloned();
        let (flag, inline) = match a.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f, Some(v.to_owned())),
            _ => (a.as_str(), None),
        };
        let takes = GH_VALUE_OPTIONS.contains(&flag);
        let v = || inline.clone().or(value.clone());
        match flag {
            "-R" | "--repo" => target = v(),
            "-X" | "--method" => method = v().map(|m| m.to_uppercase()),
            "-f" | "-F" | "--field" | "--raw-field" | "--input" => fields = true,
            f if f.starts_with("-R") && f.len() > 2 => target = Some(f[2..].to_owned()),
            f if f.starts_with("-X") && f.len() > 2 => method = Some(f[2..].to_uppercase()),
            f if !f.starts_with('-') => positional.push(a),
            _ => {}
        }
        i += if takes && inline.is_none() { 2 } else { 1 };
    }
    let group = positional.first().copied().unwrap_or_default();
    let verb = positional.get(1).copied().unwrap_or_default();
    // The PR or issue it acts on, when given as a URL (a whole word, not text mentioning one).
    let url = positional
        .get(2)
        .filter(|p| p.starts_with("https://github.com/") && !p.contains(char::is_whitespace))
        .map(|p| (*p).to_owned());
    (group, verb, target, url, method, fields)
}

/// `owner/name` of `owner/name/...` (a REST path after `repos/`), lowercase.
fn slug_of_path(path: &str) -> Option<String> {
    let mut parts = path.split(['/', '?']);
    let (o, n) = (parts.next()?, parts.next()?);
    (!o.is_empty() && !n.is_empty()).then(|| format!("{o}/{n}").to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_what_the_user_types_for_a_mirror() {
        let ok = |s: &str| parse(s).unwrap();
        let want = ("besu-eth".to_owned(), "sec-besu".to_owned());
        assert_eq!(ok("besu-eth/sec-besu"), want);
        assert_eq!(ok(" https://github.com/besu-eth/sec-besu/ "), want);
        assert_eq!(ok("https://github.com/besu-eth/sec-besu.git"), want);
        assert_eq!(ok("github.com/besu-eth/sec-besu/issues"), want);
        for bad in ["", "sec-besu", "besu-eth/", "a/b/c", "a b/c", "-x/y"] {
            assert!(parse(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn refuses_what_would_write_outside_the_mirror() {
        let m = "besu-eth/sec-besu";
        let refused = |c: &str| refusal(c, m).is_some();
        for c in [
            "gh pr create -R besu-eth/besu --title x",
            "gh pr create --repo=besu-eth/besu",
            "gh -R besu-eth/besu issue create -t x -b y",
            "GH_REPO=besu-eth/besu gh issue create -t x",
            "export GH_REPO=besu-eth/besu; gh pr create",
            "unset GH_REPO && gh pr create --fill",
            "env -u GH_REPO gh pr create --fill",
            "gh pr comment https://github.com/besu-eth/besu/pull/12 --body hi",
            "cd x && gh issue comment 5 -R daniellehrner/besu -b 'see'",
            "gh repo create besu-fix --public --source=. --push",
            "gh repo fork",
            "gh gist create Fix.java",
            "gh api repos/besu-eth/besu/issues -f title=x",
            "gh api -X POST /repos/besu-eth/besu/pulls --input body.json",
            "gh api graphql -f query='mutation { addComment(input: {}) { clientMutationId } }'",
            "git push --no-verify origin HEAD",
            "git -C ../x push -u daniellehrner b --no-verify",
            "git -c core.hooksPath=/dev/null push origin",
            "git config core.hooksPath /tmp/none",
            "echo $(gh pr create -R besu-eth/besu)",
        ] {
            assert!(refused(c), "{c}");
        }
        for c in [
            "gh pr create --fill",
            "gh pr create -R besu-eth/sec-besu --title x",
            "gh pr create -R BESU-ETH/Sec-Besu",
            "GH_REPO=besu-eth/sec-besu gh issue create -t x",
            "gh pr view 11455 -R besu-eth/besu",
            "gh issue list -R besu-eth/besu --search crash",
            "gh pr diff https://github.com/besu-eth/besu/pull/12",
            "gh api repos/besu-eth/besu/pulls/12",
            "gh api repos/besu-eth/sec-besu/issues/3/comments -f body=x",
            "gh api graphql -f query='query { viewer { login } }'",
            "git push",
            "git push -u sec-besu fix-1",
            "git commit -m 'never git push --no-verify here'",
            "gh pr create --body \"$(cat <<'EOF'\nDon't run git push --no-verify\nEOF\n)\"",
            "ls -la",
        ] {
            assert!(!refused(c), "{c}");
        }
    }

    /// A clone of "GitHub's" o/proj with remotes for it (origin), its private mirror
    /// o/proj-sec (sec) and the viewer's fork me/proj (me, where pushes go by default), all
    /// local bare repos behind their GitHub URLs; and the mirror's Repo.
    fn clone_with_mirror(root: &Path) -> (PathBuf, Repo) {
        use crate::testkit::{commit, git as tgit};
        let clone = root.join("proj");
        std::fs::create_dir_all(&clone).unwrap();
        tgit(&clone, &["init", "-q"]);
        crate::testkit::write(&clone.join("a.txt"), "a\n");
        commit(&clone, "first", "me");
        for (remote, slug) in [
            ("origin", "o/proj"),
            ("sec", "o/proj-sec"),
            ("me", "me/proj"),
        ] {
            let bare = root.join("github").join(format!("{slug}.git"));
            std::fs::create_dir_all(&bare).unwrap();
            tgit(&bare, &["init", "-q", "--bare"]);
            let url = format!("git@github.com:{slug}.git");
            tgit(&clone, &["remote", "add", remote, &url]);
            tgit(
                &clone,
                &["config", &format!("url.{}.insteadOf", bare.display()), &url],
            );
            tgit(&clone, &["push", "-q", remote, "main"]);
        }
        tgit(&clone, &["config", "remote.pushDefault", "me"]);
        let mirror = Repo {
            id: 2,
            path: clone.clone(),
            owner: "o".into(),
            name: "proj-sec".into(),
            base_remote: "sec".into(),
            viewer: "me".into(),
            mirror_of: Some(1),
        };
        (clone, mirror)
    }

    fn on_github(root: &Path, slug: &str, branch: &str) -> bool {
        git_str(
            &root.join("github").join(format!("{slug}.git")),
            &[
                "rev-parse",
                "--verify",
                "--quiet",
                &format!("refs/heads/{branch}"),
            ],
        )
        .is_ok()
    }

    #[test]
    fn the_mirrors_work_is_pushed_to_it_and_nowhere_else() {
        use crate::testkit::{commit, git as tgit, write};
        let root = tempfile::tempdir().unwrap();
        let root = root.path();
        let (clone, mirror) = clone_with_mirror(root);
        assert_eq!(install_guard(&clone).unwrap(), None);

        // A worktree on a mirror branch (as Ariekei makes for a mirror's issue).
        let wt = root.join("proj_proj-sec-issue1");
        tgit(
            &clone,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "fix",
                wt.to_str().unwrap(),
                "main",
            ],
        );
        mark(&mirror, &wt, Some("fix"), "o/proj-sec").unwrap();
        write(&wt.join("fix.txt"), "secret\n");
        commit(&wt, "the fix", "me");

        // Without pushRemote, remote.pushDefault would send it to the public fork.
        tgit(&wt, &["push", "-q"]);
        assert!(on_github(root, "o/proj-sec", "fix"));

        // Anywhere else is refused, by branch (also from the main clone) and by worktree.
        let refused = |dir: &Path, args: &[&str]| {
            let e = git(dir, args).unwrap_err();
            assert!(
                e.contains("belongs to the private o/proj-sec"),
                "{args:?}: {e}"
            );
        };
        refused(&wt, &["push", "me", "fix"]);
        refused(&wt, &["push", "origin", "HEAD:refs/heads/fix"]);
        refused(&wt, &["push", "git@github.com:o/proj.git", "fix"]);
        refused(&clone, &["push", "me", "fix"]);
        tgit(&wt, &["checkout", "-q", "-b", "other"]);
        refused(&wt, &["push", "me", "other"]);
        assert!(!on_github(root, "me/proj", "fix") && !on_github(root, "me/proj", "other"));
        assert!(!on_github(root, "o/proj", "fix"));

        // After the disclosure: --no-verify. And the primary's work is let through.
        tgit(&wt, &["push", "-q", "--no-verify", "origin", "fix"]);
        assert!(on_github(root, "o/proj", "fix"));
        tgit(&clone, &["checkout", "-q", "-b", "public"]);
        tgit(&clone, &["push", "-q", "me", "public"]);
        assert!(on_github(root, "me/proj", "public"));
    }

    #[test]
    fn someone_elses_pre_push_hook_is_left_alone() {
        let root = tempfile::tempdir().unwrap();
        let (clone, _) = clone_with_mirror(root.path());
        let hook = clone.join(".git/hooks/pre-push");
        std::fs::write(&hook, "#!/bin/sh\nexit 0\n").unwrap();
        let note = install_guard(&clone).unwrap().unwrap();
        assert!(note.contains("ariekei-mirror-guard"), "{note}");
        assert_eq!(
            std::fs::read_to_string(&hook).unwrap(),
            "#!/bin/sh\nexit 0\n"
        );
        assert!(clone.join(".git/hooks/ariekei-mirror-guard").exists());
        // Ariekei's own is updated in place.
        std::fs::write(&hook, format!("{GUARD_MARK}\nold\n")).unwrap();
        assert_eq!(install_guard(&clone).unwrap(), None);
        assert_eq!(std::fs::read_to_string(&hook).unwrap(), GUARD);
    }

    #[test]
    fn the_note_names_the_mirror_its_remote_and_the_public_repo() {
        let mirror = Repo {
            id: 2,
            path: "/src/besu".into(),
            owner: "besu-eth".into(),
            name: "sec-besu".into(),
            base_remote: "sec-besu".into(),
            viewer: "me".into(),
            mirror_of: Some(1),
        };
        let primary = Repo {
            id: 1,
            name: "besu".into(),
            base_remote: "origin".into(),
            mirror_of: None,
            ..mirror.clone()
        };
        let note = note(&mirror, Some(&primary));
        for part in [
            "besu-eth/sec-besu",
            "besu-eth/besu",
            "`sec-besu` remote",
            "GH_REPO",
        ] {
            assert!(note.contains(part), "{part}: {note}");
        }
    }
}

#[cfg(test)]
mod app_tests {
    use crate::testkit::{TestApp, commit, git as tgit, write};
    use crate::workitem;
    use serde_json::json;
    use std::path::Path;

    /// A bare "GitHub" repo for `slug` (a copy of the app's upstream) behind its ssh URL.
    fn github_repo(app: &TestApp, slug: &str) -> std::path::PathBuf {
        let bare = app.path(&format!("github-{}", slug.replace('/', "-")));
        tgit(
            app.root.path(),
            &[
                "clone",
                "-q",
                "--bare",
                &app.upstream.to_string_lossy(),
                &bare.to_string_lossy(),
            ],
        );
        tgit(
            &app.clone,
            &[
                "config",
                &format!("url.{}.insteadOf", bare.display()),
                &format!("git@github.com:{slug}.git"),
            ],
        );
        bare
    }

    fn config(dir: &Path, key: &str) -> String {
        crate::git::git_str(dir, &["config", "--get", key]).unwrap_or_default()
    }

    #[test]
    fn a_mirrors_items_are_listed_with_the_repos_and_their_work_stays_on_it() {
        let app = TestApp::new();
        let s = &app.state;
        let sec = github_repo(&app, "owner/proj-sec");
        // The user's public fork, where pushes go by default.
        let fork = github_repo(&app, "me/proj");
        tgit(
            &app.clone,
            &["remote", "add", "me", "git@github.com:me/proj.git"],
        );
        tgit(&app.clone, &["config", "remote.pushDefault", "me"]);
        let mut data = app.data();
        data["private"] = json!(["proj-sec"]);
        data["public"] = json!(["proj-copy"]);
        data["issues"] = json!([
            {"number": 1, "title": "Public bug", "author": "alice", "assigned": true},
            {"number": 1, "title": "Leak in the parser", "author": "alice", "assigned": true,
             "repo": "proj-sec"},
        ]);
        app.set_data(&data);

        // Only a private repository other than the repo itself.
        let add = |name: &str| workitem::add_mirror(s, Some(app.repo.clone()), name);
        assert!(add("owner/proj").unwrap_err().contains("itself"));
        assert!(add("owner/nowhere").is_err());
        assert!(add("owner/proj-copy").unwrap_err().contains("is public"));
        let added = add("https://github.com/owner/proj-sec").unwrap();
        let mirror = added.repo;
        assert_eq!(added.note, None);
        assert_eq!(
            (mirror.base_remote.as_str(), mirror.mirror_of),
            ("proj-sec", Some(app.repo.id))
        );
        assert_eq!(
            config(&app.clone, "remote.proj-sec.url"),
            "git@github.com:owner/proj-sec.git"
        );
        assert!(
            std::fs::read_to_string(app.clone.join(".git/hooks/pre-push"))
                .unwrap()
                .contains(super::GUARD_MARK)
        );

        // Both issues #1 are listed, the mirror's tagged.
        crate::poller::poll(s, &app.repo, true).unwrap();
        crate::poller::poll(s, &mirror, true).unwrap();
        let mut tags: Vec<(String, Option<String>)> = app
            .rows()
            .into_iter()
            .map(|r| (r.title, r.mirror))
            .collect();
        tags.sort();
        assert_eq!(
            tags,
            [
                ("Leak in the parser".to_owned(), Some("proj-sec".to_owned())),
                ("Public bug".to_owned(), None)
            ]
        );

        // Its worktree is named after the mirror and pushes there, not to the fork.
        let id = s.store.items(mirror.id).unwrap()[0].id;
        let prepared = workitem::prepare(s, id).unwrap();
        assert!(
            prepared.path.ends_with("proj_proj-sec-issue1"),
            "{}",
            prepared.path.display()
        );
        assert!(
            prepared.branch.starts_with("proj-sec-issue-1-"),
            "{}",
            prepared.branch
        );
        let b = &prepared.branch;
        assert_eq!(
            config(&app.clone, &format!("branch.{b}.pushRemote")),
            "proj-sec"
        );
        write(&prepared.path.join("fix.txt"), "fixed\n");
        commit(&prepared.path, "Fix the leak", "me");
        tgit(&prepared.path, &["push", "-q"]);
        let has = |bare: &Path| {
            crate::git::git_str(
                bare,
                &[
                    "rev-parse",
                    "--verify",
                    "--quiet",
                    &format!("refs/heads/{b}"),
                ],
            )
            .is_ok()
        };
        assert!(has(&sec) && !has(&fork));
        assert!(crate::git::git(&prepared.path, &["push", "-q", "me", b]).is_err());

        // Its agent gets GH_REPO and the note.
        let plan = workitem::session_plan(s, &s.data_dir, id).unwrap();
        assert_eq!(
            plan.env,
            [("GH_REPO".to_owned(), "owner/proj-sec".to_owned())]
        );
        let context = std::fs::read_to_string(plan.context_file.unwrap()).unwrap();
        assert!(
            context.contains("Leak in the parser") && context.contains("## Private repository")
        );

        // A task of the repo, moved to the mirror: numbered across both, it pushes there now.
        let task = workitem::create_task(s, Some(app.repo.clone()), "Secret work", "", "").unwrap();
        let before = s.store.item(task).unwrap();
        let branch = before.branch.clone().unwrap();
        assert_eq!(
            config(&app.clone, &format!("branch.{branch}.pushRemote")),
            ""
        );
        assert_eq!(workitem::move_task(s, task, mirror.id).unwrap(), None);
        let after = s.store.item(task).unwrap();
        assert_eq!((after.repo_id, after.number), (mirror.id, before.number));
        assert_eq!(
            config(&app.clone, &format!("branch.{branch}.pushRemote")),
            "proj-sec"
        );
        assert_eq!(
            config(&app.clone, &format!("branch.{branch}.remote")),
            "proj-sec"
        );
        assert_eq!(
            config(&app.clone, &format!("branch.{branch}.ariekeiMirror")),
            "owner/proj-sec"
        );
        let wt = after.worktree_path.unwrap();
        assert!(
            crate::git::git(&wt, &["push", "-q", "origin", &branch])
                .unwrap_err()
                .contains("belongs to the private owner/proj-sec")
        );
        // A task can't be moved to a repo that is not its repo's mirror, nor can an issue.
        assert!(workitem::move_task(s, task, app.repo.id).is_err());
        assert!(workitem::move_task(s, id, mirror.id).is_err());

        // A mirror's task is created in it and numbered after the others.
        let t2 = workitem::create_task(s, Some(mirror.clone()), "More", "", "").unwrap();
        assert_eq!(s.store.item(t2).unwrap().number, before.number - 1);

        // Removing the repo removes its mirror.
        crate::lifecycle::remove_repo(s, app.repo.id).unwrap();
        assert_eq!(s.store.repo(mirror.id).unwrap(), None);
    }
}
