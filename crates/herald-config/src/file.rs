use std::collections::BTreeMap;
use std::num::{NonZeroU64, NonZeroUsize};

use herald_core::{
    AreaPaths, AttributeSpec, Duration, Edits, GroupLate, GroupMode, GroupUntil, Inbound, Kind,
    LateRelease, LinkSpec, Pinning, Prereleases, Rate, Sandbox, SecretRef, Size, Withdraw,
};
use schemars::JsonSchema;
use serde::Deserialize;
use toml::Spanned;

use crate::peers::{Peer, Rotation};

mod apps;
mod federation;
mod records;

pub(crate) use apps::{AppFile, OidcFile};
pub(crate) use federation::{FederationFile, PagesFile};
pub(crate) use records::{Keep, RecordsFile};

/// `[markers]`: the installation's marker key (ADR-0011).
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[serde(rename = "Markers")]
pub(crate) struct MarkersFile {
    /// The secret holding the marker key, such as `HERALD_MARKER_SECRET`.
    pub(crate) key: SecretRef,
}

/// A thread a route opens, and when housekeeping closes it.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[serde(rename = "Thread")]
pub(crate) struct ThreadFile {
    /// The template of the thread's name.
    #[schemars(
        with = "String",
        example = "{{ event.project }} {{ event.attributes.version }}"
    )]
    pub(crate) name: Spanned<String>,
    /// How long after the announcement the thread is archived.
    pub(crate) archive_after: Option<Duration>,
    /// How long after the announcement the thread is locked, and archived.
    pub(crate) lock_after: Option<Duration>,
}

/// How a route gathers the events that share a key into one announcement.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[serde(rename = "Group")]
pub(crate) struct GroupFile {
    /// The template of the key events are grouped by, on one line; an
    /// event whose key renders empty, or longer than 256 bytes, is
    /// announced alone.
    #[schemars(with = "String", example = "{{ event.attributes.version }}")]
    pub(crate) key: Spanned<String>,
    /// How long a group gathers its members, from when herald sees the
    /// first; "30m" where absent, and at most "7d".
    #[schemars(with = "Option<Duration>")]
    pub(crate) window: Option<Spanned<Duration>>,
    /// The projects whose event leads a group, as globs ignoring case, the
    /// first that matches a member first; the first member leads where none
    /// does.
    #[serde(default)]
    #[schemars(with = "Vec<String>", example = GroupFile::LEAD)]
    pub(crate) lead: Vec<Spanned<String>>,
    /// How the group is announced: held until it is due, then announced as
    /// one (`hold`), or the first member announced at once and its post
    /// edited as each other comes within the window (`edit`), where the
    /// destination edits a post.
    #[serde(default)]
    pub(crate) mode: GroupMode,
    /// When a held group is due: when its window passes (`window`), or as
    /// soon as the member that leads it comes (`lead`).
    #[serde(default)]
    pub(crate) until: GroupUntil,
    /// What becomes of a member that comes after its group was announced:
    /// announced alone (`post`), edited into the group's post where the
    /// destination edits one and announced alone otherwise (`edit`), or
    /// left unannounced (`skip`).
    #[serde(default)]
    pub(crate) late: GroupLate,
    /// How many members a group takes, each member more announced alone;
    /// 25 where absent, and at most 100.
    #[schemars(with = "Option<usize>", range(min = 1, max = 100))]
    pub(crate) most: Option<Spanned<usize>>,
}

impl GroupFile {
    /// The lead the schema shows as an example.
    const LEAD: [&str; 1] = ["lemonfiber"];
}

/// Who a route mentions.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[serde(rename = "Mentions")]
pub(crate) struct MentionsFile {
    /// The roles it pings, by the destination's ids for them.
    #[serde(default)]
    pub(crate) roles: Vec<String>,
    /// Whether it mentions the subscribers herald keeps.
    #[serde(default)]
    pub(crate) subscribers: bool,
}

/// A link a declared kind's events carry: its name, or its name and its
/// button's label.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
#[serde(rename = "DeclaredLink")]
pub(crate) enum LinkFile {
    /// A link whose button is labelled by its name.
    Named(String),
    /// A link and its button's label.
    Labelled(LabelledLinkFile),
}

/// A link and its button's label.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[serde(rename = "LabelledLink")]
pub(crate) struct LabelledLinkFile {
    /// Its name, as `event.links.<name>` reads it.
    pub(crate) name: String,
    /// Its button's label.
    pub(crate) label: String,
}

