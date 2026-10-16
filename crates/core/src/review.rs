// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Turning a report into a GitHub issue: the draft a moderator reviews before anything reaches
//! GitHub. Issues are project material that people and agents read, so the clean-room rule
//! applies (Suhina's AGENTS.md rule 1) and other products are named only where a file format is
//! handled (rule 8). Reports often compare with another DAW and show its screenshots, so the
//! card leads the moderator through cleaning them out:
//!
//! 1. the text, with words from the setting `REVIEW_WORDS` in bold and a button that replaces
//!    them with "another DAW";
//! 2. the screenshots, none selected by default: only those that show Suhina alone go in;
//! 3. a confirmation, before which **Create issue** stays disabled.
//!
//! The draft lives in the bot's state for 15 minutes under a short random key that every button
//! carries in its custom id (`track:<action>:<key>`).

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::bug::REPORTED_BY;
use crate::interaction::{Attachment, Interaction};
use crate::reply::shorten;

pub const NAME: &str = "track";
pub const ADD_AS_BUG: &str = "Add as bug";
pub const ADD_TO_BACKLOG: &str = "Add to backlog";
pub const ID_PREFIX: &str = "track:";
/// How long a draft is kept, in seconds; also the life of the interaction that shows it.
pub const DRAFT_SECONDS: u64 = 15 * 60;
/// What replaces another product's name.
pub const REPLACEMENT: &str = "another DAW";
const TITLE_LIMIT: usize = 256;
const BODY_LIMIT: usize = 4000;

/// Hidden from members without the permission by default; a server administrator allows the
/// staff roles in Server Settings › Integrations (docs/operations.md). The Worker checks the
/// roles anyway.
fn staff_only() -> Value {
    json!("0")
}

pub fn definitions() -> Vec<Value> {
    vec![
        json!({
            "name": NAME, "type": 1, "contexts": [0], "integration_types": [0],
            "description": "Turn this bug-reports or feature-requests post into a GitHub issue, after a review",
            "default_member_permissions": staff_only(),
        }),
        json!({ "name": ADD_AS_BUG, "type": 3, "contexts": [0], "integration_types": [0],
                "default_member_permissions": staff_only() }),
        json!({ "name": ADD_TO_BACKLOG, "type": 3, "contexts": [0], "integration_types": [0],
                "default_member_permissions": staff_only() }),
    ]
}

