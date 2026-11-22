use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Serialize};

/// What kind of thing an event announces: a name a source gives it, or a
/// kind the config declares.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Kind(Cow<'static, str>);

impl Kind {
    /// A release on a forge.
    pub const RELEASE: Self = Self(Cow::Borrowed("release"));
    /// A new version of a package on a registry.
    pub const PACKAGE_VERSION: Self = Self(Cow::Borrowed("package-version"));
    /// An entry in a feed.
    pub const FEED_ENTRY: Self = Self(Cow::Borrowed("feed-entry"));
    /// Anything an app says.
    pub const MESSAGE: Self = Self(Cow::Borrowed("message"));
    /// A CI run on a forge.
    pub const RUN: Self = Self(Cow::Borrowed("run"));
    /// A security alert on a forge.
    pub const ALERT: Self = Self(Cow::Borrowed("alert"));
    /// A pull request, or a GitLab merge request, on a forge.
    pub const PULL_REQUEST: Self = Self(Cow::Borrowed("pull-request"));
    /// An issue on a forge.
    pub const ISSUE: Self = Self(Cow::Borrowed("issue"));
    /// A report of a view's open pull requests, for one window.
    pub const REPORT: Self = Self(Cow::Borrowed("report"));
    /// A nudge: an open pull request that waited past a rule's stage.
    pub const NUDGE: Self = Self(Cow::Borrowed("nudge"));
    /// A rule's plan to re-run a flaky check once (ADR-0065).
    pub const RERUN: Self = Self(Cow::Borrowed("rerun"));

    /// Whether only herald itself announces its events, as a report from a
    /// view or a rule's nudge: no app and no other herald's feed gives one.
    #[must_use]
    pub fn own(&self) -> bool {
        *self == Self::REPORT || *self == Self::NUDGE || *self == Self::RERUN
    }

    /// The kind named `name`.
    ///
    /// # Errors
    ///
    /// Where `name` is not words of lowercase letters and digits joined by
    /// `-`.
    pub fn new(name: &str) -> Result<Self, String> {
        let words = !name.is_empty()
            && name.split('-').all(|word| {
                !word.is_empty()
                    && word
                        .bytes()
                        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
            });
        if words {
            Ok(Self(Cow::Owned(name.to_owned())))
        } else {
            Err(format!(
                "\"{name}\" is not a kind; name it in lowercase letters and digits, words joined by '-'"
            ))
        }
    }

    /// The kind's name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for Kind {
    type Error = String;

    fn try_from(name: String) -> Result<Self, String> {
        Self::new(&name)
    }
}

impl From<Kind> for String {
    fn from(kind: Kind) -> Self {
        kind.0.into_owned()
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl JsonSchema for Kind {
    fn schema_name() -> Cow<'static, str> {
        "Kind".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "What kind of thing an event announces: release, package-version, feed-entry, message, run, alert, or a kind the config declares.",
            "type": "string",
            "pattern": "^[a-z0-9]+(-[a-z0-9]+)*$"
        })
    }
}

/// One of an event's attributes, as its kind defines them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Attribute {
    /// Yes or no.
    Flag(bool),
    /// A number.
    Number(serde_json::Number),
    /// Text, or a web address.
    Text(String),
}

/// What type an attribute holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum AttributeType {
    /// Text.
    Text,
    /// Yes or no.
    Flag,
    /// A number.
    Number,
    /// An `https` or `http` address.
    Url,
}

impl AttributeType {
    fn holds(self, attribute: &Attribute) -> bool {
        match (self, attribute) {
            (Self::Text, Attribute::Text(_))
            | (Self::Flag, Attribute::Flag(_))
            | (Self::Number, Attribute::Number(_)) => true,
            (Self::Url, Attribute::Text(url)) => crate::address::web_url(url),
            _ => false,
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Flag => "a flag",
            Self::Number => "a number",
            Self::Url => "an https or http address",
        }
    }
}

