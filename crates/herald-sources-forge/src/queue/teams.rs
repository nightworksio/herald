//! The teams of the organization or group owning a source's repositories,
//! read for a view's load where the source says `teams = "forge"`, each
//! with its members' logins, held for the round alone (ADR-0077).

use herald_core::{MOST_FORGE_TEAMS, MOST_TEAM_MEMBERS, Team, Teams};
use herald_kinds::{Teamed, Unpolled};
use herald_ports::{DynHttp, Erased, Http as _};
use herald_wire::answered;
use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::forge::{OWNER, whole};
use crate::limited::limited;
use crate::{Forge, Origin, Token};

/// What a forge's listing of teams is called in an error.
const TEAMS: &str = "team listing";

/// What a forge's listing of a team's members is called in an error.
const MEMBERS: &str = "team member listing";

/// What a request names a team by.
const TEAM: &str = "team";

/// The answers a forge refuses a listing of teams or members with: no
/// token, a token without the permission, or no such organization or group
/// as the token sees.
const REFUSED: [u16; 3] = [401, 403, 404];

/// The most teams or members one page lists on `forge`: Forgejo and Gitea
/// list at most 50 by default.
const fn page(forge: Forge) -> usize {
    match forge {
        Forge::Forgejo => 50,
        Forge::GitHub | Forge::GitLab => 100,
    }
}

/// The permission a token on `forge` needs to read its teams.
pub(crate) const fn permission(forge: Forge) -> &'static str {
    match forge {
        Forge::GitHub => {
            "Members: read as a GitHub App or fine-grained token, or read:org as a classic token, on GitHub Enterprise Server and GHE.com too"
        }
        Forge::GitLab => {
            "read_api, as a member of the group who may see its members, on GitLab self-managed and Dedicated too"
        }
        Forge::Forgejo => {
            "read:organization, as a member of the organization who may see its teams, on Gitea too"
        }
    }
}

/// The most requests one read of the teams of an owner on `forge` takes:
/// the pages listing them, and the pages listing each one's members.
pub(crate) const fn cost(forge: Forge) -> usize {
    let page = page(forge);
    MOST_FORGE_TEAMS.div_ceil(page) + MOST_FORGE_TEAMS * MOST_TEAM_MEMBERS.div_ceil(page)
}

/// What doctor says of a source on `forge` reading its forge's teams, after
/// the source's name.
pub(crate) fn note(forge: Forge) -> String {
    let which = match forge {
        Forge::GitHub => {
            "its organization's teams, each named org/team as a review request names it"
        }
        Forge::GitLab => {
            "its group and each group under it, each named by its full path as a CODEOWNERS file names it, with its direct members"
        }
        Forge::Forgejo => "its organization's teams, each named org/team",
    };
    format!(
        "reads {which}, for each view showing its load, each round it is read: at most {MOST_FORGE_TEAMS} teams, each with at most {MOST_TEAM_MEMBERS} members, by at most {} requests, none conditional, since herald keeps no team. Their members' logins are held for the round alone, to tell which team a review request or a review counts for, and are never shown or kept; a team the config names keeps the config's members. A forge team's name and figures reach only a route that says private = true and herald queue with --private or [cli] private. Its token needs {}; a forge that refuses leaves the load to the config's teams, so an area only a forge team gives members to folds, and herald doctor --online names the permission; a forge whose rate limit is reached stops the view's read until the limit resets, never folding.",
        cost(forge),
        permission(forge)
    )
}

/// The owner of the repository at the full path `repository` on `forge`:
/// its organization or user, or its GitLab project's group or namespace.
pub(crate) fn owner_of(forge: Forge, repository: &str) -> &str {
    let split = match forge {
        Forge::GitLab => repository.rsplit_once('/'),
        Forge::GitHub | Forge::Forgejo => repository.split_once('/'),
    };
    split.map_or(repository, |(owner, _)| owner)
}

/// `host` without its port, as a pull request's page names its host.
fn portless(host: &str) -> &str {
    host.rsplit_once(':')
        .filter(|(_, port)| !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()))
        .map_or(host, |(named, _)| named)
}

/// A GitHub team, as its listing names it.
#[derive(Deserialize)]
struct Slugged {
    slug: String,
}

/// A Forgejo or Gitea team, as its listing names it.
#[derive(Deserialize)]
struct Numbered {
    id: u64,
    name: String,
}

/// A GitLab group under another, as its listing names it.
#[derive(Deserialize)]
struct Grouped {
    id: u64,
    full_path: String,
}

/// A team's member, as each forge's listing names it.
#[derive(Deserialize)]
struct Member {
    #[serde(alias = "username")]
    login: String,
}

/// How a source reads: the API it asks under and its token.
type Reading<'a> = (&'a Origin, Option<&'a Token>);

