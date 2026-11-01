use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::key::Key;
use crate::profile::{Features, Pins, Threads};
use crate::route::Delivery;

/// One step of an announcement, in the order a destination takes them.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Step {
    /// Post the announcement.
    Post,
    /// Crosspost it.
    Crosspost,
    /// Open its thread.
    OpenThread,
    /// Post an overflow chunk, counted from one after the announcement.
    Chunk(u32),
    /// Pin it.
    Pin,
    /// Unpin the route's earlier pin.
    Unpin,
    /// Deliver it to one recipient's inbox.
    Deliver(String),
}

/// What a destination answered when a step landed.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Facts {
    /// The id of the message the step made or acted on.
    pub message: Option<String>,
    /// The id of the thread it made or posted into.
    pub thread: Option<String>,
    /// The message's permalink.
    pub permalink: Option<String>,
}

/// Where a step stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum State {
    /// A run sent it and recorded no answer, so it may or may not have landed.
    Attempted {
        /// When it was sent.
        at: Timestamp,
    },
    /// It landed.
    Landed {
        /// What the destination answered.
        facts: Facts,
    },
}

/// One step and where it stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    /// The step.
    pub step: Step,
    /// Where it stands.
    pub state: State,
}

/// The run holding an announcement, and until when.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lease {
    /// The run holding it.
    pub holder: String,
    /// When the hold lapses.
    pub until: Timestamp,
}

/// The ledger's entry for one announcement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// The announcement's key.
    pub key: Key,
    /// Each step a run has sent, and where it stands.
    pub records: Vec<Record>,
    /// The run holding the announcement, if one is.
    pub lease: Option<Lease>,
}

impl Entry {
    /// An entry for `key` with nothing sent.
    #[must_use]
    pub fn new(key: Key) -> Self {
        Self {
            key,
            records: Vec::new(),
            lease: None,
        }
    }

    /// The entry held by `run` until `until`.
    #[must_use]
    pub fn claimed(mut self, run: &str, until: Timestamp) -> Self {
        self.lease = Some(Lease {
            holder: run.to_owned(),
            until,
        });
        self
    }

    /// The entry with no run holding it.
    #[must_use]
    pub fn released(mut self) -> Self {
        self.lease = None;
        self
    }

    /// The entry with `step` sent at `at`, its answer not yet recorded.
    #[must_use]
    pub fn attempted(self, step: Step, at: Timestamp) -> Self {
        self.with(step, State::Attempted { at })
    }

    /// The entry with `step` landed, as `facts` say.
    #[must_use]
    pub fn landed(self, step: Step, facts: Facts) -> Self {
        self.with(step, State::Landed { facts })
    }

    /// What `step` answered, if it landed.
    #[must_use]
    pub fn facts(&self, step: &Step) -> Option<&Facts> {
        self.records
            .iter()
            .find(|record| &record.step == step)
            .and_then(|record| match &record.state {
                State::Landed { facts } => Some(facts),
                State::Attempted { .. } => None,
            })
    }

    fn with(mut self, step: Step, state: State) -> Self {
        self.records.retain(|record| record.step != step);
        self.records.push(Record { step, state });
        self
    }

    fn state(&self, step: &Step) -> Option<&State> {
        self.records
            .iter()
            .find(|record| &record.step == step)
            .map(|record| &record.state)
    }
}

/// The steps a route's delivery asks of a destination with `features`, for an
/// announcement of `parts` parts and the inboxes of `recipients`.
#[must_use]
pub fn steps(
    delivery: &Delivery,
    features: &Features,
    parts: u32,
    recipients: &[String],
) -> Vec<Step> {
    let mut steps = vec![Step::Post];
    if delivery.crosspost && features.crosspost {
        steps.push(Step::Crosspost);
    }
    if delivery.thread.is_some() && features.threads == Threads::FromMessage {
        steps.push(Step::OpenThread);
    }
    steps.extend((1..parts).map(Step::Chunk));
    if delivery.pin.is_some() {
        match features.pins {
            Pins::None => {}
            Pins::Many => steps.extend([Step::Pin, Step::Unpin]),
            Pins::One => steps.extend([Step::Unpin, Step::Pin]),
        }
    }
    steps.extend(recipients.iter().cloned().map(Step::Deliver));
    steps
}

/// A step to send, and whether to ask the destination first if an earlier run
/// already landed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Planned {
    /// The step.
    pub step: Step,
    /// Whether an earlier attempt left it unknown.
    pub verify_first: bool,
}

/// What a destination answered to one step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sent {
    /// The step landed, as these facts say.
    Landed(Facts),
    /// The destination asked to be asked again after this long.
    RetryAfter(jiff::SignedDuration),
    /// The destination refused the step, for this reason.
    Refused(String),
}

