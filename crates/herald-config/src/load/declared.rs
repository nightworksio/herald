use herald_core::{Kind, KindSpec, KindSpecs, PRERELEASE, VERSION, readable};

use super::{Loader, Value};
use crate::file::KindFile;

impl Loader<'_> {
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
            let mut sound = true;
            for link in file.links.iter().filter(|link| !readable(link)) {
                self.error(
                    &span,
                    &format!("{path}.links"),
                    format!("\"{link}\" cannot name a link; name it in lowercase letters, digits and '_', starting with a letter"),
                );
                sound = false;
            }
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
                    file.links,
                    file.attributes,
                ));
            }
        }
        specs
    }
}
