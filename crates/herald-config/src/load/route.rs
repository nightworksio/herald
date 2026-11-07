use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use herald_core::{
    Delivery, DestinationName, Filter, Kind, Mentions, Pattern, Route, RouteId, SourceName,
    TemplateName, Thread,
};
use toml::Spanned;
use toml::de::{DeTable, DeValue};

use super::apps::Named;
use super::{Loader, Value, Written, list};
use crate::config::{Destination, RouteConfig, RouteOptions, Source};
use crate::file::{ROUTE_KEYS, RouteFile};

/// A route as its keys say, before its templates are compiled.
pub(super) struct Read {
    pub(super) config: RouteConfig,
    pub(super) title_span: Range<usize>,
    pub(super) body_span: Range<usize>,
    pub(super) body_path: String,
    pub(super) sample_kinds: BTreeSet<Kind>,
    pub(super) thread: Option<Spanned<String>>,
    pub(super) id_span: Range<usize>,
    pub(super) path: String,
}

impl Loader<'_> {
    fn split<'i>(&mut self, value: Value<'i>, path: &str) -> Option<(Value<'i>, Value<'i>)> {
        let (span, table) = self.table(value, path)?;
        let (common, rest): (DeTable<'i>, DeTable<'i>) = table
            .into_iter()
            .partition(|(key, _)| ROUTE_KEYS.contains(&key.get_ref().as_ref()));
        Some((
            Spanned::new(span.clone(), DeValue::Table(common)),
            Spanned::new(span, DeValue::Table(rest)),
        ))
    }

    fn sources_named(
        &mut self,
        route: &RouteFile,
        path: &str,
        sources: &BTreeMap<SourceName, Source>,
        apps: &BTreeMap<String, Named>,
    ) -> bool {
        let mut sound = true;
        for source in &route.sources {
            let name = source.get_ref();
            if !sources.contains_key(&SourceName::new(name)) && !apps.contains_key(name) {
                let mut names: Vec<&str> = sources
                    .keys()
                    .map(SourceName::as_str)
                    .chain(apps.keys().map(String::as_str))
                    .collect();
                names.sort_unstable();
                self.error(
                    &source.span(),
                    &format!("{path}.sources"),
                    format!(
                        "no source or app is named \"{name}\"; the sources and apps are {}",
                        list(&names)
                    ),
                );
                sound = false;
            }
        }
        sound
    }

    fn placed(
        &mut self,
        reached: &Destination,
        rest: Value<'_>,
        route: &RouteFile,
        path: &str,
        destination: &DestinationName,
        sound: &mut bool,
    ) -> Option<(RouteOptions, &'static str)> {
        let span = route.id.span();
        Some(match reached {
            Destination::Discord(reached) => {
                let options = self.typed::<herald_dest_discord::RouteOptions>(rest, path)?;
                if reached.token.is_some() && options.channel.is_none() {
                    self.error(
                        &span,
                        path,
                        "a Discord bot posts to a channel; give channel = \"<the channel's id>\"",
                    );
                    *sound = false;
                }
                if let Some(subscribe) = &options.subscribe
                    && let Err(message) = discord_subscribe(reached, subscribe, destination)
                {
                    self.error(&span, &format!("{path}.subscribe"), message);
                    *sound = false;
                }
                (RouteOptions::Discord(options), "Discord")
            }
            Destination::Mastodon(_) => {
                let options = self.typed::<herald_dest_fediverse::RouteOptions>(rest, path)?;
                if let Err(message) = options.check() {
                    self.error(&span, path, message);
                    *sound = false;
                }
                (RouteOptions::Mastodon(options), "Mastodon")
            }
            Destination::Matrix(_) => {
                let options = self.typed::<herald_dest_matrix::RouteOptions>(rest, path)?;
                if let Err(message) = options.check() {
                    self.error(&span, path, message);
                    *sound = false;
                }
                (RouteOptions::Matrix(options), "Matrix")
            }
            Destination::Slack(reached) => {
                let options = self.typed::<herald_dest_slack::RouteOptions>(rest, path)?;
                if let Err(message) = options.check() {
                    self.error(&span, path, message);
                    *sound = false;
                }
                if reached.token.is_some() && options.channel.is_none() {
                    self.error(
                        &span,
                        path,
                        "a Slack bot posts to a channel; give channel = \"<the channel's id>\"",
                    );
                    *sound = false;
                }
                if options.subscribe.is_some()
                    && let Err(message) =
                        slack_subscribe(reached, route.mentions.subscribers, destination)
                {
                    self.error(&span, &format!("{path}.subscribe"), message);
                    *sound = false;
                }
                (RouteOptions::Slack(options), "Slack")
            }
            Destination::ActivityPub(_) => {
                let options = self.typed::<herald_dest_fediverse::ActorRouteOptions>(rest, path)?;
                if let Err(message) = options.check() {
                    self.error(&span, path, message);
                    *sound = false;
                }
                (
                    RouteOptions::ActivityPub(options),
                    "herald's ActivityPub actor",
                )
            }
        })
    }

    #[allow(clippy::too_many_lines)]
    pub(super) fn route(
        &mut self,
        index: usize,
        value: Value<'_>,
        destinations: &BTreeMap<DestinationName, (Destination, Range<usize>)>,
        sources: &BTreeMap<SourceName, Source>,
        apps: &BTreeMap<String, Named>,
        templates: &BTreeMap<TemplateName, Written>,
    ) -> Option<Read> {
        let path = format!("routes[{index}]");
        let (common, rest) = self.split(value, &path)?;
        let route = self.typed::<RouteFile>(common, &path)?;
        let mut sound = self.sources_named(&route, &path, sources, apps);
        let destination = DestinationName::new(route.destination.get_ref());
        let Some((reached, _)) = destinations.get(&destination) else {
            let names: Vec<&str> = destinations.keys().map(DestinationName::as_str).collect();
            self.error(
                &route.destination.span(),
                &format!("{path}.destination"),
                format!(
                    "no destination is named \"{}\"; the destinations are {}",
                    route.destination.get_ref(),
                    list(&names)
                ),
            );
            return None;
        };
        let template = TemplateName::new(route.template.get_ref());
        let written = templates.get(&template);
        if written.is_none() {
            let names: Vec<&str> = templates.keys().map(TemplateName::as_str).collect();
            self.error(
                &route.template.span(),
                &format!("{path}.template"),
                format!(
                    "no template is named \"{}\"; the templates are {}",
                    route.template.get_ref(),
                    list(&names)
                ),
            );
            sound = false;
        }
        let span = route.id.span();
        let (options, name) =
            self.placed(reached, rest, &route, &path, &destination, &mut sound)?;
        let (profile, as_bot) = match (&options, reached) {
            (RouteOptions::Discord(placed), Destination::Discord(reached)) => (
                herald_dest_discord::profile(reached.token.is_some(), placed.forum),
                Some(herald_dest_discord::profile(true, placed.forum)),
            ),
            (RouteOptions::Slack(_), Destination::Slack(reached)) => (
                herald_dest_slack::profile(reached.token.is_some()),
                Some(herald_dest_slack::profile(true)),
            ),
            (RouteOptions::Mastodon(placed), _) => (herald_dest_fediverse::profile(placed), None),
            (RouteOptions::ActivityPub(_), _) => (herald_dest_fediverse::actor_profile(), None),
            (_, _) => (herald_dest_matrix::profile(), None),
        };
        let thread_name = route.thread.as_ref().map(|thread| thread.name.clone());
        let delivery = Delivery {
            crosspost: route.crosspost,
            thread: route.thread.map(|thread| Thread {
                name: thread.name.into_inner(),
                archive_after: thread.archive_after.map(|after| after.0),
                lock_after: thread.lock_after.map(|after| after.0),
            }),
            pin: route.pin,
            mentions: Mentions {
                roles: route
                    .mentions
                    .roles
                    .into_iter()
                    .chain(options.opt_in_role())
                    .fold(Vec::new(), |mut roles, role| {
                        if !roles.contains(&role) {
                            roles.push(role);
                        }
                        roles
                    })
                    .into_iter()
                    .map(herald_core::RoleId::new)
                    .collect(),
                subscribers: route.mentions.subscribers,
            },
        };
        let lacking = profile.features.lacking(&delivery);
        let bot_lacks = as_bot.map(|bot| bot.features.lacking(&delivery));
        for key in lacking
            .into_iter()
            .filter(|key| !(options.threads_each_post() && *key == "thread"))
        {
            let remedy = if bot_lacks
                .as_ref()
                .is_some_and(|lacks| !lacks.contains(&key))
            {
                "or reach it with a bot token".to_owned()
            } else {
                format!("as {name} has no way to do it")
            };
            self.error(
                &span,
                &format!("{path}.{key}"),
                format!(
                    "the destination \"{}\" cannot do this as it is reached; remove {key}, {remedy}",
                    destination.as_str()
                ),
            );
            sound = false;
        }
        let (Some(written), true) = (written, sound) else {
            return None;
        };
        let sample_kinds: BTreeSet<Kind> = if route.kinds.is_empty() {
            route
                .sources
                .iter()
                .flat_map(|source| {
                    let name = source.get_ref();
                    let app = apps.contains_key(name).then_some(Kind::Message);
                    sources
                        .get(&SourceName::new(name))
                        .map(|source| source.kinds().to_vec())
                        .unwrap_or_default()
                        .into_iter()
                        .chain(app)
                })
                .collect()
        } else {
            route.kinds.iter().copied().collect()
        };
        if let Some(versions) = &route.versions
            && sample_kinds.iter().any(|kind| !kind.versioned())
        {
            self.error(
                &versions.span(),
                &format!("{path}.versions"),
                "feed entries and app messages carry no version, so a route with versions takes none of them; give kinds = [\"release\", \"package-version\"], or remove versions",
            );
            return None;
        }
        let config = RouteConfig {
            route: Route {
                id: RouteId::new(route.id.get_ref()),
                filter: Filter {
                    sources: route
                        .sources
                        .iter()
                        .map(|source| SourceName::new(source.get_ref()))
                        .collect(),
                    kinds: route.kinds,
                    prereleases: route.prereleases,
                    versions: route
                        .versions
                        .map(|versions| Pattern::new(versions.into_inner())),
                },
                destination,
                template,
                delivery,
            },
            options,
            profile,
        };
        Some(Read {
            config,
            title_span: written.title.span(),
            body_span: written.body.span(),
            body_path: written.body_path.clone(),
            sample_kinds,
            thread: thread_name,
            id_span: route.id.span(),
            path,
        })
    }
}

