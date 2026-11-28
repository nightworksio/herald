use std::sync::Arc;

use herald_core::{
    Address, Cursor, Event, KeyForm, Polled, PrereleaseRule, Scheme, Selection, Signal, Signed,
    Validators, unreserved,
};
use herald_core::{media, status};
use herald_kinds::{Credential, Hooked, Minted, Unpolled};
use herald_ports::{Method, Request, Response, Secret, SecretText};
use serde::Deserialize;

use crate::options::{Signals, SourceOptions};
use herald_wire::{Reading, SourceError};

use crate::wire::{ForgeRelease, event, read};
use crate::{forgejo, github, gitlab};

/// A GitHub webhook: `X-Hub-Signature-256: sha256=<hex>`, an HMAC-SHA256 of the body.
const GITHUB: Scheme = Scheme {
    name: "github",
    signed: Signed::Body {
        headers: &["X-Hub-Signature-256"],
        prefix: "sha256=",
    },
    key: KeyForm::Bytes,
};

/// A GitLab webhook signed with a signing token, as Standard Webhooks sign.
const GITLAB: Scheme = Scheme {
    name: "gitlab",
    signed: Signed::StandardWebhooks {
        id: "webhook-id",
        timestamp: "webhook-timestamp",
        signature: "webhook-signature",
    },
    key: KeyForm::Whsec,
};

/// A Forgejo or Gitea webhook: `X-Forgejo-Signature` or `X-Gitea-Signature`, the hex HMAC-SHA256 of the body.
const FORGEJO: Scheme = Scheme {
    name: "forgejo",
    signed: Signed::Body {
        headers: &["X-Forgejo-Signature", "X-Gitea-Signature"],
        prefix: "",
    },
    key: KeyForm::Bytes,
};

/// Why a GitLab webhook may go unserved (ADR-0007).
const UNSIGNED: &str = "GitLab signs a webhook only with a signing token, which an older self-managed instance does not offer; where it cannot sign, remove webhook and let herald poll the project.";

/// A forge whose releases herald announces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Forge {
    /// GitHub, or a GitHub Enterprise Server.
    GitHub,
    /// GitLab.com, or a self-managed GitLab.
    GitLab,
    /// Forgejo or Gitea, such as Codeberg.
    Forgejo,
}

/// The most names a GitLab project's path holds: its top group, 20
/// subgroups and its own name.
const GITLAB_DEPTH: usize = 22;

/// How a forge's API is asked, and what each of its answers is called in
/// an error.
pub(crate) struct Api {
    /// The base URL of its public instance.
    pub(crate) url: &'static str,
    /// The query parameter that sizes a page.
    pub(crate) page: &'static str,
    /// The path to a release, before its tag.
    pub(crate) tagged: &'static str,
    pub(crate) event: &'static str,
    pub(crate) repository: &'static str,
    pub(crate) releases: &'static str,
    pub(crate) release: &'static str,
    pub(crate) tags: &'static str,
    pub(crate) tag: &'static str,
    /// What its listing of an owner's repositories is called.
    pub(crate) listing: &'static str,
    /// What a file it serves raw is called.
    pub(crate) file: &'static str,
    /// What its listing of packages is called.
    pub(crate) packages: &'static str,
    /// What its listing of a package's versions is called.
    pub(crate) versions: &'static str,
    /// The page size an owner's listing asks for where `listed_page` is
    /// absent.
    pub(crate) listed_page: std::num::NonZeroUsize,
}

/// Why a webhook delivery of another event announces nothing.
const OTHER_EVENT: &str = "the delivery is not of a release, a tag or a package";
/// What a delivery to an owner source of anything else is answered.
const OWNED_OTHER_EVENT: &str =
    "the delivery is not of a release, a tag, a package or the owner's repositories";
