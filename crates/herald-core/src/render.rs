use std::fmt;

use std::borrow::Cow;
use std::collections::BTreeSet;

use minijinja::value::{Serde, merge_maps};
use minijinja::{
    AutoEscape, Environment, ErrorKind, Output, State, UndefinedBehavior, Value, context,
    escape_formatter,
};

use crate::announce::{body_template, title_template};
use crate::bounds::Sandbox;
use crate::event::Event;
use crate::group::Group;
use crate::key::RouteId;
use crate::markup::escaped;
use crate::members::Listed;
use crate::queue::Showing;
use crate::route::TemplateName;

const MARKDOWN: &str = "markdown";

/// What a template renders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Form {
    /// Markdown, into which every value but the event's body is placed as
    /// literal text, so a title cannot write markup, a mention or a link.
    Markdown,
    /// One line of plain text, for a field no destination reads as markup.
    Line,
}

/// What a template is rendered with.
#[derive(Debug, Clone, Copy)]
pub struct Context<'a> {
    /// The event.
    pub event: &'a Event,
    /// The route rendering it.
    pub route: &'a RouteId,
    /// The running herald's version.
    pub herald: &'a str,
    /// The group the event leads, where the route gathers events into one
    /// announcement; a template reads its members as `events`, or the event
    /// alone where there is none.
    pub group: Option<Group<'a>>,
    /// The view the event reports, where it is a report; a template reads
    /// it as `view`.
    pub view: Option<Showing<'a>>,
}

/// Why a template could not be compiled or rendered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateError {
    /// The template's name.
    pub name: String,
    /// What went wrong, as minijinja says it.
    pub reason: String,
}

impl fmt::Display for TemplateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "template {}: {}", self.name, self.reason)
    }
}

/// Templates compiled into a sandbox: no filesystem, a fuel limit, a recursion
/// limit, an output cap, and undefined values refused when printed.
#[derive(Debug, Clone)]
pub struct Templates {
    environment: Environment<'static>,
    output: usize,
}

impl Templates {
    /// The templates `sources` holds, by name, each compiled into
    /// `sandbox`.
    ///
    /// # Errors
    ///
    /// Every template that does not compile, with minijinja's reason.
    pub fn compile(
        sources: &[(&str, &str, Form)],
        sandbox: Sandbox,
    ) -> Result<Self, Vec<TemplateError>> {
        let markdown: BTreeSet<String> = sources
            .iter()
            .filter(|(_, _, form)| *form == Form::Markdown)
            .map(|(name, _, _)| (*name).to_owned())
            .collect();
        let mut environment = sandboxed(sandbox, markdown);
        let errors: Vec<TemplateError> = sources
            .iter()
            .filter_map(|&(name, source, _)| {
                environment
                    .add_template_owned(name.to_owned(), source.to_owned())
                    .err()
                    .map(|error| failure(name, &error))
            })
            .collect();
        if errors.is_empty() {
            Ok(Self {
                environment,
                output: sandbox.output,
            })
        } else {
            Err(errors)
        }
    }

    /// Templates that render an event's own title and body, as they are,
    /// under `template`.
    #[must_use]
    pub fn verbatim(template: &TemplateName, sandbox: Sandbox) -> Self {
        let body = body_template(template);
        let mut environment = sandboxed(sandbox, BTreeSet::from([body.clone()]));
        for (name, source) in [
            (title_template(template), "{{ event.title }}"),
            (body, "{{ event.body }}"),
        ] {
            let _ = environment.add_template_owned(name, source);
        }
        Self {
            environment,
            output: sandbox.output,
        }
    }

    /// The template `name` rendered with `context`.
    ///
    /// # Errors
    ///
    /// When there is no such template, when it prints an undefined value, runs
    /// out of fuel or recurses too deep, or when its output passes the cap.
    pub fn render(&self, name: &str, context: &Context<'_>) -> Result<String, TemplateError> {
        let template = self
            .environment
            .get_template(name)
            .map_err(|error| failure(name, &error))?;
        let mut capped = Capped {
            written: Vec::new(),
            passed: false,
            cap: self.output,
        };
        let rendered = template.render_captured_to(
            context! {
                event => event_value(context.event),
                events => grouped_values(context),
                group => context.group.map(|group| context! { key => group.key }),
                view => context.view.map(shown::view_value),
                route => context.route.as_str(),
                herald => context.herald,
            },
            &mut capped,
        );
        if capped.passed {
            return Err(TemplateError {
                name: name.to_owned(),
                reason: format!("its output passes the cap of {} bytes", self.output),
            });
        }
        rendered.map_err(
            |error| match lacked(template.source(), &error, context.event) {
                Some(attribute) => TemplateError {
                    name: name.to_owned(),
                    reason: format!(
                        "it prints event.attributes.{attribute}, which {} events do not carry",
                        context.event.kind()
                    ),
                },
                None => failure(name, &error),
            },
        )?;
        Ok(String::from_utf8_lossy(&capped.written).into_owned())
    }

