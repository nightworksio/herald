use std::collections::BTreeSet;

use schemars::JsonSchema;
use serde::Deserialize;
use url::{Host, Url};

/// Whom herald's actors federate with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    /// Every server and actor that `block` does not name.
    #[default]
    Open,
    /// Only the servers and actors `allow` names, and none that `block`
    /// names.
    Allowlist,
}

/// Who decides whether a follow of herald's actors is taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Approve {
    /// herald, by the lists and caps alone.
    #[default]
    Auto,
    /// The operator, through `herald followers approve` or `reject`: a
    /// follow waits until then, and its follower is not delivered to.
    Manual,
}

/// How many may follow each of herald's actors, as `followers` writes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FollowerCaps {
    /// The most followers one actor keeps; no limit where unset.
    pub most: Option<usize>,
    /// The most followers one actor keeps from any one server; no limit
    /// where unset.
    pub per_domain: Option<usize>,
}

/// The servers and actors a list names, each looked up at once however
/// long the list: a domain names every actor on it and on its subdomains,
/// an id names one actor.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct Listed {
    domains: BTreeSet<String>,
    actors: BTreeSet<String>,
}

impl Listed {
    fn of(entries: &[String]) -> Self {
        let mut listed = Self::default();
        for entry in entries {
            if entry.contains("://") {
                listed.actors.extend(canonical(entry));
            } else {
                listed
                    .domains
                    .insert(entry.trim_end_matches('.').to_ascii_lowercase());
            }
        }
        listed
    }

    /// Whether the list names the actor `id`, each host compared in
    /// lowercase and without a trailing dot.
    fn names(&self, id: &str) -> bool {
        if canonical(id).is_some_and(|id| self.actors.contains(&id)) {
            return true;
        }
        let Some(host) = host(id) else {
            return false;
        };
        let mut rest = host.as_str();
        loop {
            if self.domains.contains(rest) {
                return true;
            }
            match rest.split_once('.') {
                Some((_, parent)) => rest = parent,
                None => return false,
            }
        }
    }
}

/// Whom an actor federates with, and how many may follow it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Policy {
    mode: Mode,
    block: Listed,
    allow: Listed,
    caps: FollowerCaps,
    approve: Approve,
}

impl Policy {
    /// The policy of `mode`, blocking whom `block` names and, in an
    /// allowlist, allowing whom `allow` names: each a domain, naming every
    /// actor on it and on its subdomains, or an actor's id.
    #[must_use]
    pub fn new(mode: Mode, block: &[String], allow: &[String], caps: FollowerCaps) -> Self {
        Self {
            mode,
            block: Listed::of(block),
            allow: Listed::of(allow),
            caps,
            approve: Approve::Auto,
        }
    }

    /// The policy, its follows taken as `approve` says.
    #[must_use]
    pub fn approving(self, approve: Approve) -> Self {
        Self { approve, ..self }
    }

    /// Whether a follow waits for the operator's approval.
    #[must_use]
    pub const fn manual(&self) -> bool {
        matches!(self.approve, Approve::Manual)
    }

    /// Whether the actor `id` may follow herald's actor and be delivered to.
    #[must_use]
    pub fn admits(&self, id: &str) -> bool {
        if self.block.names(id) {
            return false;
        }
        match self.mode {
            Mode::Open => true,
            Mode::Allowlist => self.allow.names(id),
        }
    }

    /// Whether `follower` finds room beside `following`, the actors that
    /// follow now: one already following always does.
    #[must_use]
    pub fn room(&self, follower: &str, following: &[&str]) -> bool {
        if following.contains(&follower) {
            return true;
        }
        if self.caps.most.is_some_and(|most| following.len() >= most) {
            return false;
        }
        let server = host(follower);
        self.caps.per_domain.is_none_or(|most| {
            following
                .iter()
                .filter(|other| host(other) == server)
                .count()
                < most
        })
    }
}

/// The host of the actor `id`, in lowercase and without a trailing dot.
fn host(id: &str) -> Option<String> {
    match Url::parse(id).ok()?.host()? {
        Host::Domain(host) => Some(host.trim_end_matches('.').to_ascii_lowercase()),
        _ => None,
    }
}

/// The actor `id` with its host as [`host`] writes it.
fn canonical(id: &str) -> Option<String> {
    let mut url = Url::parse(id).ok()?;
    url.set_host(Some(&host(id)?)).ok()?;
    Some(url.into())
}

/// A shared block list, as `lists` writes it: a file of domains to block in
/// the CSV form Mastodon exports, pinned by the SHA-256 of its text.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SharedList {
    /// The file, as a path beside `herald.toml`.
    pub file: String,
    /// The SHA-256 of the file's text, in hex: a list that changes is not
    /// read until its new digest is pinned.
    pub sha256: String,
}

