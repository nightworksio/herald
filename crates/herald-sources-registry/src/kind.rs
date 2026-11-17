use std::sync::Arc;

use herald_core::{Cursor, Kind, Polled, SecretRef, Visibility};
use herald_kinds::{Credential, Fault, Poll, Polling, SourceConfig, SourceKind, Unpolled};
use herald_ports::{Boxed, DynHttp, Erased, Http as _, Secret};

use crate::namespace::Namespaced;
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
            const NAME: &str = $registry.kind();
            const DESCRIPTION: &str = $description;

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
        &[Kind::PACKAGE_VERSION]
    }

    fn backfill(&self) -> usize {
        self.options.backfill
    }

    fn moving_tags(&self) -> bool {
        self.registry.moving_tags()
    }

    fn origins(&self) -> bool {
        true
    }

    fn unlisted(&self) -> bool {
        self.options.namespace_of(self.registry).is_some()
            && (self.options.listed_visibility() != Visibility::Public
                || (self.registry.lists_visibility() && self.options.token.is_some()))
    }

    fn projects(&self) -> Vec<String> {
        self.options
            .package
            .iter()
            .map(|name| {
                Package::new(self.registry, name, &self.options)
                    .project()
                    .to_owned()
            })
            .collect()
    }

    fn poller(&self, token: Option<Credential>, _: bool) -> Polling {
        let token = token.and_then(Credential::named);
        match (
            &self.options.package,
            self.options.namespace_of(self.registry),
        ) {
            (None, Some(namespace)) => Polling::Many(Arc::new(Namespaced::new(
                self.registry,
                namespace,
                &self.options,
                token,
            ))),
            (package, _) => Polling::One(Arc::new(Versions {
                package: Package::new(
                    self.registry,
                    package.as_deref().unwrap_or_default(),
                    &self.options,
                ),
                token,
            })),
        }
    }
}

/// A package's versions, polled with the token the config names.
#[derive(Debug)]
pub(crate) struct Versions {
    pub(crate) package: Package,
    pub(crate) token: Option<Secret>,
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
        _: jiff::Timestamp,
    ) -> Boxed<'a, Result<Option<Polled>, Unpolled>> {
        Box::pin(async move {
            let package = &self.package;
            let bearer = bearer(package, http, self.token.as_ref()).await?;
            let http = Erased(http);
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

/// The token `package` is asked with through `http`: the bearer token
/// `token` is exchanged for, where the registry exchanges one, and `token`
/// itself otherwise.
pub(crate) async fn bearer(
    package: &Package,
    http: &dyn DynHttp,
    token: Option<&Secret>,
) -> Result<Option<Secret>, Unpolled> {
    let Some((token, request)) =
        token.and_then(|token| package.token_request(token).map(|request| (token, request)))
    else {
        return Ok(token.cloned());
    };
    let answer = Erased(http)
        .send(request)
        .await
        .map_err(Unpolled::unreachable)?;
    package
        .exchanged(token, &answer)
        .map(Some)
        .map_err(Unpolled::unreadable)
}

#[cfg(test)]
mod tests;
