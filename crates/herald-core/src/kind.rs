use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Serialize};

/// What kind of thing an event announces: a name a source gives it, or a
/// kind the config declares.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Kind(Cow<'static, str>);

impl Kind {
    /// A release on a forge.
    pub const RELEASE: Self = Self(Cow::Borrowed("release"));
    /// A new version of a package on a registry.
    pub const PACKAGE_VERSION: Self = Self(Cow::Borrowed("package-version"));
    /// An entry in a feed.
    pub const FEED_ENTRY: Self = Self(Cow::Borrowed("feed-entry"));
    /// Anything an app says.
    pub const MESSAGE: Self = Self(Cow::Borrowed("message"));

    /// The kind named `name`.
    ///
    /// # Errors
    ///
    /// Where `name` is not words of lowercase letters and digits joined by
    /// `-`.
    pub fn new(name: &str) -> Result<Self, String> {
        let words = !name.is_empty()
            && name.split('-').all(|word| {
                !word.is_empty()
                    && word
                        .bytes()
                        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
            });
        if words {
            Ok(Self(Cow::Owned(name.to_owned())))
        } else {
            Err(format!(
                "\"{name}\" is not a kind; name it in lowercase letters and digits, words joined by '-'"
            ))
        }
    }

    /// The kind's name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for Kind {
    type Error = String;

    fn try_from(name: String) -> Result<Self, String> {
        Self::new(&name)
    }
}

impl From<Kind> for String {
    fn from(kind: Kind) -> Self {
        kind.0.into_owned()
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl JsonSchema for Kind {
    fn schema_name() -> Cow<'static, str> {
        "Kind".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "What kind of thing an event announces: release, package-version, feed-entry, message, or a kind the config declares.",
            "type": "string",
            "pattern": "^[a-z0-9]+(-[a-z0-9]+)*$"
        })
    }
}

/// One of an event's attributes, as its kind defines them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Attribute {
    /// Yes or no.
    Flag(bool),
    /// A number.
    Number(serde_json::Number),
    /// Text, or a web address.
    Text(String),
}

/// What type an attribute holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum AttributeType {
    /// Text.
    Text,
    /// Yes or no.
    Flag,
    /// A number.
    Number,
    /// An `https` or `http` address.
    Url,
}

impl AttributeType {
    fn holds(self, attribute: &Attribute) -> bool {
        match (self, attribute) {
            (Self::Text, Attribute::Text(_))
            | (Self::Flag, Attribute::Flag(_))
            | (Self::Number, Attribute::Number(_)) => true,
            (Self::Url, Attribute::Text(url)) => crate::address::web_url(url),
            _ => false,
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Flag => "a flag",
            Self::Number => "a number",
            Self::Url => "an https or http address",
        }
    }
}

/// One attribute a kind's events carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AttributeSpec {
    /// What it holds.
    #[serde(rename = "type")]
    pub ty: AttributeType,
    /// Whether every event of the kind carries it.
    #[serde(default)]
    pub required: bool,
}

/// The attribute every versioned kind carries.
pub const VERSION: &str = "version";
/// The attribute that marks a pre-release.
pub const PRERELEASE: &str = "prerelease";

/// Whether `name` can name an attribute or a link as a template reads it:
/// lowercase letters, digits and `_`, starting with a letter.
#[must_use]
pub fn readable(name: &str) -> bool {
    name.bytes()
        .next()
        .is_some_and(|first| first.is_ascii_lowercase())
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

/// The attributes of a versioned kind's event: its version, and whether it
/// is a pre-release.
#[must_use]
pub fn versioned(version: impl Into<String>, prerelease: bool) -> BTreeMap<String, Attribute> {
    BTreeMap::from([
        (VERSION.to_owned(), Attribute::Text(version.into())),
        (PRERELEASE.to_owned(), Attribute::Flag(prerelease)),
    ])
}

/// The version `attributes` carry, where they carry one.
pub(crate) fn version_in(attributes: &BTreeMap<String, Attribute>) -> Option<&str> {
    match attributes.get(VERSION) {
        Some(Attribute::Text(version)) => Some(version),
        _ => None,
    }
}

/// Whether `attributes` mark a pre-release.
pub(crate) fn prerelease_in(attributes: &BTreeMap<String, Attribute>) -> bool {
    attributes.get(PRERELEASE) == Some(&Attribute::Flag(true))
}

/// One link a kind's events carry, which becomes a button where the event
/// carries it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkSpec {
    /// Its name, as `event.links.<name>` reads it.
    pub name: String,
    /// Its button's label, where the config gives one.
    pub label: Option<String>,
}

