use std::future::ready;

use herald_core::{Announcement, Chore, Declined, Delivery, Entry, Profile, Sent, Step};
use herald_kinds::{Audience, Probe, Probing, Reach, Signing, UnreadPosts};
use herald_ports::{Base, Boxed, DynHttp, DynMarkers, HttpError, Response};

use crate::email::{EMAIL, Email, UNCHECKED};

/// Why herald reads back no message it mailed.
const UNREAD: &str = "a message mailed cannot be read back";

impl Reach for Email {
    fn profile(&self) -> Profile {
        Self::profile(self)
    }

    fn base(&self) -> Base<'_> {
        Self::base(self)
    }

    fn send<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        signing: Signing<'a>,
        step: &'a Step,
        announcement: &'a Announcement,
        _: &'a Delivery,
        _: &'a Entry,
    ) -> Boxed<'a, Result<Sent, HttpError>> {
        Box::pin(Self::send(self, http, step, announcement, signing.now))
    }

    fn housekeep<'a>(
        &'a self,
        _: &'a dyn DynHttp,
        _: Chore,
        _: &'a str,
    ) -> Boxed<'a, Result<Sent, HttpError>> {
        Box::pin(ready(Ok(Sent::Refused(
            Declined::Cannot {
                platform: EMAIL,
                what: "archive or lock a thread",
            }
            .to_string(),
        ))))
    }

    fn posts<'a>(&'a self, _: &'a dyn DynHttp) -> Box<dyn DynMarkers + 'a> {
        Box::new(UnreadPosts(UNREAD))
    }

    fn audience<'a>(&'a self, _: &'a dyn DynHttp) -> Boxed<'a, Audience> {
        Box::pin(ready(Audience::Unread(UNCHECKED.to_owned())))
    }

    fn keeps_edits(&self) -> bool {
        true
    }

    fn declines(&self, answer: &Response) -> bool {
        self.carrier().declines(answer)
    }

    fn probe<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        _: &'a Delivery,
        probing: Probing,
    ) -> Boxed<'a, Vec<Probe>> {
        Box::pin(self.carrier().probe(http, probing.now))
    }
}
