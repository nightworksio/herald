use super::*;

fn overdue() -> MilestoneChoice {
    choice(Milestones {
        actions: Some(vec![MilestoneAction::Overdue]),
        ..Milestones::default()
    })
}

fn due(milestone: Value, day: &str) -> Value {
    let mut milestone = milestone;
    milestone["due_on"] = json!(format!("{day}T08:00:00Z"));
    milestone
}

fn after(day: &str) -> Timestamp {
    format!("{day}T00:00:00Z").parse().unwrap()
}

#[test]
fn an_open_milestone_past_its_due_day_is_announced_overdue_once_a_due_day() {
    let unchanged = || answer(304, &Value::Null);
    let cursor = kept(&[
        ("milestone", &["1", "2"]),
        (
            "milestone/due",
            &["1:2026-12-01", "2:2026-12-05", "9:2026-11-01"],
        ),
        ("milestone/overdue", &["2:2026-11-30", "9:2026-11-01"]),
    ]);
    let http = ScriptedHttp::new(vec![unchanged(), answer(200, &github(1, "open", "v1"))]);
    let polled = poll_at(
        Forge::GitHub,
        &http,
        (Some(&cursor), after("2026-12-02")),
        (&overdue(), None),
    )
    .unwrap();
    assert!(polled.changed);
    assert_eq!(
        http.asked()[1].0,
        "https://api.github.com/repos/o/r/milestones/1"
    );
    let [event] = polled.found.events.as_slice() else {
        panic!("{:?}", polled.found.events);
    };
    assert_eq!(
        identity(event),
        "github.com/7/milestone/1/overdue/2026-12-01"
    );
    assert_eq!(
        event.title(),
        "Milestone v1 overdue: 12 of 14 closed, due 2026-12-01"
    );
    assert_eq!(event.occurred(), after("2026-12-02"));
    assert_eq!(
        event.attribute(ACTION),
        Some(&Attribute::Text("overdue".to_owned()))
    );
    assert_eq!(
        ids(&polled.found),
        BTreeMap::from([
            ("milestone", vec!["1", "2"]),
            ("milestone/closed", vec![]),
            ("milestone/due", vec!["1:2026-12-01", "2:2026-12-05"]),
            ("milestone/overdue", vec!["1:2026-12-01"])
        ])
    );
    let announced = kept(&[
        ("milestone", &["1", "2"]),
        ("milestone/due", &["1:2026-12-01", "2:2026-12-05"]),
        ("milestone/overdue", &["1:2026-12-01"]),
    ]);
    let again = ScriptedHttp::new(vec![unchanged()]);
    let quiet = poll_at(
        Forge::GitHub,
        &again,
        (Some(&announced), after("2026-12-03")),
        (&overdue(), None),
    )
    .unwrap();
    assert!(!quiet.changed);
    assert_eq!(quiet.found.events, Vec::<Event>::new());
    assert_eq!(again.asked().len(), 1);
    let moved = ScriptedHttp::new(vec![answer(
        200,
        &json!([
            due(github(1, "open", "v1"), "2026-12-03"),
            due(github(2, "open", "v2"), "2026-12-05")
        ]),
    )]);
    let polled = poll_at(
        Forge::GitHub,
        &moved,
        (Some(&announced), after("2026-12-04")),
        (&overdue(), None),
    )
    .unwrap();
    assert_eq!(moved.asked().len(), 1);
    let identities: Vec<&str> = polled.found.events.iter().map(identity).collect();
    assert_eq!(identities, ["github.com/7/milestone/1/overdue/2026-12-03"]);
    assert_eq!(ids(&polled.found)["milestone/overdue"], ["1:2026-12-03"]);
    let mut milestone = github(1, "open", "v1");
    milestone["due_on"] = Value::Null;
    let undated = ScriptedHttp::new(vec![
        answer(200, &json!([milestone])),
        answer(404, &json!({ "message": "Not Found" })),
    ]);
    let polled = poll_at(
        Forge::GitHub,
        &undated,
        (Some(&announced), after("2026-12-04")),
        (&overdue(), None),
    )
    .unwrap();
    assert_eq!(polled.found.events, Vec::<Event>::new());
    assert_eq!(ids(&polled.found)["milestone/due"], Vec::<&str>::new());
    assert_eq!(ids(&polled.found)["milestone/overdue"], Vec::<&str>::new());
}

