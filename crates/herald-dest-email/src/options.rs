use std::fmt;

use herald_core::{SecretRef, Symbols, WaitOptions};
use schemars::JsonSchema;
use serde::de::{self, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::address::Mailbox;

/// How each message of an email destination leaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[schemars(rename = "EmailTransport")]
pub enum Transport {
    /// Amazon SES, by `SendEmail` of its API version 2, each request signed
    /// with AWS Signature Version 4.
    Ses,
}

/// An email destination, as `[destinations.*]` writes it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DestinationOptions {
    /// How each message leaves: `ses`, Amazon SES's API.
    pub transport: Transport,
    /// Who each message is from, written `local@domain` or
    /// `Name <local@domain>`; its domain names each message's id.
    pub from: Mailbox,
    /// Where replies go, where not to `from`.
    pub reply_to: Option<Mailbox>,
    /// The most bytes a message holds, 262144 where unset: an announcement
    /// whose message would be longer is cut into more messages, or refused.
    pub max_bytes: Option<usize>,
    /// The AWS region Amazon SES sends from, such as `eu-west-1` or
    /// `us-gov-west-1`.
    pub region: Option<String>,
    /// Whether Amazon SES is reached at its region's FIPS endpoint,
    /// `email-fips.<region>`, where AWS gives the region one; false where
    /// unset.
    pub fips: Option<bool>,
    /// Whether Amazon SES is reached at its region's endpoint on IPv4 and
    /// IPv6, under `api.aws`; false where unset.
    pub dual_stack: Option<bool>,
    /// The base URL Amazon SES is reached at, in place of its region's own
    /// endpoint, such as a VPC endpoint's: an https URL of a host alone.
    pub endpoint: Option<String>,
    /// The secret holding the access key's id; `AWS_ACCESS_KEY_ID` where
    /// unset.
    pub access_key_id: Option<SecretRef>,
    /// The secret holding the secret access key; `AWS_SECRET_ACCESS_KEY`
    /// where unset.
    pub secret_access_key: Option<SecretRef>,
    /// The secret holding a session token, for temporary credentials such
    /// as a role's, `AWS_SESSION_TOKEN` among them; none is sent where
    /// unset.
    pub session_token: Option<SecretRef>,
    /// The Amazon SES configuration set each message is sent with, where it
    /// names one.
    pub configuration_set: Option<String>,
    /// The characters its messages write where its markup has none: a task
    /// done and to do, a rule and a text list's bullet.
    pub symbols: Option<Symbols>,
    /// How long it waits before sending again where its transport answers
    /// that it is unwell or limited and names no wait.
    pub waits: Option<WaitOptions>,
}

/// What a route says about email alone: whom each message goes to.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RouteOptions {
    /// The addresses written in each message's `To` field.
    #[serde(default, skip_serializing_if = "Addresses::is_empty")]
    pub to: Addresses,
    /// The addresses written in each message's `Cc` field.
    #[serde(default, skip_serializing_if = "Addresses::is_empty")]
    pub cc: Addresses,
    /// The addresses each message goes to unnamed in it.
    #[serde(default, skip_serializing_if = "Addresses::is_empty")]
    pub bcc: Addresses,
}

/// A list of mailboxes, written as a list or, for one, as its text alone.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Addresses(pub Vec<Mailbox>);

impl Addresses {
    /// Whether it lists none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl Serialize for Addresses {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.iter().map(Mailbox::written))
    }
}

struct Listed;

impl<'de> Visitor<'de> for Listed {
    type Value = Addresses;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("an address or a list of addresses")
    }

    fn visit_str<E: de::Error>(self, text: &str) -> Result<Addresses, E> {
        Mailbox::read(text).map(|mailbox| Addresses(vec![mailbox]))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Addresses, A::Error> {
        let mut listed = Vec::new();
        while let Some(mailbox) = seq.next_element()? {
            listed.push(mailbox);
        }
        Ok(Addresses(listed))
    }
}

impl<'de> Deserialize<'de> for Addresses {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(Listed)
    }
}

impl JsonSchema for Addresses {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Addresses".into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        let mailbox = generator.subschema_for::<Mailbox>();
        schemars::json_schema!({
            "description": "An address, or a list of addresses.",
            "anyOf": [mailbox, { "type": "array", "items": mailbox }]
        })
    }
}
