use std::sync::Arc;

use herald_core::{Cursor, Kind, Polled, SecretRef};
use herald_kinds::{Fault, Poll, Polling, SourceConfig, SourceKind, Unpolled};
use herald_ports::{Boxed, DynHttp, Erased, Http as _, Secret};

use crate::feed::untitled;
use crate::{Feed, FeedOptions};

/// An RSS or Atom feed, as a kind of source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedKind {}

impl SourceKind for FeedKind {
    const NAME: &str = "feed";
    const DESCRIPTION: &str = "An RSS or Atom feed's entries.";

    type File = FeedOptions;
    type Options = FeedOptions;

    fn checked(file: FeedOptions) -> Result<FeedOptions, Fault> {
        file.check().map(|()| file).map_err(Fault::here)
    }
}

impl SourceConfig for FeedOptions {
    fn kind(&self) -> &'static str {
        FeedKind::NAME
    }

    fn token(&self) -> Option<&SecretRef> {
        None
    }

    fn kinds(&self) -> &'static [Kind] {
        &[Kind::FEED_ENTRY]
    }

    fn backfill(&self) -> usize {
        self.backfill
    }

    fn projects(&self) -> Vec<String> {
        vec![untitled(&self.url)]
    }

    fn poller(&self, _: Option<Secret>, _: bool) -> Polling {
        Polling::One(Arc::new(Entries(Feed::new(self))))
    }
}

/// A feed's entries.
#[derive(Debug)]
struct Entries(Feed);

impl Poll for Entries {
    fn api(&self) -> &str {
        self.0.api()
    }

    fn remembered(&self) -> usize {
        self.0.reading().remembered
    }

    fn polled<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        cursor: Option<&'a Cursor>,
    ) -> Boxed<'a, Result<Option<Polled>, Unpolled>> {
        Box::pin(async move {
            let answer = Erased(http)
                .send(self.0.request(cursor))
                .await
                .map_err(Unpolled::unreachable)?;
            if Feed::unchanged(&answer) {
                return Ok(None);
            }
            self.0
                .entries(&answer)
                .map(Some)
                .map_err(Unpolled::unreadable)
        })
    }
}

#[cfg(test)]
mod tests;
