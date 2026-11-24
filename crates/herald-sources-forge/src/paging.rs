//! A forge's listing read a page at a time, up to a bound.

use herald_kinds::Unpolled;
use herald_ports::{DynHttp, Erased, Http as _, Request};
use herald_wire::answered;
use serde::de::DeserializeOwned;

use crate::Forge;
use crate::limited::limited;

/// The answers a forge refuses a listing with: no token, a token without
/// the permission, or nothing there as the token sees.
const REFUSED: [u16; 3] = [401, 403, 404];

/// The most items one page lists on `forge`: Forgejo and Gitea list at
/// most 50 by default.
pub(crate) const fn page(forge: Forge) -> usize {
    match forge {
        Forge::Forgejo => 50,
        Forge::GitHub | Forge::GitLab => 100,
    }
}

/// Each item of the listing `ask` makes the request for, given the query
/// that sizes and numbers a page, at most `most`, a page at a time through
/// `http`, read as `what`, with whether the listing was read whole: never
/// where it holds more than `most`; none where `forge` refuses it.
///
/// # Errors
///
/// Where a request could not be made or asked, the rate limit was reached,
/// or a page is not a listing.
pub(crate) async fn paged<T: DeserializeOwned>(
    http: &Erased<'_, dyn DynHttp + '_>,
    forge: Forge,
    ask: &(dyn Fn(&str) -> Result<Request, Unpolled> + Sync),
    (most, what): (usize, &'static str),
) -> Result<Option<(Vec<T>, bool)>, Unpolled> {
    let size = page(forge);
    let sized = match forge {
        Forge::Forgejo => "limit",
        Forge::GitHub | Forge::GitLab => "per_page",
    };
    let mut read = Vec::new();
    let mut ended = false;
    for number in 1..=most.div_ceil(size) {
        let request = ask(&format!("{sized}={size}&page={number}"))?;
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
            ended = true;
            break;
        }
    }
    let whole = ended && read.len() <= most;
    read.truncate(most);
    Ok(Some((read, whole)))
}
