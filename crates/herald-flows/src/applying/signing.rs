//! An operator signed in by the forge's device flow: shown a code, then
//! asked after at the forge's interval until they finish, decline, or the
//! code expires (ADR-0068).

use herald_kinds::{Polled, Rerunner, Unpolled};
use herald_ports::{DynHttp, OperatorToken, Timer};
use jiff::SignedDuration;

use super::SigningIn;

/// How much longer each ask waits where the forge says to ask less often
/// (RFC 8628).
const SLOWER: SignedDuration = SignedDuration::from_secs(5);

/// The longest herald waits for a sign-in, whatever the forge says the
/// code lasts.
const MOST_WAITED: SignedDuration = SignedDuration::from_mins(15);

/// Why a sign-in ended where its code expired before the operator
/// finished.
pub const UNFINISHED: &str = "the sign-in was not finished before its code expired, so nothing was re-run; run the command again";

/// What an `Unpolled` says, in plain words.
pub(super) fn said(unpolled: Unpolled) -> String {
    unpolled.into_reason()
}

/// The operator signed in at the forge `rerunner` re-runs on, through
/// `web`, shown how by `told` as the source `source`'s, waiting by `timer`.
///
/// # Errors
///
/// Where the forge could not be asked, the operator declined, or the code
/// expired, saying so.
pub(super) async fn signed_in<T: Timer>(
    rerunner: &dyn Rerunner,
    (web, timer): (&dyn DynHttp, &T),
    (source, told): (&str, &dyn Fn(SigningIn<'_>)),
) -> Result<OperatorToken, String> {
    let device = rerunner.device(web).await.map_err(said)?;
    told(SigningIn {
        source,
        verification: &device.verification,
        user_code: &device.user_code,
        expires: device.expires,
    });
    let mut interval = device.interval;
    let mut waited = SignedDuration::ZERO;
    let lasts = device.expires.min(MOST_WAITED);
    loop {
        waited = waited.saturating_add(interval);
        if waited > lasts {
            return Err(UNFINISHED.to_owned());
        }
        timer.sleep(interval.unsigned_abs()).await;
        match rerunner.polled(web, &device).await.map_err(said)? {
            Polled::Pending => {}
            Polled::Slower => interval = interval.saturating_add(SLOWER),
            Polled::Signed(token) => return Ok(token),
            Polled::Ended(reason) => return Err(reason),
        }
    }
}
