use std::fmt;

use std::borrow::Cow;
use std::collections::BTreeSet;

use minijinja::value::{Serde, merge_maps};
use minijinja::{
    AutoEscape, Environment, ErrorKind, Output, State, UndefinedBehavior, Value, context,
    escape_formatter,
};

use crate::event::Event;
use crate::key::RouteId;

const FUEL: u64 = 100_000;
const RECURSION: usize = 16;
const OUTPUT_CAP: usize = 262_144;
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
}

impl Templates {
    /// The templates `sources` holds, by name, each compiled.
    ///
    /// # Errors
    ///
    /// Every template that does not compile, with minijinja's reason.
    pub fn compile(sources: &[(&str, &str, Form)]) -> Result<Self, Vec<TemplateError>> {
        let markdown: BTreeSet<String> = sources
            .iter()
            .filter(|(_, _, form)| *form == Form::Markdown)
            .map(|(name, _, _)| (*name).to_owned())
            .collect();
        let mut environment = Environment::new();
        environment.set_fuel(Some(FUEL));
        environment.set_recursion_limit(RECURSION);
        environment.set_undefined_behavior(UndefinedBehavior::SemiStrict);
        environment.set_auto_escape_callback(move |name| {
            if markdown.contains(name) {
                AutoEscape::Custom(Cow::Borrowed(MARKDOWN))
            } else {
                AutoEscape::None
            }
        });
        environment.set_formatter(format);
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
            Ok(Self { environment })
        } else {
            Err(errors)
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
        let output = template
            .render(context! {
                event => merge_maps([
                    Value::from(Serde(context.event)),
                    context! { body => Value::from_safe_string(context.event.body().to_owned()) },
                ]),
                route => context.route.as_str(),
                herald => context.herald,
            })
            .map_err(|error| failure(name, &error))?;
        if output.len() > OUTPUT_CAP {
            return Err(TemplateError {
                name: name.to_owned(),
                reason: format!("its output passes the cap of {OUTPUT_CAP} bytes"),
            });
        }
        Ok(output)
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
        escape_markdown(&text)
    };
    output.write_str(&text).map_err(minijinja::Error::from)
}

fn escape_markdown(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        if character.is_ascii_punctuation() {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

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