/// Why a webhook delivery of a repository the owner source does not take
/// announces nothing.
pub(crate) const UNSELECTED: &str = "the repository is not one the source takes";
/// Why a webhook delivery of an unpublished release announces nothing.
const UNPUBLISHED: &str = "the release is not published, or is a draft";
/// Why a webhook delivery of a release announces nothing where the source
/// announces no releases of its repository.
const UNSIGNALLED: &str = "the source announces no releases of the repository";
/// Why a webhook delivery of a tag announces nothing where the source
/// announces no tags of its repository.
const UNTAGGED: &str = "the source announces no tags of the repository";
/// Why a webhook delivery of a package announces nothing where the source
/// announces no packages of its repository.
const UNPACKAGED: &str = "the source announces no packages of the repository";
/// Why a webhook delivery of a package linked to no repository announces
/// nothing.
const UNLINKED: &str = "the package is linked to no repository";
/// The actions of GitHub's `registry_package` and `package` deliveries of a
/// version published.
const PACKAGE_ACTIONS: [&str; 2] = ["published", "updated"];
/// The action of Forgejo's `package` delivery of a version published.
const PACKAGE_CREATED: &str = "created";
/// The commit GitLab names as a tag's after a push deleted it.
const DELETED: &str = "0000000000000000000000000000000000000000";

/// A repository, as its forge names it: its numeric id outlives a rename.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Repository {
    /// The numeric id.
    pub id: u64,
    /// The name, without its owner.
    pub name: String,
    /// Its web page, where the forge names it.
    #[serde(default, rename = "html_url", alias = "web_url")]
    pub page: Option<String>,
}

/// The token a request carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    /// A token the config names.
    Named(Secret),
    /// The CI run's own token, such as `GITHUB_TOKEN` or GitLab's
    /// `CI_JOB_TOKEN`.
    Run(Secret),
    /// A GitHub App's installation token, refreshed before each round, and
    /// none where its minting failed (ADR-0041).
    Minted(Arc<Minted>),
}

impl Token {
    /// What `credential` reads with.
    #[must_use]
    pub fn of(credential: Credential) -> Self {
        match credential {
            Credential::Named(secret) => Self::Named(secret),
            Credential::Minted(minted) => Self::Minted(minted),
        }
    }
}

/// One repository on one forge, whose releases are asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Origin {
    forge: Forge,
    api: String,
    repository: String,
    reading: Reading,
}

impl Forge {
    /// Who signs its webhooks, and how.
    #[must_use]
    pub const fn scheme(self) -> Scheme {
        match self {
            Self::GitHub => GITHUB,
            Self::GitLab => GITLAB,
            Self::Forgejo => FORGEJO,
        }
    }

