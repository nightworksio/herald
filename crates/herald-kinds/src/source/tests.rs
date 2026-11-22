use std::future::ready;

use herald_core::{Cursor, Kind, Polled, SecretRef};
use herald_ports::{Base, Boxed, DynHttp, Secret};

use super::*;

/// The round each poll here is made in.
const ROUND: jiff::Timestamp = jiff::Timestamp::UNIX_EPOCH;

/// Options of a kind that keeps every default.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Plain {
    token: Option<SecretRef>,
}

/// A source that answers nothing new.
#[derive(Debug)]
struct Quiet(Option<Secret>, String);

impl Poll for Quiet {
    fn api(&self) -> &str {
        &self.1
    }

    fn remembered(&self) -> usize {
        self.0.as_ref().map_or(1, |_| 2)
    }

    fn polled<'a>(
        &'a self,
        _: &'a dyn DynHttp,
        _: Option<&'a Cursor>,
        _: jiff::Timestamp,
    ) -> Boxed<'a, Result<Option<Polled>, Unpolled>> {
        Box::pin(ready(Ok(None)))
    }
}

impl SourceConfig for Plain {
    fn kind(&self) -> &'static str {
        "plain"
    }

    fn token(&self) -> Option<&SecretRef> {
        self.token.as_ref()
    }

    fn kinds(&self) -> Vec<Kind> {
        vec![Kind::RELEASE]
    }

    fn backfill(&self) -> usize {
        3
    }

    fn poller(&self, token: Option<Credential>, _: bool) -> Polling {
        Polling::One(Arc::new(Quiet(
            token.and_then(Credential::named),
            "https://plain.example".to_owned(),
        )))
    }
}

/// A plain source whose options name `plugins` for its plugin repositories.
#[derive(Debug, PartialEq, Eq)]
struct Routing(Plain);

impl SourceConfig for Routing {
    fn kind(&self) -> &'static str {
        self.0.kind()
    }

    fn token(&self) -> Option<&SecretRef> {
        self.0.token()
    }

    fn kinds(&self) -> Vec<Kind> {
        self.0.kinds()
    }

    fn backfill(&self) -> usize {
        self.0.backfill()
    }

    fn poller(&self, token: Option<Credential>, edits: bool) -> Polling {
        self.0.poller(token, edits)
    }

    fn routed(&self) -> Vec<(String, String)> {
        vec![(
            "repositories.\"plugin-*\".routes".to_owned(),
            "plugins".to_owned(),
        )]
    }
}

fn tokened() -> Plain {
    Plain {
        token: Some(SecretRef {
            secret: "TOKEN".to_owned(),
        }),
    }
}

#[test]
fn a_source_takes_no_webhook_and_no_moving_tag_where_its_kind_keeps_the_defaults() {
    let source = Source::new(tokened());
    assert_eq!(source.webhook(), None);
    assert_eq!(source.scheme(), None);
    assert!(!source.moving_tags());
    assert!(!source.unlisted());
    assert!(!source.holds());
    assert_eq!(source.tag_window(), None);
    assert_eq!(source.notes(), Vec::<&str>::new());
    assert_eq!(source.projects(), Vec::<String>::new());
    assert_eq!(source.remembers(0), None);
    assert_eq!(source.fitted(10, 0), None);
    assert_eq!(source.grants(), None);
    assert!(source.receiver().is_none());
    assert_eq!(source.share(), None);
    assert_eq!(source.stale_after(), None);
    assert!(!source.origins());
    assert!(!source.pushed());
    assert_eq!(source.registering(), Registering::Never);
    assert_eq!(source.routed(), Vec::new());
    assert!(!source.names_route(&herald_core::RouteId::new("releases")));
    assert_eq!(
        source.secret_refs(),
        vec![&SecretRef {
            secret: "TOKEN".to_owned()
        }]
    );
    assert_eq!(source.kind(), "plain");
    assert_eq!(source.kinds(), &[Kind::RELEASE]);
    assert_eq!(source.backfill(), 3);
    assert_eq!(source.signals(), Vec::new());
    assert_eq!(source.hooked(&[], b"{}"), Ok(Hooked::Nothing(UNHOOKED)));
    assert_eq!(source.folded(&[], b"{}"), Ok(None));
    assert_eq!(source.write(), None);
    assert_eq!(source.signing(), Err(crate::UNSIGNED));
    assert_eq!(source.rerunner(None).map(|_| ()), Err(crate::UNWRITTEN));
}

