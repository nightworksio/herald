use herald_core::{Delivery, Profile, SecretRef, Symbols, WaitOptions};
use herald_kinds::{
    Checking, DestinationConfig, DestinationKind, Fault, Placed, Placement, Placing, Reached,
};
use herald_ports::Secret;
use url::Url;

use crate::email::{self, Addressing, EMAIL, Email, UNCHECKED};
use crate::message::{MOST_BYTES, Recipients};
use crate::options::{DestinationOptions, RouteOptions};
use crate::ses::{Credentials, SES, SES_BYTES, SES_RECIPIENTS, Ses, regional};

/// The kind of an email destination, as the config spells it.
pub const KIND: &str = "email";

/// The fewest bytes a config may bound a message to: room for its header
/// beside a part.
pub const FEWEST_BYTES: usize = 4_096;

/// The longest name of an Amazon SES configuration set
/// (<https://docs.aws.amazon.com/ses/latest/dg/quotas.html>, "Quotas
/// related to event publishing").
const SET_NAME: usize = 64;

/// The secrets an Amazon SES destination reads where its table names none.
const ACCESS_KEY_ID: &str = "AWS_ACCESS_KEY_ID";
const SECRET_ACCESS_KEY: &str = "AWS_SECRET_ACCESS_KEY";

impl DestinationOptions {
    /// The most bytes of a message.
    #[must_use]
    pub fn most(&self) -> usize {
        self.max_bytes.unwrap_or(MOST_BYTES)
    }

    /// The base URL its transport reaches.
    ///
    /// # Errors
    ///
    /// The key that is not written as it must be, and why.
    pub fn url(&self) -> Result<String, (&'static str, String)> {
        let region = self.region.as_deref().ok_or((
            "region",
            format!("{SES} sends from a region; give it, such as \"eu-west-1\""),
        ))?;
        let Some(endpoint) = &self.endpoint else {
            return regional(
                region,
                self.fips.unwrap_or_default(),
                self.dual_stack.unwrap_or_default(),
            );
        };
        if !region.is_empty()
            && region
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        {
            if let Some(key) = [("fips", self.fips), ("dual_stack", self.dual_stack)]
                .iter()
                .find_map(|(key, given)| given.map(|_| *key))
            {
                return Err((key, "the endpoint names the host alone; drop it".to_owned()));
            }
            if base_url(endpoint) {
                return Ok(endpoint.clone());
            }
            return Err((
                "endpoint",
                "it is not an https URL of a host alone; give one, such as \
                 \"https://email.eu-west-1.amazonaws.com\""
                    .to_owned(),
            ));
        }
        Err((
            "region",
            "it is not a region; give one, such as \"eu-west-1\"".to_owned(),
        ))
    }

    /// Each key that is not written as it must be, and why.
    fn faults(&self) -> Vec<Fault> {
        let mut faults: Vec<Fault> = self
            .url()
            .err()
            .map(|(key, why)| Fault::at(key, why))
            .into_iter()
            .collect();
        if let Some(most) = self.max_bytes
            && !(FEWEST_BYTES..=SES_BYTES).contains(&most)
        {
            faults.push(Fault::at(
                "max_bytes",
                format!(
                    "{SES} takes messages of at most {SES_BYTES} bytes, and a message needs at \
                     least {FEWEST_BYTES}; give a bound between them"
                ),
            ));
        }
        if let Some(set) = &self.configuration_set
            && !set_name(set)
        {
            faults.push(Fault::at(
                "configuration_set",
                format!(
                    "it is not a configuration set's name: at most {SET_NAME} letters, digits, \
                     hyphens and underscores"
                ),
            ));
        }
        faults
    }

    /// It, each secret its table leaves unnamed named as AWS names it.
    fn named(mut self) -> Self {
        for (given, name) in [
            (&mut self.access_key_id, ACCESS_KEY_ID),
            (&mut self.secret_access_key, SECRET_ACCESS_KEY),
        ] {
            given.get_or_insert_with(|| SecretRef {
                secret: name.to_owned(),
            });
        }
        self
    }
}

impl RouteOptions {
    /// Whom its messages go to.
    ///
    /// # Errors
    ///
    /// Why they are not recipients herald sends to.
    pub fn recipients(&self) -> Result<Recipients, crate::message::Unrecipiented> {
        Recipients::new(self.to.0.clone(), self.cc.0.clone(), self.bcc.0.clone())
    }
}

