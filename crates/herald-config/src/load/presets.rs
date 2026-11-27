//! `[presets.*]` (ADR-0092): each table adopts a preset herald ships or a
//! local one beside `herald.toml`, whose tables are read as `herald.toml`'s
//! own are, each named for the adopting table and given its keys.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use toml::Spanned;
use toml::de::{DeTable, DeValue, ValueDeserializer};

use super::sections::Sections;
use super::{Loader, Value, list};
use crate::config::Config;
use crate::error::Errors;
use crate::files::Files;
use crate::kinds::Kinds;
use crate::presets::{Preset, SHIPPED, SHIPPED_PREFIX, shipped};

/// A table of a section, as where it is written, its name and its keys.
pub(super) type Part<'i> = (String, String, Value<'i>);

/// The tables every `[presets.*]` table expands to, by section.
#[derive(Default)]
pub(super) struct Expanded<'i> {
    pub(super) routes: Vec<(String, Value<'i>)>,
    pub(super) templates: Vec<Part<'i>>,
    pub(super) views: Vec<Part<'i>>,
    pub(super) reports: Vec<Part<'i>>,
    pub(super) rules: Vec<Part<'i>>,
}

/// A `[presets.*]` table: its name, where it is written, the preset it
/// adopts and the keys it gives that preset.
pub(super) struct Adopting<'i> {
    name: String,
    span: Range<usize>,
    preset: Adopted,
    keys: DeTable<'i>,
}

/// The preset a table adopts.
enum Adopted {
    Shipped(&'static Preset),
    Local { file: String, text: String },
}

/// The sections a local preset may write, beside its `version`.
const LOCAL_SECTIONS: [&str; 5] = ["routes", "templates", "views", "reports", "rules"];

/// The sections a preset's tables are named in, as a table of tables.
const NAMED_SECTIONS: [&str; 4] = ["templates", "views", "reports", "rules"];

/// What a local preset may write of one section's tables: the keys that
/// pick and shape what herald announces from what `herald.toml` already
/// lets it read and where it already lets it post. Every other key, those
/// below with why and each destination kind's own route options, is
/// refused, and the adopting table in `herald.toml` gives it (ADR-0092
/// decision 4). A shipped `herald:` preset is herald's own data and is not
/// read against these.
struct Writable {
    section: &'static str,
    allowed: &'static [&'static str],
    refused: &'static [(&'static [&'static str], &'static str)],
}

/// What a local preset may write, section by section.
const WRITABLE: [Writable; 5] = [
    Writable {
        section: "routes",
        allowed: &[
            "id",
            "sources",
            "kinds",
            "prereleases",
            "versions",
            "match",
            "area_paths",
            "template",
            "templates",
            "group",
        ],
        refused: &[
            (
                &["destination", "crosspost", "thread", "living"],
                "it decides where the route posts",
            ),
            (&["mentions"], "it decides whom the route calls on"),
            (
                &["pin", "edits", "withdraw", "late_release"],
                "it decides what the route does to posts it made",
            ),
            (
                &["private", "vouch_unchecked"],
                "it states what herald trusts of the route's place",
            ),
            (&["act"], "it lets the route act on the forge"),
        ],
    },
    Writable {
        section: "templates",
        allowed: &["title", "body"],
        refused: &[(&["file"], "it reads a file herald.toml does not name")],
    },
    Writable {
        section: "views",
        allowed: &[
            "sources",
            "states",
            "bots",
            "repositories",
            "authors",
            "labels",
            "areas",
            "stale",
            "stale_only",
            "group_by",
            "bumps",
            "rules",
            "release",
            "flaky",
            "deciding",
            "conflicts",
            "queue",
            "mode",
            "reconcile",
            "most_open",
            "most_pulls",
            "broken_at",
        ],
        refused: &[
            (
                &[
                    "overviews",
                    "advisories_from",
                    "advisory_repositories",
                    "most_advisories",
                    "capacity_from",
                    "capacity_repositories",
                    "load",
                    "listed_visibility",
                ],
                "it decides what the view discloses of advisories, members and teams",
            ),
            (&["decide"], "it hands pull requests to a model"),
        ],
    },
    Writable {
        section: "reports",
        allowed: &["view", "every", "at", "days", "zone"],
        refused: &[],
    },
    Writable {
        section: "rules",
        allowed: &[
            "view", "every", "at", "days", "zone", "stages", "most", "dequeued", "release",
        ],
        refused: &[(&["flaky"], "it plans re-runs of checks on the forge")],
    },
];