#[test]
fn a_source_names_a_route_only_where_its_options_route_to_it() {
    let source = Routing(tokened());
    assert!(source.names_route(&herald_core::RouteId::new("plugins")));
    assert!(!source.names_route(&herald_core::RouteId::new("releases")));
}

#[test]
fn a_source_polls_with_the_token_it_is_given() {
    let source = Source::new(tokened());
    let polling = source.poller(Some(Credential::Named(Secret::new("TOKEN", "t"))), false);
    assert_eq!(polling.api(), "https://plain.example");
    let (Polling::One(poll), Polling::One(untokened)) = (polling, source.poller(None, false))
    else {
        panic!("a plain source is polled as one");
    };
    assert_eq!(poll.remembered(), 2);
    assert_eq!(untokened.remembered(), 1);
    assert_eq!(poll.tag_window(), None);
    let http = crate::reach::tests::Silent;
    assert_eq!(
        pollster::block_on(poll.polled(&http, None, ROUND)),
        Ok(None)
    );
}

#[test]
fn a_source_is_read_as_its_own_kind_and_equal_only_to_the_same_options() {
    let source = Source::new(tokened());
    assert_eq!(source.of::<Plain>(), Some(&tokened()));
    assert_eq!(source, source.clone());
    assert_ne!(source, Source::new(Plain { token: None }));
    assert_eq!(format!("{source:?}"), format!("{:?}", tokened()));
}

#[test]
fn a_poll_that_finds_nothing_to_read_says_why() {
    assert_eq!(
        Unpolled::unreachable("down"),
        Unpolled::Unreachable("down".to_owned())
    );
    assert_eq!(
        Unpolled::unreadable("garbled"),
        Unpolled::Unreadable("garbled".to_owned())
    );
}

#[test]
fn a_listing_takes_its_members_and_an_unchanged_one_none() {
    let member = herald_core::Listed {
        id: "1".to_owned(),
        name: "herald".to_owned(),
        path: "nightworksio/herald".to_owned(),
        topics: Vec::new(),
        properties: std::collections::BTreeMap::new(),
        visibility: herald_core::Visibility::Public,
        archived: false,
        fork: false,
        template: false,
        language: None,
        linked: None,
    };
    assert_eq!(Expanded::Unchanged.members(), Vec::new());
    assert_eq!(
        Expanded::Listed {
            members: vec![member.clone()],
            owner: herald_core::Owner::User,
            pages: Vec::new(),
            file: None,
            missing: Vec::new(),
        }
        .members(),
        [member]
    );
}

/// An owner whose every member is polled on its own by a quiet poll.
#[derive(Debug)]
struct Members;

impl Expand for Members {
    fn api(&self) -> &'static str {
        "https://owner.example"
    }

    fn keep_members(&self) -> SignedDuration {
        SignedDuration::ZERO
    }

    fn remembered(&self) -> usize {
        1
    }

    fn listed<'a>(
        &'a self,
        _: &'a dyn DynHttp,
        _: Option<&'a Listing>,
    ) -> Boxed<'a, Result<Expanded, Unlisted>> {
        Box::pin(ready(Ok(Expanded::Unchanged)))
    }

    fn member(&self, repository: &Listed) -> Arc<dyn Poll> {
        Arc::new(Quiet(None, repository.path.clone()))
    }

    fn origin(&self, repository: &Listed) -> herald_core::Origin {
        herald_core::Origin::of_member("forge", "owner.example", repository)
    }

    fn project(&self, repository: &Listed) -> Result<String, String> {
        Ok(repository.name.clone())
    }

    fn takes(&self, _: &Listed) -> bool {
        true
    }

    fn fingerprint(&self) -> String {
        String::new()
    }

    fn pushed(&self) -> bool {
        false
    }
}