impl SharedList {
    /// The domains the list blocks, its file read through `read` and
    /// checked against its pinned digest.
    ///
    /// # Errors
    ///
    /// Where the file cannot be read, has another digest than the one
    /// pinned, or is no block list.
    pub fn read(
        &self,
        read: &dyn Fn(&str) -> Result<String, String>,
    ) -> Result<Vec<String>, String> {
        let file = &self.file;
        let text = read(file).map_err(|error| format!("{file} could not be read: {error}"))?;
        let digest = herald_core::digest(&text);
        if !digest.eq_ignore_ascii_case(&self.sha256) {
            return Err(format!(
                "{file} has the SHA-256 {digest}, not the {} pinned; check the list, then pin its digest",
                self.sha256
            ));
        }
        blocked_domains(&text).map_err(|error| format!("{file}: {error}"))
    }
}

/// The severities a list's row blocks with; `noop` blocks nothing.
const SEVERITIES: [&str; 2] = ["suspend", "silence"];

/// The domains `csv`, a domain block list as Mastodon exports it, blocks:
/// each row's domain, first, where its severity, second, is `suspend` or
/// `silence` or not given. A header row, starting with `domain` or
/// `#domain`, and blank rows are skipped.
///
/// # Errors
///
/// The first row whose domain is not one, naming its line.
pub fn blocked_domains(csv: &str) -> Result<Vec<String>, String> {
    let mut blocked = Vec::new();
    for (index, row) in csv.lines().enumerate() {
        let mut fields = row.split(',').map(str::trim);
        let domain = fields.next().unwrap_or_default();
        let severity = fields.next().unwrap_or_default();
        if domain.is_empty() || matches!(domain, "domain" | "#domain") {
            continue;
        }
        if !severity.is_empty() && !SEVERITIES.contains(&severity) {
            continue;
        }
        let ascii = match Host::parse(domain) {
            Ok(Host::Domain(ascii)) => ascii.trim_end_matches('.').to_ascii_lowercase(),
            _ => String::new(),
        };
        if !herald_core::domain(&ascii) {
            return Err(format!(
                "line {} names \"{domain}\", which is not a domain",
                index + 1
            ));
        }
        blocked.push(ascii);
    }
    Ok(blocked)
}

