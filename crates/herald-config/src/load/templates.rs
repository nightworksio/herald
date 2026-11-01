use std::collections::BTreeMap;
use std::ops::Range;

use herald_core::{
    Context, Form, TemplateName, Templates, body_template, thread_template, title_template,
};

use super::route::Read;
use super::{Loader, Written};
use crate::samples::samples;

impl Loader<'_> {
    pub(super) fn compile(
        &mut self,
        templates: &BTreeMap<TemplateName, Written>,
        routes: &[Read],
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
        }
        let compiled: Vec<(&str, &str, Form)> = sources
            .iter()
            .map(|(name, source, form, _, _)| (name.as_str(), source.as_str(), *form))
            .collect();
        match Templates::compile(&compiled) {
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

    pub(super) fn dry_render(&mut self, templates: &Templates, read: &Read) {
        let route = &read.config.route;
        let template_path = format!("templates.{}", route.template.as_str());
        let limits = read.profile.limits;
        let parts = [
            (
                title_template(&route.template),
                read.title_span.clone(),
                format!("{template_path}.title"),
                Some(limits.title),
            ),
            (
                body_template(&route.template),
                read.body_span.clone(),
                read.body_path.clone(),
                None,
            ),
        ];
        let thread = read.thread.as_ref().map(|thread| {
            (
                thread_template(&route.id),
                thread.span(),
                format!("{}.thread.name", read.path),
                Some(limits.thread_name),
            )
        });
        for kind in &read.sample_kinds {
            for event in samples(*kind) {
                let context = Context {
                    event: &event,
                    route: &route.id,
                    herald: env!("CARGO_PKG_VERSION"),
                };
                for (name, span, path, limit) in parts.iter().chain(thread.iter()) {
                    let rendered = match limit {
                        Some(_) => templates.render_line(name, &context),
                        None => templates.render(name, &context),
                    };
                    match (rendered, limit) {
                        (Err(failure), _) => {
                            self.error(
                                span,
                                path,
                                format!(
                                    "it does not render a sample {kind:?} event: {}",
                                    failure.reason
                                ),
                            );
                            return;
                        }
                        (Ok(text), Some(limit)) if text.chars().count() > *limit => {
                            self.error(span, path, format!(
                                "it renders {} characters for a sample event, over the destination's {limit}; shorten it",
                                text.chars().count()
                            ));
                            return;
                        }
                        (Ok(_), _) => {}
                    }
                }
            }
        }
    }
}
