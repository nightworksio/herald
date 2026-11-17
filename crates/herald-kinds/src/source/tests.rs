use std::future::ready;

use herald_core::{Cursor, Kind, Polled, SecretRef};
use herald_ports::{Base, Boxed, DynHttp, Secret};

use super::*;

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

    fn kinds(&self) -> &'static [Kind] {
        &[Kind::RELEASE]
    }

    fn backfill(&self) -> usize {
        3
    }

    fn poller(&self, token: Option<Secret>, _: bool) -> Polling {
        Polling::One(Arc::new(Quiet(token, "https://plain.example".to_owned())))
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

    fn kinds(&self) -> &'static [Kind] {
        self.0.kinds()
    }

    fn backfill(&self) -> usize {
        self.0.backfill()
    }

    fn poller(&self, token: Option<Secret>, edits: bool) -> Polling {
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
    assert!(!source.origins());
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
    let polling = source.poller(Some(Secret::new("TOKEN", "t")), false);
    assert_eq!(polling.api(), "https://plain.example");
    let (Polling::One(poll), Polling::One(untokened)) = (polling, source.poller(None, false))
    else {
        panic!("a plain source is polled as one");
    };
    assert_eq!(poll.remembered(), 2);
    assert_eq!(untokened.remembered(), 1);
    assert_eq!(poll.tag_window(), None);
    let http = crate::reach::tests::Silent;
    assert_eq!(pollster::block_on(poll.polled(&http, None)), Ok(None));
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
        pollster::block_on(owner.polled(&http, &[(&one, None), (&two, None)], &Rounded::Unread)),
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
        pollster::block_on(Beside(bases.clone()).polled(&crate::reach::tests::Silent, None)),
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