/// What a message command does, for `/help` and docs/commands.md (Discord keeps a message
/// command's own description empty).
pub fn message_command_about(name: &str) -> &'static str {
    match name {
        ADD_AS_BUG => "Turn a message into a GitHub bug issue, after a review",
        ADD_TO_BACKLOG => "Turn a message into a GitHub feature request, after a review",
        _ => "",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    Bug,
    Feature,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Source {
    /// A post in the bug-reports forum: its thread, whose starter message has the same id.
    Thread { thread_id: String },
    /// A single message elsewhere.
    Message,
}

/// A forum post as read from Discord.
#[derive(Debug, Clone)]
pub struct Post<'a> {
    pub thread_id: &'a str,
    pub title: &'a str,
    pub starter: &'a str,
    pub attachments: Vec<Attachment>,
    pub tags: &'a [String],
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Similar {
    pub number: u64,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Draft {
    /// The moderator who opened it; only they may use its buttons.
    pub owner: String,
    pub kind: Kind,
    pub source: Source,
    pub title: String,
    pub body: String,
    pub labels: Vec<String>,
    pub screenshots: Vec<Attachment>,
    /// Indexes into `screenshots`.
    pub selected: Vec<usize>,
    pub reviewed: bool,
    /// May set Priority and Phase.
    pub triage: bool,
    pub priority: Option<String>,
    pub phase: Option<String>,
    /// The board's Phase options, read when the draft is made (triagers only).
    pub phases: Vec<String>,
    pub similar: Vec<Similar>,
}

/// Forum tag → GitHub label. Tags without a label (MIDI, Networking, Other/unknown, and the
/// bot's own Tracked and Fixed) add none.
pub const TAG_LABELS: [(&str, &str); 6] = [
    ("Mac", "macos"),
    ("Windows", "windows"),
    ("Linux", "linux"),
    ("UI", "ui"),
    ("Audio", "audio-io"),
    ("Third-party/plugins", "plug-ins"),
];

pub fn labels(kind: Kind, tags: &[String]) -> Vec<String> {
    let mut out = vec![
        match kind {
            Kind::Bug => "bug",
            Kind::Feature => "enhancement",
        }
        .to_owned(),
        "from-discord".to_owned(),
    ];
    for tag in tags {
        if let Some((_, label)) = TAG_LABELS
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(tag.trim()))
            && !out.iter().any(|have| have == label)
        {
            out.push((*label).to_owned());
        }
    }
    out
}

/// Images only, at most ten (a select menu's limit).
fn images(attachments: Vec<Attachment>) -> Vec<Attachment> {
    attachments
        .into_iter()
        .filter(|a| {
            a.content_type
                .as_deref()
                .is_some_and(|t| t.starts_with("image/"))
        })
        .take(10)
        .collect()
}

impl Draft {
    /// From a forum post (bug-reports or feature-requests): its title, its starter message and
    /// its tags. The `/bug` line that names the reporter is left out.
    pub fn from_post(owner: &str, kind: Kind, post: Post<'_>, triage: bool) -> Self {
        let body: Vec<&str> = post
            .starter
            .lines()
            .skip_while(|line| line.starts_with(REPORTED_BY) || line.trim().is_empty())
            .collect();
        Self::new(
            owner,
            kind,
            Source::Thread {
                thread_id: post.thread_id.to_owned(),
            },
            post.title,
            &body.join(
                "
",
            ),
            post.attachments,
            labels(kind, post.tags),
            triage,
        )
    }

    /// From one message: the title is its first line.
    pub fn from_message(
        owner: &str,
        kind: Kind,
        content: &str,
        attachments: Vec<Attachment>,
        triage: bool,
    ) -> Self {
        let title = content
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("Report from Discord");
        Self::new(
            owner,
            kind,
            Source::Message,
            title,
            content,
            attachments,
            labels(kind, &[]),
            triage,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn new(
        owner: &str,
        kind: Kind,
        source: Source,
        title: &str,
        body: &str,
        attachments: Vec<Attachment>,
        labels: Vec<String>,
        triage: bool,
    ) -> Self {
        Self {
            owner: owner.to_owned(),
            kind,
            source,
            title: shorten(title.trim(), TITLE_LIMIT),
            body: shorten(body.trim(), BODY_LIMIT),
            labels,
            screenshots: images(attachments),
            selected: Vec::new(),
            reviewed: false,
            triage,
            priority: None,
            phase: None,
            phases: Vec::new(),
            similar: Vec::new(),
        }
    }

    /// The selected screenshots, in order.
    pub fn chosen(&self) -> Vec<&Attachment> {
        self.selected
            .iter()
            .filter_map(|i| self.screenshots.get(*i))
            .collect()
    }

    /// The menu's values are the indexes as text; anything else is ignored. A change of the text
    /// or the screenshots asks for the review again.
    pub fn select(&mut self, values: &[String]) {
        self.selected = values
            .iter()
            .filter_map(|v| v.parse::<usize>().ok())
            .filter(|i| *i < self.screenshots.len())
            .collect();
        self.selected.sort_unstable();
        self.selected.dedup();
        self.reviewed = false;
    }

    pub fn replace_words(&mut self, words: &[String]) {
        self.title = replace(&self.title, words, REPLACEMENT);
        self.body = replace(&self.body, words, REPLACEMENT);
        self.reviewed = false;
    }

    /// Takes the edit form's fields. Priority and Phase only from a triager, and only values the
    /// board has.
    pub fn apply_edit(&mut self, edit: &Edit) {
        if let Some(title) = &edit.title {
            self.title = shorten(title.trim(), TITLE_LIMIT);
        }
        if let Some(body) = &edit.body {
            self.body = shorten(body.trim(), BODY_LIMIT);
        }
        if self.triage {
            self.priority = edit
                .priority
                .iter()
                .find(|p| PRIORITIES.contains(&p.as_str()))
                .cloned();
            self.phase = edit.phase.iter().find(|p| self.phases.contains(p)).cloned();
        }
        self.reviewed = false;
    }
}

/// The submitted edit form.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Edit {
    pub title: Option<String>,
    pub body: Option<String>,
    pub priority: Vec<String>,
    pub phase: Vec<String>,
}

impl Edit {
    pub fn from_submit(interaction: &Interaction) -> Self {
        Self {
            title: interaction.modal_value("title").map(str::to_owned),
            body: interaction.modal_value("body").map(str::to_owned),
            priority: interaction.modal_values("priority"),
            phase: interaction.modal_values("phase"),
        }
    }
}

pub const PRIORITIES: [&str; 3] = ["P0", "P1", "P2"];

// ---------------------------------------------------------------------------------------------
// Words to review

/// `REVIEW_WORDS`: a JSON array of strings; `None` when missing or malformed.
pub fn parse_words(setting: &str) -> Option<Vec<String>> {
    let words: Vec<String> = serde_json::from_str(setting).ok()?;
    let words: Vec<String> = words
        .into_iter()
        .map(|w| w.trim().to_owned())
        .filter(|w| !w.is_empty())
        .collect();
    Some(words)
}

/// Byte ranges of whole-word, ASCII-case-insensitive matches of any word, without overlaps.
fn matches(text: &str, words: &[String]) -> Vec<(usize, usize)> {
    let lower = text.to_ascii_lowercase();
    let is_word = |c: Option<char>| c.is_some_and(char::is_alphanumeric);
    let mut found: Vec<(usize, usize)> = Vec::new();
    for word in words {
        let needle = word.to_ascii_lowercase();
        let mut from = 0;
        while let Some(at) = lower.get(from..).and_then(|rest| rest.find(&needle)) {
            let start = from + at;
            let end = start + needle.len();
            let before = text.get(..start).and_then(|s| s.chars().next_back());
            let after = text.get(end..).and_then(|s| s.chars().next());
            if !is_word(before)
                && !is_word(after)
                && !found.iter().any(|(s, e)| start < *e && *s < end)
            {
                found.push((start, end));
            }
            from = end.max(start + 1);
        }
    }
    found.sort_unstable();
    found
}

/// The words found, as written in the text, each once.
pub fn found(text: &str, words: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (start, end) in matches(text, words) {
        if let Some(word) = text.get(start..end)
            && !out.iter().any(|w| w.eq_ignore_ascii_case(word))
        {
            out.push(word.to_owned());
        }
    }
    out
}

fn rewrite(text: &str, words: &[String], with: impl Fn(&str) -> String) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for (start, end) in matches(text, words) {
        out.push_str(text.get(last..start).unwrap_or_default());
        out.push_str(&with(text.get(start..end).unwrap_or_default()));
        last = end;
    }
    out.push_str(text.get(last..).unwrap_or_default());
    out
}

pub fn bold(text: &str, words: &[String]) -> String {
    rewrite(text, words, |word| format!("**{word}**"))
}

pub fn replace(text: &str, words: &[String], with: &str) -> String {
    rewrite(text, words, |_| with.to_owned())
}

// ---------------------------------------------------------------------------------------------
// The card

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Screenshots,
    Edit,
    Replace,
    Check,
    Create,
    Cancel,
    EditSubmit,
}

