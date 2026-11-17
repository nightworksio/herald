use herald_sources_forge::Discovery;

use super::*;

/// The listing `gate` keeps, marked stale as a delivery marks it.
fn staled(store: &Failing) {
    let gate = SourceName::new("gate");
    let kept = pollster::block_on(store.inner.listing(&gate))
        .unwrap()
        .unwrap();
    let stale = Listing {
        stale: true,
        ..kept.value
    };
    pollster::block_on(store.inner.put_listing(&gate, &stale, Some(kept.version))).unwrap();
}

#[test]
fn an_owner_discovering_by_push_lists_only_where_its_listing_is_missing_stale_or_of_another_selection()
 {
    let store = healthy();
    let pushed = owned(|options| options.discovery = Some(Discovery::Push));
    joined(&store, &pushed);
    let later = NOON + SignedDuration::from_hours(1);
    let unlisted =
        || Script::answering(&[(&released("herald"), 304, ""), (&released("gate"), 304, "")]);
    let quiet = [
        listed(2, &[], &[]),
        member("herald", Sweep::Unchanged),
        member("gate", Sweep::Unchanged),
    ];
    assert_eq!(polled(&pushed, &store, &unlisted(), later), quiet);
    staled(&store);
    let relisted = Script::answering(&[
        (LISTING, 304, ""),
        (PROBE, 200, "[]"),
        (&released("herald"), 304, ""),
        (&released("gate"), 304, ""),
    ]);
    assert_eq!(polled(&pushed, &store, &relisted, later), quiet);
    assert!(!listing(&store).unwrap().stale);
    assert_eq!(polled(&pushed, &store, &unlisted(), later), quiet);
    let narrowed = owned(|options| {
        options.discovery = Some(Discovery::Push);
        options.exclude = vec!["gate".to_owned()];
    });
    let two = repositories(&[(1, "herald"), (2, "gate")]);
    let whole = Script::answering(&[(LISTING, 200, &two), (&released("herald"), 304, "")]);
    assert_eq!(
        polled(&narrowed, &store, &whole, later),
        [
            listed(1, &[], &["gate"]),
            member("herald", Sweep::Unchanged)
        ]
    );
}

#[test]
fn an_owner_listing_each_round_lists_though_its_listing_is_fresh_and_clears_it_once_stale() {
    let store = healthy();
    let config = owned(|_| {});
    joined(&store, &config);
    staled(&store);
    let later = NOON + SignedDuration::from_hours(1);
    let relisted = Script::answering(&[
        (LISTING, 304, ""),
        (PROBE, 200, "[]"),
        (&released("herald"), 304, ""),
        (&released("gate"), 304, ""),
    ]);
    polled(&config, &store, &relisted, later);
    assert!(!listing(&store).unwrap().stale);
}
