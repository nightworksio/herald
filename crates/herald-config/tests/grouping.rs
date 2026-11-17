//! A route's `group`, read with its defaults, and every mistake in it named
//! by key.

mod common;

#[cfg(test)]
mod grouping {
    use herald_config::load;
    use herald_core::{GroupLate, GroupMode, GroupUntil, Grouping, RouteId};
    use jiff::SignedDuration;

    const HEAD: &str = r#"version = 1

[store]
kind = "sqlite"

[destinations.discord]
kind = "discord"
token = { secret = "HERALD_DISCORD_TOKEN" }

[destinations.team]
kind = "slack"
webhook = { secret = "HERALD_SLACK_WEBHOOK" }

[destinations.fedi]
kind = "activitypub"
domain = "herald.example"
key = { secret = "HERALD_ACTIVITYPUB_KEY" }

[sources.org]
kind = "github"
owner = "lemonfiber"

[templates.t]
title = "{{ event.title }}"
body = "{% for member in events %}{{ member.project }}{% endfor %}"

[templates.train]
title = "{% if group %}{{ group.key }}{% else %}{{ event.title }}{% endif %} with {{ events | length }}"
body = "{{ event.body }}"

[templates.bare]
title = "{{ group.key }}"
body = "{{ event.body }}"
"#;

    fn nothing(_: &str) -> Result<String, String> {
        Err("no file".to_owned())
    }

    fn loaded(routes: &str) -> Result<herald_config::Config, String> {
        load(
            "herald.toml",
            &format!("{HEAD}\n{routes}"),
            &nothing,
            &crate::common::kinds(),
        )
        .map_err(|error| error.to_string())
    }

    fn discord(template: &str, group: &str) -> String {
        format!(
            "[[routes]]\nid = \"train\"\nsources = [\"org\"]\ndestination = \"discord\"\nchannel = \"1566200000000000100\"\ntemplate = \"{template}\"\n{group}\n"
        )
    }

    fn grouping(config: &herald_config::Config) -> Option<&Grouping> {
        config
            .routes
            .iter()
            .find(|route| route.route.id == RouteId::new("train"))
            .and_then(|route| route.route.delivery.group.as_ref())
    }

    #[test]
    fn a_group_reads_its_key_and_takes_its_defaults() {
        let config = loaded(&discord(
            "train",
            "group = { key = \"{{ event.attributes.version }}\" }",
        ))
        .unwrap();
        let group = grouping(&config).unwrap();
        assert_eq!(group.key, "{{ event.attributes.version }}");
        assert_eq!(group.window, SignedDuration::from_mins(30));
        assert_eq!(group.lead, Vec::<herald_core::Glob>::new());
        assert_eq!(group.mode, GroupMode::Hold);
        assert_eq!(group.until, GroupUntil::Window);
        assert_eq!(group.late, GroupLate::Post);
        assert_eq!(group.most, 25);
    }

    #[test]
    fn a_group_reads_every_key_it_gives() {
        let config = loaded(&discord(
            "train",
            "group = { key = \"{{ event.attributes.version }}\", window = \"7d\", lead = [\"lemonfiber\", \"core-*\"], mode = \"edit\", until = \"lead\", late = \"edit\", most = 100 }",
        ))
        .unwrap();
        let group = grouping(&config).unwrap();
        assert_eq!(group.window, SignedDuration::from_hours(168));
        assert_eq!(
            group
                .lead
                .iter()
                .map(herald_core::Glob::as_str)
                .collect::<Vec<_>>(),
            ["lemonfiber", "core-*"]
        );
        assert_eq!(group.mode, GroupMode::Edit);
        assert_eq!(group.until, GroupUntil::Lead);
        assert_eq!(group.late, GroupLate::Edit);
        assert_eq!(group.most, 100);
        let skipping = loaded(&discord(
            "train",
            "group = { key = \"{{ event.project }}\", late = \"skip\", most = 1 }",
        ))
        .unwrap();
        assert_eq!(grouping(&skipping).unwrap().late, GroupLate::Skip);
        assert_eq!(grouping(&skipping).unwrap().most, 1);
    }

    #[test]
    fn a_route_without_a_group_groups_nothing_and_its_template_reads_the_event_as_its_events() {
        let config = loaded(&discord("t", "")).unwrap();
        assert_eq!(grouping(&config), None);
    }

