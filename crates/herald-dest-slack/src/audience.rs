//! Who may read a Slack route's channel, asked right before a run's first
//! advisories section is sent there: only a private channel shared with no
//! other organization is sent one (ADR-0067).

use herald_kinds::Audience;
use herald_ports::Http;
use serde_json::Value;

use crate::options::OrgShared;
use crate::request::get;
use crate::slack::Slack;
use crate::web_api::CONVERSATIONS_INFO;

/// What herald asks before a run's first advisory send, as doctor names
/// it.
pub const ASKS: &str = "Slack's conversations.info whether the channel is private and shared with no other organization, through the bot token, which needs the groups:read scope (channels:read for a public channel)";

/// Why a route reached by an incoming webhook is not checked, and how to
/// make it checkable.
pub const UNCHECKED: &str = "a Slack incoming webhook cannot read who may read its channel; give the destination a bot token with the groups:read scope, so herald checks it";

/// Why a channel was not read where the bot token lacks the scope.
pub const SCOPELESS: &str = "the bot token lacks the scope conversations.info needs to say who may read the channel: groups:read for a private channel, channels:read for a public one; add it to the app and reinstall it";

/// The requests the check takes.
pub(crate) const REQUESTS: usize = 1;

/// Who may read the channel of `slack`'s route, asked through `http`.
pub(crate) async fn audience<H: Http>(slack: &Slack, http: &H) -> Audience {
    if slack.credential.bot().is_none() {
        return Audience::Unread(UNCHECKED.to_owned());
    }
    let Some(channel) = slack.route.channel.as_deref() else {
        return Audience::Unread("the route names no channel".to_owned());
    };
    let query = format!("channel={}", herald_core::unreserved(channel));
    let answer = match http.send(get(slack, CONVERSATIONS_INFO, &query)).await {
        Ok(answer) => answer,
        Err(error) => {
            return Audience::Unread(format!(
                "Slack could not be asked who may read the channel: {error}"
            ));
        }
    };
    let said: Value = serde_json::from_slice(&answer.body).unwrap_or_default();
    if said.get("ok") != Some(&Value::Bool(true)) {
        return Audience::Unread(match said.get("error").and_then(Value::as_str) {
            Some("missing_scope") => SCOPELESS.to_owned(),
            Some(error) => format!("Slack would not say who may read the channel ({error})"),
            None => format!(
                "Slack would not say who may read the channel, as it answered {}",
                answer.status
            ),
        });
    }
    let flag = |name: &str| {
        said.get("channel")
            .and_then(|channel| channel.get(name))
            .and_then(Value::as_bool)
    };
    read(
        [flag("is_private"), flag("is_ext_shared"), flag("is_shared")],
        [flag("is_pending_ext_shared"), flag("is_org_shared")],
        slack.org_shared,
    )
}

/// Who may read a channel Slack says is `[private, ext_shared, shared]`,
/// each required, and `[pending, org_shared]`, each false where unsaid,
/// an organization-wide channel counting as `org_shared` says.
fn read(
    [private, ext_shared, shared]: [Option<bool>; 3],
    [pending, org]: [Option<bool>; 2],
    org_shared: OrgShared,
) -> Audience {
    let (Some(private), Some(ext_shared), Some(shared)) = (private, ext_shared, shared) else {
        return Audience::Unread(
            "Slack did not say whether the channel is private and whom it is shared with"
                .to_owned(),
        );
    };
    let (pending, org) = (pending.unwrap_or(false), org.unwrap_or(false));
    let public = if !private {
        Some("the channel is public: anyone in its workspace may join it")
    } else if ext_shared || pending {
        Some("the channel is shared with another organization through Slack Connect")
    } else if org && org_shared == OrgShared::Public {
        Some(
            "the channel is shared with every workspace of its Enterprise Grid organization, which counts as public unless the destination says org_shared = \"private\"",
        )
    } else if shared && !org {
        Some("the channel is shared beyond its workspace")
    } else {
        None
    };
    public.map_or(Audience::Private, |why| Audience::Public(why.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_private_channel_shared_with_no_other_organization_is_private() {
        let private = OrgShared::Private;
        let some = Some;
        assert_eq!(
            read(
                [some(true), some(false), some(false)],
                [None, None],
                private
            ),
            Audience::Private
        );
        assert_eq!(
            read(
                [some(true), some(false), some(true)],
                [None, some(true)],
                private
            ),
            Audience::Private
        );
        assert!(matches!(
            read(
                [some(true), some(false), some(true)],
                [None, some(true)],
                OrgShared::default()
            ),
            Audience::Public(_)
        ));
        for (flags, more, org_shared) in [
            (
                [some(false), some(false), some(false)],
                [None, None],
                private,
            ),
            ([some(true), some(true), some(true)], [None, None], private),
            (
                [some(true), some(false), some(false)],
                [some(true), None],
                private,
            ),
            (
                [some(true), some(false), some(true)],
                [None, some(true)],
                OrgShared::Public,
            ),
            ([some(true), some(false), some(true)], [None, None], private),
        ] {
            assert!(
                matches!(read(flags, more, org_shared), Audience::Public(_)),
                "{flags:?} {more:?}"
            );
        }
        for unsaid in [
            [None, some(false), some(false)],
            [some(true), None, some(false)],
            [some(true), some(false), None],
        ] {
            assert!(matches!(
                read(unsaid, [None, None], private),
                Audience::Unread(_)
            ));
        }
    }
}
