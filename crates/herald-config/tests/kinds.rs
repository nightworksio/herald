//! `[kinds]`: the kinds of event the config declares, read with their
//! attributes and links, checked against each route's templates, and every
//! mistake in them named by key.

mod common;

#[cfg(test)]
mod kinds {
    use herald_config::load;
    use herald_core::{
        AttributeSpec, AttributeType, Kind, LinkSpec, PRERELEASE, Phrase, VERSION, versioned,
    };

    const EXAMPLE: &str = r#"version = 1

[kinds.deploy]
description = "A deploy of a site."
links = ["site"]
attributes = { region = { type = "text", required = true }, live = { type = "flag" } }

[kinds.build]
versioned = true

[store]
kind = "git-branch"
key = { secret = "HERALD_STORE_KEY" }

[destinations.discord]
kind = "discord"
token = { secret = "HERALD_DISCORD_TOKEN" }

[apps.ci]
projects = ["herald"]

[[routes]]
id = "deploys"
sources = ["ci"]
kinds = ["deploy"]
destination = "discord"
channel = "1566200000000000100"
template = "deploy"

[[routes]]
id = "builds"
sources = ["ci"]
kinds = ["build"]
versions = "1.*"
destination = "discord"
channel = "1566200000000000200"
template = "build"

[templates.deploy]
title = "{{ event.project }} in {{ event.attributes.region }}{% if event.attributes.live %}, live{% endif %}"
body = "{{ event.links.site }}"

[templates.build]
title = "{{ event.project }} {{ event.attributes.version }}"
body = "{{ event.body }}"
"#;

    fn nothing(_: &str) -> Result<String, String> {
        Err("no file".to_owned())
    }

    fn errors(text: &str) -> String {
        load("herald.toml", text, &nothing, &crate::common::kinds())
            .unwrap_err()
            .to_string()
    }

    #[test]
    fn a_declared_kind_is_known_beside_the_kinds_herald_ships() {
        let config = load("herald.toml", EXAMPLE, &nothing, &crate::common::kinds()).unwrap();
        let deploy = config.kinds.get(&Kind::new("deploy").unwrap()).unwrap();
        assert_eq!(deploy.description, "A deploy of a site.");
        assert!(!deploy.versioned);
        assert_eq!(deploy.links, [LinkSpec::named("site")]);
        assert_eq!(
            deploy.attributes["region"],
            AttributeSpec {
                ty: AttributeType::Text,
                required: true
            }
        );
        assert_eq!(
            deploy.attributes["live"],
            AttributeSpec {
                ty: AttributeType::Flag,
                required: false
            }
        );
        assert_eq!(deploy.attributes.len(), 2);
        let build = config.kinds.get(&Kind::new("build").unwrap()).unwrap();
        assert!(build.versioned);
        assert!(build.check(&versioned("1.0.0", true)).is_ok());
        let known: Vec<&str> = config.kinds.iter().map(|spec| spec.kind.as_str()).collect();
        assert_eq!(
            known,
            [
                "release",
                "package-version",
                "feed-entry",
                "message",
                "build",
                "deploy"
            ]
        );
    }

    #[test]
    fn a_template_printing_an_attribute_a_kind_lacks_is_named_with_its_path() {
        let release = EXAMPLE.replace("kinds = [\"deploy\"]", "kinds = [\"deploy\", \"release\"]");
        let error = errors(&release);
        assert!(
            error.contains("templates.deploy.title: it does not render a sample release event: it prints event.attributes.region, which a release event does not carry"),
            "{error}"
        );
        let every = EXAMPLE.replace("kinds = [\"deploy\"]\n", "");
        let error = errors(&every);
        assert!(
            error.contains("it prints event.attributes.region, which a build event does not carry"),
            "{error}"
        );
    }

    #[test]
    fn a_kind_with_a_name_that_is_not_one_or_that_herald_ships_is_refused() {
        let error = errors(&EXAMPLE.replace("[kinds.build]", "[kinds.Build]"));
        assert!(
            error.contains("kinds.Build: \"Build\" is not a kind; name it in lowercase letters and digits, words joined by '-'"),
            "{error}"
        );
        let error = errors(&EXAMPLE.replace("[kinds.build]", "[kinds.release]"));
        assert!(
            error.contains(
                "kinds.release: herald ships the kind \"release\"; give yours another name"
            ),
            "{error}"
        );
        let error = errors(&EXAMPLE.replace("versioned = true", "versioned = true\nbuttons = 1"));
        assert!(
            error.contains("kinds.build.buttons: unknown field `buttons`"),
            "{error}"
        );
        assert!(
            error.contains("routes[1].kinds[0]: there is no kind \"build\""),
            "{error}"
        );
    }

