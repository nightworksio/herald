//! `[presets]` (ADR-0092): a shipped or a local preset expanded into the
//! tables it writes, named for the adopting table, given its keys, and every
//! mistake told at the adopting table.

use std::collections::BTreeMap;

use herald_config::{Config, Files, UNCONTAINED, Unread, Within, load, load_with};
use herald_core::{Kind, Prereleases, SourceName};

const EXAMPLE: &str = r#"version = 1

[store]
kind = "git-branch"
key = { secret = "HERALD_STORE_KEY" }

[destinations.discord]
kind = "discord"
token = { secret = "HERALD_DISCORD_TOKEN" }

[sources.app]
kind = "github"
repository = "nightworksio/app"
"#;

const RELEASES: &str = r#"[presets.releases]
use = "herald:announce-releases"
destination = "discord"
sources = ["app"]
channel = "1566200000000000100"
"#;

/// Files beside the config, by name; a local preset read only by name.
struct Beside(BTreeMap<&'static str, String>);

impl Files for Beside {
    fn read(&self, path: &str, _: Within<'_>) -> Result<String, Unread> {
        Err(Unread::Failed(format!("no file {path}")))
    }

    fn read_inside(&self, path: &str) -> Result<String, String> {
        self.0
            .get(path)
            .cloned()
            .ok_or_else(|| format!("no file {path}"))
    }
}

fn with(tables: &str) -> String {
    format!("{EXAMPLE}\n{tables}")
}

fn nothing(_: &str) -> Result<String, String> {
    Err("no file".to_owned())
}

fn loaded(text: &str) -> Config {
    load("herald.toml", text, &nothing, &crate::common::kinds()).unwrap()
}

fn errors(text: &str) -> String {
    load("herald.toml", text, &nothing, &crate::common::kinds())
        .unwrap_err()
        .to_string()
}

fn local(text: &str, file: &str) -> Result<Config, String> {
    load_with(
        "herald.toml",
        text,
        &Beside(BTreeMap::from([("./team.toml", file.to_owned())])),
        &crate::common::kinds(),
    )
    .map_err(|errors| errors.to_string())
}

/// The line `needle` starts on in `text`, counted from one.
fn line_of(text: &str, needle: &str) -> usize {
    text[..text.find(needle).unwrap()].lines().count() + 1
}

#[test]
fn announcing_releases_is_one_route_and_its_template_named_for_the_adopting_table() {
    let config = loaded(&with(RELEASES));
    let [route] = config.routes.as_slice() else {
        panic!("one route: {:?}", config.routes.len());
    };
    assert_eq!(route.route.id.as_str(), "releases-releases");
    assert_eq!(route.route.destination.as_str(), "discord");
    assert_eq!(route.route.filter.sources, [SourceName::new("app")]);
    assert_eq!(route.route.filter.kinds, [Kind::RELEASE]);
    assert_eq!(route.route.filter.prereleases, Prereleases::Include);
    assert_eq!(
        route
            .route
            .template
            .as_ref()
            .map(herald_core::TemplateName::as_str),
        Some("releases-release")
    );
}

#[test]
fn two_tables_adopt_one_preset_and_their_tables_never_collide() {
    let second = RELEASES.replace("[presets.releases]", "[presets.again]");
    let config = loaded(&with(&format!("{RELEASES}\n{second}")));
    let ids: Vec<&str> = config
        .routes
        .iter()
        .map(|route| route.route.id.as_str())
        .collect();
    assert_eq!(ids, ["again-releases", "releases-releases"]);
}

#[test]
fn a_key_the_table_gives_wins_over_the_presets() {
    let config = loaded(&with(&format!(
        "{RELEASES}prereleases = \"exclude\"\nid = \"mine\"\n"
    )));
    assert_eq!(
        config.routes[0].route.filter.prereleases,
        Prereleases::Exclude
    );
    assert_eq!(config.routes[0].route.id.as_str(), "mine");
}

#[test]
fn a_template_key_lands_in_the_template_and_its_mistake_is_told_at_the_key() {
    let text = with(&format!("{RELEASES}title = \"{{% nonsense\"\n"));
    let errors = errors(&text);
    let line = line_of(&text, "title = ");
    assert!(
        errors.contains(&format!("herald.toml:{line}:"))
            && errors.contains("presets.releases.title"),
        "{errors}"
    );
}

#[test]
fn a_missing_parameter_is_told_at_the_adopting_table() {
    let text =
        with("[presets.releases]\nuse = \"herald:announce-releases\"\nsources = [\"app\"]\n");
    let errors = errors(&text);
    let line = line_of(&text, "[presets.releases]");
    assert!(
        errors.contains(&format!(
            "herald.toml:{line}:1 presets.releases: give destination: The destination its routes announce to"
        )),
        "{errors}"
    );
}

#[test]
fn a_mistake_in_a_value_the_table_gives_is_told_at_that_value_under_the_preset() {
    let text = with(&RELEASES.replace("destination = \"discord\"", "destination = \"nowhere\""));
    let errors = errors(&text);
    let line = line_of(&text, "destination = \"nowhere\"");
    assert!(
        errors.contains(&format!("herald.toml:{line}:"))
            && errors.contains("presets.releases.destination: no destination is named \"nowhere\""),
        "{errors}"
    );
}

#[test]
fn a_key_no_table_of_the_preset_takes_is_named_under_the_preset() {
    let text = with(&format!("{RELEASES}colour = \"red\"\n"));
    let errors = errors(&text);
    assert!(errors.contains("presets.releases"), "{errors}");
    assert!(errors.contains("colour"), "{errors}");
    assert!(!errors.contains("routes["), "{errors}");
}

#[test]
fn a_preset_herald_does_not_ship_is_refused_with_those_it_does() {
    let errors = errors(&with(&RELEASES.replace("announce-releases", "nothing")));
    assert!(
        errors.contains(
            "presets.releases.use: herald ships no preset \"herald:nothing\"; it ships \"herald:announce-releases\""
        ),
        "{errors}"
    );
}

#[test]
fn a_table_the_preset_makes_may_not_be_named_by_another() {
    let errors = errors(&with(&format!(
        "{RELEASES}\n[templates.releases-release]\ntitle = \"t\"\nbody = \"b\"\n"
    )));
    assert!(
        errors.contains(
            "presets.releases: it makes [templates.releases-release], which another table names"
        ),
        "{errors}"
    );
}

#[test]
fn a_table_without_use_is_told_what_to_give() {
    let errors = errors(&with("[presets.releases]\ndestination = \"discord\"\n"));
    assert!(
        errors.contains("presets.releases: give the preset it adopts"),
        "{errors}"
    );
}

const TEAM: &str = r#"version = 1

[[routes]]
id = "notes"
kinds = ["release"]
template = "notes"

[templates.notes]
title = "{{ event.project }}"
body = "{{ event.body }}"
"#;

const ADOPT_TEAM: &str = r#"[presets.team]
use = "./team.toml"
destination = "discord"
sources = ["app"]
channel = "1566200000000000100"
"#;

#[test]
fn a_local_preset_s_tables_are_named_for_the_adopting_table_and_so_are_its_references() {
    let config = local(&with(ADOPT_TEAM), TEAM).unwrap();
    let route = &config.routes[0].route;
    assert_eq!(route.id.as_str(), "team-notes");
    assert_eq!(
        route
            .template
            .as_ref()
            .map(herald_core::TemplateName::as_str),
        Some("team-notes")
    );
    assert_eq!(route.destination.as_str(), "discord");
}

#[test]
fn a_host_that_cannot_follow_links_reads_no_local_preset() {
    let errors = errors(&with(ADOPT_TEAM));
    assert!(errors.contains(UNCONTAINED), "{errors}");
}

#[test]
fn a_local_preset_is_named_from_the_config_s_directory_with_no_way_out() {
    for path in [
        "team.toml",
        "../team.toml",
        "./../team.toml",
        "./a/../team.toml",
        "./a//team.toml",
        "./",
        "./a/./team.toml",
        "/etc/team.toml",
        ".\\team.toml",
    ] {
        let text = with(&ADOPT_TEAM.replace("./team.toml", path));
        let errors = local(&text, TEAM).unwrap_err();
        assert!(
            errors.contains("presets.team.use: use names a preset herald ships"),
            "{path}: {errors}"
        );
    }
}

#[test]
fn a_local_preset_states_the_config_format_it_was_written_for() {
    let errors = local(&with(ADOPT_TEAM), &TEAM.replace("version = 1\n", "")).unwrap_err();
    assert!(
        errors.contains("./team.toml:1:1: give the config format"),
        "{errors}"
    );
    let errors = local(
        &with(ADOPT_TEAM),
        &TEAM.replace("version = 1", "version = 2"),
    )
    .unwrap_err();
    assert!(
        errors.contains("./team.toml:1:11: this herald reads version 1 of the config"),
        "{errors}"
    );
}

#[test]
fn a_local_preset_writes_only_routes_templates_views_reports_and_rules() {
    let text = with(ADOPT_TEAM);
    let line = line_of(&text, "[presets.team]");
    for (section, said) in [
        (
            "[destinations.x]\nkind = \"discord\"\n",
            "a local preset writes \"routes\"",
        ),
        (
            "[presets.inner]\nuse = \"herald:announce-releases\"\n",
            "a local preset does not use another preset",
        ),
        ("[store]\nkind = \"markers\"\n", "give store in herald.toml"),
    ] {
        let errors = local(&text, &format!("{TEAM}\n{section}")).unwrap_err();
        assert!(
            errors.contains(&format!("herald.toml:{line}:1 presets.team.")),
            "{errors}"
        );
        assert!(errors.contains("./team.toml:12:"), "{errors}");
        assert!(errors.contains(said), "{section}: {errors}");
    }
}

#[test]
fn a_local_preset_writes_no_secret_destination_privacy_or_template_file() {
    for (written, key) in [
        ("destination = \"discord\"", "destination"),
        ("private = true", "private"),
        ("vouch_unchecked = true", "vouch_unchecked"),
    ] {
        let file = TEAM.replace(
            "template = \"notes\"",
            &format!("template = \"notes\"\n{written}"),
        );
        let errors = local(&with(ADOPT_TEAM), &file).unwrap_err();
        assert!(
            errors.contains(&format!(
                "./team.toml:7:1: a local preset does not write {key}"
            )),
            "{errors}"
        );
    }
    let file = TEAM.replace("body = \"{{ event.body }}\"", "file = \"notes.md\"");
    let errors = local(&with(ADOPT_TEAM), &file).unwrap_err();
    assert!(
        errors.contains("a local preset does not write file"),
        "{errors}"
    );
    let file = TEAM.replace(
        "kinds = [\"release\"]",
        "kinds = [\"release\"]\nthread = { name = { secret = \"X\" } }",
    );
    let errors = local(&with(ADOPT_TEAM), &file).unwrap_err();
    assert!(
        errors.contains("a local preset names no secret"),
        "{errors}"
    );
}

#[test]
fn a_key_herald_does_not_know_in_a_local_preset_names_the_file_and_its_line() {
    let file = TEAM.replace(
        "template = \"notes\"",
        "template = \"notes\"\ncolour = \"red\"",
    );
    let text = with(ADOPT_TEAM);
    let errors = local(&text, &file).unwrap_err();
    let line = line_of(&text, "[presets.team]");
    assert!(
        errors.contains(&format!("herald.toml:{line}:1 presets.team"))
            && errors.contains("./team.toml:7:"),
        "{errors}"
    );
    assert!(errors.contains("colour"), "{errors}");
}

#[test]
fn a_local_preset_that_is_no_toml_is_told_in_its_own_lines() {
    let errors = local(&with(ADOPT_TEAM), "version = 1\n[[routes]\n").unwrap_err();
    assert!(errors.contains("./team.toml:2:"), "{errors}");
}

const FULL: &str = r#"version = 1

[[routes]]
id = "notes"
sources = ["app"]
kinds = ["release"]
template = "notes"
templates = { discord = "short" }

[[routes]]
id = "digest"
sources = ["weekly"]
template = "herald:queue"

[[routes]]
id = "nudges"
sources = ["review"]
template = "herald:nudge"

[templates.notes]
title = "{{ event.project }}"
body = "{{ event.body }}"

[templates.short]
title = "{{ event.project }}"
body = "{{ event.attributes.version }}"

[views.mine]
stale = "3d"

[reports.weekly]
view = "mine"
at = "08:30"

[rules.review]
view = "mine"
stages = ["2d"]
"#;

const ADOPT_FULL: &str = r#"[presets.team]
use = "./team.toml"
destination = "discord"
channel = "1566200000000000100"
"#;

#[test]
fn a_local_preset_s_views_reports_and_rules_and_every_name_naming_them_are_renamed() {
    let config = local(&with(ADOPT_FULL), FULL).unwrap();
    let ids: Vec<&str> = config
        .routes
        .iter()
        .map(|route| route.route.id.as_str())
        .collect();
    assert_eq!(ids, ["team-notes", "team-digest", "team-nudges"]);
    assert_eq!(
        config.routes[1].route.filter.sources,
        [SourceName::new("team-weekly")]
    );
    assert_eq!(
        config.routes[2].route.filter.sources,
        [SourceName::new("team-review")]
    );
    assert_eq!(
        config.routes[0].route.filter.sources,
        [SourceName::new("app")]
    );
    assert!(config.views.contains_key("team-mine"));
    assert_eq!(config.reports["team-weekly"].view, "team-mine");
    assert_eq!(config.rules["team-review"].watch.view, "team-mine");
}

#[test]
fn each_table_a_local_preset_makes_may_not_be_named_by_another() {
    for (section, table) in [
        (
            "templates",
            "[templates.team-short]\ntitle = \"t\"\nbody = \"b\"\n",
        ),
        ("views", "[views.team-mine]\n"),
        ("reports", "[reports.team-weekly]\n"),
        ("rules", "[rules.team-review]\nstages = [\"2d\"]\n"),
    ] {
        let errors = local(&with(&format!("{ADOPT_FULL}\n{table}")), FULL).unwrap_err();
        let name = table.split(['[', ']']).nth(1).unwrap();
        assert!(
            errors.contains(&format!(
                "presets.team: it makes [{name}], which another table names"
            )),
            "{section}: {errors}"
        );
    }
}

#[test]
fn a_route_id_the_preset_makes_and_another_route_has_is_told_at_the_adopting_table() {
    let text = with(&format!(
        "[[routes]]\nid = \"releases-releases\"\nsources = [\"app\"]\ndestination = \"discord\"\nchannel = \"1566200000000000100\"\ntemplate = \"t\"\n\n[templates.t]\ntitle = \"t\"\nbody = \"b\"\n\n{RELEASES}"
    ));
    let errors = errors(&text);
    let line = line_of(&text, "[presets.releases]");
    assert!(
        errors.contains(&format!(
            "herald.toml:{line}:1 presets.releases.id: another route has the id \"releases-releases\""
        )),
        "{errors}"
    );
}

#[test]
fn a_key_a_preset_with_no_route_neither_takes_nor_writes_is_refused_with_those_it_does() {
    let file = "version = 1\n\n[templates.notes]\ntitle = \"t\"\nbody = \"b\"\n";
    let errors = local(
        &with("[presets.team]\nuse = \"./team.toml\"\ncolour = \"red\"\n"),
        file,
    )
    .unwrap_err();
    assert!(
        errors.contains(
            "presets.team.colour: the preset takes no key \"colour\"; it takes \"body\", \"title\""
        ),
        "{errors}"
    );
}

#[test]
fn each_mistake_of_a_local_preset_is_told_once() {
    for (written, said) in [
        (
            "[destinations.x]\nkind = \"discord\"\n",
            "a local preset writes",
        ),
        (
            "[templates.other]\ntitle = \"t\"\nbody = \"b\"\nfile = \"notes.md\"\n",
            "does not write file",
        ),
        (
            "[[routes]]\nid = \"r\"\nkinds = [\"release\"]\ntemplate = \"notes\"\nthread = { name = { secret = \"X\" } }\n",
            "names no secret",
        ),
        (
            "[[routes]]\nid = \"r\"\nkinds = [\"release\"]\ntemplate = \"notes\"\nsources = [{ secret = \"X\" }]\n",
            "names no secret",
        ),
    ] {
        let errors = local(&with(ADOPT_TEAM), &format!("{TEAM}\n{written}")).unwrap_err();
        assert_eq!(errors.lines().count(), 1, "{errors}");
        assert!(errors.contains(said), "{errors}");
    }
}

#[test]
fn a_section_a_local_preset_writes_in_another_shape_is_told_how_to_write_it() {
    for (written, said) in [
        (
            "[routes]\nid = \"r\"\n",
            "presets.team.routes: ./team.toml:2:1: routes is an array of tables; write [[routes]]",
        ),
        (
            "templates = 1\n",
            "presets.team.templates: ./team.toml:2:13: templates is a table of tables; write [templates.<name>]",
        ),
    ] {
        let errors = local(&with(ADOPT_TEAM), &format!("version = 1\n{written}")).unwrap_err();
        assert!(errors.contains(said), "{errors}");
    }
}

#[test]
fn each_of_two_local_presets_is_told_in_its_own_file() {
    let text = with(&format!(
        "{ADOPT_TEAM}\n[presets.other]\nuse = \"./other.toml\"\ndestination = \"discord\"\n"
    ));
    let errors = load_with(
        "herald.toml",
        &text,
        &Beside(BTreeMap::from([
            ("./team.toml", TEAM.to_owned()),
            (
                "./other.toml",
                "[templates.x]\ntitle = \"t\"\nbody = \"b\"\n".to_owned(),
            ),
        ])),
        &crate::common::kinds(),
    )
    .unwrap_err()
    .to_string();
    let line = line_of(&text, "[presets.other]");
    assert!(
        errors.contains(&format!(
            "herald.toml:{line}:1 presets.other.version: ./other.toml:1:1: give the config format"
        )),
        "{errors}"
    );
    assert!(!errors.contains("./team.toml"), "{errors}");
}

#[test]
fn a_local_preset_sets_no_key_that_decides_where_a_route_posts() {
    let file = TEAM.replace(
        "template = \"notes\"",
        "template = \"notes\"\nchannel = \"1566200000000000999\"",
    );
    let adopting = ADOPT_TEAM.replace("channel = \"1566200000000000100\"\n", "");
    let read = local(&with(&adopting), &file);
    let placed = read
        .as_ref()
        .map(|config| format!("{:?}", config.routes[0].options));
    let errors = read.as_ref().err().cloned().unwrap_or_default();
    assert!(
        errors.contains("./team.toml:7:1: a local preset does not write channel"),
        "the preset placed its route: {placed:?}"
    );
}

#[test]
fn the_expansion_of_a_shipped_preset_is_its_tables_as_given() {
    let tables =
        herald_config::expansion("herald.toml", &with(RELEASES), &nothing, "releases").unwrap();
    let route = &tables["routes"][0];
    assert_eq!(route["id"].as_str(), Some("releases-releases"));
    assert_eq!(route["destination"].as_str(), Some("discord"));
    assert_eq!(route["channel"].as_str(), Some("1566200000000000100"));
    assert_eq!(route["template"].as_str(), Some("releases-release"));
    assert_eq!(
        tables["templates"]["releases-release"]["body"].as_str(),
        Some("{{ event.body }}")
    );
    assert_eq!(tables.len(), 2);
}

#[test]
fn the_expansion_of_a_local_preset_holds_each_of_its_sections_and_no_other_table_s() {
    let text = with(&format!("{ADOPT_FULL}\n{RELEASES}"));
    let files = Beside(BTreeMap::from([("./team.toml", FULL.to_owned())]));
    let tables = herald_config::expansion("herald.toml", &text, &files, "team").unwrap();
    let names = |section: &str| -> Vec<String> {
        tables[section]
            .as_table()
            .unwrap()
            .keys()
            .cloned()
            .collect()
    };
    assert_eq!(names("templates"), ["team-notes", "team-short"]);
    assert_eq!(names("views"), ["team-mine"]);
    assert_eq!(names("reports"), ["team-weekly"]);
    assert_eq!(names("rules"), ["team-review"]);
    assert_eq!(tables["routes"].as_array().unwrap().len(), 3);
}

#[test]
fn the_expansion_of_a_table_the_config_does_not_adopt_is_refused() {
    let errors = herald_config::expansion("herald.toml", &with(RELEASES), &nothing, "other")
        .unwrap_err()
        .to_string();
    assert_eq!(
        errors,
        "herald.toml:1:1 presets.other: herald.toml adopts no preset as [presets.other]"
    );
    let errors = herald_config::expansion("herald.toml", &with("[presets.x]\n"), &nothing, "x")
        .unwrap_err()
        .to_string();
    assert!(errors.contains("give the preset it adopts"), "{errors}");
}

#[test]
fn a_preset_table_and_its_use_are_of_their_shapes() {
    let scalar = errors(&with("[presets]\nreleases = 3\n"));
    assert!(scalar.contains("presets.releases"), "{scalar}");
    let used = errors(&with("[presets.releases]\nuse = 3\n"));
    assert!(used.contains("presets.releases.use"), "{used}");
}

#[test]
fn a_local_preset_s_routes_and_named_tables_are_tables() {
    let route = local(&with(ADOPT_TEAM), "version = 1\nroutes = [1]\n").unwrap_err();
    assert!(
        route.contains("a route is a table; write [[routes]]"),
        "{route}"
    );
    let template = local(&with(ADOPT_TEAM), "version = 1\n[templates]\nnotes = 3\n").unwrap_err();
    assert!(
        template.contains("templates.notes is a table; write [templates.notes]"),
        "{template}"
    );
    let named = local(
        &with(ADOPT_TEAM),
        &TEAM.replace("template = \"notes\"", "template = 3"),
    )
    .unwrap_err();
    assert!(named.contains("presets.team"), "{named}");
}

#[test]
fn a_syntax_mistake_is_told_in_the_file_that_makes_it() {
    let preset = local(
        &with(ADOPT_TEAM),
        &TEAM.replace("kinds = [\"release\"]", "kinds = [\"release\""),
    )
    .unwrap_err();
    assert!(
        preset.contains("presets.team: ./team.toml:6:1: missing comma between array elements"),
        "{preset}"
    );
    let text = format!("{}broken =", with(ADOPT_TEAM));
    let config = local(&text, TEAM).unwrap_err();
    let told = format!(
        "herald.toml:{}:9 string values must be quoted",
        text.lines().count()
    );
    assert!(config.contains(&told), "{told}: {config}");
    assert!(!config.contains("./team.toml:"), "{config}");
}

#[test]
fn a_parameter_lands_in_the_tables_its_preset_names() {
    let config = loaded(&with(&format!(
        "{RELEASES}title = \"{{{{ event.project }}}} is out\"\n"
    )));
    assert_eq!(config.routes.len(), 1);
}

#[test]
fn a_mistake_in_a_second_local_preset_is_told_in_its_own_file() {
    let text = with(&format!(
        "{ADOPT_TEAM}\n[presets.more]\nuse = \"./more.toml\"\ndestination = \"discord\"\nsources = [\"app\"]\nchannel = \"1566200000000000101\"\n"
    ));
    let files = Beside(BTreeMap::from([
        ("./more.toml", TEAM.to_owned()),
        (
            "./team.toml",
            TEAM.replace("kinds = [\"release\"]", "kinds = [\"release\""),
        ),
    ]));
    let error = load_with("herald.toml", &text, &files, &crate::common::kinds())
        .unwrap_err()
        .to_string();
    assert!(error.contains("presets.team: ./team.toml:6:1:"), "{error}");
    assert!(!error.contains("./more.toml:"), "{error}");
}

#[test]
fn a_mistake_in_a_local_preset_s_value_names_its_key() {
    let error = local(
        &with(ADOPT_TEAM),
        &TEAM.replace("title = \"{{ event.project }}\"", "title = 3"),
    )
    .unwrap_err();
    assert!(
        error.contains("presets.team.title: ./team.toml:9:9: invalid type"),
        "{error}"
    );
}

const REMINDERS: &str = r#"[presets.reviews]
use = "herald:review-reminders"
destination = "discord"
channel = "1566200000000000200"
"#;

#[test]
fn review_reminders_are_a_rule_over_a_view_of_pulls_waiting_on_review_and_a_route_of_its_nudges() {
    let config = loaded(&with(&format!("{REMINDERS}sources = [\"app\"]\n")));
    let [route] = config.routes.as_slice() else {
        panic!("one route: {:?}", config.routes.len());
    };
    assert_eq!(route.route.id.as_str(), "reviews-reminders");
    assert_eq!(route.route.destination.as_str(), "discord");
    assert_eq!(
        route.route.filter.sources,
        [SourceName::new("reviews-reviews")]
    );
    assert_eq!(
        route
            .route
            .template
            .as_ref()
            .map(herald_core::TemplateName::as_str),
        Some("herald:nudge")
    );
    let view = &config.views["reviews-reviews"];
    assert_eq!(view.sources, [SourceName::new("app")]);
    assert_eq!(view.view.states, [herald_core::Turn::WaitingOnReview]);
    let rule = &config.rules["reviews-reviews"];
    assert_eq!(rule.watch.view, "reviews-reviews");
    assert_eq!(rule.watch.schedule.at, jiff::civil::time(9, 0, 0, 0));
    assert_eq!(rule.watch.schedule.days.len(), 5);
    assert!(
        !rule
            .watch
            .schedule
            .days
            .contains(&jiff::civil::Weekday::Saturday)
    );
    assert_eq!(rule.watch.schedule.zone, jiff::tz::TimeZone::UTC);
    let stages: Vec<i64> = rule
        .stages
        .as_ref()
        .unwrap()
        .as_slice()
        .iter()
        .map(jiff::SignedDuration::as_hours)
        .collect();
    assert_eq!(stages, [48, 168]);
}

#[test]
fn review_reminders_given_no_sources_watch_every_forge_source_and_take_their_window_from_the_table()
{
    let config = loaded(&with(&format!(
        "{REMINDERS}at = \"08:30\"\ndays = [\"sat\"]\nzone = \"Europe/Amsterdam\"\nstages = [\"1d\"]\n"
    )));
    assert_eq!(
        config.views["reviews-reviews"].sources,
        [SourceName::new("app")]
    );
    let schedule = &config.rules["reviews-reviews"].watch.schedule;
    assert_eq!(schedule.at, jiff::civil::time(8, 30, 0, 0));
    assert_eq!(schedule.days, [jiff::civil::Weekday::Saturday]);
    assert_eq!(
        schedule.zone,
        jiff::tz::TimeZone::get("Europe/Amsterdam").unwrap()
    );
    assert_eq!(
        config.rules["reviews-reviews"]
            .stages
            .as_ref()
            .unwrap()
            .as_slice()
            .len(),
        1
    );
}

const QUEUE: &str = r#"[presets.queue]
use = "herald:merge-queue-small-team"
source = "app"
branch = "main"
method = "squash"
write = { token = { secret = "HERALD_MERGE_TOKEN" } }
"#;

#[test]
fn a_small_team_s_merge_queue_is_one_queue_named_for_the_adopting_table_entered_by_herald_s_label()
{
    let config = loaded(&with(QUEUE));
    let [(name, queue)] = config.merge_queues.iter().collect::<Vec<_>>()[..] else {
        panic!("one queue: {:?}", config.merge_queues.keys());
    };
    assert_eq!(name, "queue-queue");
    assert_eq!(queue.source, SourceName::new("app"));
    assert_eq!(queue.branch, "main");
    assert_eq!(queue.method, Some(herald_core::merging::Method::Squash));
    assert_eq!(queue.runs, herald_core::merging::Runs::Forge);
    assert_eq!(
        queue.enter,
        herald_core::merging::Enter::Label(herald_core::merging::ENTER_LABEL.to_owned())
    );
    assert_eq!(
        queue
            .write
            .as_ref()
            .and_then(|write| write.token.as_ref())
            .map(|token| token.secret.as_str()),
        Some("HERALD_MERGE_TOKEN")
    );
    assert_eq!(config.routes.len(), 0);
}

#[test]
fn a_small_team_s_merge_queue_takes_who_runs_it_and_its_gesture_from_the_table() {
    let config = loaded(&with(&format!(
        "{QUEUE}runs = \"herald\"\nenter = {{ comment = \"/merge\" }}\n"
    )));
    let queue = &config.merge_queues["queue-queue"];
    assert_eq!(queue.runs, herald_core::merging::Runs::Herald);
    assert_eq!(
        queue.enter,
        herald_core::merging::Enter::Comment("/merge".to_owned())
    );
}

#[test]
fn a_merge_queue_a_preset_makes_is_held_to_every_rule_a_written_one_is() {
    let text = with(&format!(
        "{QUEUE}\n[merge_queues.mine]\nsource = \"app\"\nbranch = \"main\"\n"
    ));
    let twice = errors(&text);
    assert!(
        twice.contains("merges into main of nightworksio/app already; give one table per branch"),
        "{twice}"
    );
    let unknown = errors(&with(
        &QUEUE.replace("source = \"app\"", "source = \"nowhere\""),
    ));
    let line = line_of(&with(QUEUE), "[presets.queue]");
    assert!(
        unknown.contains("presets.queue.source: \"nowhere\" is no source")
            && unknown.contains(&format!("herald.toml:{}:", line + 2)),
        "{unknown}"
    );
    let missing = errors(&with(&QUEUE.replace("method = \"squash\"\n", "")));
    assert!(
        missing.contains("presets.queue: give method: How it merges"),
        "{missing}"
    );
}

#[test]
fn a_merge_queue_a_preset_makes_may_not_be_named_by_another() {
    let errors = errors(&with(&format!(
        "{QUEUE}\n[merge_queues.queue-queue]\nsource = \"app\"\nbranch = \"next\"\n"
    )));
    assert!(
        errors.contains(
            "presets.queue: it makes [merge_queues.queue-queue], which another table names"
        ),
        "{errors}"
    );
}

#[test]
fn a_local_preset_writes_no_merge_queue() {
    let file = "version = 1\n\n[merge_queues.q]\nsource = \"app\"\nbranch = \"main\"\n";
    let errors = local(&with("[presets.team]\nuse = \"./team.toml\"\n"), file).unwrap_err();
    assert!(
        errors.contains("presets.team.merge_queues: ./team.toml:3:2: a local preset writes"),
        "{errors}"
    );
}

#[test]
fn the_expansion_of_a_merge_queue_preset_is_its_queue_as_given() {
    let tables = herald_config::expansion("herald.toml", &with(QUEUE), &nothing, "queue").unwrap();
    let queue = &tables["merge_queues"]["queue-queue"];
    assert_eq!(queue["source"].as_str(), Some("app"));
    assert_eq!(queue["method"].as_str(), Some("squash"));
    assert_eq!(
        queue["write"]["token"]["secret"].as_str(),
        Some("HERALD_MERGE_TOKEN")
    );
    assert_eq!(tables.len(), 1);
}

const ALERTS: &str = r#"[presets.security]
use = "herald:security-alerts-private"
destination = "discord"
sources = ["alerting"]
channel = "1566200000000000500"
"#;

const ALERTING: &str = r#"[sources.alerting]
kind = "github"
repository = "nightworksio/app"
signals = ["release", "alert"]
"#;

#[test]
fn security_alerts_go_to_one_route_the_preset_states_is_private() {
    let config = loaded(&with(&format!("{ALERTING}\n{ALERTS}")));
    let [route] = config.routes.as_slice() else {
        panic!("one route: {:?}", config.routes.len());
    };
    assert_eq!(route.route.id.as_str(), "security-alerts");
    assert_eq!(route.route.filter.kinds, [Kind::ALERT]);
    assert_eq!(route.route.filter.sources, [SourceName::new("alerting")]);
    assert!(route.private);
    assert_eq!(
        route
            .route
            .template
            .as_ref()
            .map(herald_core::TemplateName::as_str),
        Some("security-alert")
    );
}