const ACTIONS: [(Action, &str); 7] = [
    (Action::Screenshots, "shots"),
    (Action::Edit, "edit"),
    (Action::Replace, "replace"),
    (Action::Check, "check"),
    (Action::Create, "create"),
    (Action::Cancel, "cancel"),
    (Action::EditSubmit, "editsubmit"),
];

pub fn custom_id(action: Action, key: &str) -> String {
    let name = ACTIONS
        .iter()
        .find(|(a, _)| *a == action)
        .map_or("", |(_, n)| n);
    format!("{ID_PREFIX}{name}:{key}")
}

/// `track:<action>:<key>`; the key is hex.
pub fn parse_custom_id(id: &str) -> Option<(Action, String)> {
    let rest = id.strip_prefix(ID_PREFIX)?;
    let (name, key) = rest.split_once(':')?;
    let action = ACTIONS.iter().find(|(_, n)| *n == name)?.0;
    (!key.is_empty() && key.len() <= 32 && key.chars().all(|c| c.is_ascii_hexdigit()))
        .then(|| (action, key.to_owned()))
}

fn button(label: &str, style: u8, id: String, disabled: bool) -> Value {
    json!({ "type": 2, "style": style, "label": label, "custom_id": id, "disabled": disabled })
}

/// The review card, as the body of a message edit. `words` is `None` when `REVIEW_WORDS` is
/// missing or malformed; `copying` is false when screenshots cannot be copied (no bucket), and
/// then none are offered.
pub fn card(draft: &Draft, key: &str, words: Option<&[String]>, copying: bool) -> Value {
    let words_found = words.map(|w| {
        let mut all = found(&draft.title, w);
        for word in found(&draft.body, w) {
            if !all.iter().any(|have| have.eq_ignore_ascii_case(&word)) {
                all.push(word);
            }
        }
        all
    });
    let shown = |text: &str| words.map_or_else(|| text.to_owned(), |w| bold(text, w));

    let mut lines = vec![
        "**Review before it goes to GitHub** (only you see this)".to_owned(),
        "Issues are read by people and agents: leave out other products' names and anything copied from them.".to_owned(),
        String::new(),
        format!("**Title:** {}", shown(&draft.title)),
        format!("**Labels:** {}", draft.labels.join(", ")),
    ];
    if draft.priority.is_some() || draft.phase.is_some() {
        lines.push(format!(
            "**Board:** {} · {}",
            draft.priority.as_deref().unwrap_or("no priority"),
            draft.phase.as_deref().unwrap_or("no phase")
        ));
    }
    let quoted: Vec<String> = shorten(&shown(&draft.body), 900)
        .lines()
        .map(|l| format!("> {l}"))
        .collect();
    lines.push(quoted.join("\n"));
    lines.push(String::new());
    match &words_found {
        None => lines.push(
            "Product name check is off (REVIEW_WORDS not set): read the text yourself.".to_owned(),
        ),
        Some(found) if found.is_empty() => lines.push("No product names found.".to_owned()),
        Some(found) => lines.push(format!(
            "**{} product name{} found:** {}. Use **Replace with \"{REPLACEMENT}\"** or **Edit**.",
            found.len(),
            if found.len() == 1 { "" } else { "s" },
            found.join(", ")
        )),
    }
    if !draft.screenshots.is_empty() {
        lines.push(if copying {
            format!(
                "**Screenshots:** {} in the report, {} selected. Select only screenshots that show Suhina alone.",
                draft.screenshots.len(),
                draft.selected.len()
            )
        } else {
            format!(
                "**Screenshots:** {} in the report. Copying screenshots is not set up, so they stay in Discord.",
                draft.screenshots.len()
            )
        });
    }
    if !draft.similar.is_empty() {
        let similar: Vec<String> = draft
            .similar
            .iter()
            .map(|s| format!("#{} {}", s.number, s.title))
            .collect();
        lines.push(format!(
            "**Similar open issues:** {}",
            shorten(&similar.join(" · "), 300)
        ));
    }
    lines.push(if draft.reviewed {
        "✅ Checked. **Create issue** is ready.".to_owned()
    } else {
        "Check the text and screenshots, then confirm.".to_owned()
    });

    let mut rows = Vec::new();
    if copying && !draft.screenshots.is_empty() {
        let options: Vec<Value> = draft
            .screenshots
            .iter()
            .enumerate()
            .map(|(i, shot)| {
                json!({
                    "label": shorten(&format!("{}. {}", i + 1, shot.filename), 100),
                    "value": i.to_string(),
                    "description": crate::nightly::megabytes(shot.size),
                    "default": draft.selected.contains(&i),
                })
            })
            .collect();
        rows.push(json!({ "type": 1, "components": [{
            "type": 3, "custom_id": custom_id(Action::Screenshots, key),
            "placeholder": "Screenshots that show Suhina alone (none by default)",
            "min_values": 0, "max_values": options.len(), "options": options,
        }]}));
    }
    let nothing_to_replace = words_found.as_ref().is_none_or(Vec::is_empty);
    rows.push(json!({ "type": 1, "components": [
        button("Edit", 2, custom_id(Action::Edit, key), false),
        button(&format!("Replace with \"{REPLACEMENT}\""), 2, custom_id(Action::Replace, key), nothing_to_replace),
        button(if draft.reviewed { "Checked ✓" } else { "I've checked the text and screenshots" }, 3, custom_id(Action::Check, key), draft.reviewed),
    ]}));
    rows.push(json!({ "type": 1, "components": [
        button("Create issue", 1, custom_id(Action::Create, key), !draft.reviewed),
        button("Cancel", 4, custom_id(Action::Cancel, key), false),
    ]}));

    json!({
        "content": shorten(&lines.join("\n"), crate::reply::CONTENT_LIMIT),
        "components": rows,
        "allowed_mentions": { "parse": [] },
    })
}

