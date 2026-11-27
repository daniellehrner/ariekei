// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

//! The impact map: how an item's change affects the rest of the code, beyond its diff. The
//! item's agent makes it with Ariekei's tools (`mcp`) in its worktree, where it has the whole
//! repository: an overview of what the change means for the project as a whole, the code
//! outside the diff it reaches ("affected"), code elsewhere that does the same ("similar"),
//! and the way from an entry point to the changed code ("path"). Every place it names is
//! checked against the worktree's HEAD and quoted from there. The Impact tab shows the map
//! and the code at each place, also of files the diff does not touch.

use crate::AppState;
use crate::drafts::{int, string};
use crate::store::{ImpactEntry, ImpactLocation, ImpactMap};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const KINDS: [&str; 3] = ["affected", "similar", "path"];
/// Places one entry may have.
const MAX_LOCATIONS: usize = 40;
/// Lines of a place quoted for the agent's listing and the tab's list (the code pane shows
/// the whole file).
const QUOTED_LINES: usize = 40;

/// What every agent is told about the tools (MCP `instructions`, after the drafts').
pub const INSTRUCTIONS: &str = "When the user asks you to map a change (how it affects the rest \
of the code), record the map with set_change_overview and add_impact: they read it in Ariekei's \
Impact tab, beside the diff, with the code of every place you name.";

/// The tools (MCP `tools/list`).
pub fn tools() -> Value {
    let place = json!({
        "type": "object",
        "properties": {
            "path": {"type": "string", "description": "From the repository's root."},
            "line": {"type": "integer", "minimum": 1, "description": "The last line."},
            "start_line": {"type": "integer", "minimum": 1, "description": "The first line, when it is several."},
            "note": {"type": "string", "description": "What these lines are and how the change bears on them, in a few words."}
        },
        "required": ["path", "line"]
    });
    let changed = json!({
        "type": "object",
        "description": "The lines of the change this entry follows from: the diff links to the entry there.",
        "properties": {
            "path": {"type": "string"},
            "line": {"type": "integer", "minimum": 1},
            "start_line": {"type": "integer", "minimum": 1}
        },
        "required": ["path", "line"]
    });
    let lines = "Line numbers are those of the files as committed at the worktree's HEAD; Ariekei \
checks every place and quotes it from there.";
    let entry = |required: &[&str]| {
        json!({
            "type": "object",
            "properties": {
                "kind": {"type": "string", "enum": KINDS, "description": "\"affected\": code outside the diff that the change reaches. \"similar\": code elsewhere that does the same thing. \"path\": the way from an entry point to the changed code, a place per step, in order."},
                "title": {"type": "string", "description": "One line: what is affected and how, what is similar, or which way it is."},
                "body": {"type": "string", "description": "Markdown, short: how these places are affected or differ, and what the reviewer should check."},
                "attention": {"type": "boolean", "description": "Worth the reviewer's attention: behaviour may break here, or the change may have missed this place."},
                "changed": changed,
                "locations": {"type": "array", "items": place, "minItems": 1, "maxItems": MAX_LOCATIONS}
            },
            "required": required
        })
    };
    let mut update = entry(&["id"]);
    update["properties"]["id"] = json!({"type": "integer"});
    update["properties"]["changed"] = json!({"description": "As for add_impact; null removes it."});
    json!([
        {
            "name": "set_change_overview",
            "description": "Sets the overview of the item's impact map: what the change means \
    for the project as a whole. Replaces the previous overview.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "overview": {"type": "string", "description": "Markdown: a few short paragraphs or bullets."}
                },
                "required": ["overview"]
            }
        },
        {
            "name": "add_impact",
            "description": format!("Adds an entry to the item's impact map, which the user \
    reads in Ariekei's Impact tab. {lines}"),
            "inputSchema": entry(&["kind", "title", "locations"])
        },
        {
            "name": "list_impact",
            "description": "Lists the item's impact map: the overview and every entry with its id.",
            "inputSchema": {"type": "object", "properties": {}}
        },
        {
            "name": "update_impact",
            "description": format!("Changes an entry of the impact map: only the fields \
    given. {lines}"),
            "inputSchema": update
        },
        {
            "name": "delete_impact",
            "description": "Deletes an entry of the impact map.",
            "inputSchema": {
                "type": "object",
                "properties": {"id": {"type": "integer"}},
                "required": ["id"]
            }
        },
        {
            "name": "clear_impact",
            "description": "Deletes the whole impact map, to map the change again.",
            "inputSchema": {"type": "object", "properties": {}}
        }
    ])
}

