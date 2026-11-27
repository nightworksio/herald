//! Whom herald's ActivityPub actors federate with, as `[federation]` and
//! an ActivityPub destination write it: the mode, who takes a follow, the
//! follower caps, and shared block lists.

use schemars::JsonSchema;
use serde::Deserialize;
use url::{Host, Url};

/// Whom herald's actors federate with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    /// Every server and actor that `block` does not name.
    #[default]
    Open,
    /// Only the servers and actors `allow` names, and none that `block`
    /// names.
    Allowlist,
}

/// Who decides whether a follow of herald's actors is taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Approve {
    /// herald, by the lists and caps alone.
    #[default]
    Auto,
    /// The operator, through `herald followers approve` or `reject`: a
    /// follow waits until then, and its follower is not delivered to.
    Manual,
}

/// How many may follow each of herald's actors, as `followers` writes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FollowerCaps {
    /// The most followers one actor keeps; no limit where unset.
    pub most: Option<usize>,
    /// The most followers one actor keeps from any one server; no limit
    /// where unset.
    pub per_domain: Option<usize>,
}

/// A shared block list, as `lists` writes it: a file of domains to block in
/// the CSV form Mastodon exports, pinned by the SHA-256 of its text.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SharedList {
    /// The file, as a path beside `herald.toml`.
    #[schemars(example = "blocklist.csv")]
    pub file: String,
    /// The SHA-256 of the file's text, in hex: a list that changes is not
    /// read until its new digest is pinned.
    pub sha256: String,
}

impl SharedList {
    /// The domains the list blocks, its file read through `read` and
    /// checked against its pinned digest.
    ///
    /// # Errors
    ///
    /// Where the file cannot be read, has another digest than the one
    /// pinned, or is no block list.
    pub fn read(
        &self,
        read: &dyn Fn(&str) -> Result<String, String>,
    ) -> Result<Vec<String>, String> {
        let file = &self.file;
        let text = read(file).map_err(|error| format!("{file} could not be read: {error}"))?;
        let digest = crate::digest(&text);
        if !digest.eq_ignore_ascii_case(&self.sha256) {
            return Err(format!(
                "{file} has the SHA-256 {digest}, not the {} pinned; check the list, then pin its digest",
                self.sha256
            ));
        }
        blocked_domains(&text).map_err(|error| format!("{file}: {error}"))
    }
}

/// The severities a list's row blocks with; `noop` blocks nothing.
const SEVERITIES: [&str; 2] = ["suspend", "silence"];

/// The domains `csv`, a domain block list as Mastodon exports it, blocks:
/// each row's domain, first, where its severity, second, is `suspend` or
/// `silence` or not given. A header row, starting with `domain` or
/// `#domain`, and blank rows are skipped.
///
/// # Errors
///
/// The first row whose domain is not one, naming its line.
fn blocked_domains(csv: &str) -> Result<Vec<String>, String> {
    let mut blocked = Vec::new();
    for (index, row) in csv.lines().enumerate() {
        let mut fields = row.split(',').map(str::trim);
        let domain = fields.next().unwrap_or_default();
        let severity = fields.next().unwrap_or_default();
        if domain.is_empty() || matches!(domain, "domain" | "#domain") {
            continue;
        }
        if !severity.is_empty() && !SEVERITIES.contains(&severity) {
            continue;
        }
        let ascii = match Host::parse(domain) {
            Ok(Host::Domain(ascii)) => ascii.trim_end_matches('.').to_ascii_lowercase(),
            _ => String::new(),
        };
        if !crate::domain(&ascii) {
            return Err(format!(
                "line {} names no domain; give each row a domain first",
                index + 1
            ));
        }
        blocked.push(ascii);
    }
    Ok(blocked)
}

/// Whether every entry of `block` and `allow` is a domain or an actor's
/// id, and each of the follower caps `followers` sets is at least 1.
///
/// # Errors
///
/// The first that is not, naming the fix.
pub fn followed_by(
    block: &[String],
    allow: &[String],
    followers: Option<FollowerCaps>,
) -> Result<(), String> {
    if let Some(refused) = block.iter().chain(allow).find_map(|entry| unlisted(entry)) {
        return Err(refused);
    }
    let caps = followers.unwrap_or_default();
    if caps.most == Some(0) || caps.per_domain == Some(0) {
        return Err(
            "followers.most and followers.per_domain take at least 1; leave one out for no limit"
                .to_owned(),
        );
    }
    Ok(())
}