#[test]
fn an_owner_by_default_registers_no_webhook_on_its_members() {
    let member = Listed {
        id: "1".to_owned(),
        name: "a".to_owned(),
        path: "o/a".to_owned(),
        topics: Vec::new(),
        properties: std::collections::BTreeMap::new(),
        visibility: herald_core::Visibility::Public,
        archived: false,
        fork: false,
        template: false,
        language: None,
        linked: None,
    };
    assert_eq!(Members.registering(), Registering::Never);
    let secret = Secret::new("HOOK", "s");
    assert_eq!(
        pollster::block_on(Members.register(
            &crate::reach::tests::Silent,
            &member,
            ("https://herald.example/hooks/o", &secret)
        )),
        Ok(Registered::Found)
    );
    let http = crate::reach::tests::Silent;
    assert_eq!(
        pollster::block_on(Members.hooked(&http, &member, "https://herald.example")),
        Ok(true)
    );
    assert_eq!(pollster::block_on(Members.may_register(&http)), Ok(()));
}

#[test]
fn an_owner_by_default_polls_each_member_on_its_own_and_follows_no_events() {
    struct Nowhere;
    impl herald_ports::Http for Nowhere {
        #[expect(
            clippy::unused_async_trait_impl,
            reason = "a fake answers at once, and is written as the async trait it fakes"
        )]
        async fn send(
            &self,
            _: herald_ports::Request,
        ) -> Result<herald_ports::Response, herald_ports::HttpError> {
            Err(herald_ports::HttpError::TimedOut)
        }
    }
    let member = |name: &str| herald_core::Listed {
        id: name.to_owned(),
        name: name.to_owned(),
        path: format!("o/{name}"),
        topics: Vec::new(),
        properties: std::collections::BTreeMap::new(),
        visibility: herald_core::Visibility::Public,
        archived: false,
        fork: false,
        template: false,
        language: None,
        linked: None,
    };
    let (one, two) = (member("one"), member("two"));
    let http = Nowhere;
    let owner = Members;
    assert_eq!(owner.batch(), std::num::NonZeroUsize::MIN);
    assert_eq!(owner.tag_window(&one), None);
    assert_eq!(owner.reach(&one), herald_core::Reach::default());
    assert_eq!(owner.origin(&one).repository, "o/one");
    assert_eq!(
        pollster::block_on(owner.polled(
            &http,
            &[(&one, None), (&two, None)],
            &Rounded::Unread,
            ROUND
        )),
        [Ok(None), Ok(None)]
    );
    let rounded = pollster::block_on(owner.rounded(&http, herald_core::Owner::User)).unwrap();
    assert!(matches!(rounded, Rounded::Nothing));
    assert_eq!(rounded.named(), None);
    assert_eq!(Rounded::Unread.named(), None);
    let named = std::collections::BTreeSet::from(["one".to_owned()]);
    let read = Rounded::Read {
        named: named.clone(),
        read: Arc::new(()),
    };
    assert_eq!(read.named(), Some(&named));
    assert_eq!(
        pollster::block_on(owner.changed(&http, herald_core::Owner::User, None)),
        None
    );
}

/// A poll and an owner asking a second base beside their API.
#[derive(Debug)]
struct Beside(Vec<String>);

impl Poll for Beside {
    fn api(&self) -> &str {
        &self.0[0]
    }

    fn bases(&self) -> &[String] {
        &self.0
    }

    fn remembered(&self) -> usize {
        1
    }

    fn polled<'a>(
        &'a self,
        _: &'a dyn DynHttp,
        _: Option<&'a Cursor>,
        _: jiff::Timestamp,
    ) -> Boxed<'a, Result<Option<Polled>, Unpolled>> {
        Box::pin(ready(Ok(None)))
    }
}

#[derive(Debug)]
struct OwnerBeside(Vec<String>);

impl Expand for OwnerBeside {
    fn api(&self) -> &str {
        &self.0[0]
    }

    fn bases(&self) -> &[String] {
        &self.0
    }

    fn keep_members(&self) -> SignedDuration {
        SignedDuration::ZERO
    }

    fn pushed(&self) -> bool {
        false
    }

    fn remembered(&self) -> usize {
        1
    }

