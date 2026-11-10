//! What a homeserver says of a bot's token and of its power in each route's
//! room, asked with nothing posted (ADR-0002).

use std::collections::BTreeMap;

use herald_core::Delivery;
use herald_kinds::Probe;
use herald_ports::Http;
use serde::Deserialize;

use crate::matrix::Matrix;
use crate::request::{MESSAGE, PINNED_EVENTS, get, room};

/// The platform, as a probe names it.
const MATRIX: &str = "The homeserver";

/// Who a token is, as `/account/whoami` says.
#[derive(Debug, Deserialize)]
struct Whoami {
    user_id: String,
}

/// A room's power levels, with the defaults the Matrix specification gives
/// where the event leaves one out.
#[derive(Debug, Deserialize)]
struct PowerLevels {
    #[serde(default)]
    users: BTreeMap<String, i64>,
    #[serde(default)]
    users_default: i64,
    #[serde(default)]
    events: BTreeMap<String, i64>,
    #[serde(default)]
    events_default: i64,
    #[serde(default = "state_default")]
    state_default: i64,
}

/// The power a state event needs where the room names none.
const fn state_default() -> i64 {
    50
}

impl PowerLevels {
    /// What `user` lacks to post into the room and, where `pinning`, to pin
    /// there.
    fn lacking(&self, user: &str, pinning: bool) -> Vec<(&'static str, i64)> {
        let held = self.users.get(user).copied().unwrap_or(self.users_default);
        let needed = |event: &'static str, default: i64| {
            (event, self.events.get(event).copied().unwrap_or(default))
        };
        [
            Some(needed(MESSAGE, self.events_default)),
            pinning.then(|| needed(PINNED_EVENTS, self.state_default)),
        ]
        .into_iter()
        .flatten()
        .filter(|(_, level)| *level > held)
        .collect()
    }
}

/// What the homeserver `matrix` reaches says, asked through `http`, of its
/// token and of the bot's power in the route's room for `delivery`.
pub(crate) async fn probe<H: Http>(matrix: &Matrix, http: &H, delivery: &Delivery) -> Vec<Probe> {
    let answer = http.send(get(matrix, "/account/whoami")).await;
    let taken = Probe::credential(MATRIX, &answer);
    let user = answer
        .ok()
        .and_then(|answer| serde_json::from_slice::<Whoami>(&answer.body).ok());
    let (Probe::Taken, Some(Whoami { user_id })) = (&taken, user) else {
        return vec![taken];
    };
    let mut probes = vec![taken];
    let roomed = &matrix.route.room;
    let path = format!("{}/state/m.room.power_levels/", room(matrix));
    let answer = http.send(get(matrix, &path)).await;
    if let Some(withheld) = Probe::route(MATRIX, Some("room"), &answer, |status| {
        format!(
            "{user_id} cannot read the power levels of {roomed}, as the homeserver answered {status}; invite the bot to the room and have it join"
        )
    }) {
        probes.push(withheld);
        return probes;
    }
    let levels = answer
        .ok()
        .and_then(|answer| serde_json::from_slice::<PowerLevels>(&answer.body).ok());
    let Some(levels) = levels else {
        probes.push(Probe::Said(format!("the homeserver's power levels for {roomed} could not be read, so the bot's power there is not checked")));
        return probes;
    };
    for (event, level) in levels.lacking(&user_id, delivery.pin.is_some()) {
        probes.push(Probe::Route {
            key: Some("room"),
            why: format!("{user_id} needs power level {level} in {roomed} to send {event}; raise the bot's power level there"),
        });
    }
    probes
}