    /// What a webhook from the forge leaves to it, where anything.
    #[must_use]
    pub const fn unsigned(self) -> Option<&'static str> {
        match self {
            Self::GitLab => Some(UNSIGNED),
            Self::GitHub | Self::Forgejo => None,
        }
    }

    /// How many names a repository's path holds on the forge.
    #[must_use]
    pub const fn names(self) -> std::ops::RangeInclusive<usize> {
        match self {
            Self::GitHub | Self::Forgejo => 2..=2,
            Self::GitLab => 2..=GITLAB_DEPTH,
        }
    }

    /// How many names an owner's path holds on the forge: a GitLab group's
    /// holds its top group and up to 20 subgroups.
    #[must_use]
    pub const fn owners(self) -> std::ops::RangeInclusive<usize> {
        match self {
            Self::GitHub | Self::Forgejo => 1..=1,
            Self::GitLab => 1..=GITLAB_DEPTH - 1,
        }
    }

    /// How an owner is written on the forge.
    #[must_use]
    pub const fn owner_shape(self) -> &'static str {
        match self {
            Self::GitHub | Self::Forgejo => "an organization's or a user's name",
            Self::GitLab => "a group's full path, such as group/subgroup, or a user's name",
        }
    }

    /// How a repository is written on the forge.
    #[must_use]
    pub const fn shape(self) -> &'static str {
        match self {
            Self::GitHub | Self::Forgejo => "owner/name",
            Self::GitLab => "the project's full path, such as group/subgroup/name",
        }
    }

    /// Every forge.
    pub const ALL: [Self; 3] = [Self::GitHub, Self::GitLab, Self::Forgejo];

    /// Its name, as a source's `kind` in `herald.toml` writes it.
    #[must_use]
    pub const fn kind(self) -> &'static str {
        match self {
            Self::GitHub => "github",
            Self::GitLab => "gitlab",
            Self::Forgejo => "forgejo",
        }
    }

    pub(crate) const fn api(self) -> &'static Api {
        match self {
            Self::GitHub => &github::API,
            Self::GitLab => &gitlab::API,
            Self::Forgejo => &forgejo::API,
        }
    }

    /// What a webhook delivery with `headers` and `payload` asks herald to
    /// announce for the source of `repository`, which announces `signals`:
    /// the release it publishes, a poll where it made a tag or published a
    /// package version the source announces, or nothing where it is of
    /// another event, of an unpublished release or a draft, of a package
    /// linked to no repository, or of a signal the source does not
    /// announce.
    ///
    /// # Errors
    ///
    /// When the payload is too large, is not the release event its headers
    /// name, makes no event, or is of another repository than `repository`.
    pub fn hooked(
        self,
        (headers, payload): (&[(&str, &str)], &[u8]),
        (repository, host): (&str, &str),
        signals: &Signals,
    ) -> Result<Hooked, SourceError> {
        if self.of_a_package(headers, payload) {
            let what = self.api().packages;
            let Some(path) = self.path(what, payload) else {
                return Ok(Hooked::Nothing(UNLINKED));
            };
            if !path.eq_ignore_ascii_case(repository) {
                return Err(SourceError::Unreadable {
                    what,
                    reason: format!("it is not of {repository}, the repository its source names"),
                });
            }
            return Ok(Self::packaged(signals));
        }
        if self.of_a_tag(headers, payload) {
            let what = self.api().tag;
            if !self
                .path(what, payload)
                .is_some_and(|path| path.eq_ignore_ascii_case(repository))
            {
                return Err(SourceError::Unreadable {
                    what,
                    reason: format!("it is not of {repository}, the repository its source names"),
                });
            }
            return Ok(Self::tagged(signals));
        }
        if !self.of_a_release(headers) {
            return Ok(Hooked::Nothing(OTHER_EVENT));
        }
        let event = self.event(payload, signals.prerelease)?;
        let what = self.api().event;
        let path = self.path(what, payload);
        if !path.is_some_and(|path| path.eq_ignore_ascii_case(repository)) {
            return Err(SourceError::Unreadable {
                what,
                reason: format!("it is not of {repository}, the repository its source names"),
            });
        }
        if !signals.takes(Signal::Release) {
            return Ok(Hooked::Nothing(UNSIGNALLED));
        }
        let origin = crate::delivered::origin(
            self,
            host,
            crate::delivered::delivered(self, what, payload)?,
        );
        Ok(event.map_or(Hooked::Nothing(UNPUBLISHED), |event| {
            Hooked::Event(event.from_origin(origin))
        }))
    }

    /// The full path of the repository a delivery's `payload` is of.
    fn path(self, what: &'static str, payload: &[u8]) -> Option<String> {
        match self {
            Self::GitHub | Self::Forgejo => github::path(what, payload),
            Self::GitLab => gitlab::path(what, payload),
        }
    }

    /// What a delivery of a tag asks of a source announcing `signals`: a
    /// poll, where they take tags.
    fn tagged(signals: &Signals) -> Hooked {
        if signals.takes(Signal::Tag) {
            Hooked::Poll
        } else {
            Hooked::Nothing(UNTAGGED)
        }
    }

    /// What a delivery of a package asks of a source announcing `signals`:
    /// a poll, where they take packages.
    fn packaged(signals: &Signals) -> Hooked {
        if signals.takes(Signal::Package) {
            Hooked::Poll
        } else {
            Hooked::Nothing(UNPACKAGED)
        }
    }

    /// Whether a delivery with `headers` and `payload` is of a package
    /// version published: GitHub's `registry_package` or `package`
    /// published or updated, or Forgejo's `package` created.
    fn of_a_package(self, headers: &[(&str, &str)], payload: &[u8]) -> bool {
        #[derive(Deserialize)]
        struct Acted {
            action: String,
        }
        let (kinds, actions): (&[&str], &[&str]) = match self {
            Self::GitHub => (&["registry_package", "package"], &PACKAGE_ACTIONS),
            Self::Forgejo => (&["package"], &[PACKAGE_CREATED]),
            Self::GitLab => return false,
        };
        let of_kind = self
            .event_name(headers)
            .is_some_and(|name| kinds.contains(&name));
        of_kind
            && read::<Acted>(self.api().packages, payload)
                .is_ok_and(|acted| actions.contains(&acted.action.as_str()))
    }

    /// Whether a delivery with `headers` and `payload` is of a tag made:
    /// GitHub's and Forgejo's `create` of a tag, or GitLab's tag push that
    /// did not delete one.
    fn of_a_tag(self, headers: &[(&str, &str)], payload: &[u8]) -> bool {
        #[derive(Deserialize)]
        struct Created {
            ref_type: Option<String>,
            after: Option<String>,
        }
        let kind = match self {
            Self::GitHub | Self::Forgejo => "create",
            Self::GitLab => "Tag Push Hook",
        };
        let of_kind = self.event_name(headers) == Some(kind);
        of_kind
            && read::<Created>(self.api().tag, payload).is_ok_and(|created| match self {
                Self::GitHub | Self::Forgejo => created.ref_type.as_deref() == Some("tag"),
                Self::GitLab => created.after.as_deref() != Some(DELETED),
            })
    }

    /// Whether a delivery with `headers` is of a release.
    fn of_a_release(self, headers: &[(&str, &str)]) -> bool {
        self.event_name(headers) == Some(self.release_event())
    }

    /// Whether the repositories of `owner`, with its subgroups' where
    /// `subgroups` says so on GitLab, include those `holder` holds.
    pub(crate) fn holds(self, (owner, subgroups): (&str, bool), holder: &str) -> bool {
        let (holder, lowered) = (holder.to_ascii_lowercase(), owner.to_ascii_lowercase());
        holder == lowered
            || (subgroups && self == Self::GitLab && holder.starts_with(&format!("{lowered}/")))
    }

    /// What a webhook delivery of `owner`'s, `payload` with `headers`, asks
    /// herald to announce: the event of a release published in a
    /// repository `selection` admits by what the payload names, to announce
    /// once the owner's last membership record holds it, where the signals
    /// `signals` gives for the repository's name take its releases, told a
    /// pre-release as they say; or a poll where it made a tag or published
    /// a package version such a repository's signals take. A GitLab
    /// group's repositories include its subgroups' where `subgroups` says
    /// so.
    ///
    /// # Errors
    ///
    /// When the payload is not the forge's release event, or is of a
    /// repository `owner` does not hold.
    pub fn hooked_owned(
        self,
        (headers, payload): (&[(&str, &str)], &[u8]),
        (owner, subgroups): (&str, bool),
        (selection, signals): (&Selection, &dyn Fn(&str) -> Signals),
    ) -> Result<Hooked, SourceError> {
        let holds = |holder: &str| self.holds((owner, subgroups), holder);
        let unheld = |what| SourceError::Unreadable {
            what,
            reason: format!("it is not of a repository {owner}, the owner its source names, holds"),
        };
        if let Some(moved) = crate::moved::moved(self, headers, payload) {
            return if holds(&moved?) {
                Ok(Hooked::Relist)
            } else {
                Err(unheld(crate::moved::MOVED))
            };
        }
        let package = self.of_a_package(headers, payload);
        let tag = !package && self.of_a_tag(headers, payload);
        if !package && !tag && !self.of_a_release(headers) {
            return Ok(Hooked::Nothing(OWNED_OTHER_EVENT));
        }
        let what = if package {
            self.api().packages
        } else if tag {
            self.api().tag
        } else {
            self.api().event
        };
        if package && self.path(what, payload).is_none() {
            return Ok(Hooked::Nothing(UNLINKED));
        }
        let (id, delivered) = crate::delivered::delivered(self, what, payload)?;
        let signals = signals(&delivered.name);
        let event = if tag || package {
            None
        } else {
            self.event(payload, signals.prerelease)?
        };
        if !delivered
            .path
            .rsplit_once('/')
            .is_some_and(|(holder, _)| holds(holder))
        {
            return Err(unheld(what));
        }
        if !selection.admits(&delivered) {
            return Ok(Hooked::Nothing(UNSELECTED));
        }
        if package {
            return Ok(Self::packaged(&signals));
        }
        if tag {
            return Ok(Self::tagged(&signals));
        }
        if !signals.takes(Signal::Release) {
            return Ok(Hooked::Nothing(UNSIGNALLED));
        }
        Ok(
            event.map_or(Hooked::Nothing(UNPUBLISHED), |event| Hooked::Member {
                visibility: delivered.visibility,
                event,
                id,
            }),
        )
    }

    /// The event a release event's payload announces, as a webhook or as
    /// the forge's CI hands it to a run, its pre-release told by `rule`: a
    /// published release that is not a draft, or nothing.
    ///
    /// # Errors
    ///
    /// When the payload is too large, is not the forge's release event, or
    /// makes no event.
    pub fn event(self, payload: &[u8], rule: PrereleaseRule) -> Result<Option<Event>, SourceError> {
        let what = self.api().event;
        let published = match self {
            Self::GitHub | Self::Forgejo => github::published(what, payload)?,
            Self::GitLab => gitlab::published(what, payload)?,
        };
        match published {
            Some((repository, found)) => event(what, &repository, found, rule),
            None => Ok(None),
        }
    }

    /// The repository the forge's answer to [`Origin::repository_request`]
    /// describes.
    ///
    /// # Errors
    ///
    /// When the forge did not answer with the repository.
    pub fn repository(self, response: &Response) -> Result<Repository, SourceError> {
        let what = self.api().repository;
        crate::limited::answered(self, what, response)?;
        read(what, &response.body)
    }

    fn found(self, what: &'static str, body: &[u8]) -> Result<ForgeRelease, SourceError> {
        match self {
            Self::GitHub | Self::Forgejo => github::found(what, body),
            Self::GitLab => gitlab::found(what, body),
        }
    }

    fn all_found(
        self,
        what: &'static str,
        limit: usize,
        body: &[u8],
    ) -> Result<Vec<ForgeRelease>, SourceError> {
        match self {
            Self::GitHub | Self::Forgejo => github::all_found(what, limit, body),
            Self::GitLab => gitlab::all_found(what, limit, body),
        }
    }

    /// The event the release of `repository` in the forge's answer to
    /// [`Origin::release_request`] announces, its pre-release told by
    /// `rule`: a published release that is not a draft, or nothing.
    ///
    /// # Errors
    ///
    /// When the forge did not answer with a release, or it makes no event.
    pub fn release(
        self,
        repository: &Repository,
        response: &Response,
        rule: PrereleaseRule,
    ) -> Result<Option<Event>, SourceError> {
        let what = self.api().release;
        crate::limited::answered(self, what, response)?;
        event(what, repository, self.found(what, &response.body)?, rule)
    }

    /// The published releases of `repository` in the forge's answer to
    /// [`Origin::releases_request`], each pre-release told by `rule`,
    /// parsing no more than `limit` bytes.
    ///
    /// # Errors
    ///
    /// When the forge did not answer with releases, or one makes no event.
    pub fn releases(
        self,
        repository: &Repository,
        response: &Response,
        (limit, rule): (usize, PrereleaseRule),
    ) -> Result<Polled, SourceError> {
        let what = self.api().releases;
        crate::limited::answered(self, what, response)?;
        let events = self
            .all_found(what, limit, &response.body)?
            .into_iter()
            .filter_map(|found| event(what, repository, found, rule).transpose())
            .collect::<Result<Vec<Event>, SourceError>>()?;
        let validators = validators(response);
        Ok(Polled {
            events,
            etag: validators.etag,
            last_modified: validators.last_modified,
            ..Polled::default()
        })
    }

    /// Whether the forge's answer to [`Origin::releases_request`] says
    /// nothing changed since the cursor's validators.
    #[must_use]
    pub fn unchanged(response: &Response) -> bool {
        response.status == status::NOT_MODIFIED
    }
}