    fn listed<'a>(
        &'a self,
        _: &'a dyn DynHttp,
        _: Option<&'a Listing>,
    ) -> Boxed<'a, Result<Expanded, Unlisted>> {
        Box::pin(ready(Ok(Expanded::Unchanged)))
    }

    fn member(&self, repository: &Listed) -> Arc<dyn Poll> {
        Arc::new(Quiet(None, repository.path.clone()))
    }

    fn origin(&self, repository: &Listed) -> herald_core::Origin {
        herald_core::Origin::of_member("forge", "owner.example", repository)
    }

    fn project(&self, repository: &Listed) -> Result<String, String> {
        Ok(repository.name.clone())
    }

    fn takes(&self, _: &Listed) -> bool {
        true
    }

    fn fingerprint(&self) -> String {
        String::new()
    }
}

#[test]
fn a_source_taking_only_pushes_reaches_no_origin_and_probes_nothing() {
    let unpolled = Polling::Unpolled;
    assert_eq!(unpolled.api(), "");
    assert_eq!(unpolled.base(), Base::Among(&[]));
    assert_eq!(unpolled.base().bases(), Some(Vec::new()));
    assert_eq!(
        pollster::block_on(unpolled.probed(&crate::reach::tests::Silent)),
        Vec::new()
    );
}

#[test]
fn a_poll_reaches_its_api_s_origin_unless_it_names_more_bases() {
    let quiet = Polling::One(Arc::new(Quiet(None, "https://plain.example".to_owned())));
    assert_eq!(quiet.base(), Base::At("https://plain.example"));
    assert_eq!(Quiet(None, String::new()).bases(), &[] as &[String]);
    assert_eq!(
        Polling::Many(Arc::new(Members)).base(),
        Base::At("https://owner.example")
    );
    assert_eq!(Members.bases(), &[] as &[String]);
    assert_eq!(Members.owner_key(), "owner");
    assert_eq!(Members.member_nouns(), ("repository", "repositories"));
    let bases = vec![
        "https://a.example".to_owned(),
        "https://b.example".to_owned(),
    ];
    assert_eq!(
        Polling::One(Arc::new(Beside(bases.clone()))).base(),
        Base::Among(&bases)
    );
    assert_eq!(
        Polling::Many(Arc::new(OwnerBeside(bases.clone()))).base(),
        Base::Among(&bases)
    );
    assert_eq!(Beside(bases.clone()).remembered(), 1);
    assert_eq!(
        pollster::block_on(Beside(bases.clone()).polled(&crate::reach::tests::Silent, None, ROUND)),
        Ok(None)
    );
    let owner = OwnerBeside(bases);
    assert_eq!(owner.keep_members(), SignedDuration::ZERO);
    assert_eq!(owner.remembered(), 1);
    assert_eq!(owner.fingerprint(), "");
    assert_eq!(
        pollster::block_on(owner.listed(&crate::reach::tests::Silent, None)),
        Ok(Expanded::Unchanged)
    );
    let member = Listed {
        id: "1".to_owned(),
        name: "one".to_owned(),
        path: "o/one".to_owned(),
        topics: Vec::new(),
        properties: std::collections::BTreeMap::new(),
        visibility: herald_core::Visibility::Public,
        archived: false,
        fork: false,
        template: false,
        language: None,
        linked: None,
    };
    assert!(owner.takes(&member));
    assert_eq!(owner.project(&member), Ok("one".to_owned()));
    assert_eq!(owner.member(&member).api(), "o/one");
    assert_eq!(owner.origin(&member).repository, "o/one");
}

#[test]
fn an_image_shares_a_project_with_the_repository_it_is_linked_to_alone() {
    let repository = Standing::new("https://api.github.com/", "Nightworksio/Herald", false);
    assert_eq!(
        repository.repository,
        "https://api.github.com/nightworksio/herald"
    );
    let image = Standing::new("https://api.github.com", "nightworksio/herald", true);
    assert!(image.shares(&repository) && repository.shares(&image));
    assert!(!repository.shares(&repository.clone()));
    assert!(!image.shares(&image.clone()));
    let other = Standing::new("https://api.github.com", "nightworksio/other", true);
    assert!(!other.shares(&repository));
    assert_eq!(Source::new(tokened()).standing(), None);
    let member = Listed {
        id: "1".to_owned(),
        name: "one".to_owned(),
        path: "o/one".to_owned(),
        topics: Vec::new(),
        properties: std::collections::BTreeMap::new(),
        visibility: herald_core::Visibility::Public,
        archived: false,
        fork: false,
        template: false,
        language: None,
        linked: None,
    };
    assert_eq!(Members.standing(&member), None);
}

