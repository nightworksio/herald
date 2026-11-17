//! A route's `group`: the key its events are gathered by, how long, led by
//! which projects, and how the group is announced.

use herald_core::{GROUP_MOST, GROUP_MOST_ALLOWED, GROUP_WINDOW, Glob, Grouping, LONGEST_HOLD};
use herald_kinds::Placed;
use toml::Spanned;

use super::Loader;
use crate::file::GroupFile;

/// Why a route to a herald feed groups no events.
const FED: &str = "a herald feed publishes each event as an envelope of its own, so it gathers no group; remove group, or give it to a route to another destination";

/// Why a route announcing each project as an actor of its own groups no
/// events.
const ACTING: &str = "the route announces each project as an actor of its own, so a group of several projects has no one actor to announce it; give the group to a second route on a destination with one actor";

impl Loader<'_> {
    /// The grouping `file` asks of the route at `path`, placed as `options`
    /// and to a herald feed where `feeds` says, with its key's template,
    /// where the route groups and every key of it is sound; each key that
    /// is not is named as a mistake.
    pub(super) fn grouping(
        &mut self,
        file: Option<GroupFile>,
        (path, options, feeds): (&str, &Placed, bool),
    ) -> Option<(Grouping, Spanned<String>)> {
        let file = file?;
        let at = format!("{path}.group");
        let mut clean = true;
        for (refused, why) in [(options.names("").is_some(), ACTING), (feeds, FED)] {
            if refused {
                self.error(&file.key.span(), &at, why);
                clean = false;
            }
        }
        let window = file.window.map_or(GROUP_WINDOW, |window| {
            if window.get_ref().0 > LONGEST_HOLD {
                self.error(
                    &window.span(),
                    &format!("{at}.window"),
                    "a group gathers its members for 7d at most; give a shorter window",
                );
                clean = false;
            }
            window.get_ref().0
        });
        let most = file.most.map_or(GROUP_MOST, |most| {
            if !(1..=GROUP_MOST_ALLOWED).contains(most.get_ref()) {
                self.error(
                    &most.span(),
                    &format!("{at}.most"),
                    format!("a group takes from 1 to {GROUP_MOST_ALLOWED} members; give most within them"),
                );
                clean = false;
            }
            *most.get_ref()
        });
        let mut lead = Vec::new();
        for (index, written) in file.lead.iter().enumerate() {
            match Glob::new(written.get_ref()) {
                Ok(glob) => lead.push(glob),
                Err(error) => {
                    self.error(
                        &written.span(),
                        &format!("{at}.lead[{index}]"),
                        error.to_string(),
                    );
                    clean = false;
                }
            }
        }
        clean.then(|| {
            (
                Grouping {
                    key: file.key.get_ref().clone(),
                    window,
                    lead,
                    mode: file.mode,
                    until: file.until,
                    late: file.late,
                    most,
                },
                file.key,
            )
        })
    }
}