/// The validators `response` returns.
pub(crate) fn validators(response: &Response) -> Validators {
    Validators {
        etag: response.header("etag").map(ToOwned::to_owned),
        last_modified: response.header("last-modified").map(ToOwned::to_owned),
    }
}

/// `request` to `forge`, as the forge is asked: with the headers it reads,
/// and carrying `token` where there is one.
pub(crate) fn authorized(forge: Forge, request: Request, token: Option<&Token>) -> Request {
    let request = match forge {
        Forge::GitHub => herald_wire::github::headed(request),
        Forge::GitLab | Forge::Forgejo => request.accepting(media::JSON).identified(),
    };
    let current;
    let token = match token {
        Some(Token::Minted(minted)) => {
            current = minted.current().map(Token::Named);
            current.as_ref()
        }
        token => token,
    };
    match (forge, token) {
        (_, None | Some(Token::Minted(_))) => request,
        (Forge::GitLab, Some(Token::Run(token))) => {
            request.header("JOB-TOKEN", SecretText::secret(token.clone()))
        }
        (Forge::Forgejo, Some(Token::Named(token) | Token::Run(token))) => {
            herald_wire::forgejo::authorized(request, token.clone())
        }
        (Forge::GitHub | Forge::GitLab, Some(Token::Named(token) | Token::Run(token))) => {
            request.bearer(token.clone())
        }
    }
}

