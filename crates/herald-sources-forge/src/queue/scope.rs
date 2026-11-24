//! An owner's open pull requests as a view reads them: one query over the
//! owner's repositories, each pull request kept only where it is of a
//! repository the owner's selection takes, named and seen as its
//! membership record says (ADR-0062).

use std::collections::BTreeSet;

use herald_core::{Listed, Owner, Pull, Validators};
use herald_kinds::{Asked, Expand as _, Most, Relisted, Scope, Unpolled};
use herald_ports::{DynHttp, Erased, Http as _, Request, Response};

use super::reconcile::Paged;
use super::{Opened, PAGE, PER_PULL, ReadPath, github, gitlab, listing};
use crate::forge::{OWNER, authorized, whole};
use crate::limited::checked;
use crate::owner::Owned;
use crate::{Forge, Origin, SourceOptions, Token};

mod advised;
mod pushed;
mod search;

use search::Located;

/// The most results GitHub's search lists of one query.
pub(crate) const SEARCHED: usize = 1000;

/// How an owner's open pull requests are listed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lists {
    /// GitHub's issue search.
    Search,
    /// GitLab's listing of a group's merge requests.
    Group,
    /// Forgejo's issue search.
    Issues,
    /// Each repository's own listing.
    Members,
}

/// An owner source's repositories, their open pull requests read for a
/// view.
#[derive(Debug)]
pub(crate) struct Scoped {
    /// The owner on its forge, whose API every request is under.
    root: Origin,
    /// How each repository is asked.
    options: SourceOptions,
    /// The owner.
    owner: String,
    /// What it reads with.
    token: Option<Token>,
    /// The GraphQL endpoint, on GitHub.
    graphql: String,
    /// What names each repository's project.
    owned: Owned,
}

impl Scoped {
    /// The repositories of `owner` on `forge`, asked as `options` say with
    /// `token`, named by `owned`, read through `graphql` on GitHub.
    pub(crate) fn new(
        (forge, owner): (Forge, &str),
        options: &SourceOptions,
        token: Option<Token>,
        (graphql, naming): (String, Owned),
    ) -> Self {
        Self {
            root: Origin::new(forge, options, owner),
            options: options.clone(),
            owner: owner.to_owned(),
            token,
            graphql,
            owned: naming,
        }
    }

    /// The forge.
    const fn forge(&self) -> Forge {
        self.root.forge()
    }

    /// How the repositories of an owner of the kind `owner` are listed.
    const fn lists(&self, owner: Owner) -> Lists {
        match (self.forge(), owner) {
            (Forge::GitHub, _) => Lists::Search,
            (Forge::GitLab, Owner::Organization) => Lists::Group,
            (Forge::GitLab, Owner::User) => Lists::Members,
            (Forge::Forgejo, _) => Lists::Issues,
        }
    }

