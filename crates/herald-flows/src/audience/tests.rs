use herald_config::{Kinds, Uses, load};
use herald_testing::{FakeClock, FakeSecrets, FakeSigner, FakeStore, ScriptedHttp};

use super::*;
use crate::announce::Ports;
use crate::budget::Budget;

#[test]
fn a_route_this_run_does_not_reach_is_withheld_the_section_with_no_request() {
    let kinds = Kinds::new()
        .destination::<herald_dest_discord::DiscordKind>()
        .source::<herald_sources_forge::GitHubKind>()
        .store::<herald_store_sqlite::SqliteKind>();
    let config = load(
        "herald.toml",
        "version = 1\n[store]\nkind = \"sqlite\"\n[destinations.discord]\nkind = \"discord\"\ntoken = { secret = \"HERALD_DISCORD_TOKEN\" }\n[sources.open]\nkind = \"github\"\nrepository = \"o/open\"\n[[routes]]\nid = \"team\"\nsources = [\"open\"]\ndestination = \"discord\"\nchannel = \"1566200000000000100\"\ntemplate = \"t\"\nprivate = true\n[templates.t]\ntitle = \"{{ event.title }}\"\nbody = \"{{ event.body }}\"\n",
        &|_| Err("no file".to_owned()),
        &kinds,
    )
    .unwrap();
    let secrets = FakeSecrets::default().with("HERALD_DISCORD_TOKEN", "hunter2");
    let prepared = pollster::block_on(crate::prepare(
        &config,
        &secrets,
        &FakeSigner,
        Uses::Announces,
    ))
    .unwrap();
    let http = ScriptedHttp::new(Vec::new());
    let mut connected = prepared.connect(|_| Ok::<_, ()>(&http)).unwrap();
    connected.routes.clear();
    let (store, clock) = (
        FakeStore::default(),
        FakeClock::at(jiff::Timestamp::UNIX_EPOCH),
    );
    let ports = Ports {
        store: &store,
        clock: &clock,
        signer: &FakeSigner,
        budget: Budget::UNBOUNDED,
    };
    let why = pollster::block_on(withheld(
        (&config, &connected, &ports),
        (&RouteId::new("team"), 0),
        &mut Audiences::new(),
    ));
    assert_eq!(why.as_deref(), Some(UNSENT));
    assert_eq!(http.asked().len(), 0);
}