/// Why `entry` cannot be written in `block` or `allow`, where it cannot: it
/// is neither a domain nor an `https` actor id.
fn unlisted(entry: &str) -> Option<String> {
    let actor = Url::parse(entry).is_ok_and(|url| {
        url.scheme() == "https"
            && matches!(url.host(), Some(Host::Domain(_)))
            && url.port().is_none()
            && url.as_str() == entry
    });
    (!actor && !crate::domain(entry)).then(|| {
        format!(
            "\"{entry}\" is neither a domain nor an actor's id; give a host such as \"spam.example\", or an id such as \"https://spam.example/users/x\""
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listed(entries: &[&str]) -> Vec<String> {
        entries.iter().map(|entry| (*entry).to_owned()).collect()
    }

    #[test]
    fn a_shared_list_blocks_each_suspended_or_silenced_domain_and_skips_the_rest() {
        let csv = "#domain,#severity,#reject_media,#reject_reports,#public_comment,#obfuscate\n\
                   spam.example,suspend,true,true,\"spam, all of it\",false\n\
                   Loud.Example.,silence,false,false,,false\n\
                   fine.example,noop,false,false,,false\n\
                   \n\
                   bare.example\n\
                   bücher.example,suspend\n";
        assert_eq!(
            blocked_domains(csv),
            Ok(listed(&[
                "spam.example",
                "loud.example",
                "bare.example",
                "xn--bcher-kva.example"
            ]))
        );
        assert_eq!(blocked_domains("domain,severity\n"), Ok(Vec::new()));
        assert_eq!(
            blocked_domains("ok.example\nsp*m.example,suspend\n"),
            Err("line 2 names no domain; give each row a domain first".to_owned())
        );
        assert_eq!(
            blocked_domains("https://spam.example/users/x\n"),
            Err("line 1 names no domain; give each row a domain first".to_owned())
        );
    }

    #[test]
    fn a_shared_list_is_read_only_where_its_file_has_the_digest_it_is_pinned_to() {
        let csv = "spam.example,suspend\n";
        let digest = crate::digest(csv);
        let read = |file: &str| match file {
            "blocks.csv" => Ok(csv.to_owned()),
            "broken.csv" => Ok("sp*m.example\n".to_owned()),
            _ => Err("no such file".to_owned()),
        };
        let list = |file: &str, sha256: &str| SharedList {
            file: file.to_owned(),
            sha256: sha256.to_owned(),
        };
        assert_eq!(
            list("blocks.csv", &digest.to_ascii_uppercase()).read(&read),
            Ok(listed(&["spam.example"]))
        );
        assert_eq!(
            list("blocks.csv", "00").read(&read),
            Err(format!(
                "blocks.csv has the SHA-256 {digest}, not the 00 pinned; check the list, then pin its digest"
            ))
        );
        assert_eq!(
            list("gone.csv", &digest).read(&read),
            Err("gone.csv could not be read: no such file".to_owned())
        );
        let broken = crate::digest("sp*m.example\n");
        assert_eq!(
            list("broken.csv", &broken).read(&read),
            Err("broken.csv: line 1 names no domain; give each row a domain first".to_owned())
        );
    }

    #[test]
    fn an_entry_is_a_domain_or_an_https_actor_id() {
        for entry in [
            "spam.example",
            "eu.spam.example",
            "https://spam.example/users/x",
        ] {
            assert_eq!(unlisted(entry), None, "{entry}");
        }
        for entry in [
            "http://spam.example/users/x",
            "https://127.0.0.1/users/x",
            "Spam.example",
            "spam",
            "https://spam.example:8443/users/x",
            "",
        ] {
            let refused = unlisted(entry).unwrap_or_default();
            assert!(
                refused.starts_with(&format!(
                    "\"{entry}\" is neither a domain nor an actor's id"
                )),
                "{entry}: {refused}"
            );
        }
    }

    #[test]
    fn follower_caps_take_at_least_one_and_every_entry_is_checked() {
        assert_eq!(
            followed_by(
                &listed(&["spam.example"]),
                &listed(&["https://ok.example/users/a"]),
                None
            ),
            Ok(())
        );
        for caps in [
            FollowerCaps {
                most: Some(0),
                per_domain: None,
            },
            FollowerCaps {
                most: None,
                per_domain: Some(0),
            },
        ] {
            assert_eq!(
                followed_by(&[], &[], Some(caps)),
                Err("followers.most and followers.per_domain take at least 1; leave one out for no limit".to_owned())
            );
        }
        assert!(followed_by(&[], &listed(&["spam"]), None).is_err());
        assert!(followed_by(&listed(&["spam"]), &[], None).is_err());
    }
}