/// The card once it is done: a line and no buttons.
pub fn closed_card(text: &str) -> Value {
    json!({ "content": shorten(text, crate::reply::CONTENT_LIMIT), "components": [], "allowed_mentions": { "parse": [] } })
}

/// The edit form, filled with the draft.
pub fn edit_modal(draft: &Draft, key: &str) -> Value {
    let field = |label: &str, id: &str, style: u8, value: &str, max: usize| {
        json!({ "type": 18, "label": label, "component": {
            "type": 4, "custom_id": id, "style": style, "value": shorten(value, max), "max_length": max, "required": true,
        }})
    };
    let mut components = vec![
        field("Title", "title", 1, &draft.title, TITLE_LIMIT),
        field("Text", "body", 2, &draft.body, BODY_LIMIT),
    ];
    if draft.triage {
        let options = |values: Vec<&str>, current: Option<&str>| -> Vec<Value> {
            values
                .into_iter()
                .take(25)
                .map(|v| json!({ "label": v, "value": v, "default": current == Some(v) }))
                .collect()
        };
        components.push(json!({ "type": 18, "label": "Priority (optional)", "component": {
            "type": 3, "custom_id": "priority", "min_values": 0, "max_values": 1, "required": false,
            "options": options(PRIORITIES.to_vec(), draft.priority.as_deref()),
        }}));
        if !draft.phases.is_empty() {
            components.push(json!({ "type": 18, "label": "Phase (optional)", "component": {
                "type": 3, "custom_id": "phase", "min_values": 0, "max_values": 1, "required": false,
                "options": options(draft.phases.iter().map(String::as_str).collect(), draft.phase.as_deref()),
            }}));
        }
    }
    json!({ "type": 9, "data": {
        "custom_id": custom_id(Action::EditSubmit, key),
        "title": "Edit the issue",
        "components": components,
    }})
}