impl From<LinkFile> for LinkSpec {
    fn from(file: LinkFile) -> Self {
        match file {
            LinkFile::Named(name) => Self::named(name),
            LinkFile::Labelled(labelled) => Self {
                name: labelled.name,
                label: Some(labelled.label),
            },
        }
    }
}

/// A kind of event the config declares, which its apps may announce.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[serde(rename = "DeclaredKind")]
pub(crate) struct KindFile {
    /// What it is, in a sentence.
    #[serde(default)]
    pub(crate) description: String,
    /// Whether its events carry a version, as `attributes.version`, and may
    /// be pre-releases, as `attributes.prerelease`.
    #[serde(default)]
    pub(crate) versioned: bool,
    /// The links that become its buttons, in order: each a name, labelled
    /// by its name, or a name and a label.
    #[serde(default)]
    #[schemars(length(max = LinkSpec::MOST))]
    pub(crate) links: Vec<LinkFile>,
    /// The attributes its events carry, by name, as `event.attributes.<name>`
    /// reads them.
    #[serde(default)]
    pub(crate) attributes: BTreeMap<String, AttributeSpec>,
    /// Whether its events belong in private places only, so a route taking
    /// them must say whether its place is private (`[doctor] private_kinds`).
    #[serde(default)]
    pub(crate) private: bool,
}

/// What a route says whatever its destination.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[serde(rename = "Route")]
pub(crate) struct RouteFile {
    /// The route's stable id, part of every announcement's key.
    #[schemars(with = "String", example = "herald-releases")]
    pub(crate) id: Spanned<String>,
    /// The sources it takes events from.
    #[schemars(with = "Vec<String>", example = RouteFile::SOURCES)]
    pub(crate) sources: Vec<Spanned<String>>,
    /// The kinds of event it takes; every kind where absent.
    #[serde(default)]
    #[schemars(with = "Vec<Kind>")]
    pub(crate) kinds: Vec<Spanned<Kind>>,
    /// Which pre-releases it takes.
    #[serde(default)]
    pub(crate) prereleases: Prereleases,
    /// The versions it takes, as a pattern in which `*` stands for any run
    /// of characters and `?` for any one, such as `"*.*.*"`, which leaves
    /// out a Docker Hub tag such as `latest`; every version where absent.
    #[schemars(with = "Option<String>")]
    pub(crate) versions: Option<Spanned<String>>,
    /// The destination it announces to.
    #[schemars(with = "String", example = RouteFile::DESTINATION)]
    pub(crate) destination: Spanned<String>,
    /// The template it renders with; none on a route to a herald feed or
    /// push, which carries each event as an envelope.
    #[schemars(with = "Option<String>", example = "package-release")]
    pub(crate) template: Option<Spanned<String>>,
    /// The template it renders with in place of `template` where its
    /// destination's dialect is one of these, each keyed by the kind of
    /// destination that writes the dialect, such as `{ mastodon =
    /// "release-short" }`. Each is Markdown, converted to the dialect as
    /// `template` is; `template` where its destination's dialect has none.
    #[serde(default)]
    #[schemars(with = "BTreeMap<String, String>", example = RouteFile::templates_example())]
    pub(crate) templates: BTreeMap<String, Spanned<String>>,
    /// Whether to crosspost each announcement.
    #[serde(default)]
    pub(crate) crosspost: bool,
    /// The thread to open for each announcement.
    pub(crate) thread: Option<ThreadFile>,
    /// Whether to pin each announcement in place of the route's earlier pin,
    /// and how the earlier pin is found: `true` finds it by its marker, as
    /// "markers" does, and `false` pins nothing. A living route pins its
    /// post where absent and the destination pins.
    pub(crate) pin: Option<PinFile>,
    /// Whether it keeps one post of each report it takes, edited each
    /// window where the destination edits and posted anew each window
    /// where it cannot; it names only reports among its sources.
    #[serde(default)]
    pub(crate) living: bool,
    /// Who it mentions.
    #[serde(default)]
    pub(crate) mentions: MentionsFile,
    /// Whether its place is private, so it may take a private or internal
    /// repository's events; a place is public where absent, and a config
    /// sending such events to it does not load (`[doctor] private_to_public`).
    /// A route taking a private kind, such as `alert`, says it either way:
    /// `false` sends them to a public place on purpose (`[doctor]
    /// private_kinds`).
    pub(crate) private: Option<bool>,
    /// Whether the operator vouches that its place is private where herald
    /// cannot check who may read it, as for a Discord or Slack route reached
    /// by a webhook: with `private = true` it is then shown a view's
    /// advisories on the operator's word, which doctor lists. Refused on a
    /// route herald checks before each run's first advisory send, and on
    /// one that posts where anyone may read. False where absent.
    pub(crate) vouch_unchecked: Option<bool>,
    /// What it does with a Release whose tag it already announced: edits
    /// the tag's post to it, or posts it anew where the destination cannot
    /// edit (`edit`); posts it anew (`post`); or leaves the tag's post
    /// (`skip`).
    #[serde(default)]
    pub(crate) late_release: LateRelease,
    /// Which changed announcements it edits the post of: none, or a Release
    /// whose name or notes the forge says were edited (`release`).
    #[serde(default)]
    pub(crate) edits: Edits,
    /// What it does with an announcement whose Release the forge deletes:
    /// keeps its post (`keep`), or deletes every message herald recorded of
    /// it where the destination can (`delete`).
    #[serde(default)]
    pub(crate) withdraw: Withdraw,
    /// What of an event it narrows its sources' events by, an event taken
    /// only where every key holds; every event its other keys take where
    /// absent.
    #[serde(default, rename = "match")]
    pub(crate) matching: MatchFile,
    /// Where the paths a pull request's head changes are read, which the
    /// areas its `match` narrows by are those of: `kept`, from the pull
    /// request as herald keeps it where its head is the event's, no request
    /// more; or `delivery`, from the forge as the event is delivered, one
    /// request more for each pull request event (ADR-0072).
    #[serde(default)]
    #[schemars(with = "Option<AreaPaths>")]
    pub(crate) area_paths: Option<Spanned<AreaPaths>>,
    /// How it gathers the events that share a key into one announcement;
    /// each event is announced alone where absent.
    pub(crate) group: Option<GroupFile>,
}