    /// What `lists` is read as.
    const fn what(lists: Lists) -> &'static str {
        match lists {
            Lists::Search | Lists::Members => search::GITHUB,
            Lists::Group => gitlab::WHAT,
            Lists::Issues => search::FORGEJO,
        }
    }

    /// The request for the `number`th page of the query `lists` over the
    /// repositories of the owner, of the kind `owner`.
    fn request(&self, (lists, owner): (Lists, Owner), number: usize) -> Result<Request, Unpolled> {
        let name = &self.owner;
        let segment = whole(name, OWNER)?;
        let path = match lists {
            Lists::Search => {
                let qualifier = match owner {
                    Owner::Organization => "org",
                    Owner::User => "user",
                };
                let query = herald_core::unreserved(&format!("is:pr is:open {qualifier}:{name}"));
                format!(
                    "/search/issues?q={query}&sort=updated&order=desc&per_page={PAGE}&page={number}"
                )
            }
            Lists::Group => gitlab::group_path(name, number),
            Lists::Issues | Lists::Members => format!(
                "/repos/issues/search?type=pulls&state=open&owner={segment}&limit={PAGE}&page={number}"
            ),
        };
        Ok(self.root.rooted(&path, self.token.as_ref()))
    }

    /// The page of the query `lists` that `answer` holds, each pull request
    /// placed among `members` and those of none left out, whether another
    /// page follows, and how many the query found where it says.
    fn page(
        &self,
        lists: Lists,
        members: &[Listed],
        answer: &Response,
    ) -> Result<(Vec<Pull>, bool, Option<usize>), Unpolled> {
        let what = Self::what(lists);
        checked(self.forge(), what, answer)?;
        let parse = self.root.reading().parse;
        let (found, items, total) = match lists {
            Lists::Group => {
                let (found, items) =
                    gitlab::grouped(&answer.body, parse).map_err(Unpolled::unreadable)?;
                let found = found
                    .into_iter()
                    .map(|(id, pull)| (Located::Id(id), pull))
                    .collect();
                (found, items, None)
            }
            Lists::Issues | Lists::Members => {
                search::forgejo(&answer.body, parse).map_err(Unpolled::unreadable)?
            }
            Lists::Search => search::github(&answer.body, parse).map_err(Unpolled::unreadable)?,
        };
        let more = match lists {
            Lists::Issues | Lists::Members => items > 0,
            Lists::Search | Lists::Group => items >= PAGE,
        };
        let pulls = found
            .into_iter()
            .filter_map(|(located, pull)| self.placed(members, &located, pull))
            .collect();
        Ok((pulls, more, total))
    }

    /// `pull`, named and seen as the member of `members` that `located`
    /// finds, where one does.
    fn placed(&self, members: &[Listed], located: &Located, pull: Pull) -> Option<Pull> {
        let member = members.iter().find(|member| match located {
            Located::Id(id) => member.id == *id,
            Located::Path(path) => member.path.eq_ignore_ascii_case(path),
        })?;
        Some(Pull {
            project: self.project(member),
            repository: member.path.clone(),
            visibility: member.visibility,
            ..pull
        })
    }

    /// The project `member`'s pull requests are shown as: as its source
    /// names its releases' project, or its name where that cannot be told.
    fn project(&self, member: &Listed) -> String {
        self.owned.project(member).unwrap_or(member.name.clone())
    }

    /// The repository at `path`, asked as the source asks its owner.
    fn origin(&self, path: &str) -> Origin {
        Origin::new(self.forge(), &self.options, path)
    }

    /// How the repository `origin`, shown as `project` and seen by
    /// `visibility`, is read, at most `most` of it, each pull request's
    /// changed paths too where `files` says.
    fn reading<'a>(
        &'a self,
        origin: &'a Origin,
        (project, visibility): (&'a str, herald_core::Visibility),
        (most, files): (Most, bool),
    ) -> Opened<'a> {
        Opened {
            origin,
            token: self.token.as_ref(),
            project,
            graphql: self.graphql.as_str(),
            visibility: Some(visibility),
            most_open: most.open,
            most_pulls: most.pulls,
            stated: false,
            files,
            queue: false,
        }
    }

    /// The open pull requests of each of `members`, each repository listed
    /// on its own until `most.open` are, unconditionally.
    async fn each_member(
        &self,
        http: &Erased<'_, dyn DynHttp + '_>,
        most: Most,
        members: &[Listed],
    ) -> Result<Relisted, Unpolled> {
        let mut pulls = Vec::new();
        for member in members {
            let left = most.open.saturating_sub(pulls.len());
            if left == 0 {
                break;
            }
            let origin = self.origin(&member.path);
            let project = self.project(member);
            let most = Most {
                open: left,
                pulls: most.pulls,
            };
            let opened = self.reading(&origin, (&project, member.visibility), (most, false));
            pulls.extend(opened.relisted(http, &[]).await?.pulls());
        }
        Ok(Relisted::Listed {
            pulls,
            repository: None,
            visibility: None,
            validators: Vec::new(),
        })
    }

    /// The open pull requests of the repositories `scope` holds, at most
    /// `most.open`, each read as far as its listing reads it, asked through
    /// `http` conditional on `kept`: one query over them, or each
    /// repository's own listing where the forge has no such query for the
    /// owner or GitHub's search finds more than it lists.
    ///
    /// # Errors
    ///
    /// Where a page could not be asked or read.
    async fn relisted(
        &self,
        http: &Erased<'_, dyn DynHttp + '_>,
        (most, scope): (Most, Scope<'_>),
        kept: &[Validators],
    ) -> Result<Relisted, Unpolled> {
        let lists = self.lists(scope.owner);
        if lists == Lists::Members {
            return self.each_member(http, most, scope.members).await;
        }
        let pages = queried(self.forge(), most.open).div_ceil(PAGE);
        let query = Paged {
            pages,
            request: &|number| self.request((lists, scope.owner), number),
            read: &|answer| {
                let (pulls, more, total) = self.page(lists, scope.members, answer)?;
                Ok(((pulls, total), more))
            },
        };
        let Some((pages, validators)) = query.relisted(http, kept).await? else {
            return Ok(Relisted::Unchanged);
        };
        if pages
            .first()
            .and_then(|(_, total)| *total)
            .is_some_and(|total| total > SEARCHED)
        {
            return self.each_member(http, most, scope.members).await;
        }
        let mut pulls: Vec<Pull> = pages.into_iter().flat_map(|(pulls, _)| pulls).collect();
        pulls.truncate(most.open);
        Ok(Relisted::Listed {
            pulls,
            repository: None,
            visibility: None,
            validators,
        })
    }

    /// `pulls`, each of a repository of the owner as its listing found it,
    /// read in full: as listed on GitLab, whose listing reads them so;
    /// through GitHub's GraphQL a page of them a request, across
    /// repositories; else each one's reviews and checks, and first itself
    /// where its listing named no head, at most `most.pulls` of them and
    /// the rest read as unknown; each one's changed paths too where `files`
    /// says, on GitLab at most `most.pulls` of them; each one's merge queue
    /// entry too where `queue` says, through GraphQL with it or on GitLab
    /// by one request a project (ADR-0078).
    ///
    /// # Errors
    ///
    /// Where one could not be asked or read.
    async fn read_in_full(
        &self,
        http: &Erased<'_, dyn DynHttp + '_>,
        (most, files, queue): (Most, bool, bool),
        pulls: Vec<Pull>,
    ) -> Result<Vec<Pull>, Unpolled> {
        let path = ReadPath::of(self.forge(), self.token.is_some());
        if path == ReadPath::Graphql {
            return self.batched(http, pulls, (files, queue)).await;
        }
        let mut read = Vec::with_capacity(pulls.len());
        for (place, mut pull) in pulls.into_iter().enumerate() {
            if place < most.pulls {
                self.detailed(http, &mut pull, (path, files)).await?;
            } else if path == ReadPath::Detailed {
                pull.unread = true;
            }
            read.push(pull);
        }
        if queue && path == ReadPath::Listing {
            let repositories: BTreeSet<String> =
                read.iter().map(|pull| pull.repository.clone()).collect();
            for repository in repositories {
                let origin = self.origin(&repository);
                let held = read.iter_mut().filter(|pull| pull.repository == repository);
                super::trains::trained(http, (&origin, self.token.as_ref()), held).await?;
            }
        }
        Ok(read)
    }

    /// `pull` read further on `path`: its changed paths alone where `files`
    /// says on GitLab, whose listing read the rest; else its head where its
    /// listing named none, then its reviews, its head's checks and, where
    /// `files` says, its changed paths.
    async fn detailed(
        &self,
        http: &Erased<'_, dyn DynHttp + '_>,
        pull: &mut Pull,
        (path, files): (ReadPath, bool),
    ) -> Result<(), Unpolled> {
        let origin = self.origin(&pull.repository);
        let project = pull.project.clone();
        let reading = (Most::default(), files);
        let opened = self.reading(&origin, (&project, pull.visibility), reading);
        if path == ReadPath::Listing {
            return opened.changed(http, pull).await;
        }
        if pull.head.is_none() {
            let what = opened.what();
            let request = origin.get(&listing::one_path(pull.number), self.token.as_ref())?;
            let body = opened.asked(http, request, what).await?;
            listing::one(what, &body, origin.reading().parse, pull)
                .map_err(Unpolled::unreadable)?;
        }
        opened.detailed(http, pull).await
    }

    /// `pulls` read in full through GitHub's GraphQL, a page of them a
    /// request, each request asking for every repository its page holds.
    async fn batched(
        &self,
        http: &Erased<'_, dyn DynHttp + '_>,
        mut pulls: Vec<Pull>,
        (files, queue): (bool, bool),
    ) -> Result<Vec<Pull>, Unpolled> {
        pulls.sort_by(|one, other| one.repository.cmp(&other.repository));
        let parse = self.root.reading().parse;
        let ask = github::Ask {
            parse,
            files,
            queue,
        };
        let mut read = Vec::with_capacity(pulls.len());
        for chunk in pulls.chunks(PAGE) {
            let mut groups: Vec<(&str, Vec<u64>)> = Vec::new();
            let mut named = Vec::new();
            for pull in chunk {
                match groups.last_mut() {
                    Some((path, numbers)) if *path == pull.repository => numbers.push(pull.number),
                    _ => {
                        groups.push((&pull.repository, vec![pull.number]));
                        named.push((
                            pull.project.as_str(),
                            pull.repository.as_str(),
                            Some(pull.visibility),
                        ));
                    }
                }
            }
            let request = authorized(
                self.forge(),
                github::scoped_request(&self.graphql, &groups, ask),
                self.token.as_ref(),
            );
            let answer = http.send(request).await.map_err(Unpolled::unreachable)?;
            checked(Forge::GitHub, github::WHAT, &answer)?;
            read.extend(github::scoped(&answer.body, parse, &named).map_err(Unpolled::unreadable)?);
        }
        Ok(read)
    }
}