/// Whether tool `name` is one of these.
pub fn handles(name: &str) -> bool {
    matches!(
        name,
        "set_change_overview"
            | "add_impact"
            | "list_impact"
            | "update_impact"
            | "delete_impact"
            | "clear_impact"
    )
}

/// Runs tool `name` with `args` for the agent in terminal `terminal`: the answer for the agent,
/// or why it failed (also for the agent).
pub fn call(
    state: &AppState,
    terminal: Option<&str>,
    name: &str,
    args: &Value,
) -> Result<String, String> {
    let (id, item) = crate::drafts::agent_item(state, terminal)?;
    if !item.worktree_path.as_ref().is_some_and(|p| p.exists()) {
        return Err("A change can be mapped only in its work item's worktree".into());
    }
    let (dir, base, head) = crate::workitem::review_range(state, id)?;
    let mut at = Head {
        dir,
        base,
        head,
        files: HashMap::new(),
        hunks: HashMap::new(),
    };
    let store = &state.store;
    let answer = match name {
        "set_change_overview" => {
            let overview = string(args, "overview")?;
            store.set_impact_rev(id, &at.head, &at.base, Some(overview.trim()))?;
            format!("Set the overview of the map of {}.", short(&at.head))
        }
        "add_impact" => {
            let entry = entry(&mut at, args, None)?;
            let n = store.add_impact_entry(id, &entry)?;
            store.set_impact_rev(id, &at.head, &at.base, None)?;
            format!("Added #{n}: {}.{}", counted(&entry), notes(&entry))
        }
        "list_impact" => return Ok(listed(store.impact(id)?)),
        "update_impact" => {
            let n = i64::from(int(args, "id")?.ok_or("`id` is missing")?);
            let current = store
                .impact(id)?
                .and_then(|m| m.entries.into_iter().find(|e| e.id == n))
                .ok_or(format!("There is no entry #{n}: list_impact lists them"))?;
            let entry = entry(&mut at, args, Some(current))?;
            store.update_impact_entry(id, &entry)?;
            store.set_impact_rev(id, &at.head, &at.base, None)?;
            format!("Changed #{n}: {}.{}", counted(&entry), notes(&entry))
        }
        "delete_impact" => {
            let n = i64::from(int(args, "id")?.ok_or("`id` is missing")?);
            if !store.delete_impact_entry(id, n)? {
                return Err(format!("There is no entry #{n}"));
            }
            format!("Deleted #{n}.")
        }
        "clear_impact" => {
            store.clear_impact(id)?;
            "Deleted the map.".into()
        }
        _ => return Err(format!("There is no tool {name}")),
    };
    state.events.impact_changed(id);
    Ok(answer)
}

/// The commit a tool call checks places against, with what it read of it.
struct Head {
    dir: PathBuf,
    base: String,
    head: String,
    /// Each file's lines at `head` (`None`: not there).
    files: HashMap<String, Option<Vec<String>>>,
    /// Each file's hunks from `base` to `head`.
    hunks: HashMap<String, Vec<crate::git::Hunk>>,
}

impl Head {
    fn lines(&mut self, path: &str) -> Option<&[String]> {
        let (dir, head) = (&self.dir, &self.head);
        self.files
            .entry(path.to_owned())
            .or_insert_with(|| {
                crate::git::git(dir, &["show", &format!("{head}:{path}")])
                    .ok()
                    .map(|bytes| {
                        String::from_utf8_lossy(&bytes)
                            .lines()
                            .map(str::to_owned)
                            .collect()
                    })
            })
            .as_deref()
    }