/// What of an event a route narrows its sources' events by. Each list
/// takes any one of what it holds, and every event where empty; an event
/// with no origin, such as a feed's, is taken only where the match names
/// projects and attributes alone; and an event `not` takes is never taken.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[serde(rename = "Match")]
pub(crate) struct MatchFile {
    /// The projects taken, as globs, ignoring case.
    #[serde(default)]
    #[schemars(with = "Vec<String>", example = MatchFile::PROJECTS)]
    pub(crate) projects: Vec<Spanned<String>>,
    /// The repositories or packages taken by their full path, as globs,
    /// ignoring case, such as `"*/plugin-*"`.
    #[serde(default)]
    #[schemars(with = "Vec<String>")]
    pub(crate) repositories: Vec<Spanned<String>>,
    /// The owners taken: accounts, organizations, groups, or a registry's
    /// scopes, vendors and namespaces, as globs, ignoring case.
    #[serde(default)]
    #[schemars(with = "Vec<String>")]
    pub(crate) owners: Vec<Spanned<String>>,
    /// The forges and registries taken, as a source's `kind` names them.
    #[serde(default)]
    pub(crate) forges: Vec<String>,
    /// The hosts taken, such as `github.com` or `ghe.example`.
    #[serde(default)]
    pub(crate) hosts: Vec<String>,
    /// The topics, any one of which a repository holds.
    #[serde(default)]
    pub(crate) topics: Vec<String>,
    /// The custom properties, every one of which a repository holds with
    /// one of the values given for it; any value where none is given.
    #[serde(default)]
    pub(crate) properties: BTreeMap<String, Vec<String>>,
    /// The attributes of the event's kind, every one of which it carries
    /// with one of the values given for it, as text and ignoring case, such
    /// as `conclusion = ["failure", "timed_out"]`; any value where none is
    /// given.
    #[serde(default)]
    #[schemars(with = "Option<BTreeMap<String, Vec<String>>>", example = MatchFile::ATTRIBUTES)]
    pub(crate) attributes: Option<Spanned<BTreeMap<String, Vec<String>>>>,
    /// The areas, as globs ignoring case, any one of which the pull request
    /// an event is of falls in, each matching an area one of the route's
    /// sources names; an event whose areas are not known is not taken, and
    /// under `not` is never taken (ADR-0072).
    #[serde(default)]
    #[schemars(with = "Vec<String>")]
    pub(crate) areas: Vec<Spanned<String>>,
    /// What is never taken, written as a match is, whatever the rest of the
    /// match takes, such as `not = { attributes = { trigger = ["pull_request"] } }`.
    #[serde(default)]
    #[schemars(with = "Option<Box<MatchFile>>")]
    pub(crate) not: Option<Spanned<Box<MatchFile>>>,
}