/// What a `use` that names no preset is told.
const USE: &str = "use names a preset herald ships, as \"herald:<name>\", or a local preset, a file inside herald.toml's directory named from it as \"./<path>\" with no \"..\" in it";

/// The keys of a route, a template, a view, a report and a rule that name
/// another table of a preset, and the section of the table each names.
const REFERENCES: [(&str, &str, &str); 6] = [
    ("routes", "template", "templates"),
    ("routes", "templates", "templates"),
    ("routes", "sources", "reports"),
    ("routes", "sources", "rules"),
    ("reports", "view", "views"),
    ("rules", "view", "views"),
];

/// Where the tables of each preset are read from, past the end of
/// `herald.toml`'s text, so an error in them is told at the adopting table.
pub(super) struct Region {
    start: usize,
    end: usize,
    at: Range<usize>,
    file: Option<String>,
    text: String,
}

impl Region {
    /// Whether the offset `at`, read past `herald.toml`'s text, falls in
    /// this region.
    pub(super) const fn holds(&self, at: usize) -> bool {
        self.start <= at && at <= self.end
    }

    /// Where an error at `at` within this region is told in `herald.toml`,
    /// and `message` with the place in a local preset's file in front.
    pub(super) fn told(&self, at: usize, message: String) -> (usize, String) {
        let Some(file) = &self.file else {
            return (self.at.start, message);
        };
        let (line, column) = crate::position::position(&self.text, at - self.start);
        (self.at.start, format!("{file}:{line}:{column}: {message}"))
    }
}

