//! A channel that is no single path segment, `..` read from a config no
//! check passed: every step and read back is refused before a request
//! (ADR-0067).

use herald_core::{Sent, Step};
use herald_testing::sample::{announcement, marker};
use herald_testing::{Fixture, Replay};

use super::{bot, delivery, pinned, read_back, replacing, unpin};

const DOTTED: &str = "the Discord id in the request's path is empty or a dot segment, which names nothing in a URL path, so this step is not taken";

fn named_bot() -> Replay {
    Replay::of(
        Fixture::from_json(
            r#"{"exchanges":[{"request":{"method":"GET","url":"https://discord.com/api/v10/users/@me","headers":{"Authorization":"Bot «secret:HERALD_DISCORD_TOKEN»","User-Agent":"DiscordBot (https://github.com/nightworksio/herald, 0.0.0)"}},"response":{"status":200,"body":{"id":"1566190000000000009"}}}]}"#,
        )
        .unwrap(),
    )
}

#[test]
fn a_dotted_channel_is_read_back_unpinned_or_crossposted_never() {
    let dotted = bot("..", false);
    let read = named_bot();
    assert_eq!(
        read_back(&dotted, &read, &marker())
            .unwrap_err()
            .to_string(),
        format!("the destination's posts could not be read: {DOTTED}")
    );
    read.finish().unwrap();
    let pins = named_bot();
    assert_eq!(
        unpin(&bot("..", false), &pins, &announcement(), &pinned()),
        Sent::Refused(DOTTED.to_owned())
    );
    pins.finish().unwrap();
    let nothing = Replay::of(Fixture::default());
    assert_eq!(
        unpin(
            &dotted,
            &nothing,
            &replacing("1566200000000000008"),
            &pinned()
        ),
        Sent::Refused(DOTTED.to_owned())
    );
    assert_eq!(
        dotted
            .request(&Step::Crosspost, &announcement(), &delivery(), &pinned())
            .unwrap_err()
            .to_string(),
        DOTTED
    );
    nothing.finish().unwrap();
}
