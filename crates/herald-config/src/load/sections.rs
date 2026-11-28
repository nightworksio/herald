//! The top-level sections of `herald.toml`: the root table split into them,
//! the version checked, and the sections read in one step from their file
//! shape.

use std::collections::BTreeSet;
use std::path::{Component, Path};

use herald_core::SecretRef;
use herald_kinds::ConfiguredStore;
use herald_ports::Outbound;
use toml::Spanned;
use toml::de::{DeTable, DeValue};

use super::{Loader, Value, list};
use crate::config::{Config, Schedule, Server};
use crate::file::{FilesFile, HttpFile, MarkersFile, ScheduleFile, ServerFile};
use crate::files::Files;

/// The top-level keys of `herald.toml`.
const SECTIONS: [&str; 31] = [
    "version",
    "files",
    "presets",
    "kinds",
    "store",
    "markers",
    "destinations",
    "sources",
    "routes",
    "templates",
    "apps",
    "github_apps",
    "views",
    "merge_queues",
    "reports",
    "rules",
    "boards",
    "models",
    "tasks",
    "teams",
    "schedule",
    "server",
    "federation",
    "peers",
    "http",
    "render",
    "announce",
    "run",
    "doctor",
    "cli",
    "log",
];

/// The sections of the file, before each is read.
#[derive(Default)]
pub(super) struct Sections<'i> {
    pub(super) version: Option<Value<'i>>,
    pub(super) files: Option<Value<'i>>,
    pub(super) presets: Option<Value<'i>>,
    pub(super) kinds: Option<Value<'i>>,
    pub(super) store: Option<Value<'i>>,
    pub(super) markers: Option<Value<'i>>,
    pub(super) destinations: Option<Value<'i>>,
    pub(super) sources: Option<Value<'i>>,
    pub(super) routes: Option<Value<'i>>,
    pub(super) templates: Option<Value<'i>>,
    pub(super) apps: Option<Value<'i>>,
    pub(super) github_apps: Option<Value<'i>>,
    pub(super) views: Option<Value<'i>>,
    pub(super) merge_queues: Option<Value<'i>>,
    pub(super) reports: Option<Value<'i>>,
    pub(super) rules: Option<Value<'i>>,
    pub(super) boards: Option<Value<'i>>,
    pub(super) models: Option<Value<'i>>,
    pub(super) tasks: Option<Value<'i>>,
    pub(super) teams: Option<Value<'i>>,
    pub(super) schedule: Option<Value<'i>>,
    pub(super) server: Option<Value<'i>>,
    pub(super) federation: Option<Value<'i>>,
    pub(super) peers: Option<Value<'i>>,
    pub(super) http: Option<Value<'i>>,
    pub(super) render: Option<Value<'i>>,
    pub(super) announce: Option<Value<'i>>,
    pub(super) run: Option<Value<'i>>,
    pub(super) doctor: Option<Value<'i>>,
    pub(super) cli: Option<Value<'i>>,
    pub(super) log: Option<Value<'i>>,
}

impl Sections<'_> {
    /// Each of the sections whose tables are named, with the names its
    /// table writes.
    pub(super) fn names(&self) -> [(&'static str, BTreeSet<String>); 5] {
        let names = |value: &Option<Value<'_>>| match value.as_ref().map(Spanned::get_ref) {
            Some(DeValue::Table(table)) => {
                table.keys().map(|key| key.get_ref().to_string()).collect()
            }
            _ => BTreeSet::new(),
        };
        [
            ("templates", names(&self.templates)),
            ("views", names(&self.views)),
            ("reports", names(&self.reports)),
            ("rules", names(&self.rules)),
            ("merge_queues", names(&self.merge_queues)),
        ]
    }
}

impl Loader<'_> {
    pub(super) fn sections<'i>(&mut self, root: DeTable<'i>) -> Sections<'i> {
        let mut sections = Sections::default();
        for (key, value) in root {
            let slot = match key.get_ref().as_ref() {
                "version" => &mut sections.version,
                "files" => &mut sections.files,
                "presets" => &mut sections.presets,
                "kinds" => &mut sections.kinds,
                "store" => &mut sections.store,
                "markers" => &mut sections.markers,
                "destinations" => &mut sections.destinations,
                "sources" => &mut sections.sources,
                "routes" => &mut sections.routes,
                "templates" => &mut sections.templates,
                "apps" => &mut sections.apps,
                "github_apps" => &mut sections.github_apps,
                "views" => &mut sections.views,
                "merge_queues" => &mut sections.merge_queues,
                "reports" => &mut sections.reports,
                "rules" => &mut sections.rules,
                "boards" => &mut sections.boards,
                "models" => &mut sections.models,
                "tasks" => &mut sections.tasks,
                "teams" => &mut sections.teams,
                "schedule" => &mut sections.schedule,
                "server" => &mut sections.server,
                "federation" => &mut sections.federation,
                "peers" => &mut sections.peers,
                "http" => &mut sections.http,
                "render" => &mut sections.render,
                "announce" => &mut sections.announce,
                "run" => &mut sections.run,
                "doctor" => &mut sections.doctor,
                "cli" => &mut sections.cli,
                "log" => &mut sections.log,
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

    /// The directories `[files] allowed` names, each absolute with no `..`
    /// step and, every link followed, inside what `files` lets a config
    /// reach. No refusal repeats the path it refuses.
    pub(super) fn allowed(&mut self, value: Option<Value<'_>>, files: &dyn Files) -> Vec<String> {
        let file = value
            .and_then(|value| self.typed::<FilesFile>(value, "files"))
            .unwrap_or_default();
        let reach = files.reach();
        let mut allowed = Vec::new();
        for directory in file.allowed {
            let path = Path::new(directory.get_ref());
            let refused = if !path.is_absolute()
                || path.components().any(|step| step == Component::ParentDir)
            {
                "a directory it lists is no absolute directory; name each from the root, such as \"/etc/herald\", with no .. step".to_owned()
            } else if !files
                .resolved(directory.get_ref())
                .is_some_and(|resolved| reach.holds(&resolved))
            {
                format!(
                    "a directory it lists, every link followed, {}; herald doctor names what this host lets [files] allowed reach",
                    reach.refusal()
                )
            } else {
                allowed.push(directory.into_inner());
                continue;
            };
            self.error(&directory.span(), "files.allowed", refused);
        }
        allowed
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
        store: Option<&ConfiguredStore>,
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
