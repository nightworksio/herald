use herald_core::{
    Cursor, DestinationName, Entry, RouteId, Schedule, SourceName, State, Subscription,
};
use herald_ports::{
    Announcements, Cursors, Deliveries, Held, Inboxes, Ledger, Notes, Pins, Schedules, Store,
    StoreError, Subscriptions,
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

pub(super) async fn pins(store: &impl Pins, checks: &mut Checks) {
    let route = RouteId::new("unkept");
    refused(checks, store.pinned(&route).await, Held::Pins, "pinned");
    refused(
        checks,
        store.put_pinned(&route, "1").await,
        Held::Pins,
        "put_pinned",
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
