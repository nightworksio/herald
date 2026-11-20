//! Who may read a Matrix route's room, asked right before a run's first
//! advisories section is sent there: a room is private only where joining
//! takes an invite or a knock, or a membership of rooms that themselves
//! take one, and its history is not world-readable (ADR-0067).

use herald_kinds::Audience;
use herald_ports::Http;
use serde::Deserialize;

use crate::matrix::Matrix;
use crate::request::{get, segment};

/// What herald asks before a run's first advisory send, as doctor names
/// it.
pub const ASKS: &str = "the homeserver for the room's m.room.join_rules and m.room.history_visibility, and the join rules of each room a restricted room lets in, through the bot's token, which must have joined each of them";

/// The most rooms a restricted room's join rules may let in for herald to
/// check them all; past it the room is unread.
pub(crate) const MOST_ALLOWED: usize = 5;

/// The requests the check takes at most: the room's two state events and
/// the join rules of each room it lets in.
pub(crate) const REQUESTS: usize = 2 + MOST_ALLOWED;

/// The state event that says who may join a room.
const JOIN_RULES: &str = "m.room.join_rules";
/// The state event that says who may read a room's history.
const HISTORY_VISIBILITY: &str = "m.room.history_visibility";
/// The join rules that let only those invited or let in join.
const CLOSED: [&str; 3] = ["invite", "knock", "private"];
/// The join rules that also let in the members of the rooms they name.
const RESTRICTED: [&str; 2] = ["restricted", "knock_restricted"];
/// How a restricted room names a room whose members it lets in.
const MEMBERSHIP: &str = "m.room_membership";
/// The history visibility that lets anyone read a room, joined or not.
const WORLD_READABLE: &str = "world_readable";

#[derive(Deserialize)]
struct JoinRules {
    join_rule: String,
    #[serde(default)]
    allow: Vec<Allowed>,
}

#[derive(Deserialize)]
struct Allowed {
    #[serde(rename = "type")]
    kind: String,
    room_id: Option<String>,
}

#[derive(Deserialize)]
struct History {
    history_visibility: String,
}

/// The state event `event` of the room `roomed`, asked of `matrix`'s
/// homeserver through `http`; or why it could not be read.
async fn state<T: for<'de> Deserialize<'de>, H: Http>(
    (matrix, http): (&Matrix, &H),
    roomed: &str,
    event: &str,
) -> Result<T, String> {
    let room = segment(roomed).map_err(|declined| declined.to_string())?;
    let path = format!("/rooms/{room}/state/{event}/");
    let answer = http.send(get(matrix, &path)).await.map_err(|error| {
        format!("the homeserver could not be asked for {roomed}'s {event}: {error}")
    })?;
    if !answer.succeeded() {
        return Err(format!(
            "the homeserver would not give {roomed}'s {event}, as it answered {}; have the bot join the room",
            answer.status
        ));
    }
    serde_json::from_slice(&answer.body)
        .map_err(|_| format!("the homeserver's {event} of {roomed} could not be read"))
}

/// Why `rules` of the room `roomed` let more join than those invited or
/// let in, each room a restricted one names asked through `asking`; none
/// where they do not, or the reason it is unread.
async fn opened<H: Http>(
    asking: (&Matrix, &H),
    roomed: &str,
    rules: JoinRules,
) -> Option<Audience> {
    let rule = rules.join_rule.as_str();
    if CLOSED.contains(&rule) {
        return None;
    }
    if !RESTRICTED.contains(&rule) {
        return Some(Audience::Public(format!(
            "any Matrix user may join {roomed}, as its {JOIN_RULES} is {rule}"
        )));
    }
    if rules.allow.len() > MOST_ALLOWED {
        return Some(Audience::Unread(format!(
            "{roomed} lets in the members of {} rooms, more than the {MOST_ALLOWED} herald checks",
            rules.allow.len()
        )));
    }
    for allowed in rules.allow {
        let Some(other) = allowed.room_id.filter(|_| allowed.kind == MEMBERSHIP) else {
            return Some(Audience::Unread(format!(
                "{roomed} lets in members by a rule of the kind {}, which herald does not read",
                allowed.kind
            )));
        };
        let theirs: JoinRules = match state(asking, &other, JOIN_RULES).await {
            Ok(theirs) => theirs,
            Err(why) => return Some(Audience::Unread(why)),
        };
        if !CLOSED.contains(&theirs.join_rule.as_str()) {
            return Some(Audience::Unread(format!(
                "{roomed} lets in the members of {other}, whose {JOIN_RULES} is {}, not an invite or a knock",
                theirs.join_rule
            )));
        }
    }
    None
}

/// Who may read the room of `matrix`'s route, asked through `http`.
pub(crate) async fn audience<H: Http>(matrix: &Matrix, http: &H) -> Audience {
    let roomed = &matrix.route.room;
    let joining: JoinRules = match state((matrix, http), roomed, JOIN_RULES).await {
        Ok(joining) => joining,
        Err(why) => return Audience::Unread(why),
    };
    let history: History = match state((matrix, http), roomed, HISTORY_VISIBILITY).await {
        Ok(history) => history,
        Err(why) => return Audience::Unread(why),
    };
    if history.history_visibility == WORLD_READABLE {
        return Audience::Public(format!(
            "anyone may read the history of {roomed}, as its {HISTORY_VISIBILITY} is {WORLD_READABLE}"
        ));
    }
    opened((matrix, http), roomed, joining)
        .await
        .unwrap_or(Audience::Private)
}