/// A poll whose own check finds its auth wanting.
#[derive(Debug)]
struct Probing;

impl Poll for Probing {
    fn api(&self) -> &'static str {
        "https://probing.example"
    }

    fn remembered(&self) -> usize {
        1
    }

    fn polled<'a>(
        &'a self,
        _: &'a dyn DynHttp,
        _: Option<&'a Cursor>,
        _: jiff::Timestamp,
    ) -> Boxed<'a, Result<Option<Polled>, Unpolled>> {
        Box::pin(ready(Ok(None)))
    }

    fn probed<'a>(&'a self, _: &'a dyn DynHttp) -> Boxed<'a, Vec<Probed>> {
        Box::pin(ready(vec![Probed {
            key: "auth",
            why: "refused".to_owned(),
        }]))
    }
}

/// An owner whose own check finds its catalog wanting.
#[derive(Debug)]
struct ProbingOwner;

impl Expand for ProbingOwner {
    fn api(&self) -> &'static str {
        "https://probing.example"
    }

    fn pushed(&self) -> bool {
        false
    }

    fn keep_members(&self) -> SignedDuration {
        SignedDuration::ZERO
    }

    fn remembered(&self) -> usize {
        1
    }

    fn listed<'a>(
        &'a self,
        _: &'a dyn DynHttp,
        _: Option<&'a Listing>,
    ) -> Boxed<'a, Result<Expanded, Unlisted>> {
        Box::pin(ready(Ok(Expanded::Unchanged)))
    }

    fn member(&self, _: &Listed) -> Arc<dyn Poll> {
        Arc::new(Probing)
    }

    fn origin(&self, repository: &Listed) -> herald_core::Origin {
        herald_core::Origin::of_package("probing", "probing.example", "o", &repository.path)
    }

    fn project(&self, repository: &Listed) -> Result<String, String> {
        Ok(repository.name.clone())
    }

    fn takes(&self, _: &Listed) -> bool {
        true
    }

    fn fingerprint(&self) -> String {
        String::new()
    }

    fn probed<'a>(&'a self, _: &'a dyn DynHttp) -> Boxed<'a, Vec<Probed>> {
        Box::pin(ready(vec![Probed {
            key: "catalog",
            why: "unlisted".to_owned(),
        }]))
    }
}

#[test]
fn a_source_s_own_check_finds_nothing_unless_its_poll_or_owner_checks_something() {
    let http = crate::reach::tests::Silent;
    let quiet = Polling::One(Arc::new(Quiet(None, "https://plain.example".to_owned())));
    assert_eq!(pollster::block_on(quiet.probed(&http)), Vec::new());
    assert_eq!(
        pollster::block_on(Polling::Many(Arc::new(Members)).probed(&http)),
        Vec::new()
    );
    assert_eq!(
        pollster::block_on(Polling::One(Arc::new(Probing)).probed(&http)),
        [Probed {
            key: "auth",
            why: "refused".to_owned()
        }]
    );
    let owner = ProbingOwner;
    assert_eq!(
        pollster::block_on(Polling::Many(Arc::new(ProbingOwner)).probed(&http)),
        [Probed {
            key: "catalog",
            why: "unlisted".to_owned()
        }]
    );
    assert_eq!(owner.api(), "https://probing.example");
    assert!(!owner.pushed());
    assert_eq!(owner.keep_members(), SignedDuration::ZERO);
    assert_eq!(owner.remembered(), 1);
    assert_eq!(owner.fingerprint(), "");
    assert_eq!(
        pollster::block_on(owner.listed(&http, None)),
        Ok(Expanded::Unchanged)
    );
    let member = Listed {
        id: "1".to_owned(),
        name: "one".to_owned(),
        path: "o/one".to_owned(),
        topics: Vec::new(),
        properties: std::collections::BTreeMap::new(),
        visibility: herald_core::Visibility::Public,
        archived: false,
        fork: false,
        template: false,
        language: None,
        linked: None,
    };
    assert!(owner.takes(&member));
    assert_eq!(owner.project(&member), Ok("one".to_owned()));
    assert_eq!(owner.origin(&member).repository, "o/one");
    let poll = owner.member(&member);
    assert_eq!(poll.api(), "https://probing.example");
    assert_eq!(poll.remembered(), 1);
    assert_eq!(
        pollster::block_on(poll.polled(&http, None, ROUND)),
        Ok(None)
    );
}

