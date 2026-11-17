use super::*;

/// The pin `route` keeps forgotten where it is `post`, or as `faulty`'s
/// fault says.
pub(super) async fn pinned(faulty: &Faulty, route: &RouteId, post: &str) -> Result<(), StoreError> {
    faulty.refuses()?;
    match faulty.fault {
        Fault::KeepsForgottenPins => Ok(()),
        Fault::ForgetsAnyPin => match faulty.inner.pinned(route).await? {
            Some(pinned) => faulty.inner.forget_pinned(route, &pinned).await,
            None => Ok(()),
        },
        Fault::ForgetsOtherPins => {
            faulty
                .inner
                .forget_pinned(&RouteId::new("other"), "3")
                .await?;
            faulty.inner.forget_pinned(route, post).await
        }
        _ => faulty.inner.forget_pinned(route, post).await,
    }
}

/// The Note of `key` forgotten, or kept as `faulty`'s fault says.
pub(super) async fn note(faulty: &Faulty, key: &Key) -> Result<(), StoreError> {
    faulty.refuses()?;
    match faulty.fault {
        Fault::KeepsForgottenNotes => return Ok(()),
        Fault::HidesForgottenNotes => {
            crate::lock(&faulty.hidden).push(*key);
            return Ok(());
        }
        _ => {}
    }
    faulty.inner.forget_note(key).await
}

#[test]
fn a_store_that_forgets_wrongly_is_refused() {
    refused(&[
        (
            Fault::KeepsForgottenPins,
            "a route's pin was not forgotten as its post",
        ),
        (
            Fault::ForgetsOtherPins,
            "another route's pin was forgotten with it",
        ),
        (
            Fault::ForgetsAnyPin,
            "a route's pin was forgotten as another post",
        ),
        (
            Fault::KeepsForgottenNotes,
            "a forgotten note was still served",
        ),
        (
            Fault::HidesForgottenNotes,
            "a forgotten note stayed in its actor's outbox",
        ),
        (
            Fault::KeepsForgottenLatest,
            "a project whose latest was forgotten was still kept",
        ),
        (
            Fault::ForgetsEveryLatest,
            "another project's latest was forgotten with it",
        ),
    ]);
}
