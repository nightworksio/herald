//! The GitHub Apps sources read as, each read with its defaults, and every
//! source naming one checked against it.

mod common;

#[cfg(test)]
mod github_apps {
    use herald_config::load;
    use herald_core::SecretRef;
    use herald_kinds::{Installation, Installed};

    const HEAD: &str = r#"version = 1

[store]
kind = "sqlite"

[destinations.discord]
kind = "discord"
token = { secret = "HERALD_DISCORD_TOKEN" }

[templates.t]
title = "{{ event.title }}"
body = "{{ event.body }}"

[[routes]]
id = "releases"
sources = ["org"]
destination = "discord"
channel = "1566200000000000900"
template = "t"
"#;

    fn nothing(_: &str) -> Result<String, String> {
        Err("no file".to_owned())
    }

    fn loaded(rest: &str) -> Result<herald_config::Config, String> {
        load(
            "herald.toml",
            &format!("{HEAD}\n{rest}"),
            &nothing,
            &crate::common::kinds(),
        )
        .map_err(|error| error.to_string())
    }

    fn refused(rest: &str, expected: &str) {
        let error = loaded(rest).unwrap_err();
        assert!(error.contains(expected), "{error}");
    }

    const ORG: &str =
        "[sources.org]\nkind = \"github\"\nowner = \"lemonfiber\"\napp = \"herald\"\n";
    const APP: &str = "[github_apps.herald]\nclient_id = \"Iv23liABCDEF\"\nprivate_key = { secret = \"HERALD_APP_KEY\" }\n";

    #[test]
    fn an_app_is_read_with_github_com_s_api_and_its_sources_name_their_installation() {
        let config = loaded(&format!(
            "{ORG}{APP}webhook = {{ secret = \"HERALD_APP_HOOK\" }}\n[sources.one]\nkind = \"github\"\nrepository = \"nightworksio/herald\"\napp = \"herald\"\n[sources.fixed]\nkind = \"github\"\nowner = \"nightworksio\"\napp = \"herald\"\ninstallation = 42\n"
        ))
        .unwrap();
        let app = &config.github_apps["herald"];
        assert_eq!(app.issuer, "Iv23liABCDEF");
        assert_eq!(app.api, "https://api.github.com");
        assert_eq!(
            app.private_key,
            SecretRef {
                secret: "HERALD_APP_KEY".to_owned()
            }
        );
        assert_eq!(
            app.webhook,
            Some(SecretRef {
                secret: "HERALD_APP_HOOK".to_owned()
            })
        );
        let installed = |name: &str| {
            config.sources[&herald_core::SourceName::new(name)]
                .installed()
                .map(|installed| installed.installation)
        };
        assert_eq!(installed("org"), Some(Installation::Owner("lemonfiber")));
        assert_eq!(
            installed("one"),
            Some(Installation::Repository("nightworksio/herald"))
        );
        assert_eq!(installed("fixed"), Some(Installation::Fixed(42)));
        assert_eq!(
            config.sources[&herald_core::SourceName::new("org")].installed(),
            Some(Installed {
                app: "herald",
                api: "https://api.github.com".to_owned(),
                installation: Installation::Owner("lemonfiber"),
            })
        );
        let enterprise = loaded(&format!(
            "{ORG}api = \"https://ghe.example/api/v3\"\n[github_apps.herald]\napp_id = 7\nprivate_key = {{ secret = \"K\" }}\napi = \"https://ghe.example/api/v3\"\n"
        ))
        .unwrap();
        assert_eq!(enterprise.github_apps["herald"].issuer, "7");
        assert_eq!(enterprise.github_apps["herald"].webhook, None);
    }