impl Loader<'_> {
    /// The sections of `root`, `syntax` the mistakes its TOML makes, and
    /// the tables its `[presets]` expand to, each preset kept in `adopting`.
    pub(super) fn expanding<'a>(
        &mut self,
        (root, syntax): (DeTable<'a>, &[toml::de::Error]),
        files: &dyn Files,
        adopting: &'a mut Vec<Adopting<'a>>,
    ) -> (Sections<'a>, Expanded<'a>) {
        self.syntax(syntax);
        let mut sections = self.sections(root);
        *adopting = self.adopting(sections.presets.take(), files);
        let adopting: &'a [Adopting<'a>] = adopting;
        let expanded = self.expanded(adopting, &sections);
        (sections, expanded)
    }

    /// Each table of `[presets]`, the preset it adopts found, a local
    /// preset's file read through `files`.
    pub(super) fn adopting<'i>(
        &mut self,
        value: Option<Value<'i>>,
        files: &dyn Files,
    ) -> Vec<Adopting<'i>> {
        let mut adopting = Vec::new();
        for (name, value) in self.named(value, "presets") {
            let path = format!("presets.{name}");
            let Some((span, mut keys)) = self.table(value, &path) else {
                continue;
            };
            let Some(used) = keys.remove("use") else {
                self.error(&span, &path, format!("give the preset it adopts: {USE}"));
                continue;
            };
            let at = used.span();
            let use_path = format!("{path}.use");
            let DeValue::String(used) = used.into_inner() else {
                self.error(&at, &use_path, USE);
                continue;
            };
            let Some(preset) = self.found(&used, files, (&at, &use_path)) else {
                continue;
            };
            adopting.push(Adopting {
                name,
                span,
                preset,
                keys,
            });
        }
        adopting
    }

    /// The preset `used` names, a local one read through `files`.
    fn found(
        &mut self,
        used: &str,
        files: &dyn Files,
        (at, path): (&Range<usize>, &str),
    ) -> Option<Adopted> {
        if used.starts_with(SHIPPED_PREFIX) {
            let preset = shipped(used);
            if preset.is_none() {
                let names: Vec<&str> = SHIPPED.iter().map(|preset| preset.name).collect();
                self.error(
                    at,
                    path,
                    format!(
                        "herald ships no preset \"{used}\"; it ships {}",
                        list(&names)
                    ),
                );
            }
            return preset.map(Adopted::Shipped);
        }
        if !inside(used) {
            self.error(at, path, USE);
            return None;
        }
        match files.read_inside(used) {
            Ok(text) => Some(Adopted::Local {
                file: used.to_owned(),
                text,
            }),
            Err(reason) => {
                self.error(at, path, format!("{used} could not be read: {reason}"));
                None
            }
        }
    }

    /// The tables each of `adopting` expands to, named for the table that
    /// adopts it, none named as one of `sections` already is.
    pub(super) fn expanded<'a>(
        &mut self,
        adopting: &'a [Adopting<'a>],
        sections: &Sections<'_>,
    ) -> Expanded<'a> {
        let mut taken: BTreeMap<&str, BTreeSet<String>> = sections.names().into_iter().collect();
        let mut expanded = Expanded::default();
        for adopted in adopting {
            let path = format!("presets.{}", adopted.name);
            let Some(mut tables) = self.tables(adopted, &path) else {
                continue;
            };
            tables.renamed(&adopted.name);
            self.given(adopted, &mut tables, &path);
            for (section, name, _) in &tables.named {
                let names = taken.entry(section).or_default();
                if !names.insert(name.clone()) {
                    self.error(
                        &adopted.span,
                        &path,
                        format!(
                            "it makes [{section}.{name}], which another table names; rename one of them"
                        ),
                    );
                }
            }
            let spanned =
                |table: DeTable<'a>| Spanned::new(adopted.span.clone(), DeValue::Table(table));
            for route in tables.routes {
                expanded.routes.push((path.clone(), spanned(route)));
            }
            for (section, name, table) in tables.named {
                let part = (path.clone(), name, spanned(table));
                match section {
                    "templates" => expanded.templates.push(part),
                    "views" => expanded.views.push(part),
                    "reports" => expanded.reports.push(part),
                    _ => expanded.rules.push(part),
                }
            }
        }
        expanded
    }

    /// The tables `adopted`'s preset writes, read as a region past
    /// `herald.toml`'s text, a local preset's checked.
    fn tables<'a>(&mut self, adopted: &'a Adopting<'a>, path: &str) -> Option<Tables<'a>> {
        let (text, file) = match &adopted.preset {
            Adopted::Shipped(preset) => (preset.tables, None),
            Adopted::Local { file, text } => (text.as_str(), Some(file.clone())),
        };
        let start = self
            .regions
            .last()
            .map_or(self.text.len(), |region| region.end)
            + 1;
        self.regions.push(Region {
            start,
            end: start + text.len(),
            at: adopted.span.clone(),
            text: if file.is_some() {
                text.to_owned()
            } else {
                String::new()
            },
            file,
        });
        let (root, syntax) = DeTable::parse_recoverable(text);
        for error in &syntax {
            let at = error.span().unwrap_or(0..0);
            self.error(&(at.start + start..at.end + start), path, error.message());
        }
        let root_span = root.span();
        let DeValue::Table(mut root) = shifted(
            Spanned::new(root_span, DeValue::Table(root.into_inner())),
            start,
        )
        .into_inner() else {
            return None;
        };
        if matches!(adopted.preset, Adopted::Local { .. }) {
            self.local(&mut root, (start, path));
        }
        Some(self.split(root, path))
    }

    /// `root`, a local preset's tables read from `start`, with its
    /// version checked and every section and key it may not write
    /// refused and taken out.
    fn local(&mut self, root: &mut DeTable<'_>, (start, path): (usize, &str)) {
        match root.remove("version") {
            Some(version) if is_version(version.get_ref()) => {}
            Some(version) => self.error(
                &version.span(),
                &format!("{path}.version"),
                format!(
                    "this herald reads version {0} of the config; write version = {0}",
                    Config::VERSION
                ),
            ),
            None => self.error(
                &(start..start),
                &format!("{path}.version"),
                format!(
                    "give the config format the preset is written for: version = {}",
                    Config::VERSION
                ),
            ),
        }
        let unwritable: Vec<(String, Range<usize>)> = root
            .iter()
            .filter(|(key, _)| !LOCAL_SECTIONS.contains(&key.get_ref().as_ref()))
            .map(|(key, _)| (key.get_ref().to_string(), key.span()))
            .collect();
        for (key, span) in unwritable {
            let message = if key == "presets" {
                "a local preset does not use another preset".to_owned()
            } else {
                format!(
                    "a local preset writes {} only; give {key} in herald.toml",
                    list(&LOCAL_SECTIONS)
                )
            };
            self.error(&span, &format!("{path}.{key}"), message);
            root.remove(key.as_str());
        }
        for (section, value) in root.iter_mut() {
            let section = section.get_ref().to_string();
            for table in tables_of(&section, value.get_mut()) {
                self.refused(&section, table, path);
            }
        }
    }

    /// `table`, of a local preset's `section`, with every key it may not
    /// write refused and taken out.
    fn refused(&mut self, section: &str, table: &mut DeTable<'_>, path: &str) {
        let Some(writable) = WRITABLE.iter().find(|writable| writable.section == section) else {
            return;
        };
        let named: Vec<(String, Range<usize>, String)> = table
            .iter()
            .filter_map(|(key, value)| {
                let name = key.get_ref().to_string();
                let why = if names_secret(value.get_ref()) {
                    format!("a local preset names no secret; give {name} in the adopting table in herald.toml")
                } else if writable.allowed.contains(&name.as_str()) {
                    return None;
                } else {
                    writable.refusal(&name)
                };
                Some((name, key.span(), why))
            })
            .collect();
        for (key, span, why) in named {
            self.error(&span, &format!("{path}.{key}"), why);
            table.remove(key.as_str());
        }
    }

    /// `root`'s routes, and the tables of its other sections by name.
    fn split<'a>(&mut self, root: DeTable<'a>, path: &str) -> Tables<'a> {
        let mut tables = Tables::default();
        for (section, value) in root {
            let section = section.get_ref().to_string();
            let span = value.span();
            let named = NAMED_SECTIONS.into_iter().find(|known| *known == section);
            let at = format!("{path}.{section}");
            match (section.as_str(), named, value.into_inner()) {
                ("routes", _, DeValue::Array(routes)) => {
                    for route in routes {
                        let span = route.span();
                        match route.into_inner() {
                            DeValue::Table(route) => tables.routes.push(route),
                            _ => self.error(&span, path, "a route is a table; write [[routes]]"),
                        }
                    }
                }
                (_, Some(section), DeValue::Table(table)) => {
                    for (name, value) in table {
                        let span = value.span();
                        let name = name.into_inner().into_owned();
                        match value.into_inner() {
                            DeValue::Table(table) => tables.named.push((section, name, table)),
                            _ => self.error(
                                &span,
                                path,
                                format!("{section}.{name} is a table; write [{section}.{name}]"),
                            ),
                        }
                    }
                }
                ("routes", _, _) => {
                    self.error(&span, &at, "routes is an array of tables; write [[routes]]");
                }
                _ => self.error(
                    &span,
                    &at,
                    format!("{section} is a table of tables; write [{section}.<name>]"),
                ),
            }
        }
        tables
    }

    /// `tables` given each key `adopted` gives its preset: where a key is
    /// a parameter, written into the tables it lands in; else into each
    /// table that writes it; else into each route.
    fn given<'a>(&mut self, adopted: &'a Adopting<'a>, tables: &mut Tables<'a>, path: &str) {
        let parameters = match adopted.preset {
            Adopted::Shipped(preset) => preset.parameters,
            Adopted::Local { .. } => &[],
        };
        for parameter in parameters.iter().filter(|parameter| parameter.required) {
            if !adopted.keys.contains_key(parameter.key) {
                self.error(
                    &adopted.span,
                    path,
                    format!("give {}: {}", parameter.key, parameter.described),
                );
            }
        }
        for (key, value) in &adopted.keys {
            let name = key.get_ref().as_ref();
            let lands = parameters
                .iter()
                .find(|parameter| parameter.key == name)
                .map(|parameter| parameter.lands.section());
            let landed = tables.write(name, lands, value);
            if !landed {
                let mut takes: Vec<&str> =
                    parameters.iter().map(|parameter| parameter.key).collect();
                takes.extend(tables.written());
                takes.sort_unstable();
                takes.dedup();
                let takes = if takes.is_empty() {
                    "none".to_owned()
                } else {
                    list(&takes)
                };
                self.error(
                    &key.span(),
                    &format!("{path}.{name}"),
                    format!("the preset takes no key \"{name}\"; it takes {takes}"),
                );
            }
        }
    }
}

