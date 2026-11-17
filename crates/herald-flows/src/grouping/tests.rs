use herald_core::{
    Draft, Form, GROUP_MOST, GROUP_WINDOW, Glob, Revise, Sandbox, Templates, group_template,
};

use super::*;

const NOON: &str = "2026-11-17T12:00:00Z";

fn at(text: &str) -> Timestamp {
    text.parse().unwrap()
}

fn event(project: &str, identity: &str) -> Event {
    Event::normalise(Draft {
        identity: identity.to_owned(),
        kind: herald_core::Kind::RELEASE,
        project: project.to_owned(),
        attributes: herald_core::versioned("v1.2.0", false),
        title: format!("{project} v1.2.0"),
        body: "notes".to_owned(),
        links: std::collections::BTreeMap::new(),
        occurred: at(NOON),
    })
    .unwrap()
}

fn held(project: &str, identity: &str) -> HeldEvent {
    HeldEvent {
        source: SourceName::new("org"),
        member: None,
        event: event(project, identity),
    }
}

fn pending(events: Vec<HeldEvent>) -> Pending {
    Pending {
        hold: Hold::Group {
            route: RouteId::new("train"),
            key: "v1.2.0".to_owned(),
        },
        events,
        opened: at(NOON),
        due: at("2026-11-17T12:30:00Z"),
        closed: None,
        landed: false,
    }
}

fn grouping(mode: GroupMode, until: GroupUntil) -> Grouping {
    Grouping {
        key: "{{ event.attributes.version }}".to_owned(),
        window: GROUP_WINDOW,
        lead: vec![Glob::new("core").unwrap()],
        mode,
        until,
        late: GroupLate::Post,
        most: GROUP_MOST,
    }
}

fn identities(pending: &Pending) -> Vec<(&str, &str)> {
    pending
        .events
        .iter()
        .map(|held| (held.event.project(), held.event.identity().as_str()))
        .collect()
}

#[test]
fn a_group_takes_a_member_in_place_of_its_identity_or_beside_the_rest_up_to_its_most() {
    let open = pending(vec![held("core", "1/v1"), held("cli", "2/v1")]);
    let Taking::Took(replaced) = took(open.clone(), held("renamed", "2/v1"), 2) else {
        panic!("a member of its identity is replaced");
    };
    assert_eq!(
        identities(&replaced),
        [("core", "1/v1"), ("renamed", "2/v1")]
    );
    let Taking::Took(joined) = took(open.clone(), held("web", "3/v1"), 3) else {
        panic!("a member joins below the most");
    };
    assert_eq!(
        identities(&joined),
        [("core", "1/v1"), ("cli", "2/v1"), ("web", "3/v1")]
    );
    assert!(matches!(
        took(open.clone(), held("web", "3/v1"), 2),
        Taking::Full
    ));
    let mut edit = held("web", "3/v1");
    edit.event = edit.event.marked_edited();
    assert!(matches!(took(open.clone(), edit, 3), Taking::Unedited));
    let mut edit = held("cli", "2/v1");
    edit.event = edit.event.marked_edited();
    assert!(matches!(took(open, edit, 2), Taking::Took(_)));
}

#[test]
fn a_member_is_late_once_a_held_group_closed_or_an_edited_one_landed_and_its_window_passed() {
    let open = pending(Vec::new());
    let after = at("2026-11-17T12:30:00Z");
    assert!(!late_at(GroupMode::Hold, &open, after));
    assert!(!late_at(GroupMode::Edit, &open, after));
    let closed = Pending {
        closed: Some(at(NOON)),
        ..open.clone()
    };
    assert!(late_at(GroupMode::Hold, &closed, at(NOON)));
    assert!(!late_at(GroupMode::Edit, &closed, after));
    let landed = Pending {
        landed: true,
        ..open
    };
    assert!(!late_at(GroupMode::Hold, &landed, after));
    assert!(!late_at(
        GroupMode::Edit,
        &landed,
        at("2026-11-17T12:29:59Z")
    ));
    assert!(late_at(GroupMode::Edit, &landed, after));
}

#[test]
fn a_late_member_is_skipped_joined_where_the_post_edits_or_announced_alone() {
    assert_eq!(late_as(GroupLate::Skip, Revise::Edit), Late::Skip);
    assert_eq!(late_as(GroupLate::Edit, Revise::Edit), Late::Join);
    assert_eq!(late_as(GroupLate::Edit, Revise::Post), Late::Alone);
    assert_eq!(late_as(GroupLate::Post, Revise::Edit), Late::Alone);
}