    #[test]
    fn an_app_names_one_issuer_a_web_api_and_a_name_its_webhook_s_path_takes() {
        let key = "private_key = { secret = \"K\" }\n";
        refused(
            &format!("{ORG}[github_apps.herald]\n{key}"),
            "give client_id or app_id, one of them",
        );
        refused(
            &format!("{ORG}[github_apps.herald]\napp_id = 7\nclient_id = \"Iv1\"\n{key}"),
            "give client_id or app_id, one of them",
        );
        refused(
            &format!("{ORG}[github_apps.herald]\nclient_id = \"Iv 1\"\n{key}"),
            "\"Iv 1\" is not a client id",
        );
        refused(
            &format!("{ORG}[github_apps.herald]\nclient_id = \"\"\n{key}"),
            "\"\" is not a client id",
        );
        refused(
            &format!("{ORG}[github_apps.herald]\napp_id = 7\napi = \"ghe\"\n{key}"),
            "api \"ghe\" is not a web address",
        );
        refused(
            &format!("{ORG}[github_apps.Herald]\napp_id = 7\n{key}"),
            "\"Herald\" names its webhook's path",
        );
        refused(
            &format!("{ORG}[github_apps.herald]\napp_id = 7\n"),
            "private_key",
        );
    }

    #[test]
    fn an_app_keeps_its_tokens_only_in_a_store_that_keeps_records() {
        let kept = loaded(&format!("{ORG}{APP}store_token = true\n")).unwrap();
        assert!(kept.github_apps["herald"].store_token);
        assert!(!loaded(&format!("{ORG}{APP}")).unwrap().github_apps["herald"].store_token);
        let markers = load(
            "herald.toml",
            &format!(
                "{}\n{ORG}{APP}store_token = true\n",
                HEAD.replace("kind = \"sqlite\"", "kind = \"markers\"")
            ),
            &nothing,
            &crate::common::kinds(),
        )
        .map_err(|error| error.to_string())
        .unwrap_err();
        assert!(
            markers.contains("github_apps.herald.store_token: the markers store keeps no records, so it keeps no token"),
            "{markers}"
        );
    }

    #[test]
    fn a_source_names_an_app_the_config_gives_on_the_host_it_reads() {
        refused(ORG, "no [github_apps.herald] is given");
        refused(
            &format!("{ORG}api = \"https://ghe.example/api/v3\"\n{APP}"),
            "the App mints its tokens at https://api.github.com, and the source reads https://ghe.example/api/v3",
        );
        refused(
            &format!("{ORG}token = {{ secret = \"T\" }}\n{APP}"),
            "not both; remove one",
        );
        refused(
            "[sources.org]\nkind = \"gitlab\"\nowner = \"lemonfiber\"\napp = \"herald\"\n",
            "app is a GitHub source's",
        );
        refused(
            "[sources.org]\nkind = \"github\"\nowner = \"lemonfiber\"\ninstallation = 4\n",
            "give app, or remove installation",
        );
        refused(
            &format!(
                "[sources.org]\nkind = \"github\"\nowner = \"lemonfiber\"\napp = \"Herald\"\n{APP}"
            ),
            "\"Herald\" is not an App's name",
        );
    }

    #[test]
    fn an_app_s_key_is_read_by_a_poll_of_its_sources_and_its_webhook_by_a_server() {
        let config = loaded(&format!(
            "{ORG}{APP}webhook = {{ secret = \"HERALD_APP_HOOK\" }}\n[github_apps.idle]\nclient_id = \"Iv23liIDLE\"\nprivate_key = {{ secret = \"HERALD_IDLE_KEY\" }}\n"
        ))
        .unwrap();
        let used = |uses| {
            config
                .secrets_used(uses)
                .into_iter()
                .filter(|name| name.contains("_APP_") || name.contains("IDLE"))
                .collect::<Vec<_>>()
        };
        assert_eq!(used(herald_config::Uses::Announces), Vec::<&str>::new());
        assert_eq!(used(herald_config::Uses::Polls), ["HERALD_APP_KEY"]);
        assert_eq!(
            used(herald_config::Uses::Serves),
            ["HERALD_APP_HOOK", "HERALD_APP_KEY"]
        );
    }
}
