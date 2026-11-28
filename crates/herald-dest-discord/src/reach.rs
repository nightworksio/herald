use herald_core::{Announcement, Chore, Delivery, Entry, Profile, Sent, Step};
use herald_kinds::{Audience, Probe, Probing, Reach, Signing};
use herald_ports::{Base, Boxed, DynHttp, DynMarkers, Erased, HttpError, Response};
use serde_json::Value;

use crate::discord::Discord;
use crate::markers::DiscordMarkers;
use crate::read::json;

impl Reach for Discord {
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
        delivery: &'a Delivery,
        entry: &'a Entry,
    ) -> Boxed<'a, Result<Sent, HttpError>> {
        Box::pin(async move {
            Self::send(self, &Erased(http), step, announcement, delivery, entry).await
        })
    }

    fn housekeep<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        chore: Chore,
        thread: &'a str,
    ) -> Boxed<'a, Result<Sent, HttpError>> {
        Box::pin(async move { Self::housekeep(self, &Erased(http), chore, thread).await })
    }

    fn posts<'a>(&'a self, http: &'a dyn DynHttp) -> Box<dyn DynMarkers + 'a> {
        Box::new(DiscordMarkers::new(self, Erased(http)))
    }

    fn audience<'a>(&'a self, http: &'a dyn DynHttp) -> Boxed<'a, Audience> {
        Box::pin(async move { crate::audience::audience(self, &Erased(http)).await })
    }

    fn keeps_edits(&self) -> bool {
        false
    }

    fn declines(&self, answer: &Response) -> bool {
        json(answer).get("code").is_some_and(Value::is_u64)
    }

    fn probe<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        delivery: &'a Delivery,
        probing: Probing,
    ) -> Boxed<'a, Vec<Probe>> {
        Box::pin(async move {
            crate::probe::probe(self, &Erased(http), (delivery, probing.recorded)).await
        })
    }
}