impl Writable {
    /// Why a local preset may not write `key` of this section's tables.
    fn refusal(&self, key: &str) -> String {
        let why = self
            .refused
            .iter()
            .find(|(keys, _)| keys.contains(&key))
            .map_or_else(
                || {
                    format!(
                        "it writes only {} of a {} table",
                        list(self.allowed),
                        self.section
                    )
                },
                |(_, why)| (*why).to_owned(),
            );
        format!(
            "a local preset does not write {key}: {why}; give it in the adopting table in herald.toml"
        )
    }
}

/// A preset's tables: its routes, and its other tables with their section
/// and name.
#[derive(Default)]
struct Tables<'i> {
    routes: Vec<DeTable<'i>>,
    named: Vec<(&'static str, String, DeTable<'i>)>,
}

impl<'i> Tables<'i> {
    /// Each table named `<table>-<name>`, each route's id likewise, and
    /// every key naming another of them named so too.
    fn renamed(&mut self, table: &str) {
        let renamed = |name: &str| format!("{table}-{name}");
        let made: BTreeMap<&str, BTreeSet<String>> = NAMED_SECTIONS
            .into_iter()
            .map(|section| {
                let names = self
                    .named
                    .iter()
                    .filter(|(of, _, _)| *of == section)
                    .map(|(_, name, _)| name.clone())
                    .collect();
                (section, names)
            })
            .collect();
        for route in &mut self.routes {
            if let Some(id) = route.get_mut("id")
                && let DeValue::String(name) = id.get_mut()
            {
                *name = Cow::Owned(renamed(name));
            }
        }
        let parts = self.routes.iter_mut().map(|route| ("routes", route)).chain(
            self.named
                .iter_mut()
                .map(|(section, _, table)| (*section, table)),
        );
        for (section, part) in parts {
            for (of, key, names) in REFERENCES {
                if of != section {
                    continue;
                }
                let Some(value) = part.get_mut(key) else {
                    continue;
                };
                let own = made.get(names);
                each_name(value.get_mut(), &mut |name| {
                    if own.is_some_and(|own| own.contains(name.as_ref())) {
                        *name = Cow::Owned(renamed(name));
                    }
                });
            }
        }
        for (_, name, _) in &mut self.named {
            *name = renamed(name);
        }
    }