impl herald_kinds::Queue for Scoped {
    fn scoped(&self) -> bool {
        true
    }

    fn holds(&self, repository: &str) -> bool {
        repository.rsplit_once('/').is_some_and(|(holder, _)| {
            self.forge()
                .holds((&self.owner, self.options.subgroups()), holder)
        })
    }

    fn advisories<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        (asked, shown): (Asked<'a>, &'a [&'a str]),
        advising: (herald_core::Advising, jiff::Timestamp),
    ) -> herald_ports::Boxed<'a, Result<herald_core::Advisories, Unpolled>> {
        Box::pin(async move {
            self.advisories_of(&Erased(http), (asked, shown), advising)
                .await
        })
    }

    fn capacity<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        (asked, shown): (Asked<'a>, &'a [&'a str]),
        capacity: herald_core::Capacity,
    ) -> herald_ports::Boxed<'a, Result<herald_core::Overviews, Unpolled>> {
        Box::pin(async move {
            self.capacity_of(&Erased(http), (asked, shown), capacity)
                .await
        })
    }

    fn planned<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        (repository, head, check): (&'a str, &'a str, &'a str),
        most: usize,
    ) -> herald_ports::Boxed<'a, Result<herald_kinds::Plan, Unpolled>> {
        Box::pin(async move {
            let origin = self.origin(repository);
            let reading = (Most::default(), false);
            let opened = self.reading(&origin, ("", herald_core::Visibility::Private), reading);
            opened.planned(&Erased(http), (head, check), most).await
        })
    }

    fn changed<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        (repository, number): (&'a str, u64),
    ) -> herald_ports::Boxed<'a, Result<herald_core::Changed, Unpolled>> {
        Box::pin(async move {
            let origin = self.origin(repository);
            let reading = (Most::default(), false);
            let opened = self.reading(&origin, ("", herald_core::Visibility::Private), reading);
            opened.files_of(&Erased(http), number).await
        })
    }

    fn opened<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        asked: Asked<'a>,
    ) -> herald_ports::Boxed<'a, Result<Vec<Pull>, Unpolled>> {
        Box::pin(async move {
            let Some(scope) = asked.scope else {
                return Ok(Vec::new());
            };
            let http = Erased(http);
            let listed = self.relisted(&http, (asked.most, scope), &[]).await?;
            self.read_in_full(
                &http,
                (asked.most, asked.files, asked.queue),
                listed.pulls(),
            )
            .await
        })
    }

    fn listed<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        asked: Asked<'a>,
        validators: &'a [Validators],
    ) -> herald_ports::Boxed<'a, Result<Relisted, Unpolled>> {
        Box::pin(async move {
            match asked.scope {
                Some(scope) => {
                    self.relisted(&Erased(http), (asked.most, scope), validators)
                        .await
                }
                None => Ok(Relisted::Unchanged),
            }
        })
    }

    fn completed<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        asked: Asked<'a>,
        pulls: Vec<Pull>,
    ) -> herald_ports::Boxed<'a, Result<Vec<Pull>, Unpolled>> {
        Box::pin(async move {
            self.read_in_full(&Erased(http), (asked.most, asked.files, asked.queue), pulls)
                .await
        })
    }

    fn repository(&self) -> Option<&str> {
        None
    }

    fn codeowners<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        (repository, reference): (&'a str, Option<&'a str>),
        codeowning: (
            &'a herald_kinds::Codeowning,
            Option<&'a herald_core::KeptCodeowners>,
        ),
    ) -> herald_ports::Boxed<'a, Result<Option<herald_kinds::Found>, Unpolled>> {
        Box::pin(async move {
            let origin = self.origin(repository);
            let reading = (&origin, self.token.as_ref());
            super::codeowners::found(&Erased(http), reading, reference, codeowning).await
        })
    }

    fn named<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        name: &'a str,
    ) -> herald_ports::Boxed<'a, Result<herald_core::AreaKind, Unpolled>> {
        Box::pin(async move {
            let reading = (&self.root, self.token.as_ref());
            super::codeowners::named(&Erased(http), reading, name).await
        })
    }

    fn milestone_groups(&self) -> Option<&str> {
        self.options
            .milestone_choice()
            .group
            .then_some(self.owner.as_str())
    }

    fn milestoned<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        (headed, glob): (&'a herald_core::Headed, &'a herald_core::Glob),
        asked: (herald_core::HeaderRead, jiff::Timestamp),
    ) -> herald_ports::Boxed<'a, Result<herald_core::Header, Unpolled>> {
        Box::pin(async move {
            let member = (!headed.group).then(|| self.origin(&headed.path));
            let reading = (member.as_ref().unwrap_or(&self.root), self.token.as_ref());
            crate::milestones::header(&Erased(http), reading, (headed, glob), asked).await
        })
    }

    fn based<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        (repository, branch): (&'a str, &'a str),
        kept: Option<&'a herald_core::KeptBase>,
    ) -> herald_ports::Boxed<'a, Result<herald_kinds::BaseFound, Unpolled>> {
        Box::pin(async move {
            let origin = self.origin(repository);
            let reading = (&origin, self.token.as_ref());
            super::base::based(&Erased(http), reading, branch, kept).await
        })
    }

    fn team_owner(&self) -> String {
        format!("{} {}", self.root.api(), self.owner)
    }

    fn teams<'a>(
        &'a self,
        http: &'a dyn DynHttp,
    ) -> herald_ports::Boxed<'a, Result<herald_kinds::Teamed, Unpolled>> {
        Box::pin(async move {
            let reading = (&self.root, self.token.as_ref());
            super::teams::teams(&Erased(http), reading, &self.owner).await
        })
    }
}

