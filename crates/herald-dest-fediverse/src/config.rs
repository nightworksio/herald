use herald_core::{KeyForm, Profile, Scheme, SecretRef, Signed, Symbols, WaitOptions, Waits};
use herald_kinds::{
    DestinationConfig, DestinationKind, Fault, Placed, Placement, Placing, Reached,
};
use herald_ports::Secret;

use crate::activitypub::{ActivityPub, ActorOptions, ActorRouteOptions, actor_profile};
use crate::mastodon::{self, Mastodon};
use crate::options::{DestinationOptions, RouteOptions};

/// An activity posted to an actor's inbox, signed as HTTP Signatures do.
pub const ACTOR_SCHEME: Scheme = Scheme {
    name: "activitypub",
    signed: Signed::HttpSignature,
    key: KeyForm::RsaPem,
};

/// The kind of a Mastodon destination, as the config spells it.
pub const MASTODON: &str = "mastodon";
/// The kind of herald's own ActivityPub actors, as the config spells it.
pub const ACTIVITYPUB: &str = "activitypub";

impl DestinationConfig for DestinationOptions {
    fn kind(&self) -> &'static str {
        MASTODON
    }

    fn symbols_written(&self) -> Option<Symbols> {
        self.symbols
    }

    fn waits_written(&self) -> Option<WaitOptions> {
        self.waits
    }

    fn platform_waits(&self) -> Waits {
        Mastodon::WAITS
    }

    fn credential(&self) -> Option<&SecretRef> {
        Some(&self.token)
    }

    fn secret_refs(&self) -> Vec<&SecretRef> {
        vec![&self.token]
    }

    fn profile(&self, placed: &Placed) -> Option<(Profile, Option<Profile>)> {
        Some((mastodon::profile(placed.of::<RouteOptions>()?), None))
    }

    fn reach(&self, placed: &Placed, _: &str, secret: Secret) -> Option<Reached> {
        let placed = placed.of::<RouteOptions>()?;
        Some(Reached::new(
            Mastodon::new(&self.server, secret, placed.clone())
                .reading_back(self.read_back.unwrap_or_default())
                .waiting(self.waits()),
        ))
    }
}

impl DestinationConfig for ActorOptions {
    fn kind(&self) -> &'static str {
        ACTIVITYPUB
    }

    fn symbols_written(&self) -> Option<Symbols> {
        self.symbols
    }

    fn waits_written(&self) -> Option<WaitOptions> {
        self.waits
    }

    fn credential(&self) -> Option<&SecretRef> {
        Some(&self.key)
    }

    fn secret_refs(&self) -> Vec<&SecretRef> {
        vec![&self.key]
    }

    fn acts(&self) -> bool {
        true
    }

    fn profile(&self, placed: &Placed) -> Option<(Profile, Option<Profile>)> {
        placed.of::<ActorRouteOptions>()?;
        Some((actor_profile(), None))
    }

    fn reach(&self, placed: &Placed, base: &str, secret: Secret) -> Option<Reached> {
        let placed = placed.of::<ActorRouteOptions>()?;
        Some(Reached::new(
            ActivityPub::new(&self.domain, base, secret, placed.clone())
                .waiting(self.waits())
                .paging(self.outbox_page())
                .governed(self.policy()),
        ))
    }
}

impl Placement for RouteOptions {}

impl Placement for ActorRouteOptions {}

/// Mastodon, as a kind of destination.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MastodonKind {}

impl DestinationKind for MastodonKind {
    const NAME: &str = MASTODON;
    const DESCRIPTION: &str = "An account on a server speaking the Mastodon client API.";
    const LABEL: &str = "Mastodon";
    const ROUTE: &str = "MastodonRoute";
    const ROUTE_DESCRIPTION: &str = "A route to a Mastodon destination.";

    type Options = DestinationOptions;
    type Route = RouteOptions;

    fn placed(
        _options: &DestinationOptions,
        route: &RouteOptions,
        _placing: Placing<'_>,
    ) -> Vec<Fault> {
        route.check().err().map(Fault::here).into_iter().collect()
    }
}

/// herald's own ActivityPub actors, as a kind of destination.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActorKind {}

impl DestinationKind for ActorKind {
    const NAME: &str = ACTIVITYPUB;
    const DESCRIPTION: &str = "herald's own ActivityPub actors, on its domain.";
    const LABEL: &str = "herald's ActivityPub actor";
    const ROUTE: &str = "ActivityPubRoute";
    const ROUTE_DESCRIPTION: &str = "A route announcing as one of herald's own ActivityPub actors.";

    type Options = ActorOptions;
    type Route = ActorRouteOptions;

    fn checked(
        mut options: ActorOptions,
        read: &dyn Fn(&str) -> Result<String, String>,
    ) -> Result<(ActorOptions, Vec<Fault>), Fault> {
        for (index, list) in options.lists.clone().iter().enumerate() {
            let blocked = list
                .read(read)
                .map_err(|message| Fault::at(format!("lists[{index}]"), message))?;
            options.block.extend(blocked);
        }
        options.check().map_err(Fault::here)?;
        Ok((options, Vec::new()))
    }

    fn placed(
        _options: &ActorOptions,
        route: &ActorRouteOptions,
        _placing: Placing<'_>,
    ) -> Vec<Fault> {
        route.check().err().map(Fault::here).into_iter().collect()
    }
}
