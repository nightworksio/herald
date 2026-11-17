//! The keys that name a registry namespace whose packages a source takes,
//! and choose among them (ADR-0039).

use std::num::NonZeroUsize;

use herald_core::{
    Glob, KEEP_MEMBERS, MOST_LISTED, MOST_MEMBERS, MOST_MEMBERS_ALLOWED, Selection, Visibility,
    path_of_names, web_url,
};
use jiff::SignedDuration;

use super::RegistryOptions;
use crate::Registry;

/// How a Packagist vendor's packages are listed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum VendorListing {
    /// Packagist's `/packages/list.json?vendor=`, at `listing_api`.
    #[default]
    Packagist,
    /// A Composer repository's `packages.json`, at `api`, as Private
    /// Packagist and Satis serve it.
    Composer,
}

/// Who may see a namespace's packages, where the source declares it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Declared {
    /// Anyone: the namespace holds only public packages.
    Public,
}

/// A crates.io owner, as crates.io writes owners.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CratesOwner<'a> {
    /// A user, by login.
    User(&'a str),
    /// A GitHub team, as `github:<org>:<team>`.
    Team(&'a str),
}

impl<'a> CratesOwner<'a> {
    /// The owner `owner` writes, where it writes one.
    pub(crate) fn of(owner: &'a str) -> Option<Self> {
        let named = |name: &str| path_of_names(name, 1..=1);
        if let Some(login) = owner.strip_prefix("user:") {
            return named(login).then_some(Self::User(login));
        }
        let team = owner.strip_prefix("team:")?;
        let mut parts = team.split(':');
        (parts.next() == Some("github")
            && parts.next().is_some_and(named)
            && parts.next().is_some_and(named)
            && parts.next().is_none())
        .then_some(Self::Team(team))
    }
}

/// The keys only a namespace source may give.
const NAMESPACED: [&str; 8] = [
    "include",
    "exclude",
    "most_listed",
    "most_members",
    "keep_members",
    "listed_page",
    "visibility",
    "unscoped",
];

impl RegistryOptions {
    /// The namespace the source takes the packages of, where it names one.
    #[must_use]
    pub fn namespace_of(&self, registry: Registry) -> Option<&str> {
        match registry {
            Registry::Npm => self.scope.as_deref(),
            Registry::Packagist => self.vendor.as_deref(),
            Registry::CratesIo => self.owner.as_deref(),
            Registry::DockerHub => self.namespace.as_deref(),
        }
    }

    /// Which packages of the namespace the source takes.
    ///
    /// # Errors
    ///
    /// When `include` or `exclude` holds a glob that is not one.
    pub fn selection(&self) -> Result<Selection, String> {
        let globs = |texts: &[String]| {
            texts
                .iter()
                .map(|text| Glob::new(text).map_err(|error| error.to_string()))
                .collect::<Result<Vec<Glob>, String>>()
        };
        Ok(Selection {
            include: globs(&self.include)?,
            exclude: globs(&self.exclude)?,
            visibility: vec![
                Visibility::Public,
                Visibility::Internal,
                Visibility::Private,
            ],
            ..Selection::default()
        })
    }

    /// The most packages a namespace's listing reads.
    #[must_use]
    pub fn most_listed(&self) -> NonZeroUsize {
        self.most_listed.unwrap_or(MOST_LISTED)
    }

    /// The most packages the selection keeps.
    #[must_use]
    pub fn most_members(&self) -> NonZeroUsize {
        self.most_members.unwrap_or(MOST_MEMBERS)
    }

    /// How long a package that left the namespace is kept.
    #[must_use]
    pub fn keep_members(&self) -> SignedDuration {
        self.keep_members.map_or(KEEP_MEMBERS, |kept| kept.0)
    }

    /// How many packages one page of the namespace's listing asks for on
    /// `registry`.
    #[must_use]
    pub fn listed_page(&self, registry: Registry) -> usize {
        self.listed_page
            .map_or(registry.listed_page(), NonZeroUsize::get)
    }

    /// How a Packagist vendor's packages are listed.
    #[must_use]
    pub fn listing(&self) -> VendorListing {
        self.listing.unwrap_or_default()
    }

    /// Who may see a package the namespace's listing names without saying:
    /// anyone where the source lists anonymously or declares its packages
    /// public, and those given access where its token may list private
    /// ones.
    #[must_use]
    pub fn listed_visibility(&self) -> Visibility {
        if self.token.is_none() || self.visibility == Some(Declared::Public) {
            Visibility::Public
        } else {
            Visibility::Private
        }
    }

    /// Whether the namespace keys are sound on `registry`: one of `package`
    /// and the registry's namespace key, written as the registry writes
    /// it, with the keys that choose among its packages only beside the
    /// namespace key.
    pub(super) fn namespace_check(&self, registry: Registry) -> Result<(), String> {
        let key = registry.namespace_key();
        if let Some(other) = [
            ("scope", self.scope.is_some()),
            ("vendor", self.vendor.is_some()),
            ("owner", self.owner.is_some()),
            ("namespace", self.namespace.is_some()),
        ]
        .into_iter()
        .find_map(|(other, given)| (given && other != key).then_some(other))
        {
            return Err(format!(
                "the registry \"{}\" names a namespace by {key}; remove {other}",
                registry.kind()
            ));
        }
        let Some(namespace) = self.namespace_of(registry) else {
            if self.package.is_none() {
                return Err(format!(
                    "give package, or {key} to take every package of a namespace"
                ));
            }
            return self.unnamespaced(key);
        };
        if self.package.is_some() {
            return Err(format!(
                "package and {key} name different things; give one of them"
            ));
        }
        if !registry.writes_namespace(namespace) {
            return Err(format!(
                "\"{namespace}\" is not a {key}; give it as {}",
                registry.namespace_shape()
            ));
        }
        self.namespaced(registry)
    }

    /// Whether a source naming one package gives no key that only a
    /// namespace source takes.
    fn unnamespaced(&self, key: &str) -> Result<(), String> {
        let given = [
            !self.include.is_empty(),
            !self.exclude.is_empty(),
            self.most_listed.is_some(),
            self.most_members.is_some(),
            self.keep_members.is_some(),
            self.listed_page.is_some(),
            self.visibility.is_some(),
            self.unscoped,
        ];
        match NAMESPACED
            .into_iter()
            .zip(given)
            .find_map(|(name, given)| given.then_some(name))
        {
            Some(name) => Err(format!(
                "{name} chooses among a namespace's packages; give {key} instead of package, or remove {name}"
            )),
            None => self.listing_check(None),
        }
    }

    /// Whether the keys choosing among a namespace's packages on `registry`
    /// are sound.
    fn namespaced(&self, registry: Registry) -> Result<(), String> {
        self.selection()?;
        if self.most_members().get() > MOST_MEMBERS_ALLOWED {
            return Err(format!(
                "most_members is at most {MOST_MEMBERS_ALLOWED}; lower it"
            ));
        }
        if self.listed_page.is_some() && !registry.pages_its_listing() {
            return Err(format!(
                "the registry \"{}\" lists a namespace in one answer; remove listed_page",
                registry.kind()
            ));
        }
        if self.listed_page(registry) > registry.listed_page() {
            return Err(format!(
                "listed_page is at most {}; lower it",
                registry.listed_page()
            ));
        }
        if self.unscoped && registry != Registry::Npm {
            return Err("unscoped is an npm scope's; remove it".to_owned());
        }
        self.listing_check(Some(registry))
    }

    /// Whether `listing` and `listing_api` are given only on a Packagist
    /// vendor source, `listing_api` only where it lists through Packagist,
    /// and as a web address.
    fn listing_check(&self, namespaced: Option<Registry>) -> Result<(), String> {
        let vendor = namespaced == Some(Registry::Packagist);
        if let Some(key) = [
            ("listing", self.listing.is_some()),
            ("listing_api", self.listing_api.is_some()),
        ]
        .into_iter()
        .find_map(|(key, given)| (given && !vendor).then_some(key))
        {
            return Err(format!(
                "{key} is how a Packagist vendor is listed; give vendor on a packagist source, or remove {key}"
            ));
        }
        match &self.listing_api {
            Some(_) if self.listing() == VendorListing::Composer => Err(
                "a Composer repository is listed at api; remove listing_api, or set listing = \"packagist\"".to_owned(),
            ),
            Some(api) if !web_url(api) => Err(format!(
                "\"{api}\" is not a web address; give the base URL that serves /packages/list.json, such as https://packagist.org"
            )),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests;
