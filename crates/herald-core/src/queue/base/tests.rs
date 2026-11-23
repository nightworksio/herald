use super::*;

fn failed(names: &[&str]) -> Checks {
    Checks::Failed(names.iter().map(|name| (*name).to_owned()).collect())
}

fn key() -> BaseKey {
    BaseKey {
        source: SourceName::new("s"),
        repository: "o/r".to_owned(),
        branch: "main".to_owned(),
    }
}

fn part(etag: Option<&str>, modified: Option<&str>, checks: Checks) -> KeptCheck {
    KeptCheck {
        validators: Validators {
            etag: etag.map(str::to_owned),
            last_modified: modified.map(str::to_owned),
        },
        checks,
    }
}

fn based(repository: &str, visibility: Visibility, read: BaseRead) -> Based {
    Based {
        repository: repository.to_owned(),
        visibility,
        branch: "main".to_owned(),
        read,
    }
}

#[test]
fn checks_of_several_requests_fail_where_any_failed_each_name_once() {
    assert_eq!(
        joined([failed(&["a", "b"]), Checks::Pending, failed(&["b", "c"])]),
        failed(&["a", "b", "c"])
    );
    assert_eq!(joined([Checks::Passed, failed(&[])]), failed(&[]));
    assert_eq!(joined([Checks::Passed, Checks::Pending]), Checks::Pending);
    assert_eq!(joined([Checks::Absent, Checks::Pending]), Checks::Pending);
    assert_eq!(joined([Checks::Absent, Checks::Passed]), Checks::Passed);
    assert_eq!(joined([Checks::Absent, Checks::Absent]), Checks::Absent);
    assert_eq!(joined([]), Checks::Absent);
}

#[test]
fn a_base_branch_read_is_kept_only_with_a_validator_and_bounded_names() {
    let at = Timestamp::UNIX_EPOCH;
    let kept = KeptBase::of(key(), vec![part(Some("\"e\""), None, Checks::Passed)], at).unwrap();
    assert_eq!(
        (kept.form, kept.at, kept.key.clone()),
        (KEPT_BASE, at, key())
    );
    assert!(KeptBase::of(key(), vec![part(None, Some("then"), Checks::Pending)], at).is_some());
    assert_eq!(
        KeptBase::of(key(), vec![part(None, None, Checks::Passed)], at),
        None
    );
    let mixed = vec![
        part(Some("\"e\""), None, Checks::Passed),
        part(None, None, Checks::Passed),
    ];
    assert_eq!(KeptBase::of(key(), mixed, at), None);
    assert_eq!(KeptBase::of(key(), Vec::new(), at), None);
    let named = |count: usize| {
        let names: Vec<String> = (0..count).map(|index| index.to_string()).collect();
        vec![part(Some("\"e\""), None, Checks::Failed(names))]
    };
    assert!(KeptBase::of(key(), named(MOST_KEPT_FAILED - 1), at).is_some());
    assert!(KeptBase::of(key(), named(MOST_KEPT_FAILED), at).is_some());
    assert_eq!(KeptBase::of(key(), named(MOST_KEPT_FAILED + 1), at), None);
}

#[test]
fn a_kept_base_branch_fits_only_a_read_of_as_many_requests_in_this_form() {
    let at = Timestamp::UNIX_EPOCH;
    let parts = vec![
        part(Some("\"1\""), None, Checks::Passed),
        part(Some("\"2\""), None, failed(&["lint"])),
    ];
    let kept = KeptBase::of(key(), parts.clone(), at).unwrap();
    assert_eq!(kept.fitting(2), Some(parts.as_slice()));
    assert_eq!(kept.fitting(1), None);
    assert_eq!(kept.fitting(3), None);
    let older = KeptBase {
        form: KEPT_BASE + 1,
        ..kept
    };
    assert_eq!(older.fitting(2), None);
}

#[test]
fn a_base_branch_holds_where_a_check_failed_or_it_was_not_read() {
    let read = |read| based("o/r", Visibility::Public, read).holds();
    assert!(read(BaseRead::Checked(failed(&["build"]))));
    assert!(read(BaseRead::Checked(failed(&[]))));
    assert!(read(BaseRead::Unread("refused".to_owned())));
    assert!(!read(BaseRead::Checked(Checks::Passed)));
    assert!(!read(BaseRead::Checked(Checks::Pending)));
    assert!(!read(BaseRead::Checked(Checks::Absent)));
}

#[test]
fn a_surface_is_shown_the_base_branches_it_may_see_and_the_rest_are_withheld() {
    let passed = BaseRead::Checked(Checks::Passed);
    let bases = [
        based("o/public", Visibility::Public, passed.clone()),
        based("o/inner", Visibility::Internal, passed.clone()),
        based("o/vault", Visibility::Private, passed),
    ];
    let names = |seen: &[&Based]| -> Vec<String> {
        seen.iter().map(|based| based.repository.clone()).collect()
    };
    let public = Bases::seen(&bases, Visibility::Public);
    assert_eq!(names(&public.shown), ["o/public"]);
    assert_eq!(names(&public.withheld), ["o/inner", "o/vault"]);
    let internal = Bases::seen(&bases, Visibility::Internal);
    assert_eq!(names(&internal.shown), ["o/public", "o/inner"]);
    assert_eq!(names(&internal.withheld), ["o/vault"]);
    let private = Bases::seen(&bases, Visibility::Private);
    assert_eq!(private.shown.len(), 3);
    assert_eq!(private.withheld, Vec::<&Based>::new());
}