/// The API base URL `options` give on `forge`, without a trailing `/`.
pub(crate) fn api_of(forge: Forge, options: &SourceOptions) -> String {
    options
        .api
        .as_deref()
        .unwrap_or(forge.api().url)
        .trim_end_matches('/')
        .to_owned()
}

/// The host a forge's repositories are named on, from its API's base URL
/// `api`: without the `api.` label GitHub's and a GHE.com tenant's API
/// carry.
pub(crate) fn host_of(forge: Forge, api: &str) -> String {
    let authority = Address::parse(api)
        .as_ref()
        .map(Address::authority)
        .unwrap_or_default();
    match forge {
        Forge::GitHub => authority
            .strip_prefix("api.")
            .unwrap_or(&authority)
            .to_owned(),
        Forge::GitLab | Forge::Forgejo => authority,
    }
}

/// `path`, each of its `/`-separated parts escaped.
pub(crate) fn segments(path: &str) -> String {
    path.split('/')
        .map(unreserved)
        .collect::<Vec<_>>()
        .join("/")
}

/// What a request names a tag.
pub(crate) const TAG: &str = "tag";
/// What a request names the repository it asks of.
pub(crate) const REPOSITORY: &str = "repository's path";
/// What a request names the owner it asks of.
pub(crate) const OWNER: &str = "owner";
/// What a request names a commit or tag object.
const SHA: &str = "object id";
/// What a request names a file.
const FILE: &str = "file path";