#[test]
fn a_cursor_remembering_every_identity_takes_each_one_s_bytes_beside_the_rest() {
    let remembers = Remembers {
        identities: 10,
        each: 100,
        key: "most_tags",
        bound: Bound::Named,
        per: 1,
        beside: 0,
        besides: &[],
    };
    assert_eq!(remembers.bytes(), 1000 + Remembers::REST);
    assert!(remembers.fits(1000 + Remembers::REST));
    assert!(!remembers.fits(999 + Remembers::REST));
    assert_eq!(remembers.fitting(Remembers::REST + 1099), 10);
    assert_eq!(remembers.fitting(Remembers::REST + 1100), 11);
    assert_eq!(remembers.fitting(Remembers::REST - 1), 0);
    assert_eq!(remembers.named(Remembers::REST + 1100), 11);
    let paged = Remembers {
        per: 4,
        beside: 300,
        ..remembers
    };
    assert_eq!(paged.bytes(), 1300 + Remembers::REST);
    assert_eq!(paged.fitting(Remembers::REST + 1100), 8);
    assert_eq!(paged.named(Remembers::REST + 1100), 2);
    assert_eq!(paged.named(Remembers::REST + 1099), 1);
    assert_eq!(paged.named(Remembers::REST + 299), 0);
    let unpaged = Remembers {
        per: 0,
        ..remembers
    };
    assert_eq!(unpaged.named(Remembers::REST), usize::MAX);
    let empty = Remembers {
        each: 0,
        ..remembers
    };
    assert_eq!(empty.fitting(1), usize::MAX);
    let huge = Remembers {
        identities: usize::MAX,
        ..remembers
    };
    assert_eq!(huge.bytes(), usize::MAX);
}

/// Another herald's feed that lists nothing.
#[derive(Debug)]
struct Empty(String);

impl crate::Pull for Empty {
    fn api(&self) -> &str {
        &self.0
    }

    fn peering(&self) -> crate::Peering<'_> {
        crate::Peering::Peer("upstream")
    }

    fn remembered(&self) -> usize {
        4
    }

    fn pulled<'a>(
        &'a self,
        _: crate::Pulling<'a>,
        _: &'a crate::Trust,
        _: Option<&'a Cursor>,
    ) -> Boxed<'a, Result<Option<Polled>, crate::Unpulled>> {
        Box::pin(ready(Ok(None)))
    }
}

#[test]
fn a_pull_reaches_its_feed_s_origin_alone_and_checks_nothing_of_its_own() {
    let http = crate::reach::tests::Silent;
    let pull = Polling::Pull(Arc::new(Empty(
        "https://peer.example/herald/v1/feeds/releases".to_owned(),
    )));
    assert_eq!(pull.api(), "https://peer.example/herald/v1/feeds/releases");
    assert_eq!(
        pull.base(),
        Base::At("https://peer.example/herald/v1/feeds/releases")
    );
    assert_eq!(pollster::block_on(pull.probed(&http)), Vec::new());
    let plain = Plain { token: None };
    assert!(!plain.any_kind());
    assert!(plain.queue(None).is_none());
    assert_eq!(plain.queue_cost(crate::Most::default()), None);
    assert_eq!(plain.areas("o/r"), None);
    assert_eq!(plain.area_names(), std::collections::BTreeSet::new());
    assert_eq!(plain.codeowning("o/r"), None);
    assert!(!plain.codeowned());
    assert_eq!(plain.codeowners_notes(), Vec::<String>::new());
    assert_eq!(
        crate::Most::default(),
        crate::Most {
            open: crate::MOST_OPEN,
            pulls: crate::MOST_PULLS
        }
    );
    assert_eq!((crate::MOST_OPEN, crate::MOST_PULLS), (500, 100));
    assert_eq!(plain.peering(), None);
    assert!(plain.joined("https://peer.example").is_none());
}
