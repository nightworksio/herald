//! `[boards.*]` (ADR-0056): each board a report and a living route to it,
//! read as `[reports.*]` and `[[routes]]` read them.

use std::borrow::Cow;

use toml::Spanned;
use toml::de::{DeArray, DeValue};

use super::{Loader, Value};
use crate::views::{BOARD_KEYS, QUEUE_TEMPLATE, REPORT_KEYS};

/// A report, as where it is written, its name and its keys.
pub(super) type Boarded<'i> = (String, String, Value<'i>);

/// A board's route, as where it is written and its keys.
pub(super) type BoardRoute<'i> = (String, Value<'i>);

impl Loader<'_> {
    /// Each board of `[boards]`: its report, and its living route, whose
    /// id is the board's name, whose one source is its report, and which
    /// renders `herald:queue` where it names no template.
    pub(super) fn boards<'i>(
        &mut self,
        value: Option<Value<'i>>,
    ) -> (Vec<Boarded<'i>>, Vec<BoardRoute<'i>>) {
        let mut reports = Vec::new();
        let mut routes = Vec::new();
        for (name, value) in self.named(value, "boards") {
            let path = format!("boards.{name}");
            let span = value.span();
            let Some((report, route)) = self.parted(value, &path, &REPORT_KEYS) else {
                continue;
            };
            let DeValue::Table(mut route) = route.into_inner() else {
                continue;
            };
            let named: Vec<String> = BOARD_KEYS
                .into_iter()
                .filter(|key| route.contains_key(*key))
                .map(str::to_owned)
                .collect();
            if !named.is_empty() {
                self.error(
                    &span,
                    &path,
                    format!(
                        "a board is its own living route to its own report, so it names no {}",
                        named.join(", ")
                    ),
                );
                continue;
            }
            let spanned = |value: DeValue<'i>| Spanned::new(span.clone(), value);
            let text = |text: String| spanned(DeValue::String(Cow::Owned(text)));
            let key = |key: &'static str| Spanned::new(span.clone(), Cow::Borrowed(key));
            let mut sources = DeArray::new();
            sources.push(text(name.clone()));
            route.insert(key("id"), text(name.clone()));
            route.insert(key("sources"), spanned(DeValue::Array(sources)));
            route.insert(key("living"), spanned(DeValue::Boolean(true)));
            if !route.contains_key("template") {
                route.insert(key("template"), text(QUEUE_TEMPLATE.to_owned()));
            }
            reports.push((path.clone(), name, report));
            routes.push((path, spanned(DeValue::Table(route))));
        }
        (reports, routes)
    }
}