impl MatchFile {
    /// Whether it narrows by nothing.
    pub(crate) fn is_empty(&self) -> bool {
        self.projects.is_empty()
            && self.repositories.is_empty()
            && self.owners.is_empty()
            && self.forges.is_empty()
            && self.hosts.is_empty()
            && self.topics.is_empty()
            && self.properties.is_empty()
            && self.areas.is_empty()
            && self
                .attributes
                .as_ref()
                .is_none_or(|attributes| attributes.get_ref().is_empty())
    }

    /// The attributes the schema shows as an example.
    const ATTRIBUTES: [(&str, [&str; 2]); 1] = [("conclusion", ["failure", "timed_out"])];

    /// The projects the schema shows as an example.
    const PROJECTS: [&str; 1] = ["plugin-*"];
}

impl RouteFile {
    /// The sources the schema shows as an example.
    const SOURCES: [&str; 1] = ["herald"];
    /// The destination the schema shows as an example.
    const DESTINATION: &str = "discord";

    /// The templates by dialect the schema shows as an example.
    fn templates_example() -> BTreeMap<String, String> {
        BTreeMap::from([("mastodon".to_owned(), "release-short".to_owned())])
    }
}

/// Whether a route pins, and how it finds its earlier pin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(untagged)]
#[serde(rename = "Pin")]
pub(crate) enum PinFile {
    /// `true` pins, finding the earlier pin by its marker; `false` pins
    /// nothing.
    Flag(bool),
    /// Pins, finding the earlier pin as this says.
    By(Pinning),
}

impl PinFile {
    /// How the route pins, where it does.
    pub(crate) fn pinning(self) -> Option<Pinning> {
        match self {
            Self::Flag(pins) => pins.then(Pinning::default),
            Self::By(pinning) => Some(pinning),
        }
    }
}

/// The keys every route may give, whatever its destination.
pub(crate) const ROUTE_KEYS: [&str; 21] = [
    "id",
    "sources",
    "kinds",
    "prereleases",
    "versions",
    "destination",
    "template",
    "templates",
    "crosspost",
    "thread",
    "pin",
    "mentions",
    "private",
    "vouch_unchecked",
    "late_release",
    "edits",
    "withdraw",
    "match",
    "group",
    "living",
    "area_paths",
];

/// A template: its title, and its body inline or in a file beside the config.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[serde(rename = "Template")]
#[schemars(extend("oneOf" = [{ "required": ["body"] }, { "required": ["file"] }]))]
pub(crate) struct TemplateFile {
    /// The title, on one line.
    #[schemars(with = "String", example = "{{ event.title }}")]
    pub(crate) title: Spanned<String>,
    /// The body, in Markdown.
    #[schemars(with = "Option<String>")]
    pub(crate) body: Option<Spanned<String>>,
    /// A file beside the config holding the body.
    #[schemars(with = "Option<String>")]
    pub(crate) file: Option<Spanned<String>>,
}

/// A GitHub App herald reads as, through installation tokens it mints
/// (ADR-0041).
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[serde(rename = "GitHubApp")]
pub(crate) struct GithubAppFile {
    /// The App's id, as its settings page shows it; give it or `client_id`.
    pub(crate) app_id: Option<NonZeroU64>,
    /// The App's client id, such as `Iv23liABCDEF`, which GitHub recommends
    /// over the id; give it or `app_id`.
    pub(crate) client_id: Option<String>,
    /// The App's private key, as GitHub hands it out (PKCS#1) or as PKCS#8.
    pub(crate) private_key: SecretRef,
    /// The REST API the App's tokens are minted at; `https://api.github.com`
    /// where absent, such as `https://ghe.example/api/v3` on GitHub
    /// Enterprise Server or `https://api.example.ghe.com` on GHE.com. Each
    /// source naming the App reads that host.
    pub(crate) api: Option<String>,
    /// The secret the App's webhook deliveries are signed with, taken at
    /// `/hooks/app/<name>`.
    pub(crate) webhook: Option<SecretRef>,
    /// Whether each installation token is kept in the store between runs,
    /// so a run skips minting one while it is good; false where absent.
    /// The token, good for an hour at most, is then a credential at rest:
    /// herald keeps it encrypted with AES-256-GCM under a key derived from
    /// `private_key`, bound to its record and its expiry, in a record the
    /// store seals too.
    pub(crate) store_token: Option<bool>,
}