#[test]
fn a_past_due_day_is_not_announced_where_the_milestone_closed_moved_or_went_or_is_not_chosen() {
    let cursor = kept(&[("milestone", &["1"]), ("milestone/due", &["1:2026-12-01"])]);
    let late = after("2026-12-02");
    let read = |answered: Response, choice: &MilestoneChoice| {
        let http = ScriptedHttp::new(vec![answer(304, &Value::Null), answered]);
        let polled = poll_at(Forge::GitHub, &http, (Some(&cursor), late), (choice, None)).unwrap();
        assert!(polled.changed);
        assert_eq!(ids(&polled.found)["milestone/overdue"], ["1:2026-12-01"]);
        polled.found.events
    };
    for answered in [
        answer(200, &github(1, "closed", "v1")),
        answer(200, &due(github(1, "open", "v1"), "2026-12-09")),
        answer(404, &json!({ "message": "Not Found" })),
    ] {
        assert_eq!(read(answered, &overdue()), Vec::<Event>::new());
    }
    let titled = choice(Milestones {
        actions: Some(vec![MilestoneAction::Overdue]),
        titles: vec!["x*".to_owned()],
        ..Milestones::default()
    });
    assert_eq!(
        read(answer(200, &github(1, "open", "v1")), &titled),
        Vec::<Event>::new()
    );
    let unchosen = ScriptedHttp::new(vec![answer(304, &Value::Null)]);
    let polled = poll_at(
        Forge::GitHub,
        &unchosen,
        (Some(&cursor), late),
        (&every(), None),
    )
    .unwrap();
    assert_eq!(unchosen.asked().len(), 1);
    assert_eq!(polled.found.events, Vec::<Event>::new());
    assert_eq!(ids(&polled.found)["milestone/overdue"], ["1:2026-12-01"]);
    let first = ScriptedHttp::new(vec![answer(200, &json!([github(1, "open", "v1")]))]);
    let polled = poll_at(Forge::GitHub, &first, (None, late), (&overdue(), None)).unwrap();
    assert_eq!(first.asked().len(), 1);
    assert_eq!(polled.found.events, Vec::<Event>::new());
    assert_eq!(ids(&polled.found)["milestone/overdue"], ["1:2026-12-01"]);
    let due_today = ScriptedHttp::new(vec![answer(304, &Value::Null)]);
    let polled = poll_at(
        Forge::GitHub,
        &due_today,
        (Some(&cursor), "2026-12-01T23:59:59Z".parse().unwrap()),
        (&overdue(), None),
    )
    .unwrap();
    assert!(!polled.changed);
    assert_eq!(ids(&polled.found)["milestone/overdue"], Vec::<&str>::new());
}

#[test]
fn a_poll_keeps_the_due_days_of_no_more_than_the_open_milestones_it_follows() {
    let most = choice(Milestones {
        actions: Some(vec![MilestoneAction::Overdue]),
        most: std::num::NonZeroUsize::new(2),
        ..Milestones::default()
    });
    let http = ScriptedHttp::new(vec![answer(
        200,
        &json!([
            github(1, "open", "v1"),
            github(2, "open", "v2"),
            github(3, "open", "v3")
        ]),
    )]);
    let cursor = kept(&[
        ("milestone", &[]),
        (
            "milestone/due",
            &["4:2026-11-01", "1:nonsense", "x:2026-11-01", "5"],
        ),
        ("milestone/overdue", &["4:2026-11-01"]),
    ]);
    let polled = poll_at(
        Forge::GitHub,
        &http,
        (Some(&cursor), after("2026-12-02")),
        (&most, Some(2)),
    )
    .unwrap();
    assert_eq!(
        ids(&polled.found)["milestone/due"],
        ["1:2026-12-01", "2:2026-12-01"]
    );
    assert_eq!(
        ids(&polled.found)["milestone/overdue"],
        ["1:2026-12-01", "2:2026-12-01"]
    );
    let identities: Vec<&str> = polled.found.events.iter().map(identity).collect();
    assert_eq!(
        identities,
        [
            "github.com/7/milestone/1/overdue/2026-12-01",
            "github.com/7/milestone/2/overdue/2026-12-01"
        ]
    );
}

#[test]
fn a_gitlab_group_s_milestones_go_overdue_under_their_own_listing() {
    let statistics = json!({ "statistics": { "counts": { "all": 5, "closed": 4, "opened": 1 } } });
    let http = ScriptedHttp::new(vec![
        answer(304, &Value::Null),
        answer(200, &json!([gitlab(2, "active", true)])),
        answer(200, &statistics),
    ]);
    let grouped = choice(Milestones {
        actions: Some(vec![MilestoneAction::Overdue]),
        group: Some(true),
        ..Milestones::default()
    });
    let cursor = kept(&[("milestone", &[]), ("milestone-group", &[])]);
    let polled = poll_at(
        Forge::GitLab,
        &http,
        (Some(&cursor), after("2026-12-02")),
        (&grouped, None),
    )
    .unwrap();
    let identities: Vec<&str> = polled.found.events.iter().map(identity).collect();
    assert_eq!(
        identities,
        ["github.com/group/9/milestone/2/overdue/2026-12-01"]
    );
    assert_eq!(
        polled.found.events[0].title(),
        "Milestone v1.4 overdue: 4 of 5 closed, due 2026-12-01"
    );
    assert_eq!(
        ids(&polled.found)["milestone-group/overdue"],
        ["2:2026-12-01"]
    );
    assert_eq!(ids(&polled.found)["milestone/overdue"], Vec::<&str>::new());
}
