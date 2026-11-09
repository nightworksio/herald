use herald_core::{Announcement, Chore, Delivery, Entry, Profile, Sent, Step};
use herald_kinds::{Reach, Signing};
use herald_ports::{Boxed, DynHttp, DynMarkers, Erased, HttpError};

use crate::discord::Discord;
use crate::markers::DiscordMarkers;

impl Reach for Discord {
    fn profile(&self) -> Profile {
        Self::profile(self)
    }

    fn api(&self) -> &str {
        Self::api(self)
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
}