/// One attribute a kind's events carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AttributeSpec {
    /// What it holds.
    #[serde(rename = "type")]
    pub ty: AttributeType,
    /// Whether every event of the kind carries it.
    #[serde(default)]
    pub required: bool,
}

/// The attribute every versioned kind carries.
pub const VERSION: &str = "version";
/// The attribute that marks a pre-release.
pub const PRERELEASE: &str = "prerelease";
/// The attribute naming the package a forge's package version is of.
pub const PACKAGE: &str = "package";
/// The attribute naming the type of that package, as its forge writes it,
/// such as `container` or `npm`.
pub const PACKAGE_TYPE: &str = "package_type";
/// The link to a release's page.
pub const RELEASE_LINK: &str = "release";
/// The link to a release's changelog.
pub const CHANGELOG_LINK: &str = "changelog";
/// The link to a feed's entry.
pub const ENTRY_LINK: &str = "entry";
/// The link to a run's page.
pub const RUN_LINK: &str = "run";
/// What a run or an alert did: a run's `requested`, `in_progress` or
/// `completed`, an alert's `created`, `reopened`, `confirmed`, `fixed` or
/// `dismissed`.
pub const ACTION: &str = "action";
/// How a finished run ended, in GitHub's words: `success`, `failure`,
/// `cancelled`, `timed_out`, `skipped`, `action_required`, `neutral`,
/// `startup_failure` or `stale`.
pub const CONCLUSION: &str = "conclusion";
/// The workflow, pipeline, check suite's app or status context a run is of.
pub const WORKFLOW: &str = "workflow";
/// What started a run, as its forge names it, such as `push` or
/// `schedule`; GitLab's `merge_request_event` reads as `pull_request`.
pub const TRIGGER: &str = "trigger";
/// The branch, or tag, a run ran on.
pub const BRANCH: &str = "branch";
/// The commit a run ran on.
pub const COMMIT: &str = "commit";
/// The forge's number of a run or an alert within its repository.
pub const NUMBER: &str = "number";
/// Which attempt at a run this is, the first being 1.
pub const ATTEMPT: &str = "attempt";
/// Who started a run.
pub const ACTOR: &str = "actor";
/// The link to an alert's page.
pub const ALERT_LINK: &str = "alert";
/// What raised an alert: `code-scanning`, `dependency-scanning`,
/// `secret-scanning`, `container-scanning`, `dast`, `fuzzing` or `other`.
pub const TOOL: &str = "tool";
/// How severe an alert is: `critical`, `high`, `medium`, `low`, `info` or
/// `unknown`, or a code scanning rule's `error`, `warning` or `note` where
/// it names no security severity.
pub const SEVERITY: &str = "severity";
/// The rule, advisory, secret type or identifier an alert is of.
pub const RULE: &str = "rule";
/// The ecosystem of the package a dependency alert is of, such as `npm`.
pub const ECOSYSTEM: &str = "ecosystem";
/// What an alert points at: a manifest, a file, or a container image.
pub const PATH: &str = "path";
/// Why a secret scanning alert was resolved, such as `revoked`.
pub const RESOLUTION: &str = "resolution";
/// The link to a pull request's page.
pub const PULL_LINK: &str = "pull";
/// Who opened a pull request or an issue.
pub const AUTHOR: &str = "author";
/// The view a report shows.
pub const VIEW: &str = "view";
/// How many open pull requests a report shows.
pub const PULLS: &str = "pulls";
/// How many of the open pull requests a report shows are stale.
pub const STALE: &str = "stale";
/// How long a nudged pull request waited past which its rule's stage fires,
/// as `2d`.
pub const STAGE: &str = "stage";
/// Whose turn a nudged pull request is, as a view names the state.
pub const TURN: &str = "turn";
/// Why a nudged pull request is in its state, as `check-failed: test`.
pub const REASON: &str = "reason";
/// How long a nudged pull request has waited, as `3d`.
pub const WAITED: &str = "waited";
/// When a nudged pull request's spell of waiting began.
pub const SPELL: &str = "spell";
/// The full path of a nudged pull request's repository.
pub const REPOSITORY: &str = "repository";
/// The logins whose turn a nudged pull request is, joined by `, `.
pub const WAITING_ON: &str = "waiting_on";

