use herald_core::{
    Cursor, DestinationName, Entry, RouteId, Schedule, SourceName, State, Subscription,
};
use herald_ports::{Actions, Decisions, Pulls, Recalls};
use herald_ports::{
    Announcements, Cursors, Deliveries, Held, Inboxes, Ledger, Notes, Pins, Positions, Schedules,
    Store, StoreError, Subscriptions, Waiting,
};

use super::{Checks, GITHUB, NOON, announced, key};

fn refused<T>(checks: &mut Checks, answer: Result<T, StoreError>, held: Held, method: &str) {
    checks.ensure(
        answer.err() == Some(held.unsupported()),
        &format!("{method} was not refused as unsupported by a store keeping none of its kind"),
    );
}

/// Whether every method of each kind `store` keeps none of is refused as
/// unsupported, naming the kind.
pub(super) async fn unkept<S: Store>(store: &S, checks: &mut Checks) {
    if store.ledger().is_none() {
        ledger(&store.ledger(), checks).await;
    }
    if store.cursors().is_none() {
        cursors(&store.cursors(), checks).await;
    }
    if store.schedules().is_none() {
        schedules(&store.schedules(), checks).await;
    }
    if store.subscriptions().is_none() {
        subscriptions(&store.subscriptions(), checks).await;
    }
    if store.pins().is_none() {
        pins(&store.pins(), checks).await;
    }
    if store.deliveries().is_none() {
        deliveries(&store.deliveries(), checks).await;
    }
    if store.notes().is_none() {
        notes(&store.notes(), checks).await;
    }
    if store.announcements().is_none() {
        announcements(&store.announcements(), checks).await;
    }
    if store.inboxes().is_none() {
        inboxes(&store.inboxes(), checks).await;
    }
    if store.positions().is_none() {
        positions(&store.positions(), checks).await;
    }
    if store.decisions().is_none() {
        decisions(&store.decisions(), checks).await;
    }
    if store.waiting().is_none() {
        waiting(&store.waiting(), checks).await;
    }
    if store.pulls().is_none() {
        pulls(&store.pulls(), checks).await;
    }
    if store.recalls().is_none() {
        recalls(&store.recalls(), checks).await;
    }
    if store.actions().is_none() {
        actions(&store.actions(), checks).await;
    }
}

pub(super) async fn recalls(store: &impl Recalls, checks: &mut Checks) {
    refused(checks, store.advised().await, Held::Recalls, "advised");
    let named = super::recalls::named(&["unkept"]);
    refused(
        checks,
        store.put_advised(&named, None).await,
        Held::Recalls,
        "put_advised",
    );
}

pub(super) async fn pulls(store: &impl Pulls, checks: &mut Checks) {
    let kept = super::pulls::kept("1", 1, "unkept");
    let source = SourceName::new("unkept");
    refused(checks, store.pull(&kept.key).await, Held::Pulls, "pull");
    refused(
        checks,
        store.pulls_of(&kept.key.repository).await,
        Held::Pulls,
        "pulls_of",
    );
    refused(
        checks,
        store.put_pull(&kept, None).await,
        Held::Pulls,
        "put_pull",
    );
    refused(
        checks,
        store.expire_pulls(NOON).await,
        Held::Pulls,
        "expire_pulls",
    );
    refused(
        checks,
        store.reconciled(&source).await,
        Held::Pulls,
        "reconciled",
    );
    let reconciled = herald_core::Reconciled {
        repositories: std::collections::BTreeSet::new(),
        repository: None,
        at: None,
        validators: Vec::new(),
    };
    refused(
        checks,
        store.put_reconciled(&source, &reconciled, None).await,
        Held::Pulls,
        "put_reconciled",
    );
}

