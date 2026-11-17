use std::collections::BTreeSet;

pub use herald_core::follows::{Approve, FollowerCaps, Mode, SharedList, followed_by};
use url::{Host, Url};

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
    fn a_long_list_names_an_actor_by_its_host_or_any_parent() {
        let entries: Vec<String> = (0..5_000).map(|n| format!("s{n}.example")).collect();
        let policy = Policy::new(Mode::Open, &entries, &[], FollowerCaps::default());
        assert!(!policy.admits("https://a.b.s4999.example/users/x"));
        assert!(policy.admits("https://s5000.example/users/x"));
        assert!(policy.admits("https://example/users/x"));
    }
}
