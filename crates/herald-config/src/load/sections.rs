//! The top-level sections of `herald.toml`: the root table split into them,
//! the version checked, and the sections read in one step from their file
//! shape.

use herald_core::SecretRef;
use herald_kinds::Store;
use herald_ports::Outbound;
use toml::de::DeTable;

use super::{Loader, Value, list};
use crate::config::{Config, Schedule, Server};
use crate::file::{HttpFile, MarkersFile, ScheduleFile, ServerFile};

/// The top-level keys of `herald.toml`.
const SECTIONS: [&str; 15] = [
    "version",
    "kinds",
    "store",
    "markers",
    "destinations",
    "sources",
    "routes",
    "templates",
    "apps",
    "schedule",
    "server",
    "http",
    "render",
    "announce",
    "run",
];

/// The sections of the file, before each is read.
#[derive(Default)]
pub(super) struct Sections<'i> {
    pub(super) version: Option<Value<'i>>,
    pub(super) kinds: Option<Value<'i>>,
    pub(super) store: Option<Value<'i>>,
    pub(super) markers: Option<Value<'i>>,
    pub(super) destinations: Option<Value<'i>>,
    pub(super) sources: Option<Value<'i>>,
    pub(super) routes: Option<Value<'i>>,
    pub(super) templates: Option<Value<'i>>,
    pub(super) apps: Option<Value<'i>>,
    pub(super) schedule: Option<Value<'i>>,
    pub(super) server: Option<Value<'i>>,
    pub(super) http: Option<Value<'i>>,
    pub(super) render: Option<Value<'i>>,
    pub(super) announce: Option<Value<'i>>,
    pub(super) run: Option<Value<'i>>,
}

impl Loader<'_> {
    pub(super) fn sections<'i>(&mut self, root: DeTable<'i>) -> Sections<'i> {
        let mut sections = Sections::default();
        for (key, value) in root {
            let slot = match key.get_ref().as_ref() {
                "version" => &mut sections.version,
                "kinds" => &mut sections.kinds,
                "store" => &mut sections.store,
                "markers" => &mut sections.markers,
                "destinations" => &mut sections.destinations,
                "sources" => &mut sections.sources,
                "routes" => &mut sections.routes,
                "templates" => &mut sections.templates,
                "apps" => &mut sections.apps,
                "schedule" => &mut sections.schedule,
                "server" => &mut sections.server,
                "http" => &mut sections.http,
                "render" => &mut sections.render,
                "announce" => &mut sections.announce,
                "run" => &mut sections.run,
                other => {
                    self.error(
                        &key.span(),
                        other,
                        format!(
                            "{} has no key \"{other}\"; the keys are {}",
                            self.file,
                            list(&SECTIONS)
                        ),
                    );
                    continue;
                }
            };
            *slot = Some(value);
        }
        sections
    }

    pub(super) fn version(&mut self, value: Option<Value<'_>>) {
        let Some(value) = value else {
            self.error(
                &(0..0),
                "version",
                format!("give the config's version: version = {}", Config::VERSION),
            );
            return;
        };
        let span = value.span();
        if self.typed::<i64>(value, "version") != Some(Config::VERSION) {
            self.error(
                &span,
                "version",
                format!(
                    "this herald reads version {0} of the config; write version = {0}",
                    Config::VERSION
                ),
            );
        }
    }

    /// The secret `[markers]` names as the marker key, where it names one.
    pub(super) fn marker_key(&mut self, value: Option<Value<'_>>) -> Option<SecretRef> {
        value
            .and_then(|value| self.typed::<MarkersFile>(value, "markers"))
            .map(|markers| markers.key)
    }

    /// How often `[schedule]` polls and keeps house, or as shipped.
    pub(super) fn schedule(&mut self, value: Option<Value<'_>>) -> Schedule {
        let schedule = value
            .and_then(|value| self.typed::<ScheduleFile>(value, "schedule"))
            .unwrap_or_default();
        Schedule {
            poll: schedule.poll.map(|poll| poll.0),
            housekeep: schedule.housekeep.map(|housekeep| housekeep.0),
        }
    }

    /// `[server]` as the file writes it, or as shipped, over the store
    /// `store`.
    pub(super) fn server_section(
        &mut self,
        value: Option<Value<'_>>,
        store: Option<&Store>,
    ) -> Server {
        value
            .and_then(|value| self.typed::<ServerFile>(value, "server"))
            .map(|file| self.server(file, store))
            .unwrap_or_default()
    }

    /// `[http]` as the file writes it, or as shipped, its CA file read
    /// through `read`.
    pub(super) fn http_section(
        &mut self,
        value: Option<Value<'_>>,
        read: &dyn Fn(&str) -> Result<String, String>,
    ) -> Outbound {
        value
            .and_then(|value| self.typed::<HttpFile>(value, "http"))
            .map(|file| self.http(file, read))
            .unwrap_or_default()
    }
}