    /// The template `name` rendered with `context`, on one line.
    ///
    /// # Errors
    ///
    /// As [`Templates::render`].
    pub fn render_line(&self, name: &str, context: &Context<'_>) -> Result<String, TemplateError> {
        Ok(self
            .render(name, context)?
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" "))
    }
}

/// `event` as a template reads it, its body placed as written.
fn event_value(event: &Event) -> Value {
    merge_maps([
        Value::from(Serde(event)),
        context! { body => Value::from_safe_string(event.body().to_owned()) },
    ])
}

/// The events of `context` as a template reads them: the group's members,
/// the lead first, or the event alone where it leads none.
fn grouped_values(context: &Context<'_>) -> Vec<Value> {
    match context.group {
        Some(group) => group
            .events
            .iter()
            .map(|event| event_value(event))
            .collect(),
        None => vec![event_value(context.event)],
    }
}

/// A template of one line, compiled under its name, rendered with one
/// value whose fields it reads.
#[derive(Debug, Clone)]
struct Line {
    environment: Environment<'static>,
    output: usize,
    name: &'static str,
}

impl Line {
    fn compile(name: &'static str, source: &str, sandbox: Sandbox) -> Result<Self, TemplateError> {
        let mut environment = sandboxed(sandbox, BTreeSet::new());
        environment
            .add_template_owned(name, source.to_owned())
            .map_err(|error| failure(name, &error))?;
        Ok(Self {
            environment,
            output: sandbox.output,
            name,
        })
    }

    /// Each name the template reads that is neither one of `fields` of
    /// `value` nor a function every template has, in order.
    fn unknown(&self, value: &str, fields: &[&str]) -> Vec<String> {
        let mut unknown: Vec<String> = self
            .environment
            .get_template(self.name)
            .map(|template| template.undeclared_variables(true))
            .unwrap_or_default()
            .into_iter()
            .filter(|name| {
                let mut parts = name.split('.');
                let top = parts.next().unwrap_or_default();
                if top == value {
                    parts.next().is_some_and(|field| !fields.contains(&field))
                } else {
                    !self.environment.globals().any(|(global, _)| global == top)
                }
            })
            .collect();
        unknown.sort();
        unknown
    }

    /// The template rendered with `context`, on one line: refused where it
    /// prints nothing, saying it names nothing for `subject`.
    fn render(&self, context: Value, subject: &str) -> Result<String, TemplateError> {
        let template = self
            .environment
            .get_template(self.name)
            .map_err(|error| failure(self.name, &error))?;
        let mut capped = Capped {
            written: Vec::new(),
            passed: false,
            cap: self.output,
        };
        let rendered = template.render_captured_to(context, &mut capped);
        if capped.passed {
            return Err(TemplateError {
                name: self.name.to_owned(),
                reason: format!("its output passes the cap of {} bytes", self.output),
            });
        }
        rendered.map_err(|error| failure(self.name, &error))?;
        let named = String::from_utf8_lossy(&capped.written)
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if named.is_empty() {
            return Err(TemplateError {
                name: self.name.to_owned(),
                reason: format!("it names no {} for {subject}", self.name),
            });
        }
        Ok(named)
    }
}

/// The name a `project` template gives an owner's member as the project
/// its events are announced as (ADR-0030).
#[derive(Debug, Clone)]
pub struct ProjectName(Line);

/// The name a `project` template is compiled under.
const PROJECT: &str = "project";

/// What a `project` template reads a repository's attributes under.
const REPOSITORY: &str = "repository";

/// The attributes of a repository a `project` template reads.
pub const REPOSITORY_FIELDS: [&str; 7] = [
    "id",
    "name",
    "path",
    "owner",
    "topics",
    "properties",
    "visibility",
];

impl ProjectName {
    /// The template `source`, compiled into `sandbox`.
    ///
    /// # Errors
    ///
    /// When it does not compile, with minijinja's reason.
    pub fn compile(source: &str, sandbox: Sandbox) -> Result<Self, TemplateError> {
        Line::compile(PROJECT, source, sandbox).map(Self)
    }

    /// Each name the template reads that is neither one of a repository's
    /// attributes nor a function every template has, in order.
    #[must_use]
    pub fn unknown(&self) -> Vec<String> {
        self.0.unknown(REPOSITORY, &REPOSITORY_FIELDS)
    }

    /// The project `repository` is announced as, on one line.
    ///
    /// # Errors
    ///
    /// When the template prints an undefined value, runs out of fuel,
    /// passes its output cap, or prints nothing.
    pub fn render(&self, repository: &Listed) -> Result<String, TemplateError> {
        self.0.render(
            context! {
                repository => context! {
                    id => repository.id.as_str(),
                    name => repository.name.as_str(),
                    path => repository.path.as_str(),
                    owner => repository.owner(),
                    topics => repository.topics.clone(),
                    properties => repository.properties.clone(),
                    visibility => repository.visibility.name(),
                },
            },
            &repository.path,
        )
    }
}

