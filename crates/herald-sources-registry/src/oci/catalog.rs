//! Every repository a registry's catalog lists, each polled on its own as
//! an owner's members are (ADR-0042).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use herald_core::{Listed, Listing, Origin, Owner, Selection, unreserved};
use herald_kinds::{Expand, Expanded, Poll, Probed, Unlisted, Unpolled};
use herald_ports::{Boxed, DynHttp};
use herald_wire::answered;
use jiff::SignedDuration;

use super::image::Image;
use super::options::OciOptions;
use super::session::{Failure, Reached, Session};
use super::wire;

/// The scope of a token that reads the catalog.
const SCOPE: &str = "registry:catalog:*";

/// Every repository a registry's catalog lists that the selection takes.
#[derive(Debug)]
pub(crate) struct Catalogued {
    reached: Arc<Reached>,
    selection: Selection,
    options: OciOptions,
}

impl Catalogued {
    /// The catalog of `reached`, taken as `options` say.
    pub(crate) fn new(reached: Arc<Reached>, options: &OciOptions) -> Self {
        Self {
            reached,
            selection: options.selection().unwrap_or_default(),
            options: options.clone(),
        }
    }

    fn image(&self, repository: &Listed) -> Image {
        Image::new(
            Arc::clone(&self.reached),
            repository.path.clone(),
            repository.visibility,
        )
    }

    /// The repository `name`, as a member named by its full path, which
    /// `include` and `exclude` match.
    fn member_of(&self, name: String) -> Listed {
        Listed {
            id: name.clone(),
            name: name.clone(),
            path: name,
            topics: Vec::new(),
            properties: BTreeMap::new(),
            visibility: self.reached.visibility,
            archived: false,
            fork: false,
            template: false,
            language: None,
            linked: None,
        }
    }

    /// The repositories the catalog names, page by page, through `session`.
    async fn names(&self, session: &mut Session<'_>) -> Result<Vec<String>, Unlisted> {
        let page = self.reached.listed_page;
        let most_listed = self.options.most_listed();
        let mut names: Vec<String> = Vec::new();
        let mut known = BTreeSet::new();
        loop {
            let last = names
                .last()
                .map(|last| format!("&last={}", unreserved(last)))
                .unwrap_or_default();
            let request = session
                .get(&format!("/v2/_catalog?n={page}{last}"))
                .accepting(herald_core::media::JSON);
            let answer = session.send(request).await.map_err(Unlisted::from)?;
            let listed = answered(wire::CATALOG, &answer)
                .and_then(|()| wire::catalog(self.reached.reading.parse, &answer.body))
                .map_err(|error| Unlisted::from(Failure::read("catalog", &error)))?;
            if let Some(repeated) = listed.iter().find(|name| known.contains(*name)) {
                return Err(Unlisted::from(Failure::at(
                    "catalog",
                    format!(
                        "the registry's catalog named \"{repeated}\" twice; it does not page its catalog by last"
                    ),
                )));
            }
            let more =
                !listed.is_empty() && (wire::next(answer.header("link")) || listed.len() >= page);
            known.extend(listed.iter().cloned());
            names.extend(listed);
            if names.len() > most_listed.get() {
                return Err(Unlisted::Over(format!(
                    "the registry's catalog lists more than {most_listed} repositories, most_listed; raise it"
                )));
            }
            if !more {
                return Ok(names);
            }
        }
    }

    async fn list(&self, http: &dyn DynHttp) -> Result<Expanded, Unlisted> {
        let mut session = Session::new(&self.reached, http, SCOPE.to_owned());
        let names = self.names(&mut session).await?;
        let members: Vec<Listed> = names
            .into_iter()
            .map(|name| self.member_of(name))
            .filter(|repository| self.selection.takes(repository))
            .collect();
        let most_members = self.options.most_members();
        if members.len() > most_members.get() {
            return Err(Unlisted::Over(format!(
                "{} repositories of the catalog are taken, more than most_members, {most_members}; raise it, or narrow include and exclude",
                members.len()
            )));
        }
        Ok(Expanded::Listed {
            members,
            owner: Owner::Organization,
            pages: Vec::new(),
            file: None,
            missing: Vec::new(),
        })
    }

    /// What reaching the first repository the catalog names finds wrong;
    /// nothing where the catalog names none or cannot be listed, which its
    /// listing reports.
    async fn check(&self, http: &dyn DynHttp) -> Vec<Probed> {
        let mut session = Session::new(&self.reached, http, SCOPE.to_owned());
        let request = session.get("/v2/_catalog?n=1");
        let first = match session.send(request).await {
            Ok(answer) => answered(wire::CATALOG, &answer)
                .and_then(|()| wire::catalog(self.reached.reading.parse, &answer.body))
                .ok()
                .and_then(|names| names.into_iter().next()),
            Err(_) => None,
        };
        match first {
            Some(name) => self.image(&self.member_of(name)).check(http).await,
            None => Vec::new(),
        }
    }
}

impl Expand for Catalogued {
    fn api(&self) -> &str {
        &self.reached.registry
    }

    fn bases(&self) -> &[String] {
        &self.reached.bases
    }

    fn owner_key(&self) -> &'static str {
        "catalog"
    }

    fn pushed(&self) -> bool {
        false
    }

    fn keep_members(&self) -> SignedDuration {
        self.options.keep_members()
    }

    fn remembered(&self) -> usize {
        self.reached.reading.remembered
    }

    fn listed<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        _known: Option<&'a Listing>,
    ) -> Boxed<'a, Result<Expanded, Unlisted>> {
        Box::pin(self.list(http))
    }

    fn member(&self, repository: &Listed) -> Arc<dyn Poll> {
        Arc::new(self.image(repository))
    }

    fn origin(&self, repository: &Listed) -> Origin {
        self.image(repository).origin()
    }

    fn project(&self, repository: &Listed) -> Result<String, String> {
        Ok(self.image(repository).project().to_owned())
    }

    fn takes(&self, repository: &Listed) -> bool {
        self.selection.takes(repository)
    }

    fn fingerprint(&self) -> String {
        herald_core::digest(&format!(
            "oci\n{}\n{:?}\n{:?}\n{}",
            self.reached.registry,
            self.selection,
            self.reached.visibility,
            self.reached.listed_page
        ))
    }

    fn probed<'a>(&'a self, http: &'a dyn DynHttp) -> Boxed<'a, Vec<Probed>> {
        Box::pin(self.check(http))
    }
}

impl From<Failure> for Unlisted {
    fn from(failure: Failure) -> Self {
        Self::Failed(Unpolled::from(failure))
    }
}