    /// The change touches lines `from..=to` of `path`: it changed one of them, or deleted
    /// lines between two of them.
    fn in_diff(&mut self, path: &str, from: u32, to: u32) -> bool {
        let (dir, base, head) = (&self.dir, &self.base, &self.head);
        let hunks = self.hunks.entry(path.to_owned()).or_insert_with(|| {
            // A renamed file's changes are those of the rename, as the review shows them.
            let renames = crate::git::renames(dir, base, head).unwrap_or_default();
            let mut args = vec!["diff", "-U0", "--no-color", "--no-ext-diff", "-M"];
            args.extend([base.as_str(), head.as_str(), "--", path]);
            args.extend(renames.get(path).map(String::as_str));
            crate::git::git_str(dir, &args)
                .map(|diff| crate::git::parse_hunks(&diff))
                .unwrap_or_default()
        });
        hunks.iter().any(|h| match h.new_lines {
            // Deleted after line `new_start`.
            0 => from <= h.new_start && h.new_start < to,
            n => from < h.new_start + n && h.new_start <= to,
        })
    }

    /// The place `args` names, checked and quoted.
    fn place(&mut self, args: &Value) -> Result<ImpactLocation, String> {
        let path = string(args, "path")?;
        let path = path.trim().trim_start_matches("./").to_owned();
        let to = int(args, "line")?.ok_or("`line` is missing")?;
        let from = int(args, "start_line")?.unwrap_or(to);
        let head = short(&self.head).to_owned();
        let lines = self.lines(&path).ok_or(format!(
            "{path} is not in the worktree's HEAD ({head}): give its path from the repository's root, as committed"
        ))?;
        if from > to || to as usize > lines.len() {
            return Err(format!(
                "{path} has {} lines at HEAD ({head}): there are no lines {from}–{to}",
                lines.len()
            ));
        }
        let quoted = (to - from + 1).min(QUOTED_LINES as u32);
        let code = lines[from as usize - 1..(from + quoted - 1) as usize].join("\n");
        let note = match args.get("note") {
            Some(Value::String(s)) => s.trim().to_owned(),
            None | Some(Value::Null) => String::new(),
            Some(_) => return Err("`note` must be a text".into()),
        };
        Ok(ImpactLocation {
            in_diff: self.in_diff(&path, from, to),
            path,
            from_line: from,
            to_line: to,
            code,
            note,
        })
    }
}

/// The entry `args` describe: a new one, or `current` with the fields given changed.
fn entry(at: &mut Head, args: &Value, current: Option<ImpactEntry>) -> Result<ImpactEntry, String> {
    let given = |key: &str| args.get(key).is_some();
    let mut e = current.unwrap_or(ImpactEntry {
        id: 0,
        kind: String::new(),
        title: String::new(),
        body: String::new(),
        attention: false,
        changed: None,
        locations: vec![],
        rev: String::new(),
    });
    if given("kind") || e.kind.is_empty() {
        let kind = string(args, "kind")?;
        if !KINDS.contains(&kind.as_str()) {
            return Err(format!("`kind` is one of {}", KINDS.join(", ")));
        }
        e.kind = kind;
    }
    if given("title") || e.title.is_empty() {
        e.title = string(args, "title")?.trim().to_owned();
    }
    match args.get("body") {
        Some(Value::String(s)) => e.body = s.trim().to_owned(),
        None | Some(Value::Null) => {}
        Some(_) => return Err("`body` must be a text".into()),
    }
    match args.get("attention") {
        Some(Value::Bool(b)) => e.attention = *b,
        None | Some(Value::Null) => {}
        Some(_) => return Err("`attention` must be true or false".into()),
    }
    match args.get("changed") {
        Some(Value::Null) => e.changed = None,
        Some(changed @ Value::Object(_)) => {
            e.changed = Some(at.place(changed).map_err(|m| format!("`changed`: {m}"))?);
        }
        None => {}
        Some(_) => return Err("`changed` must be an object with `path` and `line`".into()),
    }
    match args.get("locations") {
        Some(Value::Array(list)) if !list.is_empty() && list.len() <= MAX_LOCATIONS => {
            e.locations = list
                .iter()
                .enumerate()
                .map(|(i, l)| at.place(l).map_err(|m| format!("locations[{i}]: {m}")))
                .collect::<Result<_, _>>()?;
            e.rev = at.head.clone();
        }
        None if !e.locations.is_empty() => {}
        _ => {
            return Err(format!(
                "`locations` must list 1 to {MAX_LOCATIONS} places, each with `path` and `line`"
            ));
        }
    }
    if e.rev.is_empty() {
        e.rev = at.head.clone();
    }
    Ok(e)
}

