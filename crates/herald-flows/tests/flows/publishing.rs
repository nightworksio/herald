//! The instance's feeds written as a static signed log: each feed's pages
//! and the discovery document listing their digests, signed beside it by
//! the newest key, written where a feed changed or `refresh` passed, to a
//! branch in one commit moved only where its head is still its parent.

use herald_config::{Config, Uses, load};
use herald_core::{DestinationName, Feed, FeedItem, Publication};
use herald_flows::{Connected, Ports, Publishing, Stop, prepare, publish};
use herald_peers::keys::method;
use herald_peers::pages::{Log, listing, signed};
use herald_ports::{Algorithm, CryptoError, Published as _, Secret, Signer};
use herald_testing::{FakeClock, FakeSecrets, FakeSigner};
use jiff::{SignedDuration, Timestamp};

use crate::harness::{Failing, Fault, NOON, Script, broken, healthy};

/// The instance's newest key, as the fake signer reads an Ed25519 key.
const KEY: &str = "new-federation-key-of-32-bytes!!";
/// Where the feed's newest page is served.
const HEAD: &str = "https://owner.example/log/herald/v1/feeds/releases";
/// Where the discovery document is served.
const DISCOVERY: &str = "https://owner.example/log/.well-known/herald";
/// The branch, on a log committed to the default one.
const PAGES: &str = "[federation.pages]\nurl = \"https://owner.example/log\"\n";

fn config(pages: &str) -> Config {
    let text = format!(
        r#"version = 1
[store]
kind = "sqlite"
[federation]
domain = "owner.example"
keys = [{{ secret = "HERALD_KEY" }}]
{pages}
[markers]
key = {{ secret = "HERALD_MARKER" }}
[destinations.releases]
kind = "herald"
[sources.gate]
kind = "github"
repository = "nightworksio/php-mutation-gate"
[[routes]]
id = "fed"
sources = ["gate"]
destination = "releases"
"#
    );
    load(
        "herald.toml",
        &text,
        &|_| Err("no file".to_owned()),
        &crate::harness::kinds(),
    )
    .unwrap()
}

struct World {
    config: Config,
    connected: Connected<Script>,
    script: Script,
    store: Failing,
}

impl World {
    fn new(pages: &str, repository: Option<&str>) -> Self {
        let config = config(pages);
        let secrets = FakeSecrets::default()
            .with("HERALD_KEY", KEY)
            .with("HERALD_MARKER", "marker-key")
            .with("GITHUB_TOKEN", "pages-token");
        let prepared =
            pollster::block_on(prepare(&config, &secrets, &FakeSigner, Uses::Polls)).unwrap();
        let script = Script::default();
        let connected = prepared
            .in_repository(repository)
            .connect(|_| Ok::<_, ()>(script.clone()))
            .unwrap();
        Self {
            config,
            connected,
            script,
            store: healthy(),
        }
    }

    fn branched() -> Self {
        Self::new(PAGES, Some("owner/site"))
    }

    fn answering(&self, answers: &[(&str, u16, &str)]) {
        self.script.answers.lock().unwrap().extend(
            answers
                .iter()
                .map(|(line, status, body)| ((*line).to_owned(), *status, (*body).to_owned())),
        );
    }

    fn published_with<G: Signer>(
        &self,
        signer: &G,
        at: Timestamp,
    ) -> Result<Option<Publishing>, Stop> {
        let ports = Ports {
            store: &self.store,
            clock: &FakeClock::at(at),
            signer,
            budget: herald_flows::Budget::UNBOUNDED,
        };
        pollster::block_on(publish(&self.config, &self.connected, &ports))
    }

    fn published(&self, at: Timestamp) -> Result<Option<Publishing>, Stop> {
        self.published_with(&FakeSigner, at)
    }

    fn fed(&self, sequences: &[u64]) {
        let releases = DestinationName::new("releases");
        let feed = Feed {
            newest: sequences.first().copied().unwrap_or_default(),
            items: sequences
                .iter()
                .map(|sequence| FeedItem {
                    sequence: *sequence,
                    envelope: serde_json::json!({ "identity": format!("e{sequence}") }),
                })
                .collect(),
        };
        let at = pollster::block_on(self.store.inner.feed(&releases))
            .unwrap()
            .map(|kept| kept.version);
        pollster::block_on(self.store.inner.put_feed(&releases, &feed, at)).unwrap();
    }