/// `text` as one segment of a URL's path, `/` encoded too, as the `what`
/// a request names; or why it names nothing (ADR-0067).
///
/// # Errors
///
/// Where it is empty, `.` or `..`.
pub(crate) fn whole(text: &str, what: &str) -> Result<String, Unpolled> {
    herald_core::segment(text).ok_or_else(|| {
        Unpolled::Unreadable(format!(
            "the {what} \"{text}\" is empty or a dot segment, or holds one, which names nothing in a URL path, so it is not asked for"
        ))
    })
}

/// `path`, each of its `/`-separated parts through the one segment
/// encoder, as the `what` a request names; or why it names nothing
/// (ADR-0067).
///
/// # Errors
///
/// Where a part is empty, `.` or `..`.
pub(crate) fn named(path: &str, what: &str) -> Result<String, Unpolled> {
    path.split('/')
        .map(|part| whole(part, what))
        .collect::<Result<Vec<_>, _>>()
        .map(|parts| parts.join("/"))
}

/// The base URL of an [`Origin`]'s repository, made only by
/// [`Origin::checked`] once its path is checked.
pub(crate) struct Checked<'o> {
    origin: &'o Origin,
    base: String,
}

impl Checked<'_> {
    /// The request for `path` under the repository, carrying `token`.
    pub(crate) fn get(&self, path: &str, token: Option<&Token>) -> Request {
        let request = Request::new(
            Method::Get,
            SecretText::plain(format!("{}{path}", self.base)),
        )
        .at_most(self.origin.reading.parse);
        authorized(self.origin.forge, request, token)
    }

    /// The request sending `body` as JSON to `path` under the repository
    /// with `method`, carrying `token`.
    pub(crate) fn sent(
        &self,
        method: Method,
        path: &str,
        body: &serde_json::Value,
        token: &Token,
    ) -> Request {
        let request = crate::graphql::sent(
            method,
            &format!("{}{path}", self.base),
            body,
            self.origin.reading.parse,
        );
        authorized(self.origin.forge, request, Some(token))
    }
}