/// The most of `open` pull requests one query over an owner's repositories
/// on `forge` lists: GitHub's search lists no more than [`SEARCHED`].
fn queried(forge: Forge, open: usize) -> usize {
    match forge {
        Forge::GitHub => open.min(SEARCHED),
        Forge::GitLab | Forge::Forgejo => open,
    }
}

/// What reading at most `most` open pull requests of an owner's
/// repositories on `forge` costs each round by one query over them,
/// `tokened` where its source reads with a token (ADR-0062).
#[must_use]
pub(crate) fn scope_cost(forge: Forge, tokened: bool, most: Most) -> herald_kinds::QueueCost {
    let read = ReadPath::of(forge, tokened);
    let path = match forge {
        Forge::GitHub => "search",
        Forge::GitLab => "group listing",
        Forge::Forgejo => "issue search",
    };
    let open = queried(forge, most.open);
    let pages = open.div_ceil(PAGE);
    let completing = match read {
        ReadPath::Listing => 0,
        ReadPath::Graphql => open.div_ceil(PAGE),
        ReadPath::Detailed => most.pulls.min(open) * (PER_PULL + 1),
    };
    herald_kinds::QueueCost {
        path,
        requests: pages + completing,
        detailed: read == ReadPath::Detailed,
        mergeability: !matches!((forge, read), (Forge::GitHub, ReadPath::Detailed)),
        scoped: true,
        files: super::files_cost(
            read,
            Most {
                open,
                pulls: most.pulls,
            },
        ),
        queue: super::queue_read(forge, tokened, open),
    }
}

#[cfg(test)]
mod tests;
