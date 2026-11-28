use herald_core::{Announcement, Pinning, RouteId, Step};
use herald_flows::{DRY_ID, FED, FOLLOWED, PUSHED, UNREACHED, Wire, preview, wired};
use herald_ports::{Http as _, Method, Request, SecretText};

use crate::harness::*;

fn announced(config: &herald_config::Config) -> Announcement {
    preview(config, &event("v1"), None)
        .remove(0)
        .rendered
        .unwrap()
}

fn sent(config: &herald_config::Config, route: &str) -> Wire {
    pollster::block_on(wired(config, &RouteId::new(route), &announced(config)))
}

fn shown(requests: &[Request]) -> Vec<String> {
    requests
        .iter()
        .map(|request| request.with_body().to_string())
        .collect()
}

#[test]
fn a_first_post_sends_each_step_its_destination_writes_with_each_secret_by_its_name() {
    let Wire::Sent(steps) = sent(&config(""), "releases") else {
        panic!("the route sends nothing");
    };
    assert_eq!(
        steps
            .iter()
            .map(|(step, _)| step.clone())
            .collect::<Vec<_>>(),
        all()
    );
    let channel = "https://discord.com/api/v10/channels/1566200000000000100/messages";
    let posted = &steps[0].1;
    assert_eq!(posted.len(), 1);
    assert_eq!(
        (posted[0].method, posted[0].url.redacted()),
        (Method::Post, channel.to_owned())
    );
    let authorised = posted[0]
        .headers
        .iter()
        .find(|(name, _)| name == "Authorization")
        .map(|(_, value)| (value.redacted(), value.expose()));
    assert_eq!(
        authorised,
        Some((
            "Bot «secret:HERALD_DISCORD_TOKEN»".to_owned(),
            "Bot ".to_owned()
        ))
    );
    assert!(
        String::from_utf8_lossy(&posted[0].body).contains("📦 php-mutation-gate v1"),
        "{posted:?}"
    );
    assert_eq!(
        steps[1].1[0].url.redacted(),
        format!("{channel}/{DRY_ID}/crosspost")
    );
    assert_eq!(
        steps[2].1[0].url.redacted(),
        format!("{channel}/{DRY_ID}/threads")
    );
    assert_eq!(
        shown(&steps[3].1),
        [format!(
            "PUT {channel}/pins/{DRY_ID}\nUser-Agent: DiscordBot (https://github.com/nightworksio/herald, {})\nAuthorization: Bot «secret:HERALD_DISCORD_TOKEN»",
            herald_core::itself::VERSION
        )]
    );
    assert_eq!(
        steps[4]
            .1
            .iter()
            .map(|request| (request.method, request.url.redacted()))
            .collect::<Vec<_>>(),
        [
            (
                Method::Get,
                "https://discord.com/api/v10/users/@me".to_owned()
            ),
            (Method::Get, format!("{channel}/pins?limit=50")),
        ]
    );
}

#[test]
fn a_route_keeping_its_pin_in_the_store_unpins_nothing_on_its_first_post() {
    let mut config = config("");
    config.routes[0].route.delivery.pin = Some(Pinning::Store);
    let Wire::Sent(steps) = sent(&config, "releases") else {
        panic!("the route sends nothing");
    };
    let unpinned = steps
        .iter()
        .find(|(step, _)| *step == Step::Unpin)
        .map(|(_, requests)| requests.len());
    assert_eq!(unpinned, Some(0));
    let pinned = steps
        .iter()
        .find(|(step, _)| *step == Step::Pin)
        .map(|(_, requests)| requests.len());
    assert_eq!(pinned, Some(1));
}

#[test]
fn a_route_herald_serves_delivers_or_pushes_sends_no_request_and_says_why() {
    let followed = crate::activitypub::activitypub();
    assert_eq!(
        pollster::block_on(wired(
            &followed,
            &RouteId::new("followed"),
            &announced(&config(""))
        )),
        Wire::Unsent(FOLLOWED)
    );
    let fed = crate::feeds::config("HERALD_KEY", "");
    assert_eq!(
        pollster::block_on(wired(&fed, &RouteId::new("fed"), &announced(&config("")))),
        Wire::Unsent(FED)
    );
    let pushed = crate::pushing::config("");
    assert_eq!(
        pollster::block_on(wired(
            &pushed,
            &RouteId::new("pushed"),
            &announced(&config(""))
        )),
        Wire::Unsent(PUSHED)
    );
    assert_eq!(sent(&config(""), "elsewhere"), Wire::Unsent(UNREACHED));
}

#[test]
fn a_recording_keeps_each_request_until_taken_and_answers_each_with_the_dry_id() {
    let recording = herald_flows::Recording::default();
    let request = Request::new(Method::Get, SecretText::plain("https://example.org"));
    let answer = pollster::block_on(recording.send(request.clone())).unwrap();
    assert_eq!(
        (answer.status, answer.body),
        (200, format!(r#"{{"id":"{DRY_ID}"}}"#).into_bytes())
    );
    assert_eq!(recording.taken(), [request]);
    assert_eq!(recording.taken(), []);
}
