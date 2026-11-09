use std::any::Any;
use std::future::ready;

use herald_core::{Announcement, Chore, Delivery, Entry, Profile, Sent, Step};
use herald_kinds::{Reach, Signing, Unread, same};
use herald_ports::{Boxed, DynHttp, DynMarkers, DynSigner, Erased, HttpError};

use super::deliver::ActivityPub;

/// Why herald's own actor reads nothing back: it serves its Notes rather
/// than posting them somewhere it could read.
const UNREAD: &str = "herald's ActivityPub actor reads no posts back";

impl Reach for ActivityPub {
    fn profile(&self) -> Profile {
        Self::profile(self)
    }

    fn api(&self) -> &str {
        Self::api(self)
    }

    fn send<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        signing: Signing<'a>,
        step: &'a Step,
        announcement: &'a Announcement,
        _: &'a Delivery,
        entry: &'a Entry,
    ) -> Boxed<'a, Result<Sent, HttpError>> {
        Box::pin(async move {
            let signer: Erased<'_, dyn DynSigner + '_> = Erased(signing.signer);
            Self::send(
                self,
                &Erased(http),
                &signer,
                signing.now,
                step,
                announcement,
                entry,
            )
            .await
        })
    }

    fn housekeep<'a>(
        &'a self,
        _: &'a dyn DynHttp,
        _: Chore,
        _: &'a str,
    ) -> Boxed<'a, Result<Sent, HttpError>> {
        Box::pin(ready(Ok(Sent::Refused(
            "herald's ActivityPub actor cannot archive or lock a thread".to_owned(),
        ))))
    }

    fn posts<'a>(&'a self, _: &'a dyn DynHttp) -> Box<dyn DynMarkers + 'a> {
        Box::new(Unread(UNREAD))
    }

    fn follows(&self) -> Option<&str> {
        Some(self.actor())
    }

    fn published(&self, announcement: &Announcement) -> Option<(&str, String)> {
        Some((self.actor(), self.note(announcement)))
    }

    fn admits(&self, follower: &str) -> bool {
        self.policy().admits(follower)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn same(&self, other: &dyn Reach) -> bool {
        same(self, other)
    }
}