    fn record(&self) -> Publication {
        pollster::block_on(self.store.inner.publication())
            .unwrap()
            .map(|kept| kept.value)
            .unwrap_or_default()
    }

    fn tree(&self, at: usize) -> serde_json::Value {
        serde_json::from_slice(&self.script.bodies()[at]).unwrap()
    }
}

fn first_commit() -> [(&'static str, u16, &'static str); 4] {
    [
        (
            "GET https://api.github.com/repos/owner/site/git/ref/heads/herald-pages",
            404,
            "",
        ),
        (
            "POST https://api.github.com/repos/owner/site/git/trees",
            201,
            r#"{"sha":"t1"}"#,
        ),
        (
            "POST https://api.github.com/repos/owner/site/git/commits",
            201,
            r#"{"sha":"c1"}"#,
        ),
        (
            "POST https://api.github.com/repos/owner/site/git/refs",
            201,
            "{}",
        ),
    ]
}

fn after_c1(moved: (u16, &'static str)) -> [(&'static str, u16, &'static str); 5] {
    [
        (
            "GET https://api.github.com/repos/owner/site/git/ref/heads/herald-pages",
            200,
            r#"{"object":{"sha":"c1"}}"#,
        ),
        (
            "GET https://api.github.com/repos/owner/site/git/commits/c1",
            200,
            r#"{"tree":{"sha":"t1"}}"#,
        ),
        (
            "POST https://api.github.com/repos/owner/site/git/trees",
            201,
            r#"{"sha":"t2"}"#,
        ),
        (
            "POST https://api.github.com/repos/owner/site/git/commits",
            201,
            r#"{"sha":"c2"}"#,
        ),
        (
            "PATCH https://api.github.com/repos/owner/site/git/refs/heads/herald-pages",
            moved.0,
            moved.1,
        ),
    ]
}

fn written(outcome: Result<Option<Publishing>, Stop>) -> (u64, Vec<(String, String)>, usize) {
    match outcome {
        Ok(Some(Publishing::Written {
            sequence,
            files,
            changed,
        })) => (sequence, files, changed),
        other => panic!("{other:?}"),
    }
}

fn file<'f>(files: &'f [(String, String)], path: &str) -> &'f str {
    &files.iter().find(|(named, _)| named == path).unwrap().1
}

fn paths(tree: &serde_json::Value) -> Vec<(String, bool)> {
    tree["tree"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| {
            (
                entry["path"].as_str().unwrap().to_owned(),
                entry.get("content").is_some(),
            )
        })
        .collect()
}

#[test]
fn a_feed_s_log_is_committed_whole_first_and_signed_by_the_newest_key() {
    let world = World::branched();
    world.fed(&[45, 25, 5]);
    world.answering(&first_commit());
    let (sequence, files, changed) = written(world.published_with(&Sized, NOON));
    assert_eq!(sequence, 1);
    let names: Vec<&str> = files.iter().map(|(path, _)| path.as_str()).collect();
    assert_eq!(
        names,
        [
            "herald/v1/feeds/releases",
            "herald/v1/pages/releases/1",
            "herald/v1/pages/releases/0",
            ".well-known/herald.sig",
            ".well-known/herald",
            ".nojekyll",
        ]
    );
    assert_eq!(changed, 6);
    let tree = world.tree(1);
    assert_eq!(tree.get("base_tree"), None);
    assert_eq!(paths(&tree).len(), 6);
    assert_eq!(
        world.tree(2),
        serde_json::json!({ "message": "herald: static log 1", "tree": "t1", "parents": [] })
    );
    assert_eq!(
        world.tree(3),
        serde_json::json!({ "ref": "refs/heads/herald-pages", "sha": "c1" })
    );
    let discovery = file(&files, ".well-known/herald");
    let statement = signed(
        file(&files, ".well-known/herald.sig").as_bytes(),
        DISCOVERY,
        discovery.as_bytes(),
    )
    .unwrap();
    let log = Log {
        sequence: 1,
        published: NOON,
    };
    assert_eq!(statement.log, log);
    let key: [u8; 32] = KEY.as_bytes().try_into().unwrap();
    assert_eq!(statement.proof.method, method("owner.example", &key));
    let listed = listing(discovery.as_bytes(), log, HEAD).unwrap();
    assert_eq!(listed.newest, 45);
    assert_eq!(listed.pages.len(), 3);
    let document: serde_json::Value = serde_json::from_str(discovery).unwrap();
    assert_eq!(document["api"], "https://owner.example/log/herald/v1");
    assert_eq!(document["keys"]["sequence"], 1);
    let record = world.record();
    assert_eq!(
        (record.sequence, record.published, record.commit.as_deref()),
        (1, Some(NOON), Some("c1"))
    );
    assert_eq!(record.files.len(), 4);
    assert_eq!(
        world.published(NOON + SignedDuration::from_hours(23)),
        Ok(Some(Publishing::Current(1)))
    );
    assert_eq!(world.script.seen().len(), 4);
}