/// "affected, 3 places, 1 of them in the diff".
fn counted(e: &ImpactEntry) -> String {
    let n = e.locations.len();
    let in_diff = e.locations.iter().filter(|l| l.in_diff).count();
    let mut s = format!("{}, {n} place{}", e.kind, if n == 1 { "" } else { "s" });
    if in_diff > 0 {
        s.push_str(&format!(", {in_diff} of them in the diff"));
    }
    s
}

/// What the agent should know about an entry it just wrote.
fn notes(e: &ImpactEntry) -> String {
    match &e.changed {
        Some(c) if !c.in_diff => format!(
            " Note: the change does not touch {}, so the diff shows no link to this entry.",
            place(c)
        ),
        _ => String::new(),
    }
}

fn place(l: &ImpactLocation) -> String {
    if l.from_line == l.to_line {
        format!("{}:{}", l.path, l.to_line)
    } else {
        format!("{}:{}–{}", l.path, l.from_line, l.to_line)
    }
}

fn short(rev: &str) -> &str {
    &rev[..rev.len().min(8)]
}

/// The map as the agent reads it (`list_impact`).
fn listed(map: Option<ImpactMap>) -> String {
    let Some(map) = map else {
        return "There is no map yet.".into();
    };
    let mut out = format!("Map of {} (base {}).", short(&map.rev), short(&map.base));
    if !map.overview.is_empty() {
        out.push_str(&format!("\n\nOverview:\n{}", map.overview));
    }
    for e in &map.entries {
        out.push_str(&format!("\n\n#{} {}: {}", e.id, e.kind, e.title));
        if e.attention {
            out.push_str(" (attention)");
        }
        if e.rev != map.rev {
            out.push_str(&format!(" (places at {})", short(&e.rev)));
        }
        if !e.body.is_empty() {
            out.push_str(&format!("\n{}", e.body));
        }
        if let Some(c) = &e.changed {
            out.push_str(&format!("\nFollows from {}", place(c)));
        }
        for l in &e.locations {
            out.push_str(&format!("\n- {}", place(l)));
            if !l.note.is_empty() {
                out.push_str(&format!(": {}", l.note));
            }
        }
    }
    out
}

/// The Impact tab's map, and how it stands to the head the review shows now.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Shown {
    pub map: Option<ImpactMap>,
    /// The head now (the worktree's HEAD, or the fetched PR head).
    pub head: String,
    /// Commits the head has on top of the map's.
    pub newer: u32,
    /// The map's commit is not in the head's history any more (rebased, or rewritten).
    pub rewritten: bool,
}

pub fn shown(state: &AppState, id: i64) -> Result<Shown, String> {
    let map = state.store.impact(id)?;
    let Ok((dir, _, head)) = crate::workitem::review_range(state, id) else {
        // Nothing to compare with (e.g. the worktree is gone): the map as it was.
        return Ok(Shown {
            head: map.as_ref().map(|m| m.rev.clone()).unwrap_or_default(),
            map,
            newer: 0,
            rewritten: false,
        });
    };
    let (newer, rewritten) = match &map {
        Some(m) if m.rev != head => commits_since(&dir, &m.rev, &head),
        _ => (0, false),
    };
    Ok(Shown {
        map,
        head,
        newer,
        rewritten,
    })
}

