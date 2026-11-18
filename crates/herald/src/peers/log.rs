//! `herald peer log`: what each peer asked of this herald and how it was
//! answered, newest first, read from the audit the store keeps of each
//! UTC day (ADR-0054).

use std::collections::BTreeMap;
use std::io::Write;

use herald_compose::{Host, Hosting, ServingOn, Through};
use herald_config::{Config, Uses};
use herald_core::{AuditDay, Audited, Duration, utc_day};
use herald_environment::SystemClock;
use herald_ports::{Clock as _, Keyring, Published as _, Store};
use jiff::Timestamp;
use jiff::civil::Date;
use jiff::tz::TimeZone;

use super::super::cli::Shown;
use super::super::output::say;
use super::super::stores::composed;
use super::super::{Connect, DONE, place};

/// What `herald peer log` is asked.
pub(crate) struct Logging<'a> {
    /// Only this peer, or the keys no peer holds.
    pub(crate) name: Option<&'a str>,
    /// Only requests since this day, time or length of time ago.
    pub(crate) since: Option<&'a str>,
    /// The most requests shown.
    pub(crate) limit: usize,
    /// How they are printed.
    pub(crate) format: Shown,
}

/// The requests `asked` names, read from the store `vars` names through
/// `connect`, printed to `out` newest first.
///
/// # Errors
///
/// It names no peer of the config, its `since` is no day, time or length
/// of time, or the store cannot be read.
pub(crate) async fn log(
    config: &Config,
    (vars, connect): (&BTreeMap<String, String>, Connect<'_>),
    asked: &Logging<'_>,
    out: &mut dyn Write,
) -> Result<u8, String> {
    let peers: Vec<String> = match asked.name {
        Some(name) if name == AuditDay::STRANGERS || config.peers.contains_key(name) => {
            vec![name.to_owned()]
        }
        Some(name) => return Err(format!("there is no peer {name} in [peers]")),
        None => config
            .peers
            .keys()
            .cloned()
            .chain([AuditDay::STRANGERS.to_owned()])
            .collect(),
    };
    let since = asked
        .since
        .map(|since| since_when(since, SystemClock.now()))
        .transpose()?;
    let composed = composed(
        config,
        vars,
        connect,
        Hosting::Actions,
        Uses::Polls,
        place(vars),
    )
    .await?;
    let days = composed.run(Days(peers)).await?;
    let mut entries: Vec<&Audited> = days
        .iter()
        .filter(|day| since.is_none_or(|since| day.day >= utc_day(since)))
        .flat_map(|day| &day.entries)
        .filter(|entry| since.is_none_or(|since| entry.at >= since))
        .collect();
    entries.sort_by_key(|entry| std::cmp::Reverse(entry.at));
    entries.truncate(asked.limit);
    let uncounted: u64 = days.iter().map(|day| day.more).sum();
    match asked.format {
        Shown::Json => say(out, &serde_json::to_string(&entries).unwrap_or_default()),
        Shown::Text => {
            for entry in &entries {
                say(out, &line(entry));
            }
            if uncounted > 0 {
                say(
                    out,
                    &format!("{uncounted} more requests were counted on busy days but not kept."),
                );
            }
        }
    }
    Ok(DONE)
}

/// `entry` as one line: when, by whom, what of which resource, answered
/// what and why.
fn line(entry: &Audited) -> String {
    let by = entry
        .peer
        .as_deref()
        .or(entry.keyid.as_deref())
        .unwrap_or("an unsigned request");
    format!(
        "{} {by} {} {} {} {}",
        entry.at,
        entry.action.name(),
        entry.resource,
        entry.outcome,
        entry.reason
    )
}

/// The time `since` names, at `now`: a day's start in UTC, a time, or a
/// length of time before `now`.
///
/// # Errors
///
/// It is none of these.
fn since_when(since: &str, now: Timestamp) -> Result<Timestamp, String> {
    since
        .parse::<Date>()
        .ok()
        .and_then(|day| day.to_zoned(TimeZone::UTC).ok())
        .map(|zoned| zoned.timestamp())
        .or_else(|| since.parse::<Timestamp>().ok())
        .or_else(|| Duration::parse(since).and_then(|ago| now.checked_sub(ago.0).ok()))
        .ok_or_else(|| {
            format!("--since {since} is no day, time or length of time; give one such as 2027-01-15, 2027-01-15T08:00:00Z or 7d")
        })
}

/// Every day the store keeps of each peer named.
struct Days(Vec<String>);

impl<H: Host> Through<H> for Days {
    type Done = Result<Vec<AuditDay>, String>;

    async fn through<S: Store, K: Keyring>(
        self,
        serving: &ServingOn<'_, H, S, K>,
        _: &str,
    ) -> Self::Done {
        let mut days = Vec::new();
        for peer in &self.0 {
            days.extend(
                serving
                    .ports
                    .store
                    .published()
                    .audits(peer)
                    .await
                    .map_err(|error| error.to_string())?,
            );
        }
        Ok(days)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn since_names_a_day_a_time_or_how_long_ago() {
        let now: Timestamp = "2027-01-15T08:00:00Z".parse().unwrap();
        assert_eq!(
            since_when("2027-01-14", now),
            Ok("2027-01-14T00:00:00Z".parse().unwrap())
        );
        assert_eq!(
            since_when("2027-01-14T10:00:00Z", now),
            Ok("2027-01-14T10:00:00Z".parse().unwrap())
        );
        assert_eq!(
            since_when("7d", now),
            Ok("2027-01-08T08:00:00Z".parse().unwrap())
        );
        for unknown in ["yesterday", "9999-12-31"] {
            assert!(
                since_when(unknown, now)
                    .unwrap_err()
                    .starts_with(&format!("--since {unknown} is no day")),
            );
        }
    }
}
