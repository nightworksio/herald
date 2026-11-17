use super::*;
use crate::event::tests::draft;
use crate::event::{Draft, Event};
use crate::signal::Signal;

fn of(project: &str) -> Event {
    Event::normalise(Draft {
        identity: format!("github.com/{project}/v1.0.0"),
        project: project.to_owned(),
        ..draft()
    })
    .unwrap()
}

fn grouping(lead: &[&str]) -> Grouping {
    Grouping {
        key: "{{ event.attributes.version }}".to_owned(),
        window: GROUP_WINDOW,
        lead: lead.iter().map(|glob| Glob::new(glob).unwrap()).collect(),
        mode: GroupMode::default(),
        until: GroupUntil::default(),
        late: GroupLate::default(),
        most: GROUP_MOST,
    }
}

fn projects(ordered: &[&Event]) -> Vec<String> {
    ordered
        .iter()
        .map(|event| event.project().to_owned())
        .collect()
}

#[test]
fn the_first_lead_glob_that_matches_a_member_leads_and_the_rest_keep_their_order() {
    let events = [of("plugin-a"), of("core"), of("plugin-b"), of("cli")];
    let led = grouping(&["nothing", "CORE", "plugin-*"]);
    assert_eq!(
        projects(&led.ordered(&events)),
        ["core", "plugin-a", "plugin-b", "cli"]
    );
    let by_wildcard = grouping(&["plugin-*", "core"]);
    assert_eq!(
        projects(&by_wildcard.ordered(&events)),
        ["plugin-a", "core", "plugin-b", "cli"]
    );
    let last = grouping(&["cli"]);
    assert_eq!(
        projects(&last.ordered(&events)),
        ["cli", "plugin-a", "core", "plugin-b"]
    );
}

#[test]
fn the_first_member_leads_where_no_lead_glob_matches() {
    let events = [of("plugin-a"), of("core")];
    assert_eq!(
        projects(&grouping(&[]).ordered(&events)),
        ["plugin-a", "core"]
    );
    assert_eq!(
        projects(&grouping(&["other"]).ordered(&events)),
        ["plugin-a", "core"]
    );
    assert_eq!(grouping(&["core"]).ordered(&[]), Vec::<&Event>::new());
}

#[test]
fn a_group_is_held_for_half_an_hour_at_most_a_week_and_announced_as_its_key() {
    assert_eq!(GROUP_WINDOW, SignedDuration::from_secs(1_800));
    assert_eq!(crate::LONGEST_HOLD, SignedDuration::from_secs(604_800));
    assert_eq!(GroupMode::default(), GroupMode::Hold);
    assert_eq!(group_identity("v1.2.0").as_str(), "group/v1.2.0");
    assert_ne!(group_identity("v1.2.0"), group_identity("v1.2.1"));
}

#[test]
fn a_member_leads_where_a_lead_glob_names_its_project() {
    let led = grouping(&["plugin-*", "core"]);
    assert!(led.leads(&of("core")));
    assert!(led.leads(&of("Plugin-A")));
    assert!(!led.leads(&of("cli")));
    assert!(!grouping(&[]).leads(&of("core")));
}

#[test]
fn a_group_takes_25_members_at_most_100_keyed_in_256_bytes_and_announces_late_ones_alone() {
    assert_eq!(GROUP_MOST, 25);
    assert_eq!(GROUP_MOST_ALLOWED, 100);
    assert_eq!(GROUP_KEY_MOST, 256);
    assert_eq!(GroupUntil::default(), GroupUntil::Window);
    assert_eq!(GroupLate::default(), GroupLate::Post);
}

fn signalled(signal: Signal, body: &str) -> Event {
    let mut attributes = crate::versioned("v1.0.0", false);
    attributes.insert(
        crate::SIGNAL.to_owned(),
        crate::Attribute::Text(signal.name().to_owned()),
    );
    Event::normalise(Draft {
        attributes,
        body: body.to_owned(),
        ..draft()
    })
    .unwrap()
}

#[test]
fn a_member_shows_its_signal_title_and_body() {
    let tag = signalled(Signal::Tag, "tagged");
    assert_eq!(shown(&tag).signal, Some(Signal::Tag));
    assert_eq!(shown(&tag), shown(&signalled(Signal::Tag, "tagged")));
    assert_ne!(shown(&tag), shown(&signalled(Signal::Tag, "other")));
    let retitled = Event::normalise(Draft {
        title: "renamed".to_owned(),
        ..draft()
    })
    .unwrap();
    assert_ne!(
        shown(&retitled).digest,
        shown(&of("php-mutation-gate")).digest
    );
}

#[test]
fn a_member_changes_as_its_release_or_its_edit_and_not_as_itself_again() {
    let tag = shown(&signalled(Signal::Tag, "tagged"));
    let release = signalled(Signal::Release, "notes");
    assert_eq!(Change::of(&tag, &signalled(Signal::Tag, "tagged")), None);
    assert_eq!(Change::of(&tag, &release), Some(Change::Released));
    assert_eq!(Change::of(&tag, &signalled(Signal::Tag, "moved")), None);
    let was = shown(&release);
    assert_eq!(Change::of(&was, &release), None);
    let edited = signalled(Signal::Release, "new notes").marked_edited();
    assert_eq!(Change::of(&was, &edited), Some(Change::Edited));
    assert_eq!(
        Change::of(&was, &signalled(Signal::Release, "new notes")),
        None
    );
}

#[test]
fn a_changed_group_post_is_edited_posted_or_kept_as_its_route_says() {
    let delivery = |late_release, edits| Delivery {
        late_release,
        edits,
        ..Delivery::default()
    };
    let cases = [
        (
            Change::Joined,
            LateRelease::Skip,
            Edits::None,
            true,
            Revise::Edit,
        ),
        (
            Change::Joined,
            LateRelease::Skip,
            Edits::None,
            false,
            Revise::Post,
        ),
        (
            Change::Released,
            LateRelease::Edit,
            Edits::None,
            true,
            Revise::Edit,
        ),
        (
            Change::Released,
            LateRelease::Edit,
            Edits::None,
            false,
            Revise::Post,
        ),
        (
            Change::Released,
            LateRelease::Post,
            Edits::None,
            true,
            Revise::Post,
        ),
        (
            Change::Released,
            LateRelease::Skip,
            Edits::None,
            true,
            Revise::Keep,
        ),
        (
            Change::Edited,
            LateRelease::Edit,
            Edits::Release,
            true,
            Revise::Edit,
        ),
        (
            Change::Edited,
            LateRelease::Edit,
            Edits::Release,
            false,
            Revise::Keep,
        ),
        (
            Change::Edited,
            LateRelease::Edit,
            Edits::None,
            true,
            Revise::Keep,
        ),
    ];
    for (change, late_release, edits, editable, revised) in cases {
        assert_eq!(
            change.revise(&delivery(late_release, edits), editable),
            revised,
            "{change:?} {late_release:?} {edits:?} {editable}"
        );
    }
}