// ---------------------------------------------------------------------------------------------
// The issue and the thread

/// The issue's body. Names, mentions and links no one; `images` are the copied screenshots'
/// public URLs, `left_in_discord` those not selected, `not_copied` selected ones that failed.
pub fn issue_body(
    draft: &Draft,
    images: &[String],
    left_in_discord: usize,
    not_copied: usize,
) -> String {
    let mut parts = vec![
        match draft.kind {
            Kind::Bug => "Bug report from Discord.",
            Kind::Feature => "Feature request from Discord.",
        }
        .to_owned(),
        draft.body.clone(),
    ];
    let shots: Vec<String> = images
        .iter()
        .enumerate()
        .map(|(i, url)| format!("![Screenshot {}]({url})", i + 1))
        .collect();
    if !shots.is_empty() {
        parts.push(shots.join("\n"));
    }
    let mut notes = Vec::new();
    if left_in_discord > 0 {
        notes.push(format!(
            "{left_in_discord} more screenshot{} in the Discord report.",
            plural(left_in_discord)
        ));
    }
    if not_copied > 0 {
        notes.push(format!(
            "{not_copied} screenshot{} could not be copied.",
            plural(not_copied)
        ));
    }
    if !notes.is_empty() {
        parts.push(format!("_{}_", notes.join(" ")));
    }
    parts.join("\n\n")
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

pub fn issue_type(kind: Kind) -> &'static str {
    match kind {
        Kind::Bug => "Bug",
        Kind::Feature => "Feature",
    }
}

/// The message in the thread once the issue exists.
pub fn thanks(kind: Kind, number: u64, url: &str) -> String {
    match kind {
        Kind::Bug => format!(
            "Thanks for the bug report! It's tracked on GitHub as #{number}: <{url}>. We'll post here when it's fixed."
        ),
        Kind::Feature => format!(
            "Thanks for the idea! It's on the backlog on GitHub as #{number}: <{url}>. We'll post here when it lands."
        ),
    }
}