impl DestinationConfig for DestinationOptions {
    fn kind(&self) -> &'static str {
        KIND
    }

    fn symbols_written(&self) -> Option<Symbols> {
        self.symbols
    }

    fn waits_written(&self) -> Option<WaitOptions> {
        self.waits
    }

    fn credential(&self) -> Option<&SecretRef> {
        self.secret_access_key.as_ref()
    }

    fn secret_refs(&self) -> Vec<&SecretRef> {
        self.access_key_id
            .iter()
            .chain(&self.secret_access_key)
            .chain(&self.session_token)
            .collect()
    }

    fn reads(&self) -> Vec<&SecretRef> {
        self.access_key_id
            .iter()
            .chain(&self.session_token)
            .collect()
    }

    fn profile(&self, placed: &Placed) -> Option<(Profile, Option<Profile>)> {
        placed.of::<RouteOptions>()?;
        Some((email::profile(self.most()), None))
    }

    fn public(&self, _: &Placed, _: &Delivery) -> Option<&'static str> {
        None
    }

    fn checking(&self, _: &Placed) -> Checking {
        Checking::Unchecked(UNCHECKED)
    }

    fn recalled(&self, written: &str) -> Option<Placed> {
        serde_json::from_str::<RouteOptions>(written)
            .ok()
            .filter(|route| route.recipients().is_ok())
            .map(Placed::new)
    }

    fn reach(&self, _: &Placed, _: &str, _: Secret) -> Option<Reached> {
        None
    }

    fn reach_reading(
        &self,
        placed: &Placed,
        _: &str,
        (secret, read): (Secret, Vec<Secret>),
    ) -> Option<Reached> {
        let route = placed.of::<RouteOptions>()?;
        let mut read = read.into_iter();
        let credentials = Credentials {
            access_key_id: read.next()?,
            secret_access_key: secret,
            session_token: read.next(),
        };
        let ses = Ses::new(
            &self.url().ok()?,
            self.region.as_deref()?,
            credentials,
            self.configuration_set.clone(),
        );
        let addressing = Addressing {
            from: self.from.clone(),
            reply_to: self.reply_to.clone(),
            recipients: route.recipients().ok()?,
            most: self.most(),
        };
        Some(Reached::new(
            Email::new(addressing, ses).waiting(self.waits()),
        ))
    }
}

impl Placement for RouteOptions {
    fn written(&self) -> Option<String> {
        serde_json::to_string(self).ok()
    }

    fn place(&self) -> Option<&str> {
        None
    }
}

/// Email, as a kind of destination.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmailKind {}

impl DestinationKind for EmailKind {
    const NAME: &str = KIND;
    const DESCRIPTION: &str = "Email, one message per announcement, through Amazon SES.";
    const LABEL: &str = EMAIL;
    const ROUTE: &str = "EmailRoute";
    const ROUTE_DESCRIPTION: &str = "A route to an email destination: whom its messages go to.";

    type Options = DestinationOptions;
    type Route = RouteOptions;

    const FIRST_RUN: &[herald_kinds::Variant] = crate::first_run::VARIANTS;

    fn checked(
        options: DestinationOptions,
        _read: &dyn Fn(&str) -> Result<String, String>,
    ) -> Result<(DestinationOptions, Vec<Fault>), Fault> {
        let faults = options.faults();
        Ok((options.named(), faults))
    }

    fn placed(_: &DestinationOptions, route: &RouteOptions, _: Placing<'_>) -> Vec<Fault> {
        match route.recipients() {
            Err(why) => vec![Fault::here(format!("the route {why}"))],
            Ok(recipients) if recipients.envelope().count() > SES_RECIPIENTS => {
                vec![Fault::here(format!(
                    "the route names more than the {SES_RECIPIENTS} recipients {SES} takes in \
                     one message, `to`, `cc` and `bcc` together"
                ))]
            }
            Ok(_) => Vec::new(),
        }
    }
}

/// Whether `set` is written as a configuration set's name.
fn set_name(set: &str) -> bool {
    (1..=SET_NAME).contains(&set.len())
        && set
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

/// Whether `base` is an https URL of a host, with a path at most: no
/// credentials, query or fragment.
fn base_url(base: &str) -> bool {
    Url::parse(base).is_ok_and(|url| {
        url.scheme() == "https"
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none()
    })
}
