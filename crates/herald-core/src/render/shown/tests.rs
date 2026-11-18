use jiff::Timestamp;

use super::*;
use crate::queue::tests::pull;
use crate::queue::{Board, Bots, View, board};

fn keys(value: &Value) -> Vec<String> {
    let mut keys: Vec<String> = value
        .try_iter()
        .unwrap()
        .map(|key| key.to_string())
        .collect();
    keys.sort();
    keys
}

fn shown() -> Board {
    let now = Timestamp::from_second(30 * 24 * 60 * 60).unwrap();
    let bump = crate::queue::Pull {
        number: 2,
        bot: true,
        title: "Bump serde from 1.0.1 to 1.0.2".to_owned(),
        ..pull()
    };
    board(
        &View {
            bots: Bots::Batch,
            ..View::default()
        },
        &[pull(), bump],
        now,
    )
}

#[test]
fn a_view_shows_its_name_counts_groups_and_batches_alone() {
    let board = shown();
    let value = view_value(Showing {
        view: "queue",
        board: &board,
    });
    assert_eq!(
        keys(&value),
        ["batches", "counts", "groups", "name", "stale", "total"]
    );
    assert_eq!(value.get_attr("name").unwrap().to_string(), "queue");
    assert_eq!(value.get_attr("total").unwrap().to_string(), "2");
    let counts = value.get_attr("counts").unwrap();
    assert_eq!(
        keys(&counts.get_item_by_index(0).unwrap()),
        ["count", "state"]
    );
    let group = value
        .get_attr("groups")
        .unwrap()
        .get_item_by_index(0)
        .unwrap();
    assert_eq!(keys(&group), ["name", "rows"]);
    let row = group
        .get_attr("rows")
        .unwrap()
        .get_item_by_index(0)
        .unwrap();
    assert_eq!(
        keys(&row),
        [
            "author",
            "bot",
            "decided",
            "draft",
            "labels",
            "number",
            "opened",
            "project",
            "reason",
            "repository",
            "reviewers",
            "stale",
            "state",
            "title",
            "updated",
            "url",
            "waited",
            "waited_seconds",
            "waiting_on"
        ]
    );
    let batch = value
        .get_attr("batches")
        .unwrap()
        .get_item_by_index(0)
        .unwrap();
    assert_eq!(keys(&batch), ["failing", "green", "package", "rows", "to"]);
    assert_eq!(batch.get_attr("package").unwrap().to_string(), "serde");
    assert_eq!(batch.get_attr("to").unwrap().to_string(), "1.0.2");
}

#[test]
fn a_wait_is_written_in_its_largest_whole_unit() {
    assert_eq!(waited(SignedDuration::from_secs(59)), "0m");
    assert_eq!(waited(SignedDuration::from_mins(59)), "59m");
    assert_eq!(waited(SignedDuration::from_mins(60)), "1h");
    assert_eq!(waited(SignedDuration::from_hours(23)), "23h");
    assert_eq!(waited(SignedDuration::from_hours(24)), "1d");
    assert_eq!(waited(SignedDuration::from_hours(24 * 15)), "15d");
}

#[test]
fn a_view_as_json_holds_the_keys_a_template_reads() {
    let board = shown();
    let json: serde_json::Value = serde_json::from_str(&crate::render::view_json(Showing {
        view: "queue",
        board: &board,
    }))
    .unwrap();
    let mut keys: Vec<&String> = json.as_object().unwrap().keys().collect();
    keys.sort();
    assert_eq!(
        keys,
        ["batches", "counts", "groups", "name", "stale", "total"]
    );
    assert_eq!(json["groups"][0]["rows"][0]["number"], 7);
}
