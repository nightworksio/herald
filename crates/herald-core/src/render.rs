use std::fmt;

use minijinja::value::Serde;
use minijinja::{AutoEscape, Environment, ErrorKind, UndefinedBehavior, Value, context};

use crate::event::Event;
use crate::key::RouteId;

const FUEL: u64 = 100_000;
const RECURSION: usize = 16;
const OUTPUT_CAP: usize = 256 * 1024;

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
    pub fn compile(sources: &[(&str, &str)]) -> Result<Self, Vec<TemplateError>> {
        let mut environment = Environment::new();
        environment.set_fuel(Some(FUEL));
        environment.set_recursion_limit(RECURSION);
        environment.set_undefined_behavior(UndefinedBehavior::SemiStrict);
        environment.set_auto_escape_callback(|_| AutoEscape::None);
        let errors: Vec<TemplateError> = sources
            .iter()
            .filter_map(|&(name, source)| {
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
                event => Value::from(Serde(context.event)),
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
