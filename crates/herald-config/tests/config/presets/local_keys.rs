//! The route keys a local preset writes, and those it does not (ADR-0092
//! decision 4).

use super::{ADOPT_TEAM, TEAM, local, with};

#[test]
fn a_local_preset_writes_how_a_route_treats_its_posts_and_its_destination_kind_s_options() {
    let file = TEAM.replace(
        "template = \"notes\"",
        "template = \"notes\"\nchannel = \"1566200000000000999\"\npin = false\nedits = \"release\"\nwithdraw = \"delete\"\nlate_release = \"post\"\nthread = { name = \"{{ event.project }}\" }",
    );
    let adopting = ADOPT_TEAM.replace("channel = \"1566200000000000100\"\n", "");
    let config = local(&with(&adopting), &file).unwrap();
    let route = &config.routes[0];
    assert!(
        format!("{:?}", route.options).contains("1566200000000000999"),
        "{:?}",
        route.options
    );
    let mentioning = TEAM.replace(
        "template = \"notes\"",
        "template = \"notes\"\nmentions = { nobody = true }",
    );
    let refused = local(&with(ADOPT_TEAM), &mentioning).unwrap_err();
    assert!(
        !refused.contains("a local preset does not write"),
        "{refused}"
    );
    assert!(refused.contains("mentions"), "{refused}");
}

#[test]
fn a_local_preset_does_not_say_where_a_route_its_adopting_table_trusts_posts() {
    for trusting in [
        "private = true",
        "vouch_unchecked = true",
        "viewers = [\"1566200000000000300\"]",
    ] {
        let file = TEAM.replace(
            "template = \"notes\"",
            "template = \"notes\"\nchannel = \"1566200000000000999\"",
        );
        let adopting = ADOPT_TEAM.replace(
            "channel = \"1566200000000000100\"\n",
            &format!("{trusting}\n"),
        );
        let errors = local(&with(&adopting), &file).unwrap_err();
        assert!(
            errors.contains("presets.team.channel: ./team.toml:7:1: a local preset does not write channel of a route its adopting table states what herald trusts of: it decides where the route's posts go"),
            "{trusting}: {errors}"
        );
        assert!(!errors.contains("1566200000000000999"), "{errors}");
    }
}

#[test]
fn a_local_preset_still_writes_no_destination_trust_act_or_section_beyond_its_own() {
    for (written, key, why) in [
        (
            "destination = \"discord\"",
            "destination",
            "it decides which destination the route posts to",
        ),
        (
            "crosspost = [\"discord\"]",
            "crosspost",
            "it decides which destination the route posts to",
        ),
        (
            "living = true",
            "living",
            "it decides which destination the route posts to",
        ),
        (
            "private = true",
            "private",
            "it states what herald trusts of the route's place",
        ),
        (
            "vouch_unchecked = true",
            "vouch_unchecked",
            "it states what herald trusts of the route's place",
        ),
        (
            "viewers = [\"1566200000000000300\"]",
            "viewers",
            "it states what herald trusts of the route's place",
        ),
        ("act = true", "act", "it lets the route act on the forge"),
        (
            "unheard = 1",
            "unheard",
            "and each destination kind's own route options of a routes table",
        ),
    ] {
        let file = TEAM.replace(
            "template = \"notes\"",
            &format!("template = \"notes\"\n{written}"),
        );
        let errors = local(&with(ADOPT_TEAM), &file).unwrap_err();
        assert!(
            errors.contains(&format!(
                "./team.toml:7:1: a local preset does not write {key}: "
            )) && errors.contains(why),
            "{key}: {errors}"
        );
    }
    for section in [
        "[files]\nallowed = [\"/etc\"]\n",
        "[destinations.other]\nkind = \"discord\"\ntoken = { secret = \"X\" }\n",
        "[sources.other]\nkind = \"github\"\nrepository = \"a/b\"\n",
    ] {
        let errors = local(&with(ADOPT_TEAM), &format!("{TEAM}\n{section}")).unwrap_err();
        assert!(
            errors.contains("a local preset writes \"routes\", \"templates\", \"views\", \"reports\", \"rules\" only; give"),
            "{section}: {errors}"
        );
    }
}

#[test]
fn a_local_preset_s_table_beside_its_routes_takes_its_own_keys_and_no_destination_kind_s() {
    let file = TEAM.replace(
        "body = \"{{ event.body }}\"",
        "body = \"{{ event.body }}\"\nunheard = 1",
    );
    let errors = local(&with(ADOPT_TEAM), &file).unwrap_err();
    assert!(
        errors.contains("a local preset does not write unheard: ")
            && errors.contains(" of a templates table"),
        "{errors}"
    );
    assert!(
        !errors.contains("each destination kind's own route options"),
        "{errors}"
    );
}