/// Another herald this one trusts: who it is, how it is reached, and what
/// bounds it. What it may do sits beside each resource it may do it to.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[serde(rename = "Peer")]
pub(crate) struct PeerFile {
    /// Where the peer is served, such as `https://herald.example`:
    /// `https://`, its domain, and the path its endpoints are under where
    /// they are not at the root. Its discovery document is
    /// `/.well-known/herald` under it.
    #[schemars(example = "https://herald.example")]
    pub(crate) url: String,
    /// The peer's Ed25519 keys, each a Multikey as its discovery document
    /// lists it, such as `z6Mk…`, pinned by `herald peer add` once the
    /// operator confirms them. A key is never taken from the network unless
    /// a pinned key signed it.
    #[schemars(length(min = 1))]
    pub(crate) keys: Vec<String>,
    /// Whether a new key set the peer signs with a pinned key replaces the
    /// pinned keys: `manual` (the operator pins each new key) where absent,
    /// or `signed`.
    pub(crate) rotation: Option<Rotation>,
    /// How many requests the peer may make of this herald: "120/m" where
    /// absent, at most "480/m".
    pub(crate) rate: Option<Rate>,
    /// The most bytes of a body the peer sends that are read: "2MiB" where
    /// absent, at most "8MiB". A longer one is refused before it is parsed.
    pub(crate) most: Option<Size>,
    /// The most instances an object from the peer may have passed through:
    /// 2 where absent, at most 8.
    #[schemars(range(min = 1, max = Peer::HOPS_ALLOWED))]
    pub(crate) hops: Option<u8>,
    /// Whether private events may go to the peer; false where absent.
    pub(crate) private: Option<bool>,
    /// What the peer may do with this herald's resources, each also granted
    /// on the resource itself, such as `[{ resource = "sources.upstream",
    /// actions = ["push"] }]`. None where absent.
    #[serde(default)]
    pub(crate) grants: Vec<GrantFile>,
}

/// What a peer may do with one resource of this herald.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[serde(rename = "PeerGrant")]
pub(crate) struct GrantFile {
    /// The resource, such as "sources.upstream", a `herald` source.
    #[schemars(example = &"sources.upstream")]
    pub(crate) resource: String,
    /// What the peer may do with it, such as `["push"]`.
    pub(crate) actions: std::collections::BTreeSet<herald_core::Action>,
    /// When the grant ends, in RFC 3339 such as "2027-01-01T00:00:00Z" or
    /// as a TOML datetime with its offset; never where absent.
    #[serde(default, deserialize_with = "herald_core::until")]
    #[schemars(with = "Option<String>")]
    pub(crate) until: Option<jiff::Timestamp>,
    /// The identities the peer may give as they are, each the start of
    /// identities up to a '/'. None where absent.
    #[serde(default)]
    pub(crate) namespaces: std::collections::BTreeSet<String>,
}

/// How often a driver that keeps its own schedule polls and keeps house.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[serde(rename = "Schedule")]
pub(crate) struct ScheduleFile {
    /// How often sources are polled.
    pub(crate) poll: Option<Duration>,
    /// How often threads are kept.
    pub(crate) housekeep: Option<Duration>,
}

/// `[announce]`: how a run holds and keeps an announcement.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[serde(rename = "Announce")]
pub(crate) struct AnnounceFile {
    /// How long a run holds an announcement past its last write before
    /// another may finish it; "20m" where absent, longer than the longest a
    /// host lets one run go on, a Lambda's 15 minutes.
    pub(crate) lease: Option<Duration>,
    /// How often a run writes what a project last announced again when
    /// another write lands between its read and its write; 3 where absent.
    pub(crate) latest_attempts: Option<NonZeroUsize>,
}

/// `[doctor]`: how doctor judges the config.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[serde(rename = "Doctor")]
pub(crate) struct DoctorFile {
    /// What becomes of a route that takes a private or internal
    /// repository's events and is not `private`: "refuse" where absent, so
    /// the config does not load and no such event reaches it; "warn" to load
    /// it and have doctor name it; or "allow".
    pub(crate) private_to_public: Option<crate::config::Guard>,
    /// What becomes of a route that takes a private kind's events, such as
    /// `alert`'s, and says nothing of whether its place is private:
    /// "refuse" where absent, "warn" to load it and have doctor name it, or
    /// "allow".
    pub(crate) private_kinds: Option<crate::config::Guard>,
}

