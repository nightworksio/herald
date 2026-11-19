//! A peer's feed read from its static signed log (ADR-0060): the discovery
//! document taken only once its detached signature holds for a key
//! trusted for the peer and names a log no older than one read before and
//! written within `stale_after`, and each page only where its bytes are
//! the ones the document lists.

use herald_core::{Cursor, Polled, media, status};
use herald_kinds::{Pulling, Trust, Unpulled};
use herald_peers::digest::content_digest;
use herald_peers::keys::{WELL_KNOWN, method};
use herald_peers::pages::{self, Log, signature_of};
use herald_peers::reading;
use herald_ports::{Algorithm, Erased, Http as _, Method, Request, SecretText, Verifier as _};

use super::{Pulled, Shares, unreadable};

impl Pulled {
    /// What the static log lists since `cursor`, newest first: every page
    /// down to the cursor's sequence, at most `pages` of them, or with no
    /// cursor enough for `backfill`; nothing where the cursor read this
    /// very log.
    pub(super) async fn logged(
        &self,
        pulling: Pulling<'_>,
        trust: &Trust,
        cursor: Option<&Cursor>,
    ) -> Result<Option<Polled>, Unpulled> {
        let log = self.signed(pulling, trust, cursor).await?;
        let Some((log, body)) = log else {
            return Ok(None);
        };
        let listing = pages::listing(&body, log, &self.0.api).map_err(unreadable)?;
        let since = cursor.and_then(|cursor| cursor.sequence);
        let mut polled = Polled {
            relisted: true,
            log: Some(log.sequence),
            ..Polled::default()
        };
        let mut shares = Shares::of(cursor, self.0.peer_share);
        for (url, digest) in listing.pages.iter().take(self.0.pages) {
            let read = got(pulling, trust, url).await?;
            if content_digest(&read) != *digest {
                return Err(Unpulled::Unreadable(format!(
                    "the page {url} is not the one the static log lists; its host may be serving another copy"
                )));
            }
            let page = reading::page(&read, url).map_err(unreadable)?;
            let mut reached = !page.more;
            for (sequence, envelope) in &page.items {
                if since.is_some_and(|since| *sequence <= since) {
                    reached = true;
                    break;
                }
                polled.sequence.get_or_insert(*sequence);
                self.taken(
                    (pulling, trust, cursor),
                    (*sequence, envelope),
                    (&mut polled, &mut shares),
                )
                .await?;
            }
            if self.ends(reached, cursor, &polled) {
                break;
            }
        }
        Ok(Some(self.finished(polled, shares, trust)))
    }

    /// The log the discovery document names, and the document, once its
    /// detached signature holds for a key of `trust` and names a log no
    /// older than the one `cursor` read, written no further ahead than
    /// `trust` allows and within `stale_after`; nothing where `cursor`
    /// read this very log.
    async fn signed(
        &self,
        pulling: Pulling<'_>,
        trust: &Trust,
        cursor: Option<&Cursor>,
    ) -> Result<Option<(Log, Vec<u8>)>, Unpulled> {
        let discovery = format!("{}{WELL_KNOWN}", trust.url);
        let body = got(pulling, trust, &discovery).await?;
        let signature = got(pulling, trust, &signature_of(&discovery)).await?;
        let statement = pages::signed(&signature, &discovery, &body).map_err(unreadable)?;
        let shown = &statement.proof;
        let key = trust
            .keys
            .iter()
            .find(|key| method(&trust.domain, key) == shown.method)
            .ok_or_else(|| Unpulled::Stranger(shown.method.clone()))?;
        if !Erased(pulling.verifier)
            .verify(Algorithm::Ed25519, key, &shown.message, &shown.signature)
            .await
        {
            return Err(Unpulled::Unreadable(
                "the static log's signature does not hold".to_owned(),
            ));
        }
        let log = statement.log;
        let read = cursor.and_then(|cursor| cursor.log);
        if let Some(read) = read.filter(|read| log.sequence < *read) {
            return Err(Unpulled::Unreadable(format!(
                "the static log is at {}, older than the {read} read before; its host may be serving an old copy",
                log.sequence
            )));
        }
        if log.published.as_second() > pulling.now.as_second().saturating_add(trust.skew) {
            return Err(Unpulled::Unreadable(format!(
                "the static log was written at {}, ahead of now",
                log.published
            )));
        }
        if log
            .published
            .checked_add(self.0.stale_after)
            .is_ok_and(|until| pulling.now > until)
        {
            return Err(Unpulled::Unreadable(format!(
                "the static log was written at {}, longer ago than stale_after allows; its host may be withholding newer pages",
                log.published
            )));
        }
        Ok((read != Some(log.sequence)).then_some((log, body)))
    }
}

/// The body of the file at `url`, read without asking for a change, at
/// most `trust.most` bytes.
async fn got(pulling: Pulling<'_>, trust: &Trust, url: &str) -> Result<Vec<u8>, Unpulled> {
    let request = Request::new(Method::Get, SecretText::plain(url))
        .accepting(media::JSON)
        .at_most(trust.most)
        .identified();
    let response = Erased(pulling.http)
        .send(request)
        .await
        .map_err(|error| Unpulled::Unreachable(error.to_string()))?;
    if response.status != status::OK {
        return Err(Unpulled::Unreadable(format!(
            "the static log's {url} was answered {}",
            response.status
        )));
    }
    Ok(response.body)
}