impl LinkSpec {
    /// The link `name`, its button labelled as herald words it.
    #[must_use]
    pub fn named(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            label: None,
        }
    }
}

/// What a kind of event is: whether it carries a version, the links that
/// become its buttons, and the attributes its events carry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KindSpec {
    /// The kind.
    pub kind: Kind,
    /// What it is, in a sentence.
    pub description: String,
    /// Whether its events carry a version, as the `version` attribute.
    pub versioned: bool,
    /// The links that become its buttons, in order.
    pub links: Vec<LinkSpec>,
    /// The attributes its events carry, by name.
    pub attributes: BTreeMap<String, AttributeSpec>,
}

impl KindSpec {
    /// The kind `kind`, described as `description`, whose events carry
    /// `attributes`, and a version where it is `versioned`, with the links
    /// `links` as its buttons.
    #[must_use]
    pub fn new(
        kind: Kind,
        description: impl Into<String>,
        versioned: bool,
        links: Vec<LinkSpec>,
        mut attributes: BTreeMap<String, AttributeSpec>,
    ) -> Self {
        if versioned {
            attributes.insert(
                VERSION.to_owned(),
                AttributeSpec {
                    ty: AttributeType::Text,
                    required: true,
                },
            );
            attributes.insert(
                PRERELEASE.to_owned(),
                AttributeSpec {
                    ty: AttributeType::Flag,
                    required: false,
                },
            );
        }
        Self {
            kind,
            description: description.into(),
            versioned,
            links,
            attributes,
        }
    }

    /// The kinds herald's own sources and apps announce.
    #[must_use]
    pub fn shipped() -> Vec<Self> {
        let buttons = |names: &[&str]| names.iter().map(|&name| LinkSpec::named(name)).collect();
        vec![
            Self::new(
                Kind::RELEASE,
                "A release on a forge.",
                true,
                buttons(&["release", "changelog"]),
                BTreeMap::new(),
            ),
            Self::new(
                Kind::PACKAGE_VERSION,
                "A new version of a package on a registry.",
                true,
                buttons(&["release", "changelog"]),
                BTreeMap::new(),
            ),
            Self::new(
                Kind::FEED_ENTRY,
                "An entry in a feed.",
                false,
                buttons(&["entry"]),
                BTreeMap::new(),
            ),
            Self::new(
                Kind::MESSAGE,
                "Anything an app says.",
                false,
                buttons(&["release", "changelog"]),
                BTreeMap::new(),
            ),
        ]
    }

    /// Whether `attributes` are what the kind's events carry.
    ///
    /// # Errors
    ///
    /// The first attribute that is missing, unknown or of another type,
    /// named by its path.
    pub fn check(&self, attributes: &BTreeMap<String, Attribute>) -> Result<(), String> {
        for (name, spec) in &self.attributes {
            match attributes.get(name) {
                None if spec.required => {
                    return Err(format!(
                        "attributes.{name} is missing; a {} event carries it",
                        self.kind
                    ));
                }
                Some(attribute) if !spec.ty.holds(attribute) => {
                    return Err(format!(
                        "attributes.{name} is not {}; a {} event's {name} is",
                        spec.ty.name(),
                        self.kind
                    ));
                }
                _ => {}
            }
        }
        match attributes
            .keys()
            .find(|name| !self.attributes.contains_key(*name))
        {
            Some(unknown) => Err(format!(
                "attributes.{unknown} is not an attribute of a {} event",
                self.kind
            )),
            None => Ok(()),
        }
    }
}

/// Every kind a config knows: the kinds herald ships, and those it declares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KindSpecs(Vec<KindSpec>);

impl Default for KindSpecs {
    fn default() -> Self {
        Self(KindSpec::shipped())
    }
}

impl KindSpecs {
    /// These kinds, and `spec`.
    #[must_use]
    pub fn with(mut self, spec: KindSpec) -> Self {
        self.0.push(spec);
        self
    }

    /// What the kind `kind` is, where it is known.
    #[must_use]
    pub fn get(&self, kind: &Kind) -> Option<&KindSpec> {
        self.0.iter().find(|spec| &spec.kind == kind)
    }

    /// The links that become the buttons of a `kind` event, none where the
    /// kind is not known.
    #[must_use]
    pub fn links(&self, kind: &Kind) -> &[LinkSpec] {
        self.get(kind).map_or(&[], |spec| &spec.links)
    }

    /// Every kind, in the order herald ships them and the config declares
    /// them.
    pub fn iter(&self) -> impl Iterator<Item = &KindSpec> {
        self.0.iter()
    }
}

#[cfg(test)]
mod tests;
