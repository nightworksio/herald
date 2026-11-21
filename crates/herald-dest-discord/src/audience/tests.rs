use herald_ports::Secret;
use herald_testing::ScriptedHttp;

use super::*;
use crate::RouteOptions;

#[test]
fn a_path_holding_a_dot_segment_is_never_asked() {
    let discord = Discord::new(
        herald_ports::Credential::Bot(Secret::new("HERALD_DISCORD_TOKEN", "test-token")),
        RouteOptions::default(),
        None,
    );
    let http = ScriptedHttp::new(Vec::new());
    let read: Result<Guild, String> = pollster::block_on(read(&discord, &http, &["guilds", ".."]));
    assert_eq!(
        read.err().as_deref(),
        Some(
            "the Discord id in the request's path is empty or a dot segment, which names nothing in a URL path, so this step is not taken"
        )
    );
    assert_eq!(http.asked().len(), 0);
}
