use herald_core::{
    ActorName, KeyForm, Profile, Scheme, SecretRef, Signed, Symbols, WaitOptions, Waits,
};
use herald_kinds::{
    Checking, DestinationConfig, DestinationKind, Fault, Inherited, Placed, Placement, Placing,
    Reached,
};
use herald_ports::Secret;

use crate::activitypub::{ACTOR, named_actor};
use crate::activitypub::{ActivityPub, ActorOptions, ActorRouteOptions, actor_profile};
use crate::mastodon::{self, Mastodon, PLATFORM};
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

    fn public(&self, placed: &Placed, _: &herald_core::Delivery) -> Option<&'static str> {
        let private = placed
            .of::<RouteOptions>()
            .is_some_and(|options| options.visibility == crate::options::Visibility::Private);
        (!private).then_some("its statuses are posted \"public\" or \"unlisted\", which anyone may read; set visibility = \"private\" to post to its followers alone")
    }

    fn checking(&self, placed: &Placed) -> Checking {
        match self.public(placed, &herald_core::Delivery::default()) {
            Some(_) => Checking::Unadvised,
            None => Checking::Asked {
                asks: crate::audience::ASKS,
                requests: crate::audience::REQUESTS,
            },
        }
    }

    fn recalled(&self, written: &str) -> Option<Placed> {
        serde_json::from_str::<RouteOptions>(written)
            .ok()
            .filter(|route| route.check().is_ok())
            .map(Placed::new)
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

    fn actor_domain(&self) -> Option<&str> {
        Some(&self.domain)
    }

    fn public(&self, _: &Placed, _: &herald_core::Delivery) -> Option<&'static str> {
        Some("herald's own ActivityPub actor serves its Notes to anyone who looks")
    }

    fn checking(&self, _: &Placed) -> Checking {
        Checking::Unadvised
    }

    fn recalled(&self, written: &str) -> Option<Placed> {
        serde_json::from_str::<ActorRouteOptions>(written)
            .ok()
            .filter(|route| route.check().is_ok())
            .map(Placed::new)
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
                .delivering(self.deliveries())
                .governed(self.policy()),
        ))
    }
}

impl Placement for RouteOptions {
    fn written(&self) -> Option<String> {
        serde_json::to_string(self).ok()
    }
}

impl Placement for ActorRouteOptions {
    fn written(&self) -> Option<String> {
        serde_json::to_string(self).ok()
    }

    fn actor(&self) -> Option<&str> {
        (!ActorName::templated(&self.actor)).then_some(self.actor.as_str())
    }

    fn names(&self, project: &str) -> Option<Result<String, String>> {
        Some(named_actor(&self.naming()?, project))
    }
}

/// Mastodon, as a kind of destination.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MastodonKind {}

impl DestinationKind for MastodonKind {
    const NAME: &str = MASTODON;
    const DESCRIPTION: &str = "An account on a server speaking the Mastodon client API.";
    const LABEL: &str = PLATFORM;
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
    const LABEL: &str = ACTOR;
    const ROUTE: &str = "ActivityPubRoute";
    const ROUTE_DESCRIPTION: &str = "A route announcing as one of herald's own ActivityPub actors.";

    type Options = ActorOptions;
    type Route = ActorRouteOptions;

    const INHERITS: &[Inherited] = &[
        Inherited {
            key: "domain",
            from: "domain",
            required: true,
        },
        Inherited {
            key: "key",
            from: "actor_key",
            required: true,
        },
        Inherited {
            key: "mode",
            from: "mode",
            required: false,
        },
        Inherited {
            key: "block",
            from: "block",
            required: false,
        },
        Inherited {
            key: "allow",
            from: "allow",
            required: false,
        },
        Inherited {
            key: "followers",
            from: "followers",
            required: false,
        },
        Inherited {
            key: "approve",
            from: "approve",
            required: false,
        },
        Inherited {
            key: "lists",
            from: "lists",
            required: false,
        },
    ];

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