/// `[cli]`: what herald's commands print.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[serde(rename = "Cli")]
pub(crate) struct CliFile {
    /// Whether `herald queue` and `herald rerun` print the rows and plans of
    /// private and internal repositories, and `herald poll` and `herald
    /// announce` name their events by version or title, without
    /// `--private`: false where absent, so rows are withheld and counted and
    /// events named "(private)". Set it only where their output stays
    /// private: a CI job's log on a public repository is public. Advisories
    /// are printed with `--private` alone, whatever it says. A server's log
    /// names such events "(private)" always.
    pub(crate) private: Option<bool>,
}

/// `[log]`: what herald's log writes, and the secrets kept out of
/// everything herald writes.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[serde(rename = "Log")]
pub(crate) struct LogFile {
    /// The least grave line herald's log writes: "debug", every line and
    /// what herald saw as it went, none carrying what a private repository
    /// holds; "info", every line but those, where absent; "warn", what
    /// herald left undone, passed over or refused, and what stopped or
    /// failed; or "error", what stopped or failed alone. `HERALD_LOG_LEVEL`
    /// takes its place, and a command's `--log-level` takes the place of
    /// both.
    pub(crate) level: Option<crate::config::Level>,
    /// How herald's log writes each line: "text", as words, where absent;
    /// or "json", a JSON object a line holding its `time` (RFC 3339, UTC),
    /// its `level` and its `message`. `HERALD_LOG_FORMAT` takes its place, and a command's
    /// `--log-format` takes the place of both.
    pub(crate) format: Option<crate::config::LogFormat>,
    /// The fewest characters a secret's value holds to be written as
    /// `«secret:NAME»` in everything herald writes: 8 where absent, as a
    /// shorter value would rewrite ordinary words. On GitHub Actions every
    /// secret is masked whatever its length.
    pub(crate) scrub_from: Option<NonZeroUsize>,
    /// How many secret values herald holds to scrub before its log says,
    /// once, that it holds more: 4096 where absent. Past it every value is
    /// still held and scrubbed until it can no longer be valid; raise it
    /// where a run sends that many secrets by design.
    pub(crate) scrub_warn_past: Option<NonZeroUsize>,
}

/// `[run]`: how many routes and sources a run goes through at once.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[serde(rename = "Run")]
pub(crate) struct RunFile {
    /// How many routes an event is announced to at once; 4 where absent,
    /// and 1 announces to one route after another in the file's order.
    pub(crate) routes: Option<NonZeroUsize>,
    /// How many sources a poll reads at once; 4 where absent, and 1 reads
    /// one source after another.
    pub(crate) sources: Option<NonZeroUsize>,
}

/// Where `herald server` keeps the keys inbound activities are checked with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[serde(rename = "KeyCache")]
pub(crate) enum KeyCacheFile {
    /// In memory, lost when the server stops.
    Memory,
    /// In the store, kept across restarts and shared by every instance.
    Store,
}

/// `[server.keys]`: the keys inbound activities are checked with.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[serde(rename = "Keys")]
pub(crate) struct KeysFile {
    /// Where they are kept: "memory", the default, or "store".
    #[schemars(with = "Option<KeyCacheFile>")]
    pub(crate) cache: Option<Spanned<KeyCacheFile>>,
    /// How long a key is used before its document is read again; 1h where
    /// absent.
    pub(crate) refresh_after: Option<Duration>,
    /// How long after a key is read an activity that does not hold under it
    /// is refused rather than having it read again; 1m where absent.
    pub(crate) reread_after: Option<Duration>,
    /// The most kept in memory, the one used longest ago forgotten first;
    /// 1024 where absent.
    pub(crate) most: Option<NonZeroUsize>,
    /// The most of a key's or an actor's document read; "256KiB" where
    /// absent, and at most that: a setting may only lower it.
    #[schemars(with = "Option<Size>")]
    pub(crate) document: Option<Spanned<Size>>,
}

