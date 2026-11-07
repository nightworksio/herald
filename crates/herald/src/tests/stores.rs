use super::world::*;
use crate::*;

#[test]
fn a_release_kept_only_in_its_post_is_announced_once_and_poll_stops_without_cursors() {
    let mut setup = setup(&CONFIG.replace("kind = \"git-branch\"", MARKERS));
    setup.vars.remove("GITHUB_TOKEN");
    let world = World::default();
    discord(
        &world,
        &[
            ("/users/@me", 200, r#"{"id":"99"}"#),
            (
                "/channels/1566200000000000100/messages?limit=100",
                200,
                "[]",
            ),
            (
                "/channels/1566200000000000100/messages",
                200,
                r#"{"id":"1"}"#,
            ),
        ],
    );
    let announced = herald(&setup, &world, &ANNOUNCE);
    assert_eq!(
        (announced.code, announced.out.as_str()),
        (DONE, "releases: announced (post)\n")
    );
    setup
        .vars
        .insert("GITHUB_ACTIONS".to_owned(), "true".to_owned());
    let polled = herald(&setup, &world, &POLL);
    assert_eq!(
        (polled.code, polled.out.as_str()),
        (
            STOPPED,
            "::add-mask::discord-token\ncli: stopped: this store keeps no cursors; choose a store that does. The next poll asks again.\n"
        )
    );
    let housekept = herald(&setup, &world, &["housekeep"]);
    assert!(
        housekept.out.ends_with("No chore is due.\n"),
        "{}",
        housekept.out
    );
}

#[test]
fn a_release_is_announced_once_through_the_actions_cache() {
    let mut setup = setup(&CONFIG.replace(
        "kind = \"git-branch\"",
        "kind = \"actions-cache\"\nprefix = \"run-7\"",
    ));
    let world = World::default();
    discord(
        &world,
        &[(
            "/channels/1566200000000000100/messages",
            200,
            r#"{"id":"1"}"#,
        )],
    );
    let unset = herald(&setup, &world, &ANNOUNCE);
    assert_eq!(
        unset.err,
        "ACTIONS_RESULTS_URL is not set; herald reads it from the CI run\n"
    );
    setup.vars.insert(
        "ACTIONS_RESULTS_URL".to_owned(),
        "https://results.example".to_owned(),
    );
    let tokenless = herald(&setup, &world, &ANNOUNCE);
    assert_eq!(
        tokenless.err,
        "ACTIONS_RUNTIME_TOKEN is not set; the actions-cache store runs through herald's Action\n"
    );
    setup
        .vars
        .insert("ACTIONS_RUNTIME_TOKEN".to_owned(), "runtime".to_owned());
    let first = herald(&setup, &world, &ANNOUNCE);
    assert_eq!(
        (first.code, first.out.as_str(), first.err.as_str()),
        (DONE, "releases: announced (post)\n", "")
    );
    let again = herald(&setup, &world, &ANNOUNCE);
    assert_eq!(again.out, "releases: already announced; nothing sent\n");
    assert!(
        world
            .state
            .lock()
            .unwrap()
            .cache
            .keys()
            .all(|key| key.starts_with("run-7-"))
    );
}

#[test]
fn a_release_is_announced_once_through_the_repository_s_variables() {
    let config = CONFIG.replace(
        "kind = \"git-branch\"",
        "kind = \"repo-variables\"\ntoken = { secret = \"HERALD_STATE_TOKEN\" }",
    );
    let mut setup = setup(&config);
    let world = World::default();
    discord(
        &world,
        &[(
            "/channels/1566200000000000100/messages",
            200,
            r#"{"id":"1"}"#,
        )],
    );
    let unset = herald(&setup, &world, &ANNOUNCE);
    assert_eq!(
        unset.err,
        "HERALD_STATE_TOKEN is not set; the repo-variables store needs a token with the repository's Variables permission\n"
    );
    setup
        .vars
        .insert("HERALD_STATE_TOKEN".to_owned(), "variables".to_owned());
    let first = herald(&setup, &world, &ANNOUNCE);
    assert_eq!(
        (first.code, first.out.as_str(), first.err.as_str()),
        (DONE, "releases: announced (post)\n", "")
    );
    let again = herald(&setup, &world, &ANNOUNCE);
    assert_eq!(again.out, "releases: already announced; nothing sent\n");
    assert!(
        world
            .state
            .lock()
            .unwrap()
            .variables
            .keys()
            .all(|name| name.starts_with("HERALD_"))
    );
}

#[test]
fn a_release_is_announced_once_through_a_dynamodb_table() {
    let config = CONFIG.replace(
        "kind = \"git-branch\"",
        "kind = \"dynamodb\"\nregion = \"eu-west-1\"\nendpoint = \"https://dynamodb.example\"",
    );
    let mut setup = setup(&config);
    let world = World::default();
    discord(
        &world,
        &[(
            "/channels/1566200000000000100/messages",
            200,
            r#"{"id":"1"}"#,
        )],
    );
    let unset = herald(&setup, &world, &ANNOUNCE);
    assert_eq!(
        unset.err,
        "AWS_ACCESS_KEY_ID is not set; the dynamodb store signs its requests with it\n"
    );
    for (name, value) in [
        ("AWS_ACCESS_KEY_ID", "AKID"),
        ("AWS_SECRET_ACCESS_KEY", "secret"),
    ] {
        setup.vars.insert(name.to_owned(), value.to_owned());
    }
    let first = herald(&setup, &world, &ANNOUNCE);
    assert_eq!(
        (first.code, first.out.as_str(), first.err.as_str()),
        (DONE, "releases: announced (post)\n", "")
    );
    let again = herald(&setup, &world, &ANNOUNCE);
    assert_eq!(again.out, "releases: already announced; nothing sent\n");
    assert!(!world.state.lock().unwrap().items.is_empty());
}

#[test]
fn a_release_kept_in_a_sqlite_file_beside_the_config_is_announced_once() {
    let mut setup = setup(&CONFIG.replace("kind = \"git-branch\"", "kind = \"sqlite\""));
    setup.vars.remove("GITHUB_TOKEN");
    let world = World::default();
    discord(
        &world,
        &[(
            "/channels/1566200000000000100/messages",
            200,
            r#"{"id":"1"}"#,
        )],
    );
    let first = herald(&setup, &world, &ANNOUNCE);
    assert_eq!(
        (first.code, first.out.as_str(), first.err.as_str()),
        (DONE, "releases: announced (post)\n", "")
    );
    assert!(setup.directory.path().join("herald.sqlite").exists());
    let again = herald(&setup, &world, &ANNOUNCE);
    assert_eq!(again.out, "releases: already announced; nothing sent\n");
}

#[test]
fn a_sqlite_file_that_cannot_be_opened_stops_the_run() {
    let setup = setup(&CONFIG.replace(
        "kind = \"git-branch\"",
        "kind = \"sqlite\"\npath = \"missing/herald.sqlite\"",
    ));
    let stopped = herald(&setup, &World::default(), &ANNOUNCE);
    assert_eq!(stopped.code, STOPPED);
    let file = setup.directory.path().join("missing/herald.sqlite");
    assert!(
        stopped.err.starts_with(&format!(
            "the sqlite store {} could not be opened: the database refused the statements: {} could not be made:",
            file.display(),
            file.display()
        )),
        "{}",
        stopped.err
    );
    std::fs::write(
        setup.directory.path().join("other.sqlite"),
        "not a database",
    )
    .unwrap();
    let foreign = herald(
        &super::world::setup(&CONFIG.replace(
            "kind = \"git-branch\"",
            &format!(
                "kind = \"sqlite\"\npath = \"{}\"",
                setup.directory.path().join("other.sqlite").display()
            ),
        )),
        &World::default(),
        &ANNOUNCE,
    );
    assert_eq!(foreign.code, STOPPED);
    assert!(
        foreign.err.contains("could not be opened"),
        "{}",
        foreign.err
    );
}

#[test]
fn a_sqlite_file_a_newer_herald_laid_out_stops_the_run() {
    use herald_ports::Sql as _;
    let setup = setup(&CONFIG.replace("kind = \"git-branch\"", "kind = \"sqlite\""));
    let file = setup.directory.path().join("herald.sqlite");
    let newer = herald_native::NativeSqlite::open(&file).unwrap();
    let statement = |sql: &str| herald_ports::Statement {
        sql: sql.to_owned(),
        params: vec![],
    };
    pollster::block_on(newer.transaction(vec![
        statement("CREATE TABLE herald_meta (id INTEGER PRIMARY KEY, layout BIGINT, made BIGINT)"),
        statement("INSERT INTO herald_meta VALUES (1, 2, 0)"),
    ]))
    .unwrap();
    drop(newer);
    let stopped = herald(&setup, &World::default(), &ANNOUNCE);
    assert_eq!(stopped.code, STOPPED);
    assert_eq!(
        stopped.err,
        format!(
            "the sqlite store {} could not be opened: a record in the store could not be read: a newer herald laid the database out; run that herald or a later one\n",
            file.display()
        )
    );
}

/// The URL of CI's Postgres service, scoped to a schema of `name`'s own,
/// made empty; none where no Postgres is handed in, which CI refuses.
fn postgres(name: &str) -> Option<String> {
    use herald_ports::Sql as _;
    let url = std::env::var("HERALD_TEST_POSTGRES")
        .ok()
        .filter(|url| !url.is_empty());
    assert!(
        url.is_some() || std::env::var_os("CI").is_none(),
        "HERALD_TEST_POSTGRES is not set, and CI runs these tests against its Postgres service"
    );
    let url = url?;
    let schema = format!("{name}_{}", std::process::id());
    on_tokio(async {
        let sql =
            herald_native::NativePostgres::connect(&herald_ports::Secret::new("URL", url.as_str()))
                .await
                .unwrap();
        sql.transaction(
            [
                format!("DROP SCHEMA IF EXISTS {schema} CASCADE"),
                format!("CREATE SCHEMA {schema}"),
            ]
            .into_iter()
            .map(|sql| herald_ports::Statement {
                sql,
                params: vec![],
            })
            .collect(),
        )
        .await
        .unwrap();
    });
    let joined = if url.contains('?') { '&' } else { '?' };
    Some(format!("{url}{joined}options=-c%20search_path%3D{schema}"))
}

fn kept_in_postgres(url: Option<&str>) -> Setup {
    let mut setup = setup(&CONFIG.replace(
        "kind = \"git-branch\"",
        "kind = \"postgres\"\nurl = { secret = \"HERALD_POSTGRES_URL\" }",
    ));
    setup.vars.remove("GITHUB_TOKEN");
    if let Some(url) = url {
        setup
            .vars
            .insert("HERALD_POSTGRES_URL".to_owned(), url.to_owned());
    }
    setup
}

#[test]
fn a_release_kept_in_postgres_is_announced_once() {
    let Some(url) = postgres("cli_once") else {
        return;
    };
    let setup = kept_in_postgres(Some(&url));
    let world = World::default();
    discord(
        &world,
        &[(
            "/channels/1566200000000000100/messages",
            200,
            r#"{"id":"1"}"#,
        )],
    );
    let first = herald(&setup, &world, &ANNOUNCE);
    assert_eq!(
        (first.code, first.out.as_str(), first.err.as_str()),
        (DONE, "releases: announced (post)\n", "")
    );
    let again = herald(&setup, &world, &ANNOUNCE);
    assert_eq!(again.out, "releases: already announced; nothing sent\n");
}

#[test]
fn a_postgres_store_that_cannot_be_reached_or_read_stops_the_run() {
    let unset = herald(&kept_in_postgres(None), &World::default(), &ANNOUNCE);
    assert_eq!(unset.code, STOPPED);
    assert_eq!(
        unset.err,
        "HERALD_POSTGRES_URL is not set; the postgres store reads its connection URL from it\n"
    );
    let unreachable = herald(
        &kept_in_postgres(Some("postgres://herald@127.0.0.1:1/herald?sslmode=disable")),
        &World::default(),
        &ANNOUNCE,
    );
    assert_eq!(unreachable.code, STOPPED);
    assert!(
        unreachable.err.starts_with(
            "the postgres store could not be opened: the database refused the statements: "
        ),
        "{}",
        unreachable.err
    );
    let Some(url) = postgres("cli_newer") else {
        return;
    };
    on_tokio(async {
        use herald_ports::Sql as _;
        let sql =
            herald_native::NativePostgres::connect(&herald_ports::Secret::new("URL", url.as_str()))
                .await
                .unwrap();
        sql.transaction(
            [
                "CREATE TABLE herald_meta (id INTEGER PRIMARY KEY, layout BIGINT, made BIGINT)",
                "INSERT INTO herald_meta VALUES (1, 2, 0)",
            ]
            .into_iter()
            .map(|sql| herald_ports::Statement {
                sql: sql.to_owned(),
                params: vec![],
            })
            .collect(),
        )
        .await
        .unwrap();
    });
    let newer = herald(&kept_in_postgres(Some(&url)), &World::default(), &ANNOUNCE);
    assert_eq!(newer.code, STOPPED);
    assert_eq!(
        newer.err,
        "the postgres store could not be opened: a record in the store could not be read: a newer herald laid the database out; run that herald or a later one\n"
    );
}

#[test]
fn the_native_binary_opens_no_database_or_key_value_store_of_another_platform() {
    use herald_compose::{Host as _, KvAt, SqlAt};

    let vars = BTreeMap::new();
    let unreached = |base: &str| Err(format!("{base} is out of reach"));
    let native = stores::Native::new(&vars, &unreached);
    let unhosted = "only a Cloudflare Worker, Deno or Fastly Compute reaches it";
    for at in [
        SqlAt::D1("HERALD_DB"),
        SqlAt::DurableObject {
            binding: "HERALD",
            object: "herald",
        },
    ] {
        assert_eq!(
            pollster::block_on(native.sql(at)).err().as_deref(),
            Some(unhosted)
        );
    }
    assert_eq!(
        pollster::block_on(native.kv(KvAt::DenoKv(None)))
            .err()
            .as_deref(),
        Some(unhosted)
    );
}