/// The attribute naming a re-run plan's check (ADR-0065).
pub const CHECK: &str = "check";

/// The attribute counting how often a re-run plan's check flipped.
pub const FLIPS: &str = "flips";

/// The attribute naming the forge's id of the run a re-run plan would
/// re-run; its page is the [`RUN_LINK`].
pub const RUN_ID: &str = "run_id";

/// The attribute holding the exact call a re-run plan would make.
pub const CALL: &str = "call";
/// Whether a pull request's or an issue's author is a bot or an app.
pub const BOT: &str = "bot";
/// The branch a pull request merges into.
pub const BASE: &str = "base";
/// The branch a pull request merges from.
pub const HEAD: &str = "head";
/// Whether a pull request is a draft.
pub const DRAFT: &str = "draft";
/// Who merged a pull request.
pub const MERGED_BY: &str = "merged_by";
/// The link to an issue's page.
pub const ISSUE_LINK: &str = "issue";
/// Why an issue was closed: `completed`, `not_planned` or `duplicate`.
pub const STATE_REASON: &str = "state_reason";
/// A pull request's or an issue's labels, joined by ", ".
pub const LABELS: &str = "labels";
/// What a run is read from: `actions`, the forge's own CI, or `checks`, a
/// check suite or a commit status.
pub const FROM: &str = "from";

