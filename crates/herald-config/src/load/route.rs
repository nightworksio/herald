use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use herald_core::{
    Delivery, DestinationName, Filter, Kind, MentionStyle, Mentions, Pins, Profile, Route, RouteId,
    SourceName, TemplateName, Thread,
};
use toml::Spanned;
use toml::de::{DeTable, DeValue};

use super::{Loader, Value, Written, list};
use crate::config::{Destination, RouteConfig, RouteOptions, Source};
use crate::file::{ROUTE_KEYS, RouteFile};

/// A route as its keys say, before its templates are compiled.
pub(super) struct Read {
    pub(super) config: RouteConfig,
    pub(super) title_span: Range<usize>,
    pub(super) body_span: Range<usize>,
    pub(super) body_path: String,
    pub(super) profile: Profile,
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

    #[allow(clippy::too_many_lines)]
    pub(super) fn route(
        &mut self,
        index: usize,
        value: Value<'_>,
        destinations: &BTreeMap<DestinationName, (Destination, Range<usize>)>,
        sources: &BTreeMap<SourceName, Source>,
        templates: &BTreeMap<TemplateName, Written>,
    ) -> Option<Read> {
        let path = format!("routes[{index}]");
        let (common, rest) = self.split(value, &path)?;
        let route = self.typed::<RouteFile>(common, &path)?;
        let mut sound = true;
        for source in &route.sources {
            if !sources.contains_key(&SourceName::new(source.get_ref())) {
                let names: Vec<&str> = sources.keys().map(SourceName::as_str).collect();
                self.error(
                    &source.span(),
                    &format!("{path}.sources"),
                    format!(
                        "no source is named \"{}\"; the sources are {}",
                        source.get_ref(),
                        list(&names)
                    ),
                );
                sound = false;
            }
        }
        let destination = DestinationName::new(route.destination.get_ref());
        let Some((Destination::Discord(reached), _)) = destinations.get(&destination) else {
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
        let options = self.typed::<herald_dest_discord::RouteOptions>(rest, &path)?;
        let bot = reached.bot().unwrap_or_default();
        let profile = herald_dest_discord::profile(bot, options.forum);
        let span = route.id.span();
        if bot && options.channel.is_none() {
            self.error(
                &span,
                &path,
                "a Discord bot posts to a channel; give channel = \"<the channel's id>\"",
            );
            sound = false;
        }
        let wants = [
            (route.crosspost && !profile.features.crosspost, "crosspost"),
            (
                route.pin.is_some() && profile.features.pins == Pins::None,
                "pin",
            ),
            (
                !route.mentions.roles.is_empty()
                    && profile.features.mentions != MentionStyle::Roles,
                "mentions.roles",
            ),
            (
                route.mentions.subscribers && profile.features.mentions != MentionStyle::Users,
                "mentions.subscribers",
            ),
        ];
        for (_, key) in wants.iter().filter(|(lacks, _)| *lacks) {
            self.error(
                &span,
                &format!("{path}.{key}"),
                format!(
                    "the destination \"{}\" cannot do this as it is reached; remove {key}, or reach it with a bot token",
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
                .filter_map(|source| sources.get(&SourceName::new(source.get_ref())))
                .flat_map(|source| source.kinds().iter().copied())
                .collect()
        } else {
            route.kinds.iter().copied().collect()
        };
        let thread_name = route.thread.as_ref().map(|thread| thread.name.clone());
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
                },
                destination,
                template,
                delivery: Delivery {
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
                            .map(herald_core::RoleId::new)
                            .collect(),
                        subscribers: route.mentions.subscribers,
                    },
                },
            },
            options: RouteOptions::Discord(options),
        };
        Some(Read {
            config,
            title_span: written.title.span(),
            body_span: written.body.span(),
            body_path: written.body_path.clone(),
            profile,
            sample_kinds,
            thread: thread_name,
            id_span: route.id.span(),
            path,
        })
    }
}