/// `[http]`: how herald's own requests reach the services a config names.
/// None of it applies to the hosts herald reaches on others' behalf, an
/// `ActivityPub` actor's key or a follower's inbox (ADR-0007).
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[serde(rename = "Http")]
pub(crate) struct HttpFile {
    /// How long a connection may take to open; "10s" where absent.
    #[schemars(with = "Option<Duration>")]
    pub(crate) connect: Option<Spanned<Duration>>,
    /// How long a request may take, from sending it to the end of its
    /// answer; "30s" where absent. `HERALD_TIMEOUT_MS` takes its place.
    pub(crate) total: Option<Duration>,
    /// The proxy herald's own requests go through: "environment", the
    /// host's `HTTPS_PROXY` and `NO_PROXY` where it honours them; "none";
    /// or a proxy's http or https URL. "environment" where absent.
    #[schemars(with = "Option<String>")]
    pub(crate) proxy: Option<Spanned<String>>,
    /// A PEM file of the certificate authorities herald's own requests trust
    /// beside the Mozilla roots, read beside the config, such as a GitHub
    /// Enterprise Server's private CA.
    #[schemars(with = "Option<String>")]
    pub(crate) ca_file: Option<Spanned<String>>,
    /// The most bytes of a response's body herald reads; "32MiB" where
    /// absent, and at most "128MiB". A host reached on others' behalf is
    /// read no further than "32MiB" whatever this says.
    #[schemars(with = "Option<Size>")]
    pub(crate) body: Option<Spanned<Size>>,
}

/// `[server]`: how `herald server` listens, and what it takes on at once.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[serde(rename = "Server")]
pub(crate) struct ServerFile {
    /// The address and port it listens on; `--listen` and `HERALD_LISTEN`
    /// take its place. "127.0.0.1:8080" where absent, for a reverse proxy
    /// in front of it.
    #[schemars(with = "Option<String>")]
    pub(crate) listen: Option<Spanned<String>>,
    /// The path every endpoint is served under, for a reverse proxy that
    /// serves herald beside other things, as "/herald"; the root where
    /// absent. herald's ActivityPub actors' ids are under it too.
    #[schemars(with = "Option<String>")]
    pub(crate) base: Option<Spanned<String>>,
    /// herald's public address as forges and platforms reach it, such as
    /// `https://herald.example`, with `base` after it where one is given:
    /// what a webhook herald registers points at. A setup command's `--url`
    /// takes its place.
    #[schemars(with = "Option<String>")]
    pub(crate) url: Option<Spanned<String>>,
    /// How many pieces of work it does at once, each an announcement, an
    /// activity or a command a request leaves; 8 where absent.
    pub(crate) jobs: Option<NonZeroUsize>,
    /// How many pieces of work may wait for a place; a request leaving one
    /// while as many wait is answered 503. As many as `jobs` where absent.
    pub(crate) queue: Option<NonZeroUsize>,
    /// How many connections it holds at once; 256 where absent.
    pub(crate) connections: Option<NonZeroUsize>,
    /// The most an inbound request's body may hold; "1MiB" where absent,
    /// and at most "4MiB".
    #[schemars(with = "Option<Size>")]
    pub(crate) body: Option<Spanned<Size>>,
    /// How many actor documents it reads at once; 4 where absent.
    pub(crate) fetches: Option<NonZeroUsize>,
    /// How many autocompletions a session answers at once; one arriving
    /// while as many are answered is dropped. 4 where absent.
    pub(crate) completions: Option<NonZeroUsize>,
    /// How long a session a destination holds hears nothing before it
    /// checks the connection is alive, and then as long again before it
    /// opens another; "30s" where absent.
    pub(crate) silence: Option<Duration>,
    /// The keys inbound activities are checked with.
    #[serde(default)]
    pub(crate) keys: KeysFile,
    /// What a signed inbound request may carry and still be checked.
    #[serde(default)]
    pub(crate) inbound: InboundFile,
    /// How long a request whose work found no place is asked to wait, in
    /// its `Retry-After`; "30s" where absent.
    pub(crate) busy_retry: Option<Duration>,
    /// How long a connection may take to send a request's headers; "30s"
    /// where absent.
    pub(crate) read: Option<Duration>,
    /// How long a connection is held at most, its one request answered;
    /// "90s", or `read` where that is longer, where absent; at least `read`.
    #[schemars(with = "Option<Duration>")]
    pub(crate) held: Option<Spanned<Duration>>,
    /// How long the server waits after a connection could not be accepted;
    /// "100ms" where absent.
    pub(crate) accept_pause: Option<Duration>,
    /// How a session a destination holds waits before it connects again.
    #[serde(default)]
    pub(crate) sessions: SessionsFile,
    /// How the work a request leaves keeps within its host's limits.
    #[serde(default)]
    pub(crate) work: WorkFile,
}