/// Whether `name` can name an attribute or a link as a template reads it:
/// lowercase letters, digits and `_`, starting with a letter.
#[must_use]
pub fn readable(name: &str) -> bool {
    name.bytes()
        .next()
        .is_some_and(|first| first.is_ascii_lowercase())
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

/// The attributes of a versioned kind's event: its version, and whether it
/// is a pre-release.
#[must_use]
pub fn versioned(version: impl Into<String>, prerelease: bool) -> BTreeMap<String, Attribute> {
    BTreeMap::from([
        (VERSION.to_owned(), Attribute::Text(version.into())),
        (PRERELEASE.to_owned(), Attribute::Flag(prerelease)),
    ])
}

/// The version `attributes` carry, where they carry one.
pub(crate) fn version_in(attributes: &BTreeMap<String, Attribute>) -> Option<&str> {
    match attributes.get(VERSION) {
        Some(Attribute::Text(version)) => Some(version),
        _ => None,
    }
}

/// Whether `attributes` mark a pre-release.
pub(crate) fn prerelease_in(attributes: &BTreeMap<String, Attribute>) -> bool {
    attributes.get(PRERELEASE) == Some(&Attribute::Flag(true))
}

/// The attributes of a kind a forge's signals announce: the signal an
/// event is of, where its source names one, and whether it is an edit or
/// a deletion.
fn signalled() -> BTreeMap<String, AttributeSpec> {
    BTreeMap::from([
        (
            crate::signal::SIGNAL.to_owned(),
            AttributeSpec {
                ty: AttributeType::Text,
                required: false,
            },
        ),
        (
            crate::signal::EDITED.to_owned(),
            AttributeSpec {
                ty: AttributeType::Flag,
                required: false,
            },
        ),
        (
            crate::signal::WITHDRAWN.to_owned(),
            AttributeSpec {
                ty: AttributeType::Flag,
                required: false,
            },
        ),
    ])
}

/// The attributes of a package version: a forge's signals', and the
/// package and its type where a forge announces it.
fn packaged() -> BTreeMap<String, AttributeSpec> {
    let mut attributes = signalled();
    for name in [PACKAGE, PACKAGE_TYPE] {
        attributes.insert(
            name.to_owned(),
            AttributeSpec {
                ty: AttributeType::Text,
                required: false,
            },
        );
    }
    attributes
}

/// The attributes of a run: a forge's signals', and what the run is.
fn ran() -> BTreeMap<String, AttributeSpec> {
    let mut attributes = signalled();
    let typed = [
        (ACTION, AttributeType::Text, true),
        (CONCLUSION, AttributeType::Text, false),
        (WORKFLOW, AttributeType::Text, true),
        (TRIGGER, AttributeType::Text, false),
        (BRANCH, AttributeType::Text, false),
        (COMMIT, AttributeType::Text, false),
        (NUMBER, AttributeType::Number, false),
        (ATTEMPT, AttributeType::Number, false),
        (ACTOR, AttributeType::Text, false),
        (FROM, AttributeType::Text, true),
    ];
    for (name, ty, required) in typed {
        attributes.insert(name.to_owned(), AttributeSpec { ty, required });
    }
    attributes
}

/// The attributes of a pull request: a forge's signals', and what the pull
/// request is.
fn pulled() -> BTreeMap<String, AttributeSpec> {
    let mut attributes = signalled();
    let typed = [
        (ACTION, AttributeType::Text, true),
        (NUMBER, AttributeType::Number, true),
        (AUTHOR, AttributeType::Text, false),
        (BOT, AttributeType::Flag, false),
        (BASE, AttributeType::Text, false),
        (HEAD, AttributeType::Text, false),
        (COMMIT, AttributeType::Text, false),
        (DRAFT, AttributeType::Flag, false),
        (MERGED_BY, AttributeType::Text, false),
        (LABELS, AttributeType::Text, false),
    ];
    for (name, ty, required) in typed {
        attributes.insert(name.to_owned(), AttributeSpec { ty, required });
    }
    attributes
}

/// The attributes of an issue: a forge's signals', and what the issue is.
fn issued() -> BTreeMap<String, AttributeSpec> {
    let mut attributes = signalled();
    let typed = [
        (ACTION, AttributeType::Text, true),
        (NUMBER, AttributeType::Number, true),
        (AUTHOR, AttributeType::Text, false),
        (BOT, AttributeType::Flag, false),
        (STATE_REASON, AttributeType::Text, false),
        (LABELS, AttributeType::Text, false),
    ];
    for (name, ty, required) in typed {
        attributes.insert(name.to_owned(), AttributeSpec { ty, required });
    }
    attributes
}

/// The attributes of a report: the view it shows, and how many pull
/// requests and stale ones it shows.
fn reported() -> BTreeMap<String, AttributeSpec> {
    [
        (VIEW, AttributeType::Text),
        (PULLS, AttributeType::Number),
        (STALE, AttributeType::Number),
    ]
    .into_iter()
    .map(|(name, ty)| (name.to_owned(), AttributeSpec { ty, required: true }))
    .collect()
}

/// The attributes of a nudge: the rule and stage that fired, and the pull
/// request that waited.
fn nudged() -> BTreeMap<String, AttributeSpec> {
    [
        (RULE, AttributeType::Text, true),
        (STAGE, AttributeType::Text, true),
        (TURN, AttributeType::Text, true),
        (REASON, AttributeType::Text, true),
        (WAITED, AttributeType::Text, true),
        (SPELL, AttributeType::Text, true),
        (REPOSITORY, AttributeType::Text, true),
        (NUMBER, AttributeType::Number, true),
        (AUTHOR, AttributeType::Text, false),
        (BOT, AttributeType::Flag, true),
        (WAITING_ON, AttributeType::Text, false),
    ]
    .into_iter()
    .map(|(name, ty, required)| (name.to_owned(), AttributeSpec { ty, required }))
    .collect()
}

/// The attributes of a re-run plan (ADR-0065).
fn planned() -> BTreeMap<String, AttributeSpec> {
    [
        (RULE, AttributeType::Text, true),
        (CHECK, AttributeType::Text, true),
        (HEAD, AttributeType::Text, true),
        (FLIPS, AttributeType::Number, true),
        (REPOSITORY, AttributeType::Text, true),
        (NUMBER, AttributeType::Number, true),
        (RUN_ID, AttributeType::Text, false),
        (CALL, AttributeType::Text, false),
        (REASON, AttributeType::Text, false),
    ]
    .into_iter()
    .map(|(name, ty, required)| (name.to_owned(), AttributeSpec { ty, required }))
    .collect()
}

/// The attributes of an alert: a forge's signals', and what the alert is.
/// None holds what an alert found, such as a secret's value.
fn alerted() -> BTreeMap<String, AttributeSpec> {
    let mut attributes = signalled();
    let typed = [
        (ACTION, AttributeType::Text, true),
        (TOOL, AttributeType::Text, true),
        (SEVERITY, AttributeType::Text, false),
        (RULE, AttributeType::Text, false),
        (PACKAGE, AttributeType::Text, false),
        (ECOSYSTEM, AttributeType::Text, false),
        (PATH, AttributeType::Text, false),
        (NUMBER, AttributeType::Number, true),
        (RESOLUTION, AttributeType::Text, false),
    ];
    for (name, ty, required) in typed {
        attributes.insert(name.to_owned(), AttributeSpec { ty, required });
    }
    attributes
}

/// One link a kind's events carry, which becomes a button where the event
/// carries it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkSpec {
    /// Its name, as `event.links.<name>` reads it.
    pub name: String,
    /// Its button's label, where the config gives one.
    pub label: Option<String>,
}