#[test]
fn a_held_group_is_announced_once_due_closed_or_led_where_it_waits_for_its_lead() {
    let open = pending(Vec::new());
    let before = at("2026-11-17T12:29:59Z");
    let window = grouping(GroupMode::Hold, GroupUntil::Window);
    let lead = grouping(GroupMode::Hold, GroupUntil::Lead);
    let core = event("core", "1/v1");
    let cli = event("cli", "2/v1");
    assert!(!due_now(&window, &open, &core, before));
    assert!(due_now(&window, &open, &core, at("2026-11-17T12:30:00Z")));
    assert!(due_now(&lead, &open, &core, before));
    assert!(!due_now(&lead, &open, &cli, before));
    let closed = Pending {
        closed: Some(at(NOON)),
        ..open.clone()
    };
    assert!(due_now(&window, &closed, &cli, before));
    assert!(due_now(
        &grouping(GroupMode::Edit, GroupUntil::Window),
        &open,
        &cli,
        before
    ));
}

#[test]
fn a_round_announces_a_group_not_yet_landed_once_it_closed_or_is_due() {
    let open = pending(Vec::new());
    let before = at("2026-11-17T12:29:59Z");
    let due = at("2026-11-17T12:30:00Z");
    assert!(!flushed_at(&open, before));
    assert!(flushed_at(&open, due));
    let closed = Pending {
        closed: Some(at(NOON)),
        ..open.clone()
    };
    assert!(flushed_at(&closed, before));
    let landed = Pending {
        landed: true,
        ..closed
    };
    assert!(!flushed_at(&landed, due));
}

#[test]
fn a_group_fits_a_store_that_keeps_its_record_with_room_for_its_seal() {
    let one = pending(vec![held("core", "1/v1")]);
    let needed = one.bytes() + Remembers::REST;
    assert!(fits(&one, None));
    assert!(fits(&one, Some(needed)));
    assert!(!fits(&one, Some(needed - 1)));
}

fn templates(key: &str) -> Templates {
    let route = RouteId::new("train");
    Templates::compile(
        &[(&group_template(&route), key, Form::Line)],
        Sandbox::DEFAULT,
    )
    .unwrap()
}

#[test]
fn a_key_renders_on_one_line_and_names_no_group_empty_or_past_its_most() {
    let route = RouteId::new("train");
    let release = event("core", "1/v1");
    assert_eq!(
        key_of(
            &templates("{{ event.attributes.version }}\n  {{ event.project }}"),
            &route,
            &release
        ),
        Ok(Some("v1.2.0 core".to_owned()))
    );
    assert_eq!(
        key_of(&templates("{% if false %}x{% endif %}"), &route, &release),
        Ok(None)
    );
    let longest = "k".repeat(GROUP_KEY_MOST);
    assert_eq!(
        key_of(&templates(&longest), &route, &release),
        Ok(Some(longest.clone()))
    );
    assert_eq!(
        key_of(&templates(&format!("{longest}k")), &route, &release),
        Ok(None)
    );
    let Err(Stop::Template(reason)) = key_of(&templates("{{ event.nothing }}"), &route, &release)
    else {
        panic!("a key that does not render stops the route");
    };
    assert!(reason.contains("route/train/group"), "{reason}");
}

#[test]
fn a_landed_outcome_stopped_keeps_its_steps() {
    let reason = Stop::Store("gone".to_owned());
    assert_eq!(
        landed_but(
            Outcome::Sent {
                steps: vec![herald_core::Step::Post]
            },
            reason.clone()
        ),
        Outcome::Stopped {
            landed: vec![herald_core::Step::Post],
            remaining: Vec::new(),
            reason: reason.clone(),
        }
    );
    assert_eq!(
        landed_but(Outcome::AlreadyAnnounced, reason.clone()),
        Outcome::Stopped {
            landed: Vec::new(),
            remaining: Vec::new(),
            reason,
        }
    );
    assert_eq!(
        raced("v1"),
        Stop::Store("another run wrote the group v1 first".to_owned())
    );
}

#[test]
fn a_group_is_seen_as_its_strictest_member_and_by_anyone_where_none_is_bounded() {
    use herald_core::Visibility::{Internal, Private, Public};
    let seen = |visibilities: &[Option<herald_core::Visibility>]| {
        let events = visibilities
            .iter()
            .enumerate()
            .map(|(at, visibility)| {
                let mut member = held("core", &format!("{at}/v1"));
                if let Some(visibility) = visibility {
                    member.event = member.event.seen_as(*visibility);
                }
                member
            })
            .collect();
        strictest(&pending(events))
    };
    assert_eq!(seen(&[]), Public);
    assert_eq!(seen(&[None]), Public);
    assert_eq!(seen(&[Some(Public), None, Some(Internal)]), Internal);
    assert_eq!(seen(&[Some(Private), Some(Public)]), Private);
}