pub(super) async fn waiting(store: &impl Waiting, checks: &mut Checks) {
    let pending = super::pending::pending("unkept");
    refused(
        checks,
        store.pending(&pending.hold).await,
        Held::Pending,
        "pending",
    );
    refused(
        checks,
        store.every_pending().await,
        Held::Pending,
        "every_pending",
    );
    refused(
        checks,
        store.put_pending(&pending, None).await,
        Held::Pending,
        "put_pending",
    );
    refused(
        checks,
        store.drop_pending(&pending.hold).await,
        Held::Pending,
        "drop_pending",
    );
}

pub(super) async fn ledger(store: &impl Ledger, checks: &mut Checks) {
    let entry = Entry::new(key("unkept"));
    refused(checks, store.entry(&entry.key).await, Held::Ledger, "entry");
    refused(
        checks,
        store.put_entry(&entry, None).await,
        Held::Ledger,
        "put_entry",
    );
    refused(
        checks,
        store.expire_entries(NOON).await,
        Held::Ledger,
        "expire_entries",
    );
}

pub(super) async fn cursors(store: &impl Cursors, checks: &mut Checks) {
    let source = SourceName::new("unkept");
    refused(checks, store.cursor(&source).await, Held::Cursors, "cursor");
    refused(
        checks,
        store.put_cursor(&source, &Cursor::default(), None).await,
        Held::Cursors,
        "put_cursor",
    );
    refused(
        checks,
        store.listing(&source).await,
        Held::Cursors,
        "listing",
    );
    refused(
        checks,
        store
            .put_listing(&source, &super::members::listing(), None)
            .await,
        Held::Cursors,
        "put_listing",
    );
    refused(
        checks,
        store.members(&source).await,
        Held::Cursors,
        "members",
    );
    refused(
        checks,
        store
            .put_member(&source, &super::members::membership("1"), None)
            .await,
        Held::Cursors,
        "put_member",
    );
    refused(
        checks,
        store.forget_member(&source, "1").await,
        Held::Cursors,
        "forget_member",
    );
}

pub(super) async fn schedules(store: &impl Schedules, checks: &mut Checks) {
    let schedule = Schedule {
        key: key("unkept"),
        route: RouteId::new("unkept"),
        thread: "1".to_owned(),
        chore: herald_core::Chore::Lock,
        due: NOON,
    };
    refused(
        checks,
        store.every_schedule().await,
        Held::Schedules,
        "every_schedule",
    );
    refused(
        checks,
        store
            .put_schedules(std::slice::from_ref(&schedule), &[])
            .await,
        Held::Schedules,
        "put_schedules",
    );
}

pub(super) async fn subscriptions(store: &impl Subscriptions, checks: &mut Checks) {
    let subscription = Subscription {
        destination: DestinationName::new("unkept"),
        project: "herald".to_owned(),
        subscriber: "ana".to_owned(),
        inbox: None,
        pending: None,
    };
    refused(
        checks,
        store
            .subscribers(&subscription.destination, &subscription.project)
            .await,
        Held::Subscriptions,
        "subscribers",
    );
    refused(
        checks,
        store.subscribe(&subscription).await,
        Held::Subscriptions,
        "subscribe",
    );
    refused(
        checks,
        store.unsubscribe(&subscription).await,
        Held::Subscriptions,
        "unsubscribe",
    );
}

pub(super) async fn decisions(store: &impl Decisions, checks: &mut Checks) {
    let key = herald_core::Key::decided("unkept", "unkept", "{}");
    refused(
        checks,
        store.decision(&key).await,
        Held::Decisions,
        "decision",
    );
    let decided = herald_core::Decided {
        key,
        task: "unkept".to_owned(),
        subject: "unkept".to_owned(),
        answer: herald_core::Answer {
            model: "m".to_owned(),
            choice: None,
            probabilities: std::collections::BTreeMap::new(),
        },
        at: super::NOON,
    };
    refused(
        checks,
        store.put_decision(&decided).await,
        Held::Decisions,
        "put_decision",
    );
    refused(
        checks,
        store.expire_decisions(super::NOON).await,
        Held::Decisions,
        "expire_decisions",
    );
}