impl LinkSpec {
    /// The most links a kind names: the buttons Discord, which takes the
    /// fewest, draws beside its subscribe button.
    pub const MOST: usize = 24;

    /// The link `name`, its button labelled as herald words it.
    #[must_use]
    pub fn named(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            label: None,
        }
    }
}

/// What a kind of event is: whether it carries a version, the links that
/// become its buttons, and the attributes its events carry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KindSpec {
    /// The kind.
    pub kind: Kind,
    /// What it is, in a sentence.
    pub description: String,
    /// Whether its events carry a version, as the `version` attribute.
    pub versioned: bool,
    /// The links that become its buttons, in order.
    pub links: Vec<LinkSpec>,
    /// The attributes its events carry, by name.
    pub attributes: BTreeMap<String, AttributeSpec>,
    /// Whether its events belong in private places only, so a route taking
    /// them must say whether its place is private.
    pub private: bool,
}

impl KindSpec {
    /// The kind `kind`, described as `description`, whose events carry
    /// `attributes`, and a version where it is `versioned`, with the links
    /// `links` as its buttons.
    #[must_use]
    pub fn new(
        kind: Kind,
        description: impl Into<String>,
        versioned: bool,
        links: Vec<LinkSpec>,
        mut attributes: BTreeMap<String, AttributeSpec>,
    ) -> Self {
        if versioned {
            attributes.insert(
                VERSION.to_owned(),
                AttributeSpec {
                    ty: AttributeType::Text,
                    required: true,
                },
            );
            attributes.insert(
                PRERELEASE.to_owned(),
                AttributeSpec {
                    ty: AttributeType::Flag,
                    required: false,
                },
            );
        }
        Self {
            kind,
            description: description.into(),
            versioned,
            links,
            attributes,
            private: false,
        }
    }

    /// The kind, its events belonging in private places only.
    #[must_use]
    pub fn private(self) -> Self {
        Self {
            private: true,
            ..self
        }
    }

