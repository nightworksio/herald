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

/// Whom an actor federates with, and how many may follow it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Policy {
    mode: Mode,
    block: Vec<String>,
    allow: Vec<String>,
    caps: FollowerCaps,
}

impl Policy {
    /// The policy of `mode`, blocking whom `block` names and, in an
    /// allowlist, allowing whom `allow` names: each a domain, naming every
    /// actor on it and on its subdomains, or an actor's id.
    #[must_use]
    pub fn new(mode: Mode, block: &[String], allow: &[String], caps: FollowerCaps) -> Self {
        Self {
            mode,
            block: block.to_vec(),
            allow: allow.to_vec(),
            caps,
        }
    }

    /// Whether the actor `id` may follow herald's actor and be delivered to.
    #[must_use]
    pub fn admits(&self, id: &str) -> bool {
        if self.block.iter().any(|entry| names(entry, id)) {
            return false;
        }
        match self.mode {
            Mode::Open => true,
            Mode::Allowlist => self.allow.iter().any(|entry| names(entry, id)),
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

/// Whether `entry`, a domain or an actor's id, names the actor `id`, each
/// host compared in lowercase and without a trailing dot.
fn names(entry: &str, id: &str) -> bool {
    if entry.contains("://") {
        return canonical(entry).is_some_and(|entry| canonical(id) == Some(entry));
    }
    let entry = entry.trim_end_matches('.').to_ascii_lowercase();
    host(id).is_some_and(|host| {
        host == entry
            || host
                .strip_suffix(&entry)
                .is_some_and(|rest| rest.ends_with('.'))
    })
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