/// How many commits `head` has on top of `rev`, and whether `rev` is not in its history.
fn commits_since(dir: &Path, rev: &str, head: &str) -> (u32, bool) {
    if !crate::git::is_ancestor(dir, rev, head) {
        return (0, true);
    }
    let n = crate::git::git_str(dir, &["rev-list", "--count", &format!("{rev}..{head}")])
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(0);
    (n, false)
}

/// The file `path` as committed at `rev`, for the Impact tab's code pane.
pub fn file_at(state: &AppState, id: i64, rev: &str, path: &str) -> Result<String, String> {
    if rev.len() < 7 || !rev.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("{rev} is not a commit"));
    }
    let (dir, _, _) = crate::workitem::review_range(state, id)?;
    let bytes = crate::git::git(&dir, &["show", &format!("{rev}:{path}")])
        .map_err(|_| format!("{path} is not in {}", short(rev)))?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// What the agent of item `id` is asked to map its change with: anew, or (`update`) again
/// after new commits.
pub fn request(state: &AppState, id: i64, update: bool) -> Result<String, String> {
    let (dir, base, head) = crate::workitem::review_range(state, id)?;
    let commits = crate::git::git_str(&dir, &["rev-list", "--count", &format!("{base}..{head}")])
        .unwrap_or_default();
    let map = state.store.impact(id)?;
    let start = match (&map, update) {
        (Some(m), true) if m.rev != head => {
            let (_, rewritten) = commits_since(&dir, &m.rev, &head);
            let since = if rewritten {
                format!(
                    "the branch was rewritten since the map of {}",
                    short(&m.rev)
                )
            } else {
                format!("`git log {}..HEAD`", short(&m.rev))
            };
            format!(
                "The change has new commits since you mapped it ({since}). Look at the map with \
list_impact and at what the new commits change, then call clear_impact and map the change \
again for the user's review, keeping what still holds with its places at HEAD."
            )
        }
        (Some(_), _) => "Call clear_impact, then map this change for the user's review.".into(),
        (None, _) => "Map this change for the user's review.".into(),
    };
    Ok(format!(
        "{start} Use Ariekei's impact tools: the user reads the map in Ariekei's Impact tab, \
beside the diff, to see how the change affects the rest of the code. The change is \
`git diff {base8}...HEAD` ({commits} commits). Read it first, then explore the rest of the \
repository at HEAD (git grep or rg) and read the code you cite.

1. set_change_overview: what the change means for the project as a whole, in a few short \
paragraphs or bullets: behaviour before and after, who notices it (APIs, protocols, command-line \
options, configuration, stored data and formats), the modules and subsystems involved, and the \
real risks (correctness, compatibility, performance, concurrency, security). Leave out what the \
diff already says plainly.
2. add_impact, kind \"affected\": code outside the diff that the change reaches: callers and \
users of changed functions, types and constants; implementations and overrides of changed \
interfaces and base classes; code that registers, configures, serializes or reflects on them; \
and the entry points that lead there. One entry per way of being affected, with all its places, \
saying how they are affected. Set attention where behaviour may break, or a place looks missed.
3. kind \"similar\": code elsewhere that does the same thing: parallel implementations (other \
versions, variants, sibling classes), duplicated logic, an existing helper the change could have \
used; say whether each needs the same change. Set attention where it does and the change leaves \
it out.
4. kind \"path\": for each main behaviour change, the way from an entry point (an API call, a \
command, a message, a scheduled task) to the changed code, a place per step, in order.

Give `changed` (the changed lines an entry follows from) where there is one: the diff links to \
the entry there. Cite only lines you have read at HEAD. A few entries that matter beat complete \
lists: leave out uses the change does not affect. Do not change any files, and do not draft \
review comments unless the user asks.",
        base8 = short(&base),
    ))
}