    /// Whether `value` was written as `key` into the tables of the section
    /// `lands`, or, with no section, into each table that writes `key`,
    /// or into each route where none does.
    fn write(&mut self, key: &str, lands: Option<&str>, value: &Value<'i>) -> bool {
        let mut landed = false;
        let mut put = |table: &mut DeTable<'i>| {
            table.insert(
                Spanned::new(value.span(), Cow::Owned(key.to_owned())),
                value.clone(),
            );
            landed = true;
        };
        let parts = self.routes.iter_mut().map(|route| ("routes", route)).chain(
            self.named
                .iter_mut()
                .map(|(section, _, table)| (*section, table)),
        );
        let parts: Vec<(&str, &mut DeTable<'i>)> = parts.collect();
        let writes = parts.iter().any(|(_, table)| table.contains_key(key));
        for (section, table) in parts {
            let lands = lands.map_or_else(
                || (writes && table.contains_key(key)) || (!writes && section == "routes"),
                |lands| section == lands,
            );
            if lands {
                put(table);
            }
        }
        landed
    }

    /// Every key its tables write.
    fn written(&self) -> impl Iterator<Item = &str> {
        self.routes
            .iter()
            .chain(self.named.iter().map(|(_, _, table)| table))
            .flat_map(|table| table.keys().map(|key| key.get_ref().as_ref()))
    }
}