pub(super) async fn positions(store: &impl Positions, checks: &mut Checks) {
    let destination = DestinationName::new("unkept");
    refused(
        checks,
        store.position(&destination).await,
        Held::Positions,
        "position",
    );
    refused(
        checks,
        store.put_position(&destination, "s1").await,
        Held::Positions,
        "put_position",
    );
}

pub(super) async fn pins(store: &impl Pins, checks: &mut Checks) {
    let route = RouteId::new("unkept");
    refused(checks, store.pinned(&route).await, Held::Pins, "pinned");
    refused(
        checks,
        store.put_pinned(&route, "1").await,
        Held::Pins,
        "put_pinned",
    );
    refused(
        checks,
        store.forget_pinned(&route, "1").await,
        Held::Pins,
        "forget_pinned",
    );
}

pub(super) async fn deliveries(store: &impl Deliveries, checks: &mut Checks) {
    refused(
        checks,
        store.admit(GITHUB, "unkept", NOON).await,
        Held::Deliveries,
        "admit",
    );
    refused(
        checks,
        store.seen(GITHUB, "unkept").await,
        Held::Deliveries,
        "seen",
    );
    refused(
        checks,
        store.release(GITHUB, "unkept").await,
        Held::Deliveries,
        "release",
    );
    refused(
        checks,
        store.expire_deliveries(NOON).await,
        Held::Deliveries,
        "expire_deliveries",
    );
}

pub(super) async fn notes(store: &impl Notes, checks: &mut Checks) {
    let published = key("unkept");
    refused(checks, store.note(&published).await, Held::Notes, "note");
    refused(
        checks,
        store.put_note("herald", &published, "{}", NOON).await,
        Held::Notes,
        "put_note",
    );
    refused(
        checks,
        store.outbox("herald", None, 1).await,
        Held::Notes,
        "outbox",
    );
    refused(
        checks,
        store.expire_notes(NOON).await,
        Held::Notes,
        "expire_notes",
    );
    refused(
        checks,
        store.forget_note(&published).await,
        Held::Notes,
        "forget_note",
    );
}

pub(super) async fn announcements(store: &impl Announcements, checks: &mut Checks) {
    let latest = announced("unkept", "1.0.0");
    refused(
        checks,
        store.latest(&latest.project).await,
        Held::Latest,
        "latest",
    );
    refused(
        checks,
        store.every_latest().await,
        Held::Latest,
        "every_latest",
    );
    refused(
        checks,
        store.put_latest(&latest, None).await,
        Held::Latest,
        "put_latest",
    );
    refused(
        checks,
        store.forget_latest(&latest.project).await,
        Held::Latest,
        "forget_latest",
    );
}

pub(super) async fn inboxes(store: &impl Inboxes, checks: &mut Checks) {
    let published = key("unkept");
    let inbox = "https://social.example/inbox";
    refused(
        checks,
        store.delivery(&published, inbox).await,
        Held::Inboxes,
        "delivery",
    );
    refused(
        checks,
        store
            .put_delivery(&published, inbox, &State::Attempted { at: NOON })
            .await,
        Held::Inboxes,
        "put_delivery",
    );
    refused(
        checks,
        store.forget_deliveries(&published).await,
        Held::Inboxes,
        "forget_deliveries",
    );
}

pub(super) async fn actions(store: &impl Actions, checks: &mut Checks) {
    let key = super::actions::run("1", "10", "unkept");
    refused(checks, store.action(&key).await, Held::Actions, "action");
    let action = super::actions::action(key, herald_core::Reran::Calling);
    refused(
        checks,
        store.put_action(&action, None).await,
        Held::Actions,
        "put_action",
    );
    let key = super::actions::gestured(0);
    refused(checks, store.gesture(&key).await, Held::Actions, "gesture");
    refused(
        checks,
        store
            .put_gesture(&super::actions::gesture(key, None), None)
            .await,
        Held::Actions,
        "put_gesture",
    );
}
