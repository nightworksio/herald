use std::ops::Range;

use herald_core::{Kind, KindSpec, KindSpecs, LinkSpec, PRERELEASE, Phrase, VERSION, readable};

use super::{Loader, Value};
use crate::file::KindFile;

impl Loader<'_> {
    /// Whether the kind at `path` names `links` soundly: no more than a post
    /// holds buttons, each named as a link is and fitting a button's label.
    fn linked(&mut self, span: &Range<usize>, path: &str, links: &[LinkSpec]) -> bool {
        let mut sound = true;
        if links.len() > LinkSpec::MOST {
            self.error(
                span,
                &format!("{path}.links"),
                format!(
                    "a kind names at most {} links, the buttons a Discord post holds beside its subscribe button; keep {} of its {}",
                    LinkSpec::MOST,
                    LinkSpec::MOST,
                    links.len()
                ),
            );
            sound = false;
        }
        for (index, link) in links.iter().enumerate() {
            let at = format!("{path}.links[{index}]");
            if !readable(&link.name) {
                self.error(
                    span,
                    &at,
                    format!("\"{}\" cannot name a link; name it in lowercase letters, digits and '_', starting with a letter", link.name),
                );
                sound = false;
            }
            if link.label.is_none() && link.name.chars().count() > Phrase::LABEL {
                self.error(
                    span,
                    &at,
                    format!(
                        "\"{}\" is longer than a button's label of at most {} characters; give it a label, {{ name = \"{}\", label = \"…\" }}",
                        link.name,
                        Phrase::LABEL,
                        link.name
                    ),
                );
                sound = false;
            }
            if let Some(label) = &link.label
                && (label.trim().is_empty() || label.chars().count() > Phrase::LABEL)
            {
                self.error(
                    span,
                    &format!("{at}.label"),
                    format!(
                        "a button's label is 1 to {} characters; give one that fits",
                        Phrase::LABEL
                    ),
                );
                sound = false;
            }
        }
        sound
    }

    /// The kinds herald ships, and those `value` declares.
    pub(super) fn declared(&mut self, value: Option<Value<'_>>) -> KindSpecs {
        let mut specs = KindSpecs::default();
        for (name, value) in self.named(value, "kinds") {
            let path = format!("kinds.{name}");
            let span = value.span();
            let kind = match Kind::new(&name) {
                Ok(kind) => kind,
                Err(reason) => {
                    self.error(&span, &path, reason);
                    continue;
                }
            };
            if specs.get(&kind).is_some() {
                self.error(
                    &span,
                    &path,
                    format!("herald ships the kind \"{name}\"; give yours another name"),
                );
                continue;
            }
            let Some(file) = self.typed::<KindFile>(value, &path) else {
                continue;
            };
            let links: Vec<LinkSpec> = file.links.into_iter().map(LinkSpec::from).collect();
            let mut sound = self.linked(&span, &path, &links);
            for attribute in file.attributes.keys() {
                let at = format!("{path}.attributes.{attribute}");
                if [VERSION, PRERELEASE].contains(&attribute.as_str()) {
                    self.error(
                        &span,
                        &at,
                        format!("{attribute} is the attribute versioned = true gives a kind; remove it, and write versioned = true"),
                    );
                    sound = false;
                } else if !readable(attribute) {
                    self.error(
                        &span,
                        &at,
                        "an attribute is named in lowercase letters, digits and '_', starting with a letter",
                    );
                    sound = false;
                }
            }
            if sound {
                specs = specs.with(KindSpec::new(
                    kind,
                    file.description,
                    file.versioned,
                    links,
                    file.attributes,
                ));
            }
        }
        specs
    }
}