/// Whether `path` is a local preset's: `./` and then a path of names only,
/// none of them `.` or `..`.
fn inside(path: &str) -> bool {
    path.strip_prefix("./")
        .is_some_and(|rest| rest.split('/').all(|step| !matches!(step, "" | "." | "..")))
}

/// Whether `value` is the config's one version.
fn is_version(value: &DeValue<'_>) -> bool {
    matches!(value, DeValue::Integer(integer) if i64::from_str_radix(integer.as_str(), integer.radix()) == Ok(Config::VERSION))
}

/// Whether `value`, or a value within it, is a secret's reference.
fn names_secret(value: &DeValue<'_>) -> bool {
    match value {
        DeValue::Table(table) => {
            table.contains_key("secret")
                || table.values().any(|child| names_secret(child.get_ref()))
        }
        DeValue::Array(items) => items.iter().any(|item| names_secret(item.get_ref())),
        _ => false,
    }
}

/// The tables `value`, written as `section`, holds: each route of
/// `routes`, each named table of the other sections.
fn tables_of<'v, 'i>(section: &str, value: &'v mut DeValue<'i>) -> Vec<&'v mut DeTable<'i>> {
    match (section, value) {
        ("routes", DeValue::Array(routes)) => routes
            .iter_mut()
            .filter_map(|route| match route.get_mut() {
                DeValue::Table(table) => Some(table),
                _ => None,
            })
            .collect(),
        (_, DeValue::Table(named)) => named
            .iter_mut()
            .map(|(_, value)| value)
            .filter_map(|table| match table.get_mut() {
                DeValue::Table(table) => Some(table),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Each name `value` writes, as one string, an array of them, or a table
/// whose values they are, handed to `each`.
fn each_name(value: &mut DeValue<'_>, each: &mut dyn FnMut(&mut Cow<'_, str>)) {
    match value {
        DeValue::String(name) => each(name),
        DeValue::Array(items) => items
            .iter_mut()
            .for_each(|item| each_name(item.get_mut(), each)),
        DeValue::Table(table) => table
            .iter_mut()
            .map(|(_, value)| value)
            .for_each(|item| each_name(item.get_mut(), each)),
        _ => {}
    }
}

/// `value` with every span within it moved on by `by`.
fn shifted(value: Value<'_>, by: usize) -> Value<'_> {
    let moved = |span: Range<usize>| span.start + by..span.end + by;
    let span = moved(value.span());
    let inner = match value.into_inner() {
        DeValue::Table(table) => DeValue::Table(
            table
                .into_iter()
                .map(|(key, child)| {
                    let at = moved(key.span());
                    (Spanned::new(at, key.into_inner()), shifted(child, by))
                })
                .collect(),
        ),
        DeValue::Array(items) => {
            DeValue::Array(items.into_iter().map(|item| shifted(item, by)).collect())
        }
        other => other,
    };
    Spanned::new(span, inner)
}

/// The tables the config `text`, read from the file named `file`, expands
/// its `[presets.<name>]` to, as `herald.toml` would write them, a local
/// preset read through `files`.
///
/// # Errors
///
/// Where the config adopts no preset as `name`, or holds a mistake.
pub fn expansion(
    file: &str,
    text: &str,
    files: &dyn Files,
    name: &str,
) -> Result<toml::Table, Errors> {
    let kinds = Kinds::new();
    let mut loader = Loader::new(file, text, &kinds);
    let (root, syntax) = DeTable::parse_recoverable(text);
    let adopts = matches!(
        root.get_ref().get("presets").map(Spanned::get_ref),
        Some(DeValue::Table(presets)) if presets.contains_key(name)
    );
    let mut adopting = Vec::new();
    let (_, expanded) = loader.expanding((root.into_inner(), &syntax), files, &mut adopting);
    let path = format!("presets.{name}");
    if !adopts {
        loader.error(
            &(0..0),
            &path,
            format!("{file} adopts no preset as [{path}]"),
        );
    }
    if !loader.errors.is_empty() {
        return Err(loader.failed());
    }
    let mut written = toml::Table::new();
    let routes: Vec<toml::Value> = expanded
        .routes
        .into_iter()
        .filter(|(at, _)| *at == path)
        .filter_map(|(_, value)| plain(value))
        .collect();
    if !routes.is_empty() {
        written.insert("routes".to_owned(), toml::Value::Array(routes));
    }
    let named = [
        ("templates", expanded.templates),
        ("views", expanded.views),
        ("reports", expanded.reports),
        ("rules", expanded.rules),
    ];
    for (section, parts) in named {
        for (at, part, value) in parts {
            let Some(value) = plain(value).filter(|_| at == path) else {
                continue;
            };
            if let toml::Value::Table(tables) = written
                .entry(section)
                .or_insert_with(|| toml::Value::Table(toml::Table::new()))
            {
                tables.insert(part, value);
            }
        }
    }
    Ok(written)
}

/// `value` as a plain TOML value.
fn plain(value: Value<'_>) -> Option<toml::Value> {
    serde::Deserialize::deserialize(ValueDeserializer::from(value)).ok()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use schemars::JsonSchema;

    use super::WRITABLE;

    fn keys<T: JsonSchema>() -> BTreeSet<String> {
        let schema = schemars::schema_for!(T);
        schema
            .get("properties")
            .and_then(serde_json::Value::as_object)
            .map(|properties| properties.keys().cloned().collect())
            .unwrap_or_default()
    }

    #[test]
    fn every_key_of_a_table_a_local_preset_holds_is_allowed_or_refused_with_why_once() {
        for (section, written) in [
            ("routes", keys::<crate::file::RouteFile>()),
            ("templates", keys::<crate::file::TemplateFile>()),
            ("views", keys::<crate::views::ViewFile>()),
            ("reports", keys::<crate::views::ReportFile>()),
            ("rules", keys::<crate::views::RuleSchema>()),
        ] {
            let writable = WRITABLE
                .iter()
                .find(|writable| writable.section == section)
                .unwrap();
            let mut stated: Vec<&str> = writable.allowed.to_vec();
            stated.extend(writable.refused.iter().flat_map(|(keys, _)| keys.iter()));
            let once: BTreeSet<&str> = stated.iter().copied().collect();
            assert_eq!(once.len(), stated.len(), "{section}: a key is stated twice");
            let unstated: Vec<&String> = written
                .iter()
                .filter(|key| !once.contains(key.as_str()))
                .collect();
            assert!(
                unstated.is_empty(),
                "{section}: allow or refuse {unstated:?} for a local preset, saying why"
            );
            let unknown: Vec<&&str> = once
                .iter()
                .filter(|key| !written.contains(**key) && **key != "act")
                .collect();
            assert!(unknown.is_empty(), "{section}: no such key {unknown:?}");
        }
    }
}
