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
use crate::key::RouteId;
use crate::markup::escaped;
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
#[derive(Debug)]
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
                event => merge_maps([
                    Value::from(Serde(context.event)),
                    context! { body => Value::from_safe_string(context.event.body().to_owned()) },
                ]),
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
                        "it prints event.attributes.{attribute}, which a {} event does not carry",
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

#[cfg(test)]
mod tests;