#[test]
fn an_unchanged_log_is_written_again_once_refresh_passed_its_pages_left_as_they_are() {
    let world = World::branched();
    world.fed(&[45, 25, 5]);
    world.answering(&first_commit());
    written(world.published(NOON));
    world.answering(&after_c1((200, "{}")));
    let (sequence, _, changed) = written(world.published(NOON + SignedDuration::from_hours(24)));
    assert_eq!((sequence, changed), (2, 3));
    let tree = world.tree(6);
    assert_eq!(tree["base_tree"], "t1");
    assert_eq!(
        paths(&tree),
        [
            (".well-known/herald.sig".to_owned(), true),
            (".well-known/herald".to_owned(), true),
            (".nojekyll".to_owned(), true),
        ]
    );
    assert_eq!(world.tree(7)["parents"], serde_json::json!(["c1"]));
    assert_eq!(
        world.tree(8),
        serde_json::json!({ "sha": "c2", "force": false })
    );
    assert_eq!(world.record().commit.as_deref(), Some("c2"));
}

#[test]
fn a_changed_feed_writes_its_changed_pages_and_removes_those_it_no_longer_has() {
    let world = World::branched();
    world.fed(&[45, 25, 5]);
    world.answering(&first_commit());
    written(world.published(NOON));
    world.fed(&[45, 25]);
    world.answering(&after_c1((200, "{}")));
    let (sequence, _, changed) = written(world.published(NOON));
    assert_eq!((sequence, changed), (2, 5));
    assert_eq!(
        paths(&world.tree(6)),
        [
            ("herald/v1/pages/releases/1".to_owned(), true),
            (".well-known/herald.sig".to_owned(), true),
            (".well-known/herald".to_owned(), true),
            (".nojekyll".to_owned(), true),
            ("herald/v1/pages/releases/0".to_owned(), false),
        ]
    );
}

#[test]
fn a_branch_moved_by_another_run_is_written_whole_and_one_moved_meanwhile_stops_the_writing() {
    let world = World::branched();
    world.fed(&[5]);
    world.answering(&first_commit());
    written(world.published(NOON));
    world.fed(&[6, 5]);
    world.answering(&[
        (
            "GET https://api.github.com/repos/owner/site/git/ref/heads/herald-pages",
            200,
            r#"{"object":{"sha":"elsewhere"}}"#,
        ),
        (
            "POST https://api.github.com/repos/owner/site/git/trees",
            201,
            r#"{"sha":"t2"}"#,
        ),
        (
            "POST https://api.github.com/repos/owner/site/git/commits",
            201,
            r#"{"sha":"c2"}"#,
        ),
        (
            "PATCH https://api.github.com/repos/owner/site/git/refs/heads/herald-pages",
            422,
            "{}",
        ),
    ]);
    assert_eq!(
        world.published(NOON),
        Err(Stop::Store(
            "another run moved the static log's branch first; the next run writes the log again"
                .to_owned()
        ))
    );
    let tree = world.tree(5);
    assert_eq!(tree.get("base_tree"), None);
    assert_eq!(paths(&tree).len(), 4);
    assert_eq!(world.tree(6)["parents"], serde_json::json!(["elsewhere"]));
    let record = world.record();
    assert_eq!((record.sequence, record.commit.as_deref()), (2, Some("c1")));
}

#[test]
fn a_log_written_to_a_directory_alone_reaches_no_branch() {
    let world = World::new(
        "[federation.pages]\nurl = \"https://owner.example/log\"\nartifact = \"_site\"\n",
        None,
    );
    world.fed(&[5]);
    let (sequence, files, changed) = written(world.published(NOON));
    assert_eq!((sequence, files.len(), changed), (1, 4, 0));
    assert_eq!(world.script.seen(), Vec::<String>::new());
    assert_eq!(world.record().commit, None);
    let unlogged = World::new("", None);
    assert_eq!(unlogged.published(NOON), Ok(None));
}