/// Why `entry` cannot be written in `block` or `allow`, where it cannot: it
/// is neither a domain nor an `https` actor id.
pub(crate) fn unlisted(entry: &str) -> Option<String> {
    let actor = Url::parse(entry).is_ok_and(|url| {
        url.scheme() == "https"
            && matches!(url.host(), Some(Host::Domain(_)))
            && url.port().is_none()
            && url.as_str() == entry
    });
    (!actor && !herald_core::domain(entry)).then(|| {
        format!(
            "\"{entry}\" is neither a domain nor an actor's id; give a host such as \"spam.example\", or an id such as \"https://spam.example/users/x\""
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listed(entries: &[&str]) -> Vec<String> {
        entries.iter().map(|entry| (*entry).to_owned()).collect()
    }

    #[test]
    fn a_domain_names_every_actor_on_it_and_its_subdomains_and_an_id_names_one() {
        let policy = Policy::new(
            Mode::Open,
            &listed(&["spam.example", "https://good.example/users/troll"]),
            &[],
            FollowerCaps::default(),
        );
        for blocked in [
            "https://spam.example/users/a",
            "https://SPAM.example/users/a",
            "https://spam.example./users/a",
            "https://eu.spam.example/users/b",
            "https://eu.spam.example./users/b",
            "https://good.example/users/troll",
            "https://GOOD.example./users/troll",
        ] {
            assert!(!policy.admits(blocked), "{blocked}");
        }
        for admitted in [
            "https://notspam.example/users/a",
            "https://spam.example.org/users/a",
            "https://good.example/users/troll2",
            "https://good.example/users/ana",
        ] {
            assert!(policy.admits(admitted), "{admitted}");
        }
    }

    #[test]
    fn an_allowlist_admits_only_whom_it_allows_and_never_whom_it_blocks() {
        let policy = Policy::new(
            Mode::Allowlist,
            &listed(&["https://friends.example/users/eve"]),
            &listed(&["friends.example", "https://other.example/users/bo"]),
            FollowerCaps::default(),
        );
        assert!(policy.admits("https://friends.example/users/ana"));
        assert!(policy.admits("https://other.example/users/bo"));
        assert!(!policy.admits("https://other.example/users/al"));
        assert!(!policy.admits("https://friends.example/users/eve"));
        assert!(!policy.admits("https://elsewhere.example/users/x"));
        assert!(Policy::default().admits("https://elsewhere.example/users/x"));
    }

    #[test]
    fn a_follower_finds_room_within_the_caps_and_one_following_always_does() {
        let capped =
            |most, per_domain| Policy::new(Mode::Open, &[], &[], FollowerCaps { most, per_domain });
        let following = [
            "https://a.example/users/1",
            "https://a.example/users/2",
            "https://b.example/users/3",
        ];
        let new_on_a = "https://a.example/users/4";
        let new_on_c = "https://c.example/users/5";
        assert!(capped(None, None).room(new_on_a, &following));
        assert!(!capped(Some(3), None).room(new_on_c, &following));
        assert!(capped(Some(4), None).room(new_on_c, &following));
        assert!(!capped(None, Some(2)).room(new_on_a, &following));
        assert!(capped(None, Some(3)).room(new_on_a, &following));
        assert!(capped(None, Some(2)).room(new_on_c, &following));
        assert!(capped(Some(1), Some(1)).room(following[0], &following));
    }

    #[test]
    fn a_shared_list_blocks_each_suspended_or_silenced_domain_and_skips_the_rest() {
        let csv = "#domain,#severity,#reject_media,#reject_reports,#public_comment,#obfuscate\n\
                   spam.example,suspend,true,true,\"spam, all of it\",false\n\
                   Loud.Example.,silence,false,false,,false\n\
                   fine.example,noop,false,false,,false\n\
                   \n\
                   bare.example\n\
                   bücher.example,suspend\n";
        assert_eq!(
            blocked_domains(csv),
            Ok(listed(&[
                "spam.example",
                "loud.example",
                "bare.example",
                "xn--bcher-kva.example"
            ]))
        );
        assert_eq!(blocked_domains("domain,severity\n"), Ok(Vec::new()));
        assert_eq!(
            blocked_domains("ok.example\nsp*m.example,suspend\n"),
            Err("line 2 names \"sp*m.example\", which is not a domain".to_owned())
        );
        assert_eq!(
            blocked_domains("https://spam.example/users/x\n"),
            Err("line 1 names \"https://spam.example/users/x\", which is not a domain".to_owned())
        );
    }

    #[test]
    fn an_id_with_no_domain_is_named_by_no_domain_and_admitted_only_where_all_are() {
        let open = Policy::new(
            Mode::Open,
            &listed(&["example"]),
            &[],
            FollowerCaps::default(),
        );
        let allowing = Policy::new(
            Mode::Allowlist,
            &[],
            &listed(&["example"]),
            FollowerCaps::default(),
        );
        for unhosted in ["https://192.0.2.1/users/a", "not an id"] {
            assert!(open.admits(unhosted), "{unhosted}");
            assert!(!allowing.admits(unhosted), "{unhosted}");
        }
    }

    #[test]
    fn a_shared_list_is_read_only_where_its_file_has_the_digest_it_is_pinned_to() {
        let csv = "spam.example,suspend\n";
        let digest = herald_core::digest(csv);
        let read = |file: &str| match file {
            "blocks.csv" => Ok(csv.to_owned()),
            "broken.csv" => Ok("sp*m.example\n".to_owned()),
            _ => Err("no such file".to_owned()),
        };
        let list = |file: &str, sha256: &str| SharedList {
            file: file.to_owned(),
            sha256: sha256.to_owned(),
        };
        assert_eq!(
            list("blocks.csv", &digest.to_ascii_uppercase()).read(&read),
            Ok(listed(&["spam.example"]))
        );
        assert_eq!(
            list("blocks.csv", "00").read(&read),
            Err(format!(
                "blocks.csv has the SHA-256 {digest}, not the 00 pinned; check the list, then pin its digest"
            ))
        );
        assert_eq!(
            list("gone.csv", &digest).read(&read),
            Err("gone.csv could not be read: no such file".to_owned())
        );
        let broken = herald_core::digest("sp*m.example\n");
        assert_eq!(
            list("broken.csv", &broken).read(&read),
            Err("broken.csv: line 1 names \"sp*m.example\", which is not a domain".to_owned())
        );
    }

    #[test]
    fn a_long_list_names_an_actor_by_its_host_or_any_parent() {
        let entries: Vec<String> = (0..5_000).map(|n| format!("s{n}.example")).collect();
        let policy = Policy::new(Mode::Open, &entries, &[], FollowerCaps::default());
        assert!(!policy.admits("https://a.b.s4999.example/users/x"));
        assert!(policy.admits("https://s5000.example/users/x"));
        assert!(policy.admits("https://example/users/x"));
    }

    #[test]
    fn an_entry_is_a_domain_or_an_https_actor_id() {
        for entry in [
            "spam.example",
            "eu.spam.example",
            "https://spam.example/users/x",
        ] {
            assert_eq!(unlisted(entry), None, "{entry}");
        }
        for entry in [
            "http://spam.example/users/x",
            "https://127.0.0.1/users/x",
            "Spam.example",
            "spam",
            "https://spam.example:8443/users/x",
            "",
        ] {
            let refused = unlisted(entry).unwrap_or_default();
            assert!(
                refused.starts_with(&format!(
                    "\"{entry}\" is neither a domain nor an actor's id"
                )),
                "{entry}: {refused}"
            );
        }
    }
}
