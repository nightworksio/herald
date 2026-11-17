//! `[templates]`: each template's title and body, compiled with the routes'
//! thread names, and rendered against a sample of each kind its routes take.

use std::collections::BTreeMap;
use std::ops::Range;

use herald_core::{
    Context, Event, Form, Group, Sandbox, TemplateName, Templates, body_template, group_template,
    thread_template, title_template,
};
use toml::Spanned;

use super::route::Read;
use super::{Loader, Value};
use crate::file::TemplateFile;
use crate::samples::samples_from;

/// A template's sources, with where each is written.
pub(super) struct Written {
    pub(super) title: Spanned<String>,
    pub(super) body: Spanned<String>,
    pub(super) body_path: String,
}

impl Loader<'_> {
    pub(super) fn templates(
        &mut self,
        value: Option<Value<'_>>,
        read: &dyn Fn(&str) -> Result<String, String>,
    ) -> BTreeMap<TemplateName, Written> {
        let mut templates = BTreeMap::new();
        for (name, value) in self.named(value, "templates") {
            let path = format!("templates.{name}");
            let span = value.span();
            let Some(template) = self.typed::<TemplateFile>(value, &path) else {
                continue;
            };
            let body = match (template.body, template.file) {
                (Some(body), None) => Some((body, format!("{path}.body"))),
                (None, Some(file)) => match read(file.get_ref()) {
                    Ok(body) => Some((Spanned::new(file.span(), body), format!("{path}.file"))),
                    Err(reason) => {
                        self.error(
                            &file.span(),
                            &format!("{path}.file"),
                            format!(
                                "{} could not be read: {reason}; name a file beside {}, or write the body inline as body = \"…\"",
                                file.get_ref(),
                                self.file
                            ),
                        );
                        None
                    }
                },
                (None, None) => {
                    self.error(&span, &path, "give its body: body = \"…\" or file = \"…\"");
                    None
                }
                (Some(_), Some(_)) => {
                    self.error(&span, &path, "give its body or a file, not both; keep one");
                    None
                }
            };
            if let Some((body, body_path)) = body {
                templates.insert(
                    TemplateName::new(name),
                    Written {
                        title: template.title,
                        body,
                        body_path,
                    },
                );
            }
        }
        templates
    }

    /// `templates` and the thread names of `routes` compiled within
    /// `sandbox`, each route's then rendered against its sample events.
    pub(super) fn compiled(
        &mut self,
        templates: &BTreeMap<TemplateName, Written>,
        routes: &[Read],
        sandbox: Sandbox,
    ) -> Option<Templates> {
        let compiled = self.compile(templates, routes, sandbox);
        if let Some(compiled) = &compiled {
            for read in routes {
                self.dry_render(compiled, read);
            }
        }
        compiled
    }

    fn compile(
        &mut self,
        templates: &BTreeMap<TemplateName, Written>,
        routes: &[Read],
        sandbox: Sandbox,
    ) -> Option<Templates> {
        let mut sources: Vec<(String, String, Form, Range<usize>, String)> = Vec::new();
        for (name, written) in templates {
            let path = format!("templates.{}", name.as_str());
            sources.push((
                title_template(name),
                written.title.get_ref().clone(),
                Form::Line,
                written.title.span(),
                format!("{path}.title"),
            ));
            sources.push((
                body_template(name),
                written.body.get_ref().clone(),
                Form::Markdown,
                written.body.span(),
                written.body_path.clone(),
            ));
        }
        for read in routes {
            if let Some(thread) = &read.thread {
                sources.push((
                    thread_template(&read.config.route.id),
                    thread.get_ref().clone(),
                    Form::Line,
                    thread.span(),
                    format!("{}.thread.name", read.path),
                ));
            }
            if let Some(key) = &read.group {
                sources.push((
                    group_template(&read.config.route.id),
                    key.get_ref().clone(),
                    Form::Line,
                    key.span(),
                    format!("{}.group.key", read.path),
                ));
            }
        }
        let compiled: Vec<(&str, &str, Form)> = sources
            .iter()
            .map(|(name, source, form, _, _)| (name.as_str(), source.as_str(), *form))
            .collect();
        match Templates::compile(&compiled, sandbox) {
            Ok(templates) => Some(templates),
            Err(failures) => {
                for failure in failures {
                    let place = sources.iter().find(|(name, ..)| *name == failure.name);
                    let (span, path) = place
                        .map_or((0..0, String::new()), |(_, _, _, span, path)| {
                            (span.clone(), path.clone())
                        });
                    self.error(
                        &span,
                        &path,
                        format!("the template does not compile: {}", failure.reason),
                    );
                }
                None
            }
        }
    }

    fn dry_render(&mut self, templates: &Templates, read: &Read) {
        let Some(rendered) = &read.rendered else {
            return;
        };
        let route = &read.config.route;
        let template_path = format!("templates.{}", rendered.template.as_str());
        let limits = read.config.profile.limits;
        let mut parts = vec![
            (
                title_template(&rendered.template),
                rendered.title_span.clone(),
                format!("{template_path}.title"),
                Some(limits.title),
            ),
            (
                body_template(&rendered.template),
                rendered.body_span.clone(),
                rendered.body_path.clone(),
                None,
            ),
        ];
        parts.extend(read.thread.as_ref().map(|thread| {
            (
                thread_template(&route.id),
                thread.span(),
                format!("{}.thread.name", read.path),
                Some(limits.thread_name),
            )
        }));
        parts.extend(read.group.as_ref().map(|key| {
            (
                group_template(&route.id),
                key.span(),
                format!("{}.group.key", read.path),
                Some(usize::MAX),
            )
        }));
        let events: Vec<Event> = read
            .sample_kinds
            .iter()
            .filter_map(|kind| self.specs.get(kind))
            .flat_map(|spec| samples_from(spec, read.origins))
            .collect();
        let members: Vec<&Event> = events.iter().collect();
        let group = read.group.as_ref().map(|_| Group {
            key: SAMPLE_KEY,
            events: &members,
        });
        for event in &events {
            for group in std::iter::once(None).chain(group.map(Some)) {
                let context = Context {
                    event,
                    route: &route.id,
                    herald: herald_core::itself::VERSION,
                    group,
                };
                if !self.rendered_sample(templates, &context, &parts) {
                    return;
                }
            }
        }
    }

    /// Whether every one of `parts` renders `context` within its limit,
    /// each that does not said where it is written.
    fn rendered_sample(
        &mut self,
        templates: &Templates,
        context: &Context<'_>,
        parts: &[(String, Range<usize>, String, Option<usize>)],
    ) -> bool {
        let sample = match context.group {
            Some(_) => format!("group of sample {} events", context.event.kind()),
            None => format!("sample {} event", context.event.kind()),
        };
        for (name, span, path, limit) in parts {
            let rendered = match limit {
                Some(_) => templates.render_line(name, context),
                None => templates.render(name, context),
            };
            match (rendered, limit) {
                (Err(failure), _) => {
                    self.error(
                        span,
                        path,
                        format!("it does not render a {sample}: {}", failure.reason),
                    );
                    return false;
                }
                (Ok(text), Some(limit)) if text.chars().count() > *limit => {
                    self.error(
                        span,
                        path,
                        format!(
                            "it renders {} characters for a {sample}, over the destination's {limit}; shorten it",
                            text.chars().count()
                        ),
                    );
                    return false;
                }
                (Ok(_), _) => {}
            }
        }
        true
    }
}

/// The key a sample group is rendered under.
const SAMPLE_KEY: &str = "v1.0.0";