/// The name an `actor` template gives the actor each project announces as
/// (ADR-0036).
#[derive(Debug, Clone)]
pub struct ActorName {
    line: Line,
    source: String,
}

/// The name an `actor` template is compiled under.
const ACTOR: &str = "actor";

/// What an `actor` template reads the event's fields under.
const EVENT: &str = "event";

/// The fields of an event an `actor` template reads: those known of a
/// project before any of its events.
pub const ACTOR_FIELDS: [&str; 1] = ["project"];

impl ActorName {
    /// Whether `text` is a template rather than a name.
    #[must_use]
    pub fn templated(text: &str) -> bool {
        ["{{", "{%", "{#"]
            .iter()
            .any(|opening| text.contains(opening))
    }

    /// The template `source`, compiled into `sandbox`.
    ///
    /// # Errors
    ///
    /// When it does not compile, with minijinja's reason.
    pub fn compile(source: &str, sandbox: Sandbox) -> Result<Self, TemplateError> {
        Ok(Self {
            line: Line::compile(ACTOR, source, sandbox)?,
            source: source.to_owned(),
        })
    }

    /// The template, as written.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Each name the template reads that is neither one of [`ACTOR_FIELDS`]
    /// of the event nor a function every template has, in order.
    #[must_use]
    pub fn unknown(&self) -> Vec<String> {
        self.line.unknown(EVENT, &ACTOR_FIELDS)
    }

    /// The name of the actor `project` announces as, on one line.
    ///
    /// # Errors
    ///
    /// When the template prints an undefined value, runs out of fuel,
    /// passes its output cap, or prints nothing.
    pub fn render(&self, project: &str) -> Result<String, TemplateError> {
        self.line.render(
            context! { event => context! { project => project } },
            project,
        )
    }
}

impl PartialEq for ActorName {
    fn eq(&self, other: &Self) -> bool {
        self.source == other.source
    }
}

impl Eq for ActorName {}

/// An environment kept to `sandbox`, escaping what it places into the
/// templates named in `markdown` as Markdown.
fn sandboxed(sandbox: Sandbox, markdown: BTreeSet<String>) -> Environment<'static> {
    let mut environment = Environment::new();
    environment.set_fuel(Some(sandbox.fuel));
    environment.set_recursion_limit(sandbox.recursion);
    environment.set_undefined_behavior(UndefinedBehavior::SemiStrict);
    environment.set_auto_escape_callback(move |name| {
        if markdown.contains(name) {
            AutoEscape::Custom(Cow::Borrowed(MARKDOWN))
        } else {
            AutoEscape::None
        }
    });
    environment.set_formatter(format);
    environment
}

/// A template's output, refused once it passes the cap, so a template is
/// stopped there and not after building all it would print.
struct Capped {
    written: Vec<u8>,
    passed: bool,
    cap: usize,
}

impl std::io::Write for Capped {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.written.len().saturating_add(bytes.len()) > self.cap {
            self.passed = true;
            return Err(std::io::Error::other("the output passes its cap"));
        }
        self.written.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn format(
    output: &mut Output<'_>,
    state: &mut State<'_, '_>,
    value: &Value,
) -> Result<(), minijinja::Error> {
    let markdown = matches!(state.auto_escape(), AutoEscape::Custom(name) if name == MARKDOWN);
    if !markdown || value.is_undefined() {
        return escape_formatter(output, state, value);
    }
    let text = value.to_string();
    let text = if value.is_safe() {
        text
    } else {
        escaped(&text)
    };
    output.write_str(&text).map_err(minijinja::Error::from)
}

/// The attribute `event` lacks that the line of `source` where `error`
/// stopped prints, where it stopped on one.
fn lacked<'s>(source: &'s str, error: &minijinja::Error, event: &Event) -> Option<&'s str> {
    if error.kind() != ErrorKind::UndefinedError {
        return None;
    }
    let line = source.lines().nth(error.line()?.checked_sub(1)?)?;
    line.match_indices(ATTRIBUTES).find_map(|(at, _)| {
        let rest = &line[at + ATTRIBUTES.len()..];
        let end = rest
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .unwrap_or(rest.len());
        let attribute = &rest[..end];
        (!attribute.is_empty() && event.attribute(attribute).is_none()).then_some(attribute)
    })
}

/// How a template reads one of an event's attributes.
const ATTRIBUTES: &str = "event.attributes.";

fn failure(name: &str, error: &minijinja::Error) -> TemplateError {
    let reason = match error.kind() {
        ErrorKind::OutOfFuel => "it runs too long: it ran out of fuel".to_owned(),
        _ => error.to_string(),
    };
    TemplateError {
        name: name.to_owned(),
        reason,
    }
}

mod shown;

#[cfg(test)]
mod tests;