    #[test]
    fn a_link_is_labelled_by_its_name_or_by_the_label_it_gives() {
        let text = EXAMPLE.replace(
            "links = [\"site\"]",
            "links = [\"site\", { name = \"status\", label = \"Status page\" }]",
        );
        let config = load("herald.toml", &text, &nothing, &crate::common::kinds()).unwrap();
        let deploy = config.kinds.get(&Kind::new("deploy").unwrap()).unwrap();
        assert_eq!(
            deploy.links,
            [
                LinkSpec::named("site"),
                LinkSpec {
                    name: "status".to_owned(),
                    label: Some("Status page".to_owned()),
                },
            ]
        );
        let too_long = "x".repeat(Phrase::LABEL + 1);
        let padded = format!(" {}", "x".repeat(Phrase::LABEL));
        for label in ["", "  ", too_long.as_str(), padded.as_str()] {
            let error = errors(&EXAMPLE.replace(
                "links = [\"site\"]",
                &format!("links = [{{ name = \"site\", label = \"{label}\" }}]"),
            ));
            assert!(
                error.contains("kinds.deploy.links[0].label: a button's label is 1 to 75 characters; give one that fits"),
                "{error}"
            );
        }
        let fits = "x".repeat(Phrase::LABEL);
        assert!(
            load(
                "herald.toml",
                &EXAMPLE.replace(
                    "links = [\"site\"]",
                    &format!("links = [{{ name = \"site\", label = \"{fits}\" }}]"),
                ),
                &nothing,
                &crate::common::kinds()
            )
            .is_ok()
        );
        let stray = errors(&EXAMPLE.replace(
            "links = [\"site\"]",
            "links = [{ name = \"site\", label = \"Site\", colour = \"red\" }]",
        ));
        assert!(stray.contains("kinds.deploy"), "{stray}");
    }

    #[test]
    fn a_kind_names_no_more_links_than_a_post_holds_buttons_each_fitting_a_label() {
        let named = |count: usize| {
            let links: Vec<String> = std::iter::once("\"site\"".to_owned())
                .chain((1..count).map(|index| format!("\"l{index}\"")))
                .collect();
            EXAMPLE.replace(
                "links = [\"site\"]",
                &format!("links = [{}]", links.join(", ")),
            )
        };
        assert!(
            load(
                "herald.toml",
                &named(LinkSpec::MOST),
                &nothing,
                &crate::common::kinds()
            )
            .is_ok()
        );
        let crowded = errors(&named(LinkSpec::MOST + 1));
        assert!(
            crowded.contains("kinds.deploy.links: a kind names at most 24 links, the buttons a Discord post holds beside its subscribe button; keep 24 of its 25"),
            "{crowded}"
        );
        let longest = "x".repeat(Phrase::LABEL);
        let longer = "x".repeat(Phrase::LABEL + 1);
        let unlabelled = |name: &str| {
            EXAMPLE.replace(
                "links = [\"site\"]",
                &format!("links = [\"site\", \"{name}\"]"),
            )
        };
        assert!(
            load(
                "herald.toml",
                &unlabelled(&longest),
                &nothing,
                &crate::common::kinds()
            )
            .is_ok()
        );
        let long = errors(&unlabelled(&longer));
        assert!(
            long.contains(&format!("kinds.deploy.links[1]: \"{longer}\" is longer than a button's label of at most 75 characters; give it a label, {{ name = \"{longer}\", label = \"…\" }}")),
            "{long}"
        );
        let labelled = EXAMPLE.replace(
            "links = [\"site\"]",
            &format!("links = [\"site\", {{ name = \"{longer}\", label = \"Long\" }}]"),
        );
        assert!(load("herald.toml", &labelled, &nothing, &crate::common::kinds()).is_ok());
    }

    #[test]
    fn a_link_or_attribute_a_template_cannot_read_is_refused() {
        let error = errors(&EXAMPLE.replace("links = [\"site\"]", "links = [\"site\", \"Site\"]"));
        assert!(
            error.contains("kinds.deploy.links[1]: \"Site\" cannot name a link; name it in lowercase letters, digits and '_', starting with a letter"),
            "{error}"
        );
        assert!(!error.contains("\"site\" cannot"), "{error}");
        let error = errors(&EXAMPLE.replace("region = {", "Region = {"));
        assert!(
            error.contains("kinds.deploy.attributes.Region: an attribute is named in lowercase letters, digits and '_', starting with a letter"),
            "{error}"
        );
        assert!(!error.contains("attributes.live:"), "{error}");
        for attribute in [VERSION, PRERELEASE] {
            let error = errors(&EXAMPLE.replace("live = {", &format!("{attribute} = {{")));
            assert!(
                error.contains(&format!("kinds.deploy.attributes.{attribute}: {attribute} is the attribute versioned = true gives a kind; remove it, and write versioned = true")),
                "{error}"
            );
        }
    }

    #[test]
    fn a_route_with_versions_takes_a_declared_kind_only_where_it_is_versioned() {
        let error = errors(&EXAMPLE.replace("versioned = true", "versioned = false"));
        assert!(
            error.contains("routes[1].versions: \"build\" events carry no version"),
            "{error}"
        );
    }
}
