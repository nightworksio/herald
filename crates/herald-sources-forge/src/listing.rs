//! A repository's listing a poll asks under the validators it keeps, and the
//! names a kind's deliveries and listings are read as on each forge.

use herald_core::{Cursor, Validators};
use herald_kinds::Unpolled;
use herald_ports::{DynHttp, Erased, Http as _, Response};
use herald_wire::answered;

use crate::forge::validators;
use crate::{Forge, Origin, Token};

/// What something is read as on each forge.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Named {
    pub(crate) github: &'static str,
    pub(crate) gitlab: &'static str,
    pub(crate) forgejo: &'static str,
}

impl Named {
    /// What it is read as on `forge`.
    pub(crate) const fn on(&self, forge: Forge) -> &'static str {
        match forge {
            Forge::GitHub => self.github,
            Forge::GitLab => self.gitlab,
            Forge::Forgejo => self.forgejo,
        }
    }
}

/// A poll's listing: the validators it was asked under, and its answer.
#[derive(Debug)]
pub(crate) struct Asked {
    kept: Option<Validators>,
    answer: Response,
}

impl Asked {
    /// The listing at `path` of `origin`, asked through `http` with `token`,
    /// conditional on what `cursor` keeps under `key`.
    pub(crate) async fn asked(
        http: &Erased<'_, dyn DynHttp + '_>,
        (origin, token): (&Origin, Option<&Token>),
        path: &str,
        (cursor, key): (Option<&Cursor>, &str),
    ) -> Result<Self, Unpolled> {
        let kept = cursor.and_then(|cursor| cursor.listings.get(key)).cloned();
        let request = origin.get(path, token)?.conditional(
            kept.as_ref().and_then(|kept| kept.etag.as_deref()),
            kept.as_ref().and_then(|kept| kept.last_modified.as_deref()),
        );
        let answer = http.send(request).await.map_err(Unpolled::unreachable)?;
        Ok(Self { kept, answer })
    }

    /// Whether the listing changed since it was last read.
    pub(crate) fn changed(&self) -> bool {
        !Forge::unchanged(&self.answer)
    }

    /// The validators to keep for it, and its body, read as `what`, where
    /// it changed.
    pub(crate) fn read(&self, what: &'static str) -> Result<(Validators, Option<&[u8]>), Unpolled> {
        if !self.changed() {
            return Ok((self.kept.clone().unwrap_or_default(), None));
        }
        answered(what, &self.answer).map_err(Unpolled::unreadable)?;
        Ok((validators(&self.answer), Some(&self.answer.body)))
    }
}

#[cfg(test)]
mod tests;