/// The words of a title worth searching for similar issues, as a GitHub search query.
pub fn similar_query(repository: &str, title: &str) -> Option<String> {
    let words: Vec<String> = title
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() >= 4)
        .take(5)
        .map(str::to_lowercase)
        .collect();
    (!words.is_empty())
        .then(|| format!("repo:{repository} is:issue is:open {}", words.join(" OR ")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words() -> Vec<String> {
        vec!["Daw One".to_owned(), "Beatbox".to_owned()]
    }

    fn shot(name: &str) -> Attachment {
        Attachment {
            id: name.to_owned(),
            filename: name.to_owned(),
            content_type: Some("image/png".to_owned()),
            size: 1_500_000,
            url: format!("https://cdn/{name}"),
        }
    }

    fn draft() -> Draft {
        Draft::from_post(
            "7",
            Kind::Bug,
            Post {
                thread_id: "123",
                title: "Reverb sounds wrong",
                starter: "Reported by <@42>\n\n**What happened**\nIn daw one it works, in Beatbox too. Beatboxing is fine.",
                attachments: vec![shot("a.png"), shot("b.png")],
                tags: &["Mac".to_owned(), "UI".to_owned(), "MIDI".to_owned()],
            },
            false,
        )
    }

    #[test]
    fn tags_become_labels() {
        assert_eq!(draft().labels, ["bug", "from-discord", "macos", "ui"]);
        assert_eq!(labels(Kind::Feature, &[]), ["enhancement", "from-discord"]);
        assert_eq!(
            labels(
                Kind::Bug,
                &["Third-party/plugins".to_owned(), "Tracked".to_owned()]
            ),
            ["bug", "from-discord", "plug-ins"]
        );
        assert_eq!(
            labels(Kind::Bug, &["audio".to_owned(), "Audio".to_owned()]),
            ["bug", "from-discord", "audio-io"]
        );
    }

    #[test]
    fn a_feature_post_becomes_a_feature() {
        let tags = ["UI".to_owned()];
        let draft = Draft::from_post(
            "7",
            Kind::Feature,
            Post {
                thread_id: "9",
                title: "Tempo tap",
                starter: "Please",
                attachments: vec![],
                tags: &tags,
            },
            false,
        );
        assert_eq!(draft.labels, ["enhancement", "from-discord", "ui"]);
        assert!(issue_body(&draft, &[], 0, 0).starts_with("Feature request from Discord."));
        assert!(thanks(Kind::Feature, 3, "u").starts_with("Thanks for the idea!"));
        assert!(thanks(Kind::Bug, 3, "u").starts_with("Thanks for the bug report!"));
    }

    #[test]
    fn the_reporter_line_is_left_out() {
        let draft = draft();
        assert!(draft.body.starts_with("**What happened**"));
        assert!(!draft.body.contains("<@42>"));
        assert_eq!(
            draft.source,
            Source::Thread {
                thread_id: "123".to_owned()
            }
        );
    }

    #[test]
    fn finds_whole_words_only() {
        let text = &draft().body;
        assert_eq!(found(text, &words()), ["daw one", "Beatbox"]);
        assert_eq!(
            replace(text, &words(), REPLACEMENT),
            "**What happened**\nIn another DAW it works, in another DAW too. Beatboxing is fine."
        );
        assert!(bold(text, &words()).contains("In **daw one** it works"));
        assert_eq!(found("Ünïcode before beatbox", &words()), ["beatbox"]);
    }

    #[test]
    fn parses_the_setting() {
        assert_eq!(
            parse_words(r#"["A", " B ", ""]"#),
            Some(vec!["A".to_owned(), "B".to_owned()])
        );
        assert_eq!(parse_words("not json"), None);
        assert_eq!(parse_words(r#"{"a":1}"#), None);
    }

    #[test]
    fn no_screenshot_is_selected_by_default_and_changes_ask_for_a_new_review() {
        let mut draft = draft();
        assert!(draft.selected.is_empty());
        draft.reviewed = true;
        draft.select(&[
            "1".to_owned(),
            "9".to_owned(),
            "x".to_owned(),
            "1".to_owned(),
        ]);
        assert_eq!(draft.selected, [1]);
        assert!(!draft.reviewed);
        assert_eq!(draft.chosen()[0].filename, "b.png");
        draft.reviewed = true;
        draft.replace_words(&words());
        assert!(!draft.reviewed);
    }

    #[test]
    fn only_images_are_offered() {
        let mut text = shot("notes.txt");
        text.content_type = Some("text/plain".to_owned());
        let draft = Draft::from_message(
            "7",
            Kind::Feature,
            "Idea\nMore",
            vec![text, shot("c.png")],
            false,
        );
        assert_eq!(draft.screenshots.len(), 1);
        assert_eq!(draft.title, "Idea");
    }

    #[test]
    fn create_is_disabled_until_checked() {
        let mut draft = draft();
        let card_body = card(&draft, "ab12", Some(&words()), true);
        let create = &card_body["components"][2]["components"][0];
        assert_eq!(create["label"], "Create issue");
        assert_eq!(create["disabled"], true);
        let content = card_body["content"].as_str().unwrap();
        assert!(content.contains("**2 product names found:** daw one, Beatbox"));
        assert!(content.contains("2 in the report, 0 selected"));
        draft.reviewed = true;
        assert_eq!(
            card(&draft, "ab12", Some(&words()), true)["components"][2]["components"][0]["disabled"],
            false
        );
    }

    #[test]
    fn without_copying_no_screenshot_is_offered() {
        let card_body = card(&draft(), "ab12", None, false);
        assert!(
            card_body["content"]
                .as_str()
                .unwrap()
                .contains("they stay in Discord")
        );
        let rows = card_body["components"].as_array().unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["components"][0]["type"], 2);
    }

    #[test]
    fn the_card_says_when_the_check_is_off() {
        let card_body = card(&draft(), "ab12", None, true);
        assert!(
            card_body["content"]
                .as_str()
                .unwrap()
                .contains("check is off")
        );
        assert_eq!(
            card_body["components"][1]["components"][1]["disabled"],
            true
        );
    }

    #[test]
    fn custom_ids_round_trip() {
        let id = custom_id(Action::Create, "00ff");
        assert_eq!(id, "track:create:00ff");
        assert_eq!(
            parse_custom_id(&id),
            Some((Action::Create, "00ff".to_owned()))
        );
        assert_eq!(parse_custom_id("track:create:zz"), None);
        assert_eq!(parse_custom_id("track:nope:00"), None);
        assert_eq!(parse_custom_id("bug:submit"), None);
    }

    #[test]
    fn the_edit_form_has_board_fields_for_triagers_only() {
        let mut draft = draft();
        assert_eq!(
            edit_modal(&draft, "a1")["data"]["components"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        draft.triage = true;
        draft.phases = vec!["M8 UI".to_owned(), "Later".to_owned()];
        assert_eq!(
            edit_modal(&draft, "a1")["data"]["components"]
                .as_array()
                .unwrap()
                .len(),
            4
        );
    }

    #[test]
    fn edits_take_only_known_board_values() {
        let body = br#"{"type":5,"data":{"custom_id":"track:editsubmit:a1","components":[
            {"type":18,"component":{"type":4,"custom_id":"title","value":"New title"}},
            {"type":18,"component":{"type":4,"custom_id":"body","value":"New body"}},
            {"type":18,"component":{"type":3,"custom_id":"priority","values":["P1"]}},
            {"type":18,"component":{"type":3,"custom_id":"phase","values":["Nope"]}}]}}"#;
        let submit = Edit::from_submit(&Interaction::parse(body).unwrap());
        let mut draft = draft();
        draft.phases = vec!["Later".to_owned()];
        draft.apply_edit(&submit);
        assert_eq!(draft.title, "New title");
        assert_eq!(draft.priority, None, "not a triager");
        draft.triage = true;
        draft.apply_edit(&submit);
        assert_eq!(draft.priority.as_deref(), Some("P1"));
        assert_eq!(draft.phase, None, "not one of the board's phases");
    }

    #[test]
    fn the_issue_body_names_no_one() {
        let mut draft = draft();
        draft.select(&["0".to_owned()]);
        let body = issue_body(&draft, &["https://img/1.png".to_owned()], 1, 0);
        assert!(body.starts_with("Bug report from Discord.\n\n**What happened**"));
        assert!(body.contains("![Screenshot 1](https://img/1.png)"));
        assert!(body.ends_with("_1 more screenshot in the Discord report._"));
        assert!(!body.contains("<@"));
        assert!(issue_body(&draft, &[], 0, 2).contains("2 screenshots could not be copied."));
    }

    #[test]
    fn similar_query_uses_the_longer_words() {
        assert_eq!(
            similar_query("o/r", "Clip does not loop on Mac").as_deref(),
            Some("repo:o/r is:issue is:open clip OR does OR loop")
        );
        assert_eq!(similar_query("o/r", "a b"), None);
    }

    #[test]
    fn definitions_are_staff_only() {
        for definition in definitions() {
            assert_eq!(definition["default_member_permissions"], "0");
        }
    }
}
