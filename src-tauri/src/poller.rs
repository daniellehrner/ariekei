//! Keeps the Inbox current: polls GitHub for the open PRs of every repo shown in a window
//! every minute (or right away when asked) and stores the result.
//!
//! GitHub's GraphQL budget (5000 points an hour) is shared with `gh` and the agents, so a
//! poll first asks which PRs exist and when each last changed (1 point per query: the
//! viewer's, assigned, review requested, reviewed), and fetches the details (reviews,
//! threads, checks: 1 point per PR) only of PRs that changed, have checks still running, or
//! are new; every tenth poll, and one the user asked for, fetches them all (check runs
//! restarted on an unchanged PR show up then). When the budget is used up anyway, polling
//! waits for GitHub's reset instead of failing every minute.

use crate::github::{self, PrSummary};
use crate::store::{PolledPr, Repo};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::Duration;
use tauri::{AppHandle, Manager};

/// Seconds between polls; `ARIEKEI_POLL_SECS` shortens it for tests.
fn interval() -> Duration {
    Duration::from_secs(
        std::env::var("ARIEKEI_POLL_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(60),
    )
}

pub struct Poller {
    wake: Sender<()>,
}

impl Poller {
    /// Starts polling; with `enabled` false (tests with fake repos) it never polls.
    pub fn start(app: AppHandle, enabled: bool) -> Self {
        let (wake, rx) = channel();
        if enabled {
            let _ = std::thread::Builder::new()
                .name("github-poller".into())
                .spawn(move || run(app, rx));
        }
        Self { wake }
    }

    /// A poller that never polls (tests drive `poll` themselves).
    #[cfg(test)]
    pub fn idle() -> Self {
        Self { wake: channel().0 }
    }

    /// Polls now instead of waiting for the next interval.
    pub fn refresh(&self) {
        let _ = self.wake.send(());
    }
}

/// Every this many polls, all details are fetched again (see the module docs).
const FULL_EVERY: u32 = 10;

fn run(app: AppHandle, wake: Receiver<()>) {
    // The poller starts while the app state is being built (it is part of it): wait for it,
    // or a fast thread would find no state yet and never poll.
    while app.try_state::<crate::AppState>().is_none() {
        std::thread::sleep(Duration::from_millis(20));
    }
    let mut count: u32 = 0;
    let mut asked = false;
    loop {
        let state = app.state::<crate::AppState>();
        let full = asked || count.is_multiple_of(FULL_EVERY);
        count = count.wrapping_add(1);
        let mut wait = interval();
        for id in state.windows.open_repos() {
            let Ok(Some(repo)) = state.store.repo(id) else {
                continue;
            };
            match poll(&state, &repo, full) {
                Ok(()) => state.events.inbox_changed(repo.id),
                Err(error) => {
                    let (message, until_reset) = poll_failure(error);
                    wait = wait.max(until_reset.unwrap_or_default());
                    tracing::warn!(
                        "GitHub poll of {}/{} failed: {message}",
                        repo.owner,
                        repo.name
                    );
                    state.events.inbox_error(repo.id, &message);
                }
            }
        }
        asked = match wake.recv_timeout(wait) {
            Ok(()) => true,
            Err(RecvTimeoutError::Timeout) => false,
            Err(RecvTimeoutError::Disconnected) => return,
        };
        // Coalesce a burst of refresh requests into one poll.
        while wake.try_recv().is_ok() {}
    }
}

/// What the Inbox says about a failed poll, and how long to wait for GitHub's rate limit
/// to reset when that was the cause.
fn poll_failure(error: String) -> (String, Option<Duration>) {
    match rate_limited(&error) {
        Some(until_reset) => (
            format!(
                "GitHub's API rate limit is used up (it is shared with gh and your agents); \
                 Ariekei polls again in {} min.",
                until_reset.as_secs().div_ceil(60)
            ),
            Some(until_reset),
        ),
        None => (error, None),
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// If a failed poll hit GitHub's rate limit: how long until it resets.
fn rate_limited(message: &str) -> Option<Duration> {
    // The time is taken once GitHub has answered: asking takes a while.
    rate_limit_wait(message, || {
        github::rate_limit_reset().map(|reset| reset.saturating_sub(now_secs()))
    })
}

/// How long to wait after `message` (a failed poll's error), given the seconds until GitHub
/// resets the used-up limit (`until_reset`, asked only for a primary limit): until the reset,
/// at least a minute; five minutes for a secondary limit, which GitHub does not announce a
/// reset for; `None` if it is no rate limit.
fn rate_limit_wait(message: &str, until_reset: impl FnOnce() -> Option<u64>) -> Option<Duration> {
    let lower = message.to_lowercase();
    if lower.contains("secondary rate limit") {
        return Some(Duration::from_secs(300));
    }
    if !lower.contains("rate limit") {
        return None;
    }
    Some(Duration::from_secs(until_reset().unwrap_or(0).max(60)))
}

/// How long after a PR changed it may still get its first checks (none yet right after a
/// push): until then a PR without checks is fetched on every poll.
const CHECKS_START_SECS: u64 = 15 * 60;

/// Whether the stored details of a PR are still current for `stamp`: nothing changed on it,
/// and its checks are not running or about to start (check results do not change a PR's
/// `updatedAt`). `now`: Unix seconds.
fn current(stored: &PrSummary, stamp: &github::PrStamp, now: u64) -> bool {
    let checks_settled = match stored.ci.as_deref() {
        Some("PENDING" | "EXPECTED") => false,
        Some(_) => true,
        None => rfc3339_secs(&stamp.updated_at)
            .is_none_or(|changed| now.saturating_sub(changed) > CHECKS_START_SECS),
    };
    stored.updated_at == stamp.updated_at && stored.state == stamp.state && checks_settled
}

/// Unix seconds of a GitHub timestamp (`2026-10-15T05:21:11Z`).
fn rfc3339_secs(t: &str) -> Option<u64> {
    let num = |range: std::ops::Range<usize>| t.get(range)?.parse::<i64>().ok();
    let (y, m, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (hh, mm, ss) = (num(11..13)?, num(14..16)?, num(17..19)?);
    // Days since 1970-01-01 (Howard Hinnant's days_from_civil).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    u64::try_from(days * 86_400 + hh * 3600 + mm * 60 + ss).ok()
}

/// One poll of `repo`: the viewer's PRs, PRs assigned to them, requesting their review or
/// reviewed by them, and the PRs they track (by number, so merged/closed ones get their final
/// state). `full`: fetch the
/// details of every PR, not only of those that changed.
pub fn poll(state: &crate::AppState, repo: &Repo, full: bool) -> Result<(), String> {
    let (o, n, me) = (&repo.owner, &repo.name, &repo.viewer);
    let before = state.store.items(repo.id)?;
    let mine = github::search_pr_stamps(o, n, &format!("author:{me}"))?;
    let assigned = github::search_pr_stamps(o, n, &format!("assignee:{me}"))?;
    let requested = github::search_pr_stamps(o, n, &format!("review-requested:{me}"))?;
    let reviewed = github::search_pr_stamps(o, n, &format!("reviewed-by:{me}"))?;
    let numbers = |found: &[github::PrStamp]| found.iter().map(|p| p.number).collect::<Vec<_>>();
    let found = Found {
        assigned: numbers(&assigned),
        requested: numbers(&requested),
        reviewed: numbers(&reviewed),
    };
    let known: Vec<i64> = [&mine, &assigned, &requested, &reviewed]
        .into_iter()
        .flat_map(|f| numbers(f))
        .collect();
    let tracked: Vec<i64> = state
        .store
        .tracked_numbers(repo.id, "pr")?
        .into_iter()
        .filter(|t| !known.contains(t))
        .collect();
    let others = github::pr_stamps(o, n, &tracked)?;

    let mut stamps: Vec<github::PrStamp> = vec![];
    for stamp in [mine, assigned, requested, reviewed, others]
        .into_iter()
        .flatten()
    {
        if !stamps.iter().any(|s| s.number == stamp.number) {
            stamps.push(stamp);
        }
    }
    let stored = |number: i64| {
        before
            .iter()
            .find(|i| i.kind == "pr" && i.number == number)
            .and_then(|i| serde_json::from_value::<PrSummary>(i.snapshot.clone()).ok())
    };
    let stale: Vec<i64> = stamps
        .iter()
        .filter(|s| full || !stored(s.number).is_some_and(|p| current(&p, s, now_secs())))
        .map(|s| s.number)
        .collect();
    let fetched = github::prs_by_number(o, n, me, &stale)?;
    let mut prs: Vec<(PrSummary, &'static str)> = vec![];
    for stamp in &stamps {
        let pr = fetched
            .iter()
            .find(|p| p.number == stamp.number)
            .cloned()
            .or_else(|| stored(stamp.number));
        if let Some(pr) = pr {
            let role = role_of(&pr, me, &found);
            prs.push((pr, role));
        }
    }

    // Issues: the ones assigned to the viewer, plus tracked ones (e.g. with a worktree).
    let assigned = github::assigned_issues(o, n, me)?;
    let assigned_numbers: Vec<i64> = assigned.iter().map(|i| i.number).collect();
    let tracked_issues: Vec<i64> = state
        .store
        .tracked_numbers(repo.id, "issue")?
        .into_iter()
        .filter(|t| !assigned_numbers.contains(t))
        .collect();
    let issues: Vec<_> = assigned
        .into_iter()
        .chain(github::issues_by_number(o, n, me, &tracked_issues)?)
        .collect();

    let polled: Vec<PolledPr> = prs
        .iter()
        .map(|(p, role)| PolledPr {
            number: p.number,
            kind: "pr",
            role,
            open: p.state == "OPEN",
            snapshot: serde_json::to_value(p).unwrap_or_default(),
        })
        .chain(issues.iter().map(|i| PolledPr {
            number: i.number,
            kind: "issue",
            role: if assigned_numbers.contains(&i.number) {
                "assignee"
            } else {
                "other"
            },
            open: i.state == "OPEN",
            snapshot: serde_json::to_value(i).unwrap_or_default(),
        }))
        .collect();
    state.store.sync_prs(repo.id, &polled)?;

    for item in state.store.items(repo.id)? {
        let old = before.iter().find(|b| b.id == item.id);
        if item.kind == "issue" {
            if old.is_none()
                && let Some(i) = issues.iter().find(|i| i.number == item.number)
            {
                state
                    .store
                    .set_last_seen(item.id, i.latest_activity.as_deref().unwrap_or(""))?;
            }
            continue;
        }
        let Some((pr, _)) = prs.iter().find(|(p, _)| p.number == item.number) else {
            continue;
        };
        match old.and_then(|o| serde_json::from_value::<PrSummary>(o.snapshot.clone()).ok()) {
            Some(previous) => crate::notify::pr_changed(state, &item.id.to_string(), &previous, pr),
            // First sight of this PR: what is there now counts as seen, so only later
            // activity shows up as new.
            None => state
                .store
                .set_last_seen(item.id, pr.latest_activity.as_deref().unwrap_or(""))?,
        }
    }
    crate::lifecycle::after_poll(state, repo)?;
    Ok(())
}

/// The PRs a poll's searches found, by number (besides the viewer's own).
#[derive(Debug, Default)]
pub struct Found {
    pub assigned: Vec<i64>,
    pub requested: Vec<i64>,
    pub reviewed: Vec<i64>,
}

/// Why a PR is in the Inbox, the strongest reason first: assigned to the viewer, written by
/// them, review requested from them (again, after a review), reviewed by them, or none (they
/// added it).
pub fn role_of(pr: &PrSummary, viewer: &str, found: &Found) -> &'static str {
    if found.assigned.contains(&pr.number) {
        "assignee"
    } else if pr.author.eq_ignore_ascii_case(viewer) {
        "author"
    } else if found.requested.contains(&pr.number) {
        "requested"
    } else if found.reviewed.contains(&pr.number) {
        "reviewed"
    } else {
        "other"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn details_are_fetched_again_only_when_the_pr_changed_or_its_checks_run() {
        let stored: PrSummary = serde_json::from_value(serde_json::json!({
            "number": 1, "title": "t", "url": "", "isDraft": false, "updatedAt": "2026-10-01T00:00:00Z",
            "headRef": "", "headSha": "", "headOwner": "", "headRepo": "", "headSshUrl": "",
            "headHttpsUrl": "", "baseRef": "", "reviewDecision": null, "ci": "SUCCESS",
            "failingChecks": [], "unresolvedThreads": 0, "latestActivity": null, "reviews": []
        }))
        .unwrap();
        let stamp = github::PrStamp {
            number: 1,
            title: "t".into(),
            author: "me".into(),
            is_draft: false,
            state: "OPEN".into(),
            updated_at: "2026-10-01T00:00:00Z".into(),
        };
        let changed = rfc3339_secs("2026-10-01T00:00:00Z").unwrap();
        let later = changed + 3600;
        assert!(current(&stored, &stamp, later));
        let updated = github::PrStamp {
            updated_at: "2026-10-02T00:00:00Z".into(),
            ..stamp.clone()
        };
        assert!(!current(&stored, &updated, later));
        let merged = github::PrStamp {
            state: "MERGED".into(),
            ..stamp.clone()
        };
        assert!(!current(&stored, &merged, later));
        for state in ["PENDING", "EXPECTED"] {
            let running = PrSummary {
                ci: Some(state.into()),
                ..stored.clone()
            };
            assert!(!current(&running, &stamp, later), "{state}");
        }
        // No checks yet: right after a change they may still start, later there are none.
        let no_checks = PrSummary {
            ci: None,
            ..stored.clone()
        };
        assert!(!current(&no_checks, &stamp, changed + 60));
        assert!(current(&no_checks, &stamp, changed + CHECKS_START_SECS + 1));
    }

    #[test]
    fn github_timestamps_are_read_as_unix_seconds() {
        assert_eq!(rfc3339_secs("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(rfc3339_secs("2026-10-15T05:21:11Z"), Some(1_792_041_671));
        assert_eq!(rfc3339_secs("2024-02-29T12:00:00Z"), Some(1_709_208_000));
        assert_eq!(rfc3339_secs("garbage"), None);
    }

    #[test]
    fn a_used_up_rate_limit_waits_for_its_reset() {
        let never = || -> Option<u64> { panic!("only a primary limit asks for the reset") };
        assert_eq!(
            rate_limit_wait(
                "gh api failed: API rate limit exceeded for user ID 1.",
                || Some(600)
            ),
            Some(Duration::from_secs(600))
        );
        // A reset that is due or unknown still waits a minute.
        assert_eq!(
            rate_limit_wait("gh: API rate limit exceeded", || None),
            Some(Duration::from_secs(60))
        );
        assert_eq!(
            rate_limit_wait(
                "gh api failed: You have exceeded a secondary rate limit.",
                never
            ),
            Some(Duration::from_secs(300))
        );
        assert_eq!(
            rate_limit_wait("gh api failed: Could not resolve host", never),
            None
        );
    }

    mod with_app {
        use super::super::*;
        use crate::testkit::TestApp;
        use serde_json::json;

        /// The PR numbers a poll fetched the details of.
        fn detailed(app: &TestApp) -> Vec<i64> {
            let mut numbers: Vec<i64> = app
                .take_calls()
                .iter()
                .filter(|c| c["details"] == true)
                .flat_map(|c| c["numbers"].as_array().cloned().unwrap_or_default())
                .filter_map(|n| n.as_i64())
                .collect();
            numbers.sort();
            numbers
        }

        #[test]
        fn a_poll_fetches_details_only_of_prs_that_changed() {
            let app = TestApp::new();
            app.add_pr(json!({"number": 7, "title": "Seven", "head_ref": "main"}));
            app.add_pr(json!({"number": 8, "title": "Eight", "head_ref": "main"}));
            app.poll().unwrap();
            assert_eq!(detailed(&app), [7, 8], "new PRs");
            app.poll().unwrap();
            assert_eq!(detailed(&app), Vec::<i64>::new(), "nothing changed");
            app.set("prs", 8, json!({"title": "Eight, renamed"}));
            app.poll().unwrap();
            assert_eq!(detailed(&app), [8]);
            assert_eq!(app.row(8).unwrap().title, "Eight, renamed");
            // A full poll fetches all.
            poll(&app.state, &app.repo, true).unwrap();
            assert_eq!(detailed(&app), [7, 8]);
        }

        #[test]
        fn running_checks_are_followed_until_they_finish() {
            let app = TestApp::new();
            app.add_pr(json!({"number": 7, "title": "Seven", "head_ref": "main"}));
            app.poll().unwrap();
            // A push: the PR changed, its checks run.
            app.set("prs", 7, json!({"title": "Seven, pushed", "ci": "PENDING"}));
            app.poll().unwrap();
            assert_eq!(app.row(7).unwrap().ci.as_deref(), Some("PENDING"));
            detailed(&app);
            // The checks fail; the PR itself does not change (GitHub keeps its updatedAt).
            app.set("prs", 7, json!({"ci": "FAILURE", "failing": "unit-tests"}));
            app.poll().unwrap();
            assert_eq!(detailed(&app), [7]);
            let row = app.row(7).unwrap();
            assert_eq!(
                (row.ci.as_deref(), row.attention),
                (Some("FAILURE"), Some("CI failing"))
            );
            assert!(
                app.events
                    .notices()
                    .iter()
                    .any(|(kind, _, body)| kind == "ci-failing" && body.contains("unit-tests"))
            );
            // Finished: no longer followed.
            app.poll().unwrap();
            assert_eq!(detailed(&app), Vec::<i64>::new());
        }

        #[test]
        fn an_item_gone_from_github_does_not_stop_polling() {
            let app = TestApp::new();
            app.add_issue(json!({"number": 6, "title": "Old idea", "assigned": true}));
            app.add_pr(json!({"number": 7, "title": "Seven", "head_ref": "main"}));
            app.poll().unwrap();
            crate::workitem::mark_seen(&app.state, app.item(6).id).unwrap();
            app.remove("issues", 6);
            app.set("prs", 7, json!({"title": "Seven, renamed"}));
            app.poll().unwrap();
            assert!(app.row(6).is_none());
            assert_eq!(app.row(7).unwrap().title, "Seven, renamed");
        }

        #[test]
        fn a_used_up_rate_limit_says_when_polling_resumes() {
            let app = TestApp::new();
            app.rate_limited(true);
            let error = app.poll().unwrap_err();
            let (message, wait) = poll_failure(error);
            assert!(
                message.contains("rate limit is used up")
                    && message.contains("polls again in 2 min"),
                "{message}"
            );
            assert!(
                wait.is_some_and(|w| w > Duration::from_secs(60) && w <= Duration::from_secs(120)),
                "{wait:?}"
            );
            app.rate_limited(false);
            app.poll().unwrap();
            let (message, wait) = poll_failure("gh: Could not resolve host".into());
            assert_eq!(
                (message.as_str(), wait),
                ("gh: Could not resolve host", None)
            );
        }
    }
}
