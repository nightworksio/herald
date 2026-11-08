//! `[http]`: how herald's own requests reach the services a config names,
//! read with its defaults, and every value it cannot keep named by key.

#[cfg(test)]
mod http {
    use herald_config::load;
    use herald_ports::{Outbound, Proxy, Roots};
    use jiff::SignedDuration;

    const EXAMPLE: &str = r#"version = 1

[store]
kind = "git-branch"
key = { secret = "HERALD_STORE_KEY" }

[destinations.discord]
kind = "discord"
token = { secret = "HERALD_DISCORD_TOKEN" }

[sources.gate]
kind = "github"
repository = "nightworksio/php-mutation-gate"

[[routes]]
id = "releases"
sources = ["gate"]
destination = "discord"
channel = "1566200000000000100"
template = "release"

[templates.release]
title = "{{ event.project }} {{ event.version }}"
body = "{{ event.body }}"
"#;

    const CA: &str = "-----BEGIN CERTIFICATE-----\nMIIB\n-----END CERTIFICATE-----\n";

    fn files(name: &str) -> Result<String, String> {
        match name {
            "ca.pem" => Ok(CA.to_owned()),
            "notes.txt" => Ok("not a certificate".to_owned()),
            _ => Err(format!("{name}: no such file")),
        }
    }

    fn errors(text: &str) -> String {
        load("herald.toml", text, &files).unwrap_err().to_string()
    }

    #[test]
    fn a_config_without_an_http_table_keeps_the_standard_bounds() {
        let config = load("herald.toml", EXAMPLE, &files).unwrap();
        assert_eq!(config.http, Outbound::STANDARD);
        assert_eq!(config.http.connect, SignedDuration::from_secs(10));
        assert_eq!(config.http.total, SignedDuration::from_secs(30));
        assert_eq!(config.http.proxy, Proxy::Environment);
        assert_eq!(config.http.roots, None);
        assert_eq!(config.http.body, 32 << 20);
    }

    #[test]
    fn every_http_setting_is_read() {
        let config = load(
            "herald.toml",
            &format!("{EXAMPLE}\n[http]\nconnect = \"3s\"\ntotal = \"1m\"\nproxy = \"https://proxy.example:3128\"\nca_file = \"ca.pem\"\nbody = \"64MiB\"\n"),
            &files,
        )
        .unwrap();
        assert_eq!(
            config.http,
            Outbound {
                connect: SignedDuration::from_secs(3),
                total: SignedDuration::from_mins(1),
                proxy: Proxy::At("https://proxy.example:3128".to_owned()),
                roots: Some(Roots {
                    file: "ca.pem".to_owned(),
                    pem: CA.to_owned(),
                }),
                body: 64 << 20,
            }
        );
        for (written, proxy) in [("environment", Proxy::Environment), ("none", Proxy::None)] {
            let config = load(
                "herald.toml",
                &format!("{EXAMPLE}\n[http]\nproxy = \"{written}\"\n"),
                &files,
            )
            .unwrap();
            assert_eq!(config.http.proxy, proxy);
        }
    }

    #[test]
    fn a_value_herald_cannot_keep_is_named_by_its_key() {
        let slow = errors(&format!(
            "{EXAMPLE}\n[http]\nconnect = \"1m\"\ntotal = \"30s\"\n"
        ));
        assert!(slow.contains("http.connect"), "{slow}");
        assert!(slow.contains("make connect at most total"), "{slow}");
        let socks = errors(&format!("{EXAMPLE}\n[http]\nproxy = \"socks5://proxy\"\n"));
        assert!(socks.contains("http.proxy"), "{socks}");
        assert!(socks.contains("socks5://proxy\" is no proxy"), "{socks}");
        let absent = errors(&format!("{EXAMPLE}\n[http]\nca_file = \"absent.pem\"\n"));
        assert!(absent.contains("http.ca_file"), "{absent}");
        assert!(absent.contains("absent.pem: no such file"), "{absent}");
        let plain = errors(&format!("{EXAMPLE}\n[http]\nca_file = \"notes.txt\"\n"));
        assert!(
            plain.contains("notes.txt holds no PEM certificate"),
            "{plain}"
        );
        let wide = errors(&format!("{EXAMPLE}\n[http]\nbody = \"129MiB\"\n"));
        assert!(
            wide.contains("http.body: make it at most \"128MiB\", four times its default"),
            "{wide}"
        );
        let unknown = errors(&format!("{EXAMPLE}\n[http]\nretries = 3\n"));
        assert!(unknown.contains("retries"), "{unknown}");
    }
}