#[test]
fn each_refusal_of_the_branch_stops_the_writing_saying_why() {
    let unplaced = World::new(PAGES, None);
    assert_eq!(
        unplaced.published(NOON),
        Err(Stop::Refused(
            "GITHUB_REPOSITORY is not set; name the repository in [federation.pages], or run herald in the repository's CI".to_owned()
        ))
    );
    let head = "GET https://api.github.com/repos/owner/site/git/ref/heads/herald-pages";
    for (answers, stop) in [
        (
            vec![(head, 500, "")],
            Stop::Refused("GitHub answered 500 for the branch's head of the static log".to_owned()),
        ),
        (
            vec![(head, 200, "no")],
            Stop::Refused("GitHub's answer for the branch's head could not be read".to_owned()),
        ),
        (
            vec![],
            Stop::Unreachable(format!(
                "the host could not be reached: nothing scripted for {head}"
            )),
        ),
        (
            first_commit()[..3]
                .iter()
                .copied()
                .chain([(
                    "POST https://api.github.com/repos/owner/site/git/refs",
                    500,
                    "",
                )])
                .collect(),
            Stop::Refused("GitHub answered 500 for the branch of the static log".to_owned()),
        ),
    ] {
        let world = World::branched();
        world.answering(&answers);
        assert_eq!(world.published(NOON), Err(stop));
    }
}

#[test]
fn an_unreadable_answer_for_the_head_s_commit_the_tree_or_the_commit_stops_the_writing() {
    let world = World::branched();
    world.fed(&[5]);
    world.answering(&first_commit());
    written(world.published(NOON));
    for (at, newest, what) in [
        (1, 6, "the head's commit"),
        (2, 7, "the tree"),
        (3, 8, "the commit"),
    ] {
        world.fed(&[newest, 5]);
        let mut answers = after_c1((200, "{}"))[..=at].to_vec();
        answers[at].2 = "no";
        world.answering(&answers);
        assert_eq!(
            world.published(NOON),
            Err(Stop::Refused(format!(
                "GitHub's answer for {what} could not be read"
            )))
        );
    }
}

/// A signer whose signatures are as long as an Ed25519 signature, as a
/// reader of the log demands.
struct Sized;

impl Signer for Sized {
    async fn sign(
        &self,
        _: Algorithm,
        key: &Secret,
        message: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        Ok(herald_testing::fake_ed25519(
            key.expose().as_bytes(),
            message,
        ))
    }

    async fn public(&self, algorithm: Algorithm, key: &Secret) -> Result<Vec<u8>, CryptoError> {
        FakeSigner.public(algorithm, key).await
    }
}

struct Unsigning;

impl Signer for Unsigning {
    async fn sign(&self, _: Algorithm, key: &Secret, _: &[u8]) -> Result<Vec<u8>, CryptoError> {
        Err(CryptoError::BadKey {
            name: key.name().to_owned(),
        })
    }

    async fn public(&self, algorithm: Algorithm, key: &Secret) -> Result<Vec<u8>, CryptoError> {
        FakeSigner.public(algorithm, key).await
    }
}

#[test]
fn a_store_or_key_that_fails_stops_the_writing_and_another_run_s_writes_each_time_too() {
    for fault in [
        Fault::ReadsPublication,
        Fault::WritesPublication,
        Fault::ReadsFeeds,
        Fault::ReadsPublished,
    ] {
        let mut world = World::new(
            "[federation.pages]\nurl = \"https://owner.example/log\"\nartifact = \"_site\"\n",
            None,
        );
        world.store.fault = fault;
        assert_eq!(
            world.published(NOON),
            Err(Stop::Store(broken().to_string())),
            "{fault:?}"
        );
    }
    let mut world = World::new(
        "[federation.pages]\nurl = \"https://owner.example/log\"\nartifact = \"_site\"\n",
        None,
    );
    world.store.fault = Fault::ConflictsPublication;
    assert_eq!(
        world.published(NOON),
        Err(Stop::Store(format!(
            "the static log's record was written by another run each of {} times herald wrote it",
            world.config.attempts.get()
        )))
    );
    world.store.fault = Fault::Nothing;
    assert!(matches!(
        world.published_with(&Unsigning, NOON),
        Err(Stop::Proof(why)) if why.starts_with("the static log could not be signed: ")
    ));
}
