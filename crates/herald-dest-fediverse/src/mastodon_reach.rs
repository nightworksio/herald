use std::future::ready;

use herald_core::{Announcement, Chore, Delivery, Entry, Profile, Sent, Step};
use herald_kinds::{Audience, Probe, Reach, Recorded, Signing};
use herald_ports::{Base, Boxed, DynHttp, DynMarkers, Erased, HttpError, Response};

use crate::markers::MastodonMarkers;
use crate::mastodon::Mastodon;
use crate::read::{json, text};
use crate::request::cannot;

impl Reach for Mastodon {
    fn profile(&self) -> Profile {
        Self::profile(self)
    }

    fn base(&self) -> Base<'_> {
        Base::At(Self::api(self))
    }

    fn send<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        _: Signing<'a>,
        step: &'a Step,
        announcement: &'a Announcement,
        _: &'a Delivery,
        entry: &'a Entry,
    ) -> Boxed<'a, Result<Sent, HttpError>> {
        Box::pin(async move { Self::send(self, &Erased(http), step, announcement, entry).await })
    }

    fn housekeep<'a>(
        &'a self,
        _: &'a dyn DynHttp,
        _: Chore,
        _: &'a str,
    ) -> Boxed<'a, Result<Sent, HttpError>> {
        Box::pin(ready(Ok(Sent::Refused(
            cannot("archive or lock a thread").to_string(),
        ))))
    }

    fn posts<'a>(&'a self, http: &'a dyn DynHttp) -> Box<dyn DynMarkers + 'a> {
        Box::new(MastodonMarkers::new(self, Erased(http)))
    }

    fn audience<'a>(&'a self, http: &'a dyn DynHttp) -> Boxed<'a, Audience> {
        Box::pin(async move { crate::audience::audience(self, &Erased(http)).await })
    }

    fn keeps_edits(&self) -> bool {
        true
    }

    fn declines(&self, answer: &Response) -> bool {
        text(&json(answer), "error").is_some()
    }

    fn probe<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        _: &'a Delivery,
        _: Recorded,
    ) -> Boxed<'a, Vec<Probe>> {
        Box::pin(async move { crate::probe::probe(self, &Erased(http)).await })
    }
}
