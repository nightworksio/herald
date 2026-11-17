//! A container image's tags on any registry that serves the OCI
//! Distribution API, or every repository its catalog lists (ADR-0042).

use std::sync::Arc;

use herald_core::{Kind, SecretRef};
use herald_kinds::{
    Bound, Credential, Fault, Polling, Remembers, Source, SourceConfig, SourceKind,
};
use herald_ports::Secret;

mod catalog;
mod image;
mod options;
mod session;
mod wire;

pub use options::OciOptions;

use catalog::Catalogued;
use image::Image;
use session::Reached;

/// The kind's name, as a source's `kind` in `herald.toml` writes it.
const KIND: &str = "oci";

/// The most bytes an identity's tag and the record's quoting of it take.
const TAG_BYTES: usize = 128 + 5;

/// The most bytes a repository's name takes in an identity, where the
/// catalog names it.
const REPOSITORY_BYTES: usize = 255;

/// A registry serving the OCI Distribution API, as a kind of source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OciKind {}

impl SourceKind for OciKind {
    const NAME: &str = KIND;
    const DESCRIPTION: &str =
        "A container image's tags on any registry serving the OCI Distribution API.";

    type File = OciOptions;
    type Options = OciSource;

    fn checked(file: OciOptions) -> Result<OciSource, Fault> {
        file.check()
            .map(|()| OciSource::new(file))
            .map_err(|(key, message)| Fault::at(key, message))
    }
}

/// An `oci` source: the options its table reads into.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OciSource {
    /// The options.
    pub options: OciOptions,
    /// Whether the store chose `most_tags`, as the most its records hold
    /// where the options name none.
    pub fitted: bool,
}

impl OciSource {
    /// The source `options` name, its tags bounded by them.
    #[must_use]
    pub const fn new(options: OciOptions) -> Self {
        Self {
            options,
            fitted: false,
        }
    }

    fn image(&self, repository: &str, token: Option<Secret>) -> Image {
        let options = &self.options;
        Image::new(
            Arc::new(Reached::new(options, token)),
            options.repository_of(repository),
            options.visibility(),
        )
    }
}

impl SourceConfig for OciSource {
    fn kind(&self) -> &'static str {
        KIND
    }

    fn token(&self) -> Option<&SecretRef> {
        self.options.token.as_ref()
    }

    fn kinds(&self) -> &'static [Kind] {
        &[Kind::PACKAGE_VERSION]
    }

    fn backfill(&self) -> usize {
        self.options.backfill
    }

    fn origins(&self) -> bool {
        true
    }

    fn unlisted(&self) -> bool {
        self.options.visibility() != herald_core::Visibility::Public
    }

    fn projects(&self) -> Vec<String> {
        self.options
            .repository
            .iter()
            .map(|repository| self.image(repository, None).project().to_owned())
            .collect()
    }

    fn remembers(&self) -> Option<Remembers> {
        let repository = self
            .options
            .repository
            .as_ref()
            .map_or(REPOSITORY_BYTES, |repository| {
                self.options.repository_of(repository).len()
            });
        let bound = if self.fitted {
            Bound::Fitted
        } else if self.options.most_tags.is_some() || self.options.remembered.is_some() {
            Bound::Named
        } else {
            Bound::Default
        };
        Some(Remembers {
            identities: self.options.reading().remembered,
            each: self.options.registry_name().len() + 1 + repository + 1 + TAG_BYTES,
            key: self.options.remembering_key(),
            bound,
        })
    }

    fn fitted(&self, identities: usize) -> Option<Source> {
        let options = OciOptions {
            most_tags: Some(std::num::NonZeroUsize::new(identities)?),
            ..self.options.clone()
        };
        options.check().ok()?;
        Some(Source::new(Self {
            options,
            fitted: true,
        }))
    }

    fn poller(&self, token: Option<Credential>, _: bool) -> Polling {
        let token = token.and_then(Credential::named);
        match &self.options.repository {
            Some(repository) => Polling::One(Arc::new(self.image(repository, token))),
            None => Polling::Many(Arc::new(Catalogued::new(
                Arc::new(Reached::new(&self.options, token)),
                &self.options,
            ))),
        }
    }
}

#[cfg(test)]
mod tests;