    #[test]
    fn a_group_s_window_most_and_lead_are_bounded() {
        for (group, said) in [
            (
                "window = \"8d\"",
                "routes[0].group.window: a group gathers its members for 7d at most",
            ),
            (
                "most = 101",
                "routes[0].group.most: a group takes from 1 to 100 members",
            ),
            (
                "most = 0",
                "routes[0].group.most: a group takes from 1 to 100 members",
            ),
            (
                "lead = [\"core\", \"[\"]",
                "routes[0].group.lead[1]: \"[\" opens a [",
            ),
        ] {
            let error = loaded(&discord(
                "train",
                &format!("group = {{ key = \"{{{{ event.project }}}}\", {group} }}"),
            ))
            .unwrap_err();
            assert!(error.contains(said), "{error}");
        }
    }

    #[test]
    fn a_group_that_edits_does_not_load_where_the_destination_edits_no_post() {
        let route = "[[routes]]\nid = \"train\"\nsources = [\"org\"]\ndestination = \"team\"\ntemplate = \"train\"\ngroup = { key = \"{{ event.project }}\", mode = \"edit\" }\n";
        let error = loaded(route).unwrap_err();
        assert!(
            error.contains("routes[0].group.mode: the destination \"team\" cannot do this"),
            "{error}"
        );
        assert!(loaded(&route.replace(", mode = \"edit\"", "")).is_ok());
    }

    #[test]
    fn a_route_announcing_each_project_as_an_actor_groups_nothing() {
        let route = "[[routes]]\nid = \"train\"\nsources = [\"org\"]\ndestination = \"fedi\"\ntemplate = \"train\"\nactor = \"{{ event.project }}\"\ngroup = { key = \"{{ event.project }}\" }\n";
        let error = loaded(route).unwrap_err();
        assert!(
            error.contains(
                "routes[0].group: the route announces each project as an actor of its own"
            ),
            "{error}"
        );
        assert!(
            error.contains("give the group to a second route"),
            "{error}"
        );
        assert!(
            loaded(&route.replace("{{ event.project }}\"\ngroup", "lemonfiber\"\ngroup")).is_ok()
        );
    }

    #[test]
    fn a_group_s_key_and_templates_render_a_sample_group_or_do_not_load() {
        let unrendered = loaded(&discord(
            "train",
            "group = { key = \"{{ event.nothing }}\" }",
        ))
        .unwrap_err();
        assert!(
            unrendered.contains("routes[0].group.key: it does not render a sample"),
            "{unrendered}"
        );
        let uncompiled = loaded(&discord("train", "group = { key = \"{{ \" }")).unwrap_err();
        assert!(
            uncompiled.contains("routes[0].group.key: the template does not compile"),
            "{uncompiled}"
        );
        let alone = loaded(&discord(
            "bare",
            "group = { key = \"{{ event.project }}\" }",
        ))
        .unwrap_err();
        assert!(
            alone.contains("templates.bare.title: it does not render a sample release event"),
            "{alone}"
        );
        let wrong_member = "[templates.members]\ntitle = \"{{ event.title }}\"\nbody = \"{% if group %}{{ events[1].nothing }}{% endif %}\"\n";
        let error = loaded(&format!(
            "{wrong_member}{}",
            discord("members", "group = { key = \"{{ event.project }}\" }")
        ))
        .unwrap_err();
        assert!(
            error.contains("templates.members.body: it does not render a group of sample"),
            "{error}"
        );
    }

    #[test]
    fn a_route_to_a_herald_feed_groups_nothing() {
        let text = "version = 1\n[store]\nkind = \"sqlite\"\n[federation]\ndomain = \"herald.example\"\nkeys = [{ secret = \"HERALD_KEY\" }]\n[destinations.releases]\nkind = \"herald\"\n[sources.gate]\nkind = \"github\"\nrepository = \"nightworksio/php-mutation-gate\"\n[[routes]]\nid = \"fed\"\nsources = [\"gate\"]\ndestination = \"releases\"\n";
        let feeding = |extra: &str| {
            load(
                "herald.toml",
                &format!("{text}{extra}"),
                &nothing,
                &crate::common::kinds(),
            )
            .map_err(|error| error.to_string())
        };
        assert!(feeding("").is_ok());
        let error = feeding("group = { key = \"{{ event.project }}\" }\n").unwrap_err();
        assert!(
            error.contains("routes[0].group: a herald feed publishes each event as an envelope of its own, so it gathers no group"),
            "{error}"
        );
    }
}
