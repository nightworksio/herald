use std::sync::Arc;

use herald_core::{Cursor, Kind, Polled, SecretRef};
use herald_kinds::{Fault, Poll, SourceConfig, SourceKind, Unpolled};
use herald_ports::{Boxed, DynHttp, Erased, Http as _, Secret};

use crate::{Package, Registry, RegistryOptions};

/// A package's versions on a registry: the registry, and the options its
/// table reads into.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistrySource {
    /// The registry.
    pub registry: Registry,
    /// The options its table reads into.
    pub options: RegistryOptions,
}

impl RegistrySource {
    fn checked(registry: Registry, options: RegistryOptions) -> Result<Self, Fault> {
        if options.token.is_some() && !registry.takes_token() {
            return Err(Fault::at(
                "token",
                format!(
                    "the registry \"{}\" takes no token; remove token",
                    registry.kind()
                ),
            ));
        }
        options
            .check(registry)
            .map(|()| Self { registry, options })
            .map_err(Fault::here)
    }
}

macro_rules! registry_kind {
    ($kind:ident, $registry:expr, $what:literal, $description:literal) => {
        #[doc = concat!($what, ", as a kind of source.")]
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum $kind {}

        impl SourceKind for $kind {
            const NAME: &'static str = $registry.kind();
            const DESCRIPTION: &'static str = $description;

            type File = RegistryOptions;
            type Options = RegistrySource;

            fn checked(file: RegistryOptions) -> Result<RegistrySource, Fault> {
                RegistrySource::checked($registry, file)
            }
        }
    };
}

registry_kind!(
    PackagistKind,
    Registry::Packagist,
    "Packagist",
    "A package's versions on Packagist."
);
registry_kind!(
    CratesIoKind,
    Registry::CratesIo,
    "crates.io",
    "A crate's versions on crates.io."
);
registry_kind!(
    NpmKind,
    Registry::Npm,
    "The npm registry",
    "A package's versions on the npm registry."
);
registry_kind!(
    DockerHubKind,
    Registry::DockerHub,
    "Docker Hub",
    "A repository's tags on Docker Hub."
);

impl SourceConfig for RegistrySource {
    fn kind(&self) -> &'static str {
        self.registry.kind()
    }

    fn token(&self) -> Option<&SecretRef> {
        self.options.token.as_ref()
    }

    fn kinds(&self) -> &'static [Kind] {
        &[Kind::PackageVersion]
    }

    fn backfill(&self) -> usize {
        self.options.backfill
    }

    fn moving_tags(&self) -> bool {
        self.registry.moving_tags()
    }

    fn poller(&self, token: Option<Secret>) -> Arc<dyn Poll> {
        Arc::new(Versions {
            package: Package::new(self.registry, &self.options),
            token,
        })
    }
}

/// A package's versions, polled with the token the config names.
#[derive(Debug)]
struct Versions {
    package: Package,
    token: Option<Secret>,
}

impl Poll for Versions {
    fn api(&self) -> &str {
        self.package.api()
    }

    fn remembered(&self) -> usize {
        self.package.reading().remembered
    }

    fn polled<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        cursor: Option<&'a Cursor>,
    ) -> Boxed<'a, Result<Option<Polled>, Unpolled>> {
        Box::pin(async move {
            let http = Erased(http);
            let package = &self.package;
            let bearer = match self
                .token
                .as_ref()
                .and_then(|token| package.token_request(token).map(|request| (token, request)))
            {
                Some((token, request)) => {
                    let answer = http.send(request).await.map_err(Unpolled::unreachable)?;
                    Some(
                        package
                            .exchanged(token, &answer)
                            .map_err(Unpolled::unreadable)?,
                    )
                }
                None => self.token.clone(),
            };
            let answer = http
                .send(package.versions_request(cursor, bearer.as_ref()))
                .await
                .map_err(Unpolled::unreachable)?;
            if Registry::unchanged(&answer) {
                return Ok(None);
            }
            package
                .versions(&answer)
                .map(Some)
                .map_err(Unpolled::unreadable)
        })
    }
}

#[cfg(test)]
mod tests;
