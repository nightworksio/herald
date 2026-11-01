use std::collections::BTreeMap;
use std::ops::RangeInclusive;

use serde_json::Value;

use crate::fixture::{Exchange, Fixture};

/// The ids a recording renumbers: each run of a number of `digits` becomes
/// the next of a count up from `first`, the same id the same number each
/// time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ids {
    /// How many digits an id has.
    pub digits: RangeInclusive<usize>,
    /// The first number given out.
    pub first: u64,
}

/// What a recording keeps and replaces before it is written.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scrub {
    /// Each `(from, to)`, replaced wherever it appears.
    pub replace: Vec<(String, String)>,
    /// The ids renumbered, other than those `replace` writes.
    pub ids: Option<Ids>,
    /// The response headers kept, by name without regard to case. Every other
    /// response header is dropped.
    pub headers: Vec<String>,
}

impl Scrub {
    /// `fixture` with each header dropped, each value replaced and each id
    /// renumbered, in the order the exchanges were made.
    #[must_use]
    pub fn applied(&self, fixture: &Fixture) -> Fixture {
        let mut ids = Renumbering {
            scrub: self,
            given: BTreeMap::new(),
        };
        Fixture {
            exchanges: fixture
                .exchanges
                .iter()
                .map(|exchange| ids.exchange(exchange))
                .collect(),
        }
    }
}

struct Renumbering<'a> {
    scrub: &'a Scrub,
    given: BTreeMap<String, String>,
}

impl Renumbering<'_> {
    fn exchange(&mut self, exchange: &Exchange) -> Exchange {
        let mut scrubbed = exchange.clone();
        scrubbed.request.url = self.text(&exchange.request.url);
        scrubbed.request.body = exchange.request.body.as_ref().map(|body| self.value(body));
        scrubbed.response.headers.retain(|name, _| {
            self.scrub
                .headers
                .iter()
                .any(|kept| kept.eq_ignore_ascii_case(name))
        });
        scrubbed.response.body = exchange.response.body.as_ref().map(|body| self.value(body));
        scrubbed
    }

    fn value(&mut self, value: &Value) -> Value {
        match value {
            Value::String(text) => Value::String(self.text(text)),
            Value::Array(items) => {
                Value::Array(items.iter().map(|item| self.value(item)).collect())
            }
            Value::Object(fields) => Value::Object(
                fields
                    .iter()
                    .map(|(name, field)| (name.clone(), self.value(field)))
                    .collect(),
            ),
            other => other.clone(),
        }
    }

    fn text(&mut self, text: &str) -> String {
        let replaced = self
            .scrub
            .replace
            .iter()
            .fold(text.to_owned(), |text, (from, to)| {
                text.replace(from.as_str(), to)
            });
        let Some(ids) = &self.scrub.ids else {
            return replaced;
        };
        let mut out = String::new();
        let mut run = String::new();
        for character in replaced.chars().chain(std::iter::once('\0')) {
            if character.is_ascii_digit() {
                run.push(character);
                continue;
            }
            out.push_str(&self.id(std::mem::take(&mut run), ids));
            out.push(character);
        }
        out.pop();
        out
    }

    fn id(&mut self, run: String, ids: &Ids) -> String {
        let kept =
            !ids.digits.contains(&run.len()) || self.scrub.replace.iter().any(|(_, to)| *to == run);
        if kept {
            return run;
        }
        let next = ids.first + self.given.len() as u64;
        self.given
            .entry(run)
            .or_insert_with(|| next.to_string())
            .clone()
    }
}

#[cfg(test)]
mod tests;
