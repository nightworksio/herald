use std::future::ready;

use herald_core::{Announcement, Chore, Declined, Delivery, Entry, Profile, Sent, Step};
use herald_kinds::{Audience, Probe, Reach, Recorded, Signing, UNREAD, UnreadPosts};
use herald_ports::{Base, Boxed, DynHttp, DynMarkers, Erased, HttpError, Response};

use crate::teams::{TEAMS, Teams, UNCHECKED, Way, unaddressed};

/// What doctor `--online` says of a webhook it cannot ask without posting.
const UNASKED: &str = "a Workflows webhook cannot be asked whether it takes its URL without posting to it; herald doctor --online --post posts one test message there";

impl Reach for Teams {
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
        entry: &'a Entry,
    ) -> Boxed<'a, Result<Sent, HttpError>> {
        Box::pin(async move {
            Self::send(
                self,
                &Erased(http),
                (step, announcement, entry),
                signing.now,
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
            Declined::Cannot {
                platform: TEAMS,
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
        matches!(self.way, Way::Bot(_))
    }

    fn declines(&self, answer: &Response) -> bool {
        matches!(self.way, Way::Bot(_)) && crate::bot::declines(answer)
    }

    fn probe<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        _: &'a Delivery,
        _: Recorded,
    ) -> Boxed<'a, Vec<Probe>> {
        Box::pin(async move {
            match &self.way {
                Way::Bot(bot) => bot.probe(&Erased(http)).await,
                Way::Webhook { webhook, origin } => vec![match origin {
                    Some(_) => Probe::Said(UNASKED.to_owned()),
                    None => Probe::Credential(unaddressed(webhook)),
                }],
            }
        })
    }
}