fn answers_subscribing(
    answering: Option<herald_core::Answering>,
    destination: &DestinationName,
) -> Result<(), String> {
    if answering.is_some_and(|answering| answering.takes(herald_core::Command::Subscribe)) {
        Ok(())
    } else {
        Err(format!(
            "subscribing is answered through the destination's interactions; give [destinations.{}.interactions] answering subscribe",
            destination.as_str()
        ))
    }
}

fn discord_subscribe(
    reached: &herald_dest_discord::DestinationOptions,
    subscribe: &herald_dest_discord::Subscribe,
    destination: &DestinationName,
) -> Result<(), String> {
    subscribe.check()?;
    if reached.token.is_none() {
        return Err(
            "a member is given the opt-in role by a bot; give the destination a token in place of its webhook".to_owned(),
        );
    }
    answers_subscribing(
        reached
            .interactions
            .as_ref()
            .map(herald_dest_discord::Interactions::answering),
        destination,
    )
}

fn slack_subscribe(
    reached: &herald_dest_slack::DestinationOptions,
    mentions_subscribers: bool,
    destination: &DestinationName,
) -> Result<(), String> {
    if !mentions_subscribers {
        return Err(
            "a member subscribes to be mentioned; add mentions = { subscribers = true } to the route".to_owned(),
        );
    }
    answers_subscribing(
        reached
            .interactions
            .as_ref()
            .map(herald_dest_slack::Interactions::answering),
        destination,
    )
}