/// What a new agent session on item `id` starts with, if anything: on a PR someone else wrote,
/// mapping its change (again, when the map is of an older commit).
pub fn first_request(state: &AppState, id: i64) -> Option<String> {
    let (_, _, head) = crate::workitem::review_range(state, id).ok()?;
    let update = match state.store.impact(id).ok()? {
        Some(m) if m.rev == head => return None,
        Some(_) => true,
        None => false,
    };
    request(state, id, update)
        .inspect_err(|e| tracing::warn!("item {id}: no map request: {e}"))
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::{TestApp, commit, write};

    fn lines(n: u32) -> String {
        (1..=n).map(|i| format!("line {i}\n")).collect()
    }

    #[test]
    fn the_agent_maps_places_it_names_at_its_head() {
        let app = TestApp::new();
        app.push_to(
            "main",
            &[("lib.txt", &lines(30)), ("other.txt", &lines(5))],
            "me",
        );
        let (id, wt) = app.pr_with_worktree(5, "feature");
        write(
            &wt.join("lib.txt"),
            &lines(30).replace("line 20\n", "line twenty\n"),
        );
        commit(&wt, "Change line 20", "me");
        let head = crate::git::head(&wt).unwrap();
        app.start_agent(id);
        let token = app.state.terminals.token(&id.to_string()).unwrap();
        let call = |name, args| call(&app.state, Some(&token), name, &args);
        app.events.take();

        call(
            "set_change_overview",
            json!({"overview": "Line 20 is spelled out."}),
        )
        .unwrap();
        let answer = call(
            "add_impact",
            json!({
                "kind": "affected", "title": "Readers of line 20", "attention": true,
                "changed": {"path": "lib.txt", "line": 20},
                "locations": [
                    {"path": "./other.txt", "start_line": 2, "line": 3, "note": "reads it"},
                    {"path": "lib.txt", "start_line": 19, "line": 21}
                ]
            }),
        )
        .unwrap();
        assert!(
            answer.starts_with("Added #")
                && answer.ends_with("affected, 2 places, 1 of them in the diff."),
            "{answer}"
        );
        assert!(
            app.events
                .take()
                .contains(&crate::events::Sent::ImpactChanged(id))
        );
        let map = app.state.store.impact(id).unwrap().unwrap();
        assert_eq!(
            (map.rev.as_str(), map.overview.as_str()),
            (head.as_str(), "Line 20 is spelled out.")
        );
        let e = &map.entries[0];
        assert_eq!(
            e.locations
                .iter()
                .map(|l| (
                    l.path.as_str(),
                    l.from_line,
                    l.to_line,
                    l.code.as_str(),
                    l.note.as_str(),
                    l.in_diff
                ))
                .collect::<Vec<_>>(),
            [
                ("other.txt", 2, 3, "line 2\nline 3", "reads it", false),
                ("lib.txt", 19, 21, "line 19\nline twenty\nline 21", "", true)
            ]
        );
        assert!(e.attention && e.changed.as_ref().is_some_and(|c| c.in_diff));

        // A place that is not there is refused, and named.
        let wrong = call(
            "add_impact",
            json!({"kind": "similar", "title": "x", "locations": [
                {"path": "other.txt", "line": 1}, {"path": "other.txt", "line": 6}
            ]}),
        )
        .unwrap_err();
        assert!(
            wrong.starts_with("locations[1]: other.txt has 5 lines"),
            "{wrong}"
        );
        assert!(call("add_impact", json!({"kind": "similar", "title": "x", "locations": [{"path": "nope.txt", "line": 1}]})).is_err());
        assert!(call("add_impact", json!({"kind": "guess", "title": "x", "locations": [{"path": "lib.txt", "line": 1}]})).is_err());
        assert!(
            call(
                "add_impact",
                json!({"kind": "similar", "title": "x", "locations": []})
            )
            .is_err()
        );

        // `changed` on lines the change does not touch: said, kept.
        let unlinked = call(
            "add_impact",
            json!({"kind": "path", "title": "From the top", "changed": {"path": "lib.txt", "line": 1},
                   "locations": [{"path": "lib.txt", "line": 1}, {"path": "lib.txt", "line": 20}]}),
        )
        .unwrap();
        assert!(unlinked.contains("the diff shows no link"), "{unlinked}");

        // Updates change only what is given.
        let first = e.id;
        call(
            "update_impact",
            json!({"id": first, "body": "Both read it.", "changed": null}),
        )
        .unwrap();
        let e = &app.state.store.impact(id).unwrap().unwrap().entries[0];
        assert_eq!(
            (
                e.title.as_str(),
                e.body.as_str(),
                e.changed.is_none(),
                e.locations.len()
            ),
            ("Readers of line 20", "Both read it.", true, 2)
        );
        let listed = call("list_impact", json!({})).unwrap();
        assert!(
            listed.contains(&format!(
                "#{first} affected: Readers of line 20 (attention)\nBoth read it.\n- other.txt:2–3: reads it\n- lib.txt:19–21"
            )),
            "{listed}"
        );

        call("delete_impact", json!({"id": first})).unwrap();
        assert!(call("delete_impact", json!({"id": first})).is_err());
        assert_eq!(
            app.state.store.impact(id).unwrap().unwrap().entries.len(),
            1
        );
        call("clear_impact", json!({})).unwrap();
        assert!(app.state.store.impact(id).unwrap().is_none());
        assert_eq!(
            call("list_impact", json!({})).unwrap(),
            "There is no map yet."
        );
    }

    #[test]
    fn a_new_agent_on_someone_elses_pr_maps_its_change_first() {
        let app = TestApp::new();
        let plan = |id| crate::workitem::session_plan(&app.state, &app.state.data_dir, id).unwrap();
        // The user's own PR: the agent waits for them.
        let (own, _) = app.pr_with_worktree(5, "mine");
        assert_eq!(plan(own).prompt, None);

        app.push_to("theirs", &[("c.txt", "carol's\n")], "carol");
        app.add_pr(
            json!({"number": 6, "title": "Theirs", "head_ref": "theirs", "author": "carol", "requested": true}),
        );
        app.poll().unwrap();
        let id = app.item(6).id;
        crate::workitem::mark_seen(&app.state, id).unwrap();
        let wt = crate::workitem::prepare(&app.state, id).unwrap().path;
        let first = plan(id).prompt.unwrap();
        assert!(
            first.starts_with("Map this change for the user's review."),
            "{first}"
        );

        // Mapped at its head: nothing to do; a commit later: mapped again.
        let head = crate::git::head(&wt).unwrap();
        app.state
            .store
            .set_impact_rev(id, &head, "base", Some("x"))
            .unwrap();
        assert_eq!(plan(id).prompt, None);
        write(&wt.join("c.txt"), "more\n");
        commit(&wt, "More", "carol");
        assert!(
            plan(id)
                .prompt
                .unwrap()
                .contains("new commits since you mapped it")
        );
    }

    #[test]
    fn the_map_says_how_far_behind_the_head_it_is() {
        let app = TestApp::new();
        let (id, wt) = app.pr_with_worktree(5, "feature");
        app.start_agent(id);
        let token = app.state.terminals.token(&id.to_string()).unwrap();
        assert!(shown(&app.state, id).unwrap().map.is_none());
        call(
            &app.state,
            Some(&token),
            "set_change_overview",
            &json!({"overview": "x"}),
        )
        .unwrap();
        let s = shown(&app.state, id).unwrap();
        assert_eq!((s.newer, s.rewritten), (0, false));
        assert_eq!(s.head, s.map.unwrap().rev);

        write(&wt.join("n.txt"), "n\n");
        commit(&wt, "One more", "me");
        let s = shown(&app.state, id).unwrap();
        assert_eq!((s.newer, s.rewritten), (1, false));
        assert!(
            request(&app.state, id, true)
                .unwrap()
                .contains("new commits since you mapped it")
        );

        crate::testkit::git(&wt, &["reset", "-q", "--hard", "HEAD~2"]);
        assert!(shown(&app.state, id).unwrap().rewritten);

        // The code pane reads any file at any commit of the clone.
        let head = crate::git::head(&wt).unwrap();
        assert_eq!(
            file_at(&app.state, id, &head, "a.txt").unwrap(),
            "one\ntwo\nthree\n"
        );
        assert!(file_at(&app.state, id, &head, "b.txt").is_err());
        assert!(file_at(&app.state, id, "HEAD", "a.txt").is_err());
    }
}