impl Origin {
    /// `repository` on `forge`, asked as `options` say.
    #[must_use]
    pub fn new(forge: Forge, options: &SourceOptions, repository: &str) -> Self {
        Self {
            forge,
            api: api_of(forge, options),
            repository: repository.to_owned(),
            reading: options.reading(),
        }
    }

    /// The host its repositories are named on.
    #[must_use]
    pub fn host(&self) -> String {
        host_of(self.forge, &self.api)
    }

    /// Where the events of the repository the forge's answer to
    /// [`Origin::repository_request`] describes come from.
    ///
    /// # Errors
    ///
    /// When the answer is not a repository.
    pub fn described(&self, response: &Response) -> Result<herald_core::Origin, SourceError> {
        crate::delivered::answered_origin(
            self.forge,
            (&self.host(), self.forge.api().repository, &self.repository),
            &response.body,
        )
    }

    /// How a poll of it reads.
    #[must_use]
    pub const fn reading(&self) -> Reading {
        self.reading
    }

    /// The forge.
    #[must_use]
    pub const fn forge(&self) -> Forge {
        self.forge
    }

    /// The repository's full path.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.repository
    }

    /// The base URL of everything it asks.
    #[must_use]
    pub fn api(&self) -> &str {
        &self.api
    }

    /// The request for `path` under the repository, carrying `token`, the
    /// repository's path through the one segment encoder.
    ///
    /// # Errors
    ///
    /// Where a part of the repository's path is empty, `.` or `..`, which
    /// would reach another path of the API (ADR-0067).
    pub(crate) fn get(&self, path: &str, token: Option<&Token>) -> Result<Request, Unpolled> {
        Ok(self.checked()?.get(path, token))
    }

    /// The base URL of the repository, its path through the one segment
    /// encoder, from which each request under it is made with no check
    /// left to fail.
    ///
    /// # Errors
    ///
    /// Where a part of the repository's path is empty, `.` or `..`, which
    /// would reach another path of the API (ADR-0067).
    pub(crate) fn checked(&self) -> Result<Checked<'_>, Unpolled> {
        let repository = named(&self.repository, REPOSITORY)?;
        let base = match self.forge {
            Forge::GitHub | Forge::Forgejo => format!("{}/repos/{repository}", self.api),
            Forge::GitLab => gitlab::project(&self.api, &self.repository),
        };
        Ok(Checked { origin: self, base })
    }

    /// The request for `path` under the API's base URL, carrying `token`.
    pub(crate) fn rooted(&self, path: &str, token: Option<&Token>) -> Request {
        let request = Request::new(
            Method::Get,
            SecretText::plain(format!("{}{path}", self.api)),
        )
        .at_most(self.reading.parse);
        authorized(self.forge, request, token)
    }

    /// The request for the repository's numeric id and name.
    ///
    /// # Errors
    ///
    /// Where the repository's path names nothing in a URL path.
    pub fn repository_request(&self, token: Option<&Token>) -> Result<Request, Unpolled> {
        self.get("", token)
    }

    /// The request for the newest releases, conditional on the validators
    /// `cursor` holds.
    ///
    /// # Errors
    ///
    /// Where the repository's path names nothing in a URL path.
    pub fn releases_request(
        &self,
        cursor: Option<&Cursor>,
        token: Option<&Token>,
    ) -> Result<Request, Unpolled> {
        let page = self.forge.api().page;
        let request = self.get(&format!("/releases?{page}={}", self.reading.page), token)?;
        Ok(request.conditional(
            cursor.and_then(|cursor| cursor.etag.as_deref()),
            cursor.and_then(|cursor| cursor.last_modified.as_deref()),
        ))
    }

    /// The request for the newest tags, conditional on `validators` where
    /// there are any.
    ///
    /// # Errors
    ///
    /// Where the repository's path names nothing in a URL path.
    pub fn tags_request(
        &self,
        validators: Option<&Validators>,
        token: Option<&Token>,
    ) -> Result<Request, Unpolled> {
        let page = self.forge.api().page;
        let path = match self.forge {
            Forge::GitLab => format!(
                "/repository/tags?order_by=updated&sort=desc&{page}={}",
                self.reading.page
            ),
            Forge::GitHub | Forge::Forgejo => format!("/tags?{page}={}", self.reading.page),
        };
        Ok(self.get(&path, token)?.conditional(
            validators.and_then(|validators| validators.etag.as_deref()),
            validators.and_then(|validators| validators.last_modified.as_deref()),
        ))
    }

    /// The request for the tag `tag` on GitLab or Forgejo; GitHub reads a
    /// tag through its reference.
    ///
    /// # Errors
    ///
    /// Where the tag names nothing in a URL path.
    pub fn tag_request(&self, tag: &str, token: Option<&Token>) -> Result<Request, Unpolled> {
        let path = match self.forge {
            Forge::GitLab => format!("/repository/tags/{}", whole(tag, TAG)?),
            Forge::GitHub | Forge::Forgejo => format!("/tags/{}", named(tag, TAG)?),
        };
        self.get(&path, token)
    }

    /// The request for GitHub's reference of the tag `tag`, naming what it
    /// points at.
    ///
    /// # Errors
    ///
    /// Where the tag names nothing in a URL path.
    pub fn reference_request(&self, tag: &str, token: Option<&Token>) -> Result<Request, Unpolled> {
        self.get(&format!("/git/ref/tags/{}", named(tag, TAG)?), token)
    }

    /// The request for the annotated tag object `sha`.
    ///
    /// # Errors
    ///
    /// Where the sha names nothing in a URL path.
    pub fn tag_object_request(
        &self,
        sha: &str,
        token: Option<&Token>,
    ) -> Result<Request, Unpolled> {
        self.get(&format!("/git/tags/{}", whole(sha, SHA)?), token)
    }

    /// The request for GitHub's commit `sha`.
    ///
    /// # Errors
    ///
    /// Where the sha names nothing in a URL path.
    pub fn commit_request(&self, sha: &str, token: Option<&Token>) -> Result<Request, Unpolled> {
        self.get(&format!("/git/commits/{}", whole(sha, SHA)?), token)
    }

    /// The request for the raw bytes of the file at `path`, at `reference`
    /// or the default branch, conditional on `etag` where there is one.
    ///
    /// # Errors
    ///
    /// Where the path names nothing in a URL path.
    pub fn file_request(
        &self,
        (path, reference): (&str, Option<&str>),
        token: Option<&Token>,
        etag: Option<&str>,
    ) -> Result<Request, Unpolled> {
        let segments = named(path, FILE)?;
        let at = |given: Option<&str>| {
            given.map_or_else(String::new, |reference| {
                format!("?ref={}", unreserved(reference))
            })
        };
        let file = match self.forge {
            Forge::GitHub => format!("/contents/{segments}{}", at(reference)),
            Forge::GitLab => format!(
                "/repository/files/{}/raw{}",
                whole(path, FILE)?,
                at(Some(reference.unwrap_or(gitlab::DEFAULT_REF)))
            ),
            Forge::Forgejo => format!("/raw/{segments}{}", at(reference)),
        };
        let request = self.get(&file, token)?.conditional(etag, None);
        Ok(match self.forge {
            Forge::GitHub => {
                request.replacing("Accept", SecretText::plain(herald_wire::github::RAW))
            }
            Forge::GitLab | Forge::Forgejo => request,
        })
    }

    /// The request for the release tagged `tag`.
    ///
    /// # Errors
    ///
    /// Where the tag names nothing in a URL path.
    pub fn release_request(&self, tag: &str, token: Option<&Token>) -> Result<Request, Unpolled> {
        let path = format!("{}{}", self.forge.api().tagged, whole(tag, TAG)?);
        self.get(&path, token)
    }
}

#[cfg(test)]
mod tests;