    /// The kinds herald's own sources and apps announce.
    #[must_use]
    pub fn shipped() -> Vec<Self> {
        let buttons = |names: &[&str]| names.iter().map(|&name| LinkSpec::named(name)).collect();
        vec![
            Self::new(
                Kind::RELEASE,
                "A release on a forge.",
                true,
                buttons(&[RELEASE_LINK, CHANGELOG_LINK]),
                signalled(),
            ),
            Self::new(
                Kind::PACKAGE_VERSION,
                "A new version of a package on a registry.",
                true,
                buttons(&[RELEASE_LINK, CHANGELOG_LINK]),
                packaged(),
            ),
            Self::new(
                Kind::FEED_ENTRY,
                "An entry in a feed.",
                false,
                buttons(&[ENTRY_LINK]),
                BTreeMap::new(),
            ),
            Self::new(
                Kind::MESSAGE,
                "Anything an app says.",
                false,
                buttons(&[RELEASE_LINK, CHANGELOG_LINK]),
                BTreeMap::new(),
            ),
            Self::new(
                Kind::RUN,
                "A CI run on a forge: a workflow run, a pipeline, a check suite or a commit status.",
                false,
                buttons(&[RUN_LINK]),
                ran(),
            ),
            Self::new(
                Kind::ALERT,
                "A security alert on a forge: code scanning's, Dependabot's or secret scanning's, or a GitLab vulnerability.",
                false,
                buttons(&[ALERT_LINK]),
                alerted(),
            )
            .private(),
            Self::new(
                Kind::PULL_REQUEST,
                "A pull request, or a GitLab merge request, on a forge.",
                false,
                buttons(&[PULL_LINK]),
                pulled(),
            ),
            Self::new(
                Kind::ISSUE,
                "An issue on a forge.",
                false,
                buttons(&[ISSUE_LINK]),
                issued(),
            ),
            Self::new(
                Kind::REPORT,
                "A report of a view's open pull requests, for one window.",
                false,
                Vec::new(),
                reported(),
            ),
            Self::new(
                Kind::NUDGE,
                "An open pull request that waited past a rule's stage.",
                false,
                buttons(&[PULL_LINK]),
                nudged(),
            ),
            Self::new(
                Kind::RERUN,
                "A rule's plan to re-run a flaky check once.",
                false,
                buttons(&[PULL_LINK, RUN_LINK]),
                planned(),
            ),
        ]
    }

    /// Whether `attributes` are what the kind's events carry.
    ///
    /// # Errors
    ///
    /// The first attribute that is missing, unknown or of another type,
    /// named by its path.
    pub fn check(&self, attributes: &BTreeMap<String, Attribute>) -> Result<(), String> {
        for (name, spec) in &self.attributes {
            match attributes.get(name) {
                None if spec.required => {
                    return Err(format!(
                        "attributes.{name} is missing; {} events carry it",
                        self.kind
                    ));
                }
                Some(attribute) if !spec.ty.holds(attribute) => {
                    return Err(format!(
                        "attributes.{name} is not {ty}; {} events carry {name} as {ty}",
                        self.kind,
                        ty = spec.ty.name()
                    ));
                }
                _ => {}
            }
        }
        match attributes
            .keys()
            .find(|name| !self.attributes.contains_key(*name))
        {
            Some(unknown) => Err(format!(
                "attributes.{unknown} is not an attribute of {} events",
                self.kind
            )),
            None => Ok(()),
        }
    }
}

/// Every kind a config knows: the kinds herald ships, and those it declares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KindSpecs(Vec<KindSpec>);

impl Default for KindSpecs {
    fn default() -> Self {
        Self(KindSpec::shipped())
    }
}

impl KindSpecs {
    /// These kinds, and `spec`.
    #[must_use]
    pub fn with(mut self, spec: KindSpec) -> Self {
        self.0.push(spec);
        self
    }

    /// What the kind `kind` is, where it is known.
    #[must_use]
    pub fn get(&self, kind: &Kind) -> Option<&KindSpec> {
        self.0.iter().find(|spec| &spec.kind == kind)
    }

    /// The links that become the buttons of a `kind` event, none where the
    /// kind is not known.
    #[must_use]
    pub fn links(&self, kind: &Kind) -> &[LinkSpec] {
        self.get(kind).map_or(&[], |spec| &spec.links)
    }

    /// Every kind, in the order herald ships them and the config declares
    /// them.
    pub fn iter(&self) -> impl Iterator<Item = &KindSpec> {
        self.0.iter()
    }
}

#[cfg(test)]
mod tests;