/// Each item of the listing at `path` under the API of `reading`, its query
/// begun, at most `most`, a page at a time through `http`, read as `what`;
/// none where the forge refuses it.
async fn paged<T: DeserializeOwned>(
    http: &Erased<'_, dyn DynHttp + '_>,
    (origin, token): Reading<'_>,
    path: &str,
    (most, what): (usize, &'static str),
) -> Result<Option<Vec<T>>, Unpolled> {
    let forge = origin.forge();
    let size = page(forge);
    let sized = match forge {
        Forge::Forgejo => "limit",
        Forge::GitHub | Forge::GitLab => "per_page",
    };
    let mut read = Vec::new();
    for number in 1..=most.div_ceil(size) {
        let request = origin.rooted(&format!("{path}{sized}={size}&page={number}"), token);
        let answer = http.send(request).await.map_err(|_| {
            Unpolled::Unreachable(format!("the forge's {what} could not be reached"))
        })?;
        if let Some(limited) = limited(forge, what, &answer) {
            return Err(limited);
        }
        if REFUSED.contains(&answer.status) {
            return Ok(None);
        }
        answered(what, &answer).map_err(Unpolled::unreadable)?;
        let listed: Vec<T> = serde_json::from_slice(&answer.body).map_err(|_| {
            Unpolled::Unreadable(format!("the forge's {what} is not one herald reads"))
        })?;
        let more = match forge {
            Forge::Forgejo => !listed.is_empty(),
            Forge::GitHub | Forge::GitLab => listed.len() >= size,
        };
        read.extend(listed);
        if !more {
            break;
        }
    }
    read.truncate(most);
    Ok(Some(read))
}

/// Each team of `owner` on the forge of `reading`, by its name, with the
/// path listing its members, at most [`MOST_FORGE_TEAMS`]; none where the
/// forge refuses the listing.
async fn listed(
    http: &Erased<'_, dyn DynHttp + '_>,
    reading: Reading<'_>,
    owner: &str,
) -> Result<Option<Vec<(String, String)>>, Unpolled> {
    let named = whole(owner, OWNER)?;
    let bound = (MOST_FORGE_TEAMS, TEAMS);
    Ok(match reading.0.forge() {
        Forge::GitHub => {
            let path = format!("/orgs/{named}/teams?");
            let Some(teams) = paged::<Slugged>(http, reading, &path, bound).await? else {
                return Ok(None);
            };
            let mut listed = Vec::new();
            for team in teams {
                let slug = whole(&team.slug, TEAM)?;
                let members = format!("/orgs/{named}/teams/{slug}/members?");
                listed.push((format!("{owner}/{}", team.slug), members));
            }
            Some(listed)
        }
        Forge::Forgejo => {
            let path = format!("/orgs/{named}/teams?");
            paged::<Numbered>(http, reading, &path, bound)
                .await?
                .map(|teams| {
                    teams
                        .into_iter()
                        .map(|team| {
                            let members = format!("/teams/{}/members?", team.id);
                            (format!("{owner}/{}", team.name), members)
                        })
                        .collect()
                })
        }
        Forge::GitLab => {
            let path = format!("/groups/{named}/descendant_groups?");
            let under = (MOST_FORGE_TEAMS - 1, TEAMS);
            paged::<Grouped>(http, reading, &path, under)
                .await?
                .map(|groups| {
                    let itself = (owner.to_owned(), format!("/groups/{named}/members?"));
                    std::iter::once(itself)
                        .chain(groups.into_iter().map(|group| {
                            let members = format!("/groups/{}/members?", group.id);
                            (group.full_path, members)
                        }))
                        .collect()
                })
        }
    })
}

/// The teams of `owner`, read under the API of `reading` through `http`:
/// each with its members' logins on the forge's host, a team the forge
/// names unlike any team herald reads left out; refused, with the
/// permission the token needs, where the forge refuses a listing.
///
/// # Errors
///
/// Where the forge cannot be reached or a listing cannot be read.
pub(crate) async fn teams(
    http: &Erased<'_, dyn DynHttp + '_>,
    reading: Reading<'_>,
    owner: &str,
) -> Result<Teamed, Unpolled> {
    let refused = Teamed::Refused(permission(reading.0.forge()));
    let Some(listed) = listed(http, reading, owner).await? else {
        return Ok(refused);
    };
    let host = reading.0.host();
    let host = portless(&host);
    let mut teams = Teams::new();
    for (name, path) in listed {
        let bound = (MOST_TEAM_MEMBERS, MEMBERS);
        let Some(members) = paged::<Member>(http, reading, &path, bound).await? else {
            return Ok(refused);
        };
        let members: Vec<String> = members
            .into_iter()
            .map(|member| format!("{}@{host}", member.login))
            .collect();
        if let Ok(team) = Team::of(&name, &members) {
            teams.insert(name, team);
        }
    }
    Ok(Teamed::Read(teams))
}

#[cfg(test)]
mod tests;