/// `[server.work]`: how the work a request leaves keeps within its host's
/// limits. herald starts no send that could pass them, and the next run
/// sends what is left.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[serde(rename = "Work")]
pub(crate) struct WorkFile {
    /// How long the work may run after the answer; where absent, Cloudflare
    /// Workers' 30 seconds of `waitUntil` there, and unbounded on every
    /// other host.
    #[schemars(with = "Option<Duration>")]
    pub(crate) after: Option<Spanned<Duration>>,
    /// How much of `after` must be left for a send to start; "10s" where
    /// absent, and less than `after`.
    pub(crate) spare: Option<Duration>,
    /// The most outbound requests a request and the work it leaves may
    /// make, counted from when herald began answering it; where absent,
    /// Fastly Compute's 32 backend requests there (set 10 on a trial), and
    /// unbounded on every other host.
    #[schemars(with = "Option<NonZeroUsize>")]
    pub(crate) requests: Option<Spanned<NonZeroUsize>>,
    /// How many of `requests` must be left for a send to start: the send,
    /// and the writes after it where the store writes over HTTP; 4 where
    /// absent, and fewer than `requests`.
    pub(crate) spare_requests: Option<usize>,
}

/// `[server.sessions]`: how a session a destination holds waits before it
/// connects again, each wait double the last up to the longest, and how old
/// a command one taken up again answers.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[serde(rename = "Sessions")]
pub(crate) struct SessionsFile {
    /// The first wait after a session fails to connect; "1s" where absent.
    pub(crate) first_wait: Option<Duration>,
    /// The longest wait between attempts; "1m" where absent, and at least
    /// `first_wait`.
    #[schemars(with = "Option<Duration>")]
    pub(crate) longest_wait: Option<Spanned<Duration>>,
    /// Whether each wait is between half of it and all of it, so sessions
    /// that failed together do not all connect again at once; true where
    /// absent.
    pub(crate) jitter: Option<bool>,
    /// How long ago a command a session taken up again answers may have
    /// been given; one given earlier is skipped and logged; "10m" where
    /// absent.
    pub(crate) stale: Option<Duration>,
}

/// `[server.inbound]`: what a signed inbound request may carry and still be
/// checked. Each bound may be at most four times its default, and doctor
/// names one set above it.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[serde(rename = "Inbound")]
pub(crate) struct InboundFile {
    /// How far a signed timestamp may lie from now, either way, before the
    /// request is taken for a replay; "5m" where absent, and at most "20m".
    #[schemars(with = "Option<Duration>")]
    pub(crate) replay_window: Option<Spanned<Duration>>,
    /// The most signatures a request may carry, each checked in turn; 4
    /// where absent, and at most 16.
    #[schemars(with = "Option<NonZeroUsize>", range(max = Inbound::MOST.signatures))]
    pub(crate) signatures: Option<Spanned<NonZeroUsize>>,
    /// The most headers one ActivityPub signature may cover; 16 where
    /// absent, at least the 4 it must cover, and at most 64.
    #[schemars(
        with = "Option<NonZeroUsize>",
        range(min = Inbound::LEAST_COVERED, max = Inbound::MOST.covered)
    )]
    pub(crate) covered: Option<Spanned<NonZeroUsize>>,
}

/// `[render]`: how far a template may go as it renders. Each bound may be
/// at most four times its default, and doctor names one set above it.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[serde(rename = "Render")]
pub(crate) struct RenderFile {
    /// The fuel one render may burn; 100000 where absent, and at most
    /// 400000.
    #[schemars(with = "Option<NonZeroU64>", range(max = Sandbox::MOST.fuel))]
    pub(crate) fuel: Option<Spanned<NonZeroU64>>,
    /// How deep its loops and calls may nest; 16 where absent, and at most
    /// 64.
    #[schemars(with = "Option<NonZeroUsize>", range(max = Sandbox::MOST.recursion))]
    pub(crate) recursion: Option<Spanned<NonZeroUsize>>,
    /// The most it may write; "256KiB" where absent, and at most "1MiB".
    #[schemars(with = "Option<Size>")]
    pub(crate) output: Option<Spanned<Size>>,
}
