//! How herald's own requests reach the services a config names, and the
//! bounds a request on others' behalf keeps (ADR-0007).

use jiff::SignedDuration;

/// How herald's own requests reach the services a config names. None of it
/// applies to the hosts herald reaches on others' behalf (ADR-0007).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outbound {
    /// How long a connection may take to open.
    pub connect: SignedDuration,
    /// How long a request may take, from sending it to the end of its
    /// answer.
    pub total: SignedDuration,
    /// The proxy herald's own requests go through.
    pub proxy: Proxy,
    /// The certificate authorities trusted beside the Mozilla roots, where
    /// the config names a file of them.
    pub roots: Option<Roots>,
    /// The most bytes of a response's body read.
    pub body: usize,
}

impl Outbound {
    /// How long a connection may take to open where the config says
    /// nothing.
    pub const CONNECT: SignedDuration = SignedDuration::from_secs(10);
    /// How long a request may take where the config says nothing.
    pub const TOTAL: SignedDuration = SignedDuration::from_secs(30);
    /// The most bytes of a response's body read where the config says
    /// nothing: the npm registry's packument of a long-lived package is the
    /// largest answer herald asks for.
    pub const BODY: usize = 32 << 20;
    /// The most bytes of a response's body a config may let herald read:
    /// four times the default.
    pub const BODY_MOST: usize = Self::BODY * 4;
    /// The settings where the config says nothing.
    pub const STANDARD: Self = Self {
        connect: Self::CONNECT,
        total: Self::TOTAL,
        proxy: Proxy::Environment,
        roots: None,
        body: Self::BODY,
    };
    /// The most a connection to a host reached on others' behalf may take
    /// to open, whatever the config says (ADR-0007).
    pub const PUBLIC_CONNECT: SignedDuration = SignedDuration::from_secs(5);
    /// The most a request to a host reached on others' behalf may take,
    /// whatever the config says (ADR-0007).
    pub const PUBLIC_TOTAL: SignedDuration = SignedDuration::from_secs(10);

    /// These settings, taking `total` from `HERALD_TIMEOUT_MS` where the
    /// host sets it to a whole number of milliseconds.
    #[must_use]
    pub fn timed(mut self, timeout_ms: Option<&str>) -> Self {
        if let Some(total) = timeout_ms
            .and_then(|millis| millis.parse::<u64>().ok())
            .filter(|millis| *millis > 0)
            .and_then(|millis| i64::try_from(millis).ok())
        {
            self.total = SignedDuration::from_millis(total);
        }
        self
    }

    /// The bounds of a request to a host reached on others' behalf: these
    /// settings', within ADR-0007's ceilings, through no proxy and trusting
    /// only the Mozilla roots.
    #[must_use]
    pub fn public(&self) -> Self {
        Self {
            connect: self.connect.min(Self::PUBLIC_CONNECT),
            total: self.total.min(Self::PUBLIC_TOTAL),
            proxy: Proxy::None,
            roots: None,
            body: self.body.min(Self::BODY),
        }
    }
}

impl Default for Outbound {
    fn default() -> Self {
        Self::STANDARD
    }
}

/// The proxy herald's own requests go through.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Proxy {
    /// The host's `HTTPS_PROXY` and `NO_PROXY`, where the host honours them.
    Environment,
    /// None.
    None,
    /// The proxy at this http or https URL.
    At(String),
}

/// Certificate authorities trusted beside the Mozilla roots.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Roots {
    /// The file they were read from, as the config names it.
    pub file: String,
    /// Their certificates, in PEM.
    pub pem: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_whole_number_of_milliseconds_takes_the_place_of_total() {
        let timed = Outbound::STANDARD.timed(Some("2500"));
        assert_eq!(timed.total, SignedDuration::from_millis(2500));
        assert_eq!(timed.connect, Outbound::CONNECT);
        for kept in [None, Some("0"), Some("-5"), Some("soon"), Some("1.5")] {
            assert_eq!(
                Outbound::STANDARD.timed(kept),
                Outbound::STANDARD,
                "{kept:?}"
            );
        }
    }

    #[test]
    fn a_request_on_others_behalf_keeps_the_ceilings_no_proxy_and_no_roots() {
        let generous = Outbound {
            connect: SignedDuration::from_secs(20),
            total: SignedDuration::from_mins(2),
            proxy: Proxy::At("https://proxy.example".to_owned()),
            roots: Some(Roots {
                file: "ca.pem".to_owned(),
                pem: String::new(),
            }),
            body: Outbound::BODY_MOST,
        };
        assert_eq!(
            generous.public(),
            Outbound {
                connect: Outbound::PUBLIC_CONNECT,
                total: Outbound::PUBLIC_TOTAL,
                proxy: Proxy::None,
                roots: None,
                body: Outbound::BODY,
            }
        );
        let brisk = Outbound {
            connect: SignedDuration::from_secs(1),
            total: SignedDuration::from_secs(2),
            ..Outbound::STANDARD
        };
        assert_eq!(brisk.public().connect, SignedDuration::from_secs(1));
        let small = Outbound {
            body: 1024,
            ..Outbound::STANDARD
        };
        assert_eq!(small.public().body, 1024);
        assert_eq!(Outbound::BODY_MOST, 128 << 20);
        assert_eq!(brisk.public().total, SignedDuration::from_secs(2));
    }
}