/// What a run is to do with one announcement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plan {
    /// Every step landed; nothing is sent.
    Complete,
    /// Another run holds the announcement; nothing is sent by this one.
    Held {
        /// The run holding it.
        by: String,
        /// When the hold lapses.
        until: Timestamp,
    },
    /// The steps still to send, in order.
    Send(Vec<Planned>),
}

/// What `run` is to do at `now` with the announcement whose ledger entry is
/// `entry`, when the destination asks for the steps `wanted`.
#[must_use]
pub fn plan(entry: Option<&Entry>, wanted: &[Step], run: &str, now: Timestamp) -> Plan {
    let Some(entry) = entry else {
        return Plan::Send(
            wanted
                .iter()
                .map(|step| Planned {
                    step: step.clone(),
                    verify_first: false,
                })
                .collect(),
        );
    };
    if let Some(lease) = entry
        .lease
        .as_ref()
        .filter(|lease| lease.holder != run && lease.until > now)
    {
        return Plan::Held {
            by: lease.holder.clone(),
            until: lease.until,
        };
    }
    let remaining: Vec<Planned> = wanted
        .iter()
        .filter_map(|step| match entry.state(step) {
            Some(State::Landed { .. }) => None,
            Some(State::Attempted { .. }) => Some(Planned {
                step: step.clone(),
                verify_first: true,
            }),
            None => Some(Planned {
                step: step.clone(),
                verify_first: false,
            }),
        })
        .collect();
    if remaining.is_empty() {
        return Plan::Complete;
    }
    Plan::Send(remaining)
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::key::{Identity, RouteId};
    use crate::profile::MentionStyle;
    use crate::route::{Pinning, Thread};

    fn key() -> Key {
        Key::derive(&RouteId::new("route"), &Identity::new("identity"))
    }

    fn at(text: &str) -> Timestamp {
        text.parse().unwrap()
    }

    fn discord() -> Features {
        Features {
            threads: Threads::FromMessage,
            crosspost: true,
            pins: Pins::Many,
            reads_back: true,
            edits: true,
            mentions: MentionStyle::Roles,
            interactions: true,
        }
    }

    fn everything() -> Delivery {
        Delivery {
            crosspost: true,
            thread: Some(Thread {
                name: "{{ event.title }}".to_owned(),
                archive_after: None,
                lock_after: None,
            }),
            pin: Some(Pinning::Markers),
            ..Delivery::default()
        }
    }

    fn sending(steps: &[Step]) -> Plan {
        Plan::Send(
            steps
                .iter()
                .map(|step| Planned {
                    step: step.clone(),
                    verify_first: false,
                })
                .collect(),
        )
    }

    #[test]
    fn a_full_delivery_on_a_destination_with_every_feature_takes_every_step_in_order() {
        let steps = steps(
            &everything(),
            &discord(),
            3,
            &["https://a.example/inbox".to_owned()],
        );
        assert_eq!(
            steps,
            [
                Step::Post,
                Step::Crosspost,
                Step::OpenThread,
                Step::Chunk(1),
                Step::Chunk(2),
                Step::Pin,
                Step::Unpin,
                Step::Deliver("https://a.example/inbox".to_owned()),
            ]
        );
    }

    #[test]
    fn a_destination_holding_one_pin_unpins_before_it_pins() {
        let features = Features {
            pins: Pins::One,
            ..discord()
        };
        assert_eq!(
            steps(&everything(), &features, 1, &[]),
            [
                Step::Post,
                Step::Crosspost,
                Step::OpenThread,
                Step::Unpin,
                Step::Pin
            ]
        );
    }

    #[test]
    fn a_plain_delivery_of_one_part_only_posts() {
        assert_eq!(
            steps(&Delivery::default(), &discord(), 1, &[]),
            [Step::Post]
        );
    }

    #[test]
    fn a_feature_the_destination_lacks_is_no_step() {
        let features = Features {
            threads: Threads::Replies,
            crosspost: false,
            pins: Pins::None,
            ..discord()
        };
        assert_eq!(
            steps(&everything(), &features, 2, &[]),
            [Step::Post, Step::Chunk(1)]
        );
        let features = Features {
            threads: Threads::None,
            ..discord()
        };
        assert_eq!(
            steps(&everything(), &features, 1, &[]),
            [Step::Post, Step::Crosspost, Step::Pin, Step::Unpin]
        );
    }

    #[test]
    fn with_no_entry_every_step_is_sent() {
        let wanted = [Step::Post, Step::Crosspost];
        assert_eq!(
            plan(None, &wanted, "run", at("2026-10-31T12:00:00Z")),
            sending(&wanted)
        );
    }

    #[test]
    fn landed_steps_are_not_sent_again_and_attempted_ones_are_verified_first() {
        let entry = Entry::new(key())
            .landed(Step::Post, Facts::default())
            .attempted(Step::Crosspost, at("2026-10-31T12:00:00Z"));
        let wanted = [Step::Post, Step::Crosspost, Step::OpenThread];
        assert_eq!(
            plan(Some(&entry), &wanted, "run", at("2026-10-31T13:00:00Z")),
            Plan::Send(vec![
                Planned {
                    step: Step::Crosspost,
                    verify_first: true
                },
                Planned {
                    step: Step::OpenThread,
                    verify_first: false
                },
            ])
        );
    }

    #[test]
    fn every_step_landed_is_complete() {
        let entry = Entry::new(key()).landed(Step::Post, Facts::default());
        assert_eq!(
            plan(
                Some(&entry),
                &[Step::Post],
                "run",
                at("2026-10-31T12:00:00Z")
            ),
            Plan::Complete
        );
    }

    #[test]
    fn another_runs_live_lease_holds_the_announcement() {
        let entry = Entry::new(key()).claimed("other", at("2026-10-31T12:10:00Z"));
        assert_eq!(
            plan(
                Some(&entry),
                &[Step::Post],
                "run",
                at("2026-10-31T12:05:00Z")
            ),
            Plan::Held {
                by: "other".to_owned(),
                until: at("2026-10-31T12:10:00Z")
            }
        );
    }

    #[test]
    fn a_lapsed_lease_or_the_runs_own_holds_nothing() {
        let lapsed = Entry::new(key()).claimed("other", at("2026-10-31T12:10:00Z"));
        assert_eq!(
            plan(
                Some(&lapsed),
                &[Step::Post],
                "run",
                at("2026-10-31T12:10:00Z")
            ),
            sending(&[Step::Post])
        );
        let own = Entry::new(key()).claimed("run", at("2026-10-31T12:10:00Z"));
        assert_eq!(
            plan(Some(&own), &[Step::Post], "run", at("2026-10-31T12:05:00Z")),
            sending(&[Step::Post])
        );
    }

    #[test]
    fn an_entry_records_claims_attempts_and_landings() {
        let facts = Facts {
            message: Some("1".to_owned()),
            thread: Some("2".to_owned()),
            permalink: Some("https://discord.com/channels/3/4/1".to_owned()),
        };
        let entry = Entry::new(key())
            .claimed("run", at("2026-10-31T12:10:00Z"))
            .attempted(Step::Post, at("2026-10-31T12:00:00Z"));
        assert_eq!(entry.facts(&Step::Post), None);
        let entry = entry.landed(Step::Post, facts.clone());
        assert_eq!(entry.facts(&Step::Post), Some(&facts));
        assert_eq!(entry.facts(&Step::Pin), None);
        assert_eq!(entry.records.len(), 1);
        assert_eq!(
            entry.lease.as_ref().map(|lease| lease.holder.as_str()),
            Some("run")
        );
        assert_eq!(entry.released().lease, None);
    }

    #[test]
    fn an_entry_round_trips_through_json() {
        let entry = Entry::new(key())
            .claimed("run", at("2026-10-31T12:10:00Z"))
            .attempted(Step::Chunk(2), at("2026-10-31T12:00:00Z"))
            .landed(
                Step::Deliver("https://a.example/inbox".to_owned()),
                Facts::default(),
            );
        let json = serde_json::to_string(&entry).unwrap();
        assert_eq!(serde_json::from_str::<Entry>(&json).unwrap(), entry);
    }

    fn any_step() -> impl Strategy<Value = Step> {
        prop_oneof![
            Just(Step::Post),
            Just(Step::Crosspost),
            Just(Step::OpenThread),
            (1u32..5).prop_map(Step::Chunk),
            Just(Step::Pin),
            "[a-c]".prop_map(|inbox| Step::Deliver(format!("https://{inbox}.example/inbox"))),
        ]
    }

    proptest! {
        #[test]
        fn a_plan_sends_each_wanted_step_not_landed_once_and_in_order(
            wanted in proptest::collection::btree_set(any_step(), 0..8),
            landed in proptest::collection::vec(any::<bool>(), 8),
            attempted in proptest::collection::vec(any::<bool>(), 8),
        ) {
            let wanted: Vec<Step> = wanted.into_iter().collect();
            let mut entry = Entry::new(key());
            for (index, step) in wanted.iter().enumerate() {
                if attempted[index] {
                    entry = entry.attempted(step.clone(), at("2026-10-31T12:00:00Z"));
                }
                if landed[index] {
                    entry = entry.landed(step.clone(), Facts::default());
                }
            }
            let expected: Vec<Planned> = wanted
                .iter()
                .enumerate()
                .filter(|(index, _)| !landed[*index])
                .map(|(index, step)| Planned { step: step.clone(), verify_first: attempted[index] })
                .collect();
            let planned = plan(Some(&entry), &wanted, "run", at("2026-10-31T12:30:00Z"));
            if expected.is_empty() {
                prop_assert_eq!(planned, Plan::Complete);
            } else {
                prop_assert_eq!(planned, Plan::Send(expected));
            }
        }
    }
}
