use herald_core::SecretRef;
use herald_ports::{Request, Response, Secret};

/// What a destination is asked to set up to take commands (ADR-0016).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupTask<'a> {
    /// Register its commands.
    Commands,
    /// List the server `guild`'s roles, or make one named `create`.
    Roles {
        /// The server.
        guild: &'a str,
        /// The name of a role to make.
        create: Option<&'a str>,
    },
    /// Its app's manifest, its commands posted to herald where `served`
    /// says, where it takes them posted.
    Manifest {
        /// Where herald is served.
        served: Option<Served<'a>>,
    },
    /// The registration its homeserver installs, reaching herald where
    /// `served` says.
    Registration {
        /// Where herald is served.
        served: Served<'a>,
    },
}

/// Where herald is served, as a setup task is given it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Served<'a> {
    /// The address herald is served at, as it was given.
    pub url: &'a str,
    /// The path every endpoint is served under, as `[server] base` sets it.
    pub base: &'a str,
}

impl Served<'_> {
    /// The address herald's endpoints are served under: the URL, with no
    /// trailing slash, then the base.
    ///
    /// # Errors
    ///
    /// Where the URL is not an `https` or `http` address, or holds what a
    /// YAML or JSON string could be broken by.
    pub fn address(self) -> Result<String, String> {
        let url = self.url;
        let sound = herald_core::web_url(url)
            && !url.contains(|character: char| {
                character.is_whitespace() || "\"'\\".contains(character)
            });
        if sound {
            Ok(format!("{}{}", url.trim_end_matches('/'), self.base))
        } else {
            Err(format!(
                "\"{url}\" is not an address herald is served at; give one such as https://herald.example"
            ))
        }
    }
}

impl SetupTask<'_> {
    /// Why a destination of `kind` is not set up as the task asks.
    #[must_use]
    pub fn unasked(self, kind: &str) -> String {
        let task = match self {
            Self::Commands => "commands",
            Self::Roles { .. } => "roles",
            Self::Manifest { .. } => "a manifest",
            Self::Registration { .. } => "a registration",
        };
        format!("a {kind} destination is not set up with {task}")
    }
}

/// What a setup task reads before its steps are made: each secret, and how
/// many new tokens it hands out.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SetupNeeds<'a> {
    /// The secrets it reads, in the order [`Made::secrets`] holds them.
    pub secrets: Vec<&'a SecretRef>,
    /// How many new tokens it hands out.
    pub tokens: usize,
}

/// What a setup task's needs were met with.
#[derive(Debug, Default)]
pub struct Made {
    /// Each secret it reads, in the order its needs name them.
    pub secrets: Vec<Secret>,
    /// Each new token, made by the system's random generator.
    pub tokens: Vec<String>,
}

/// One step of a setup task.
#[derive(Debug)]
pub enum SetupStep {
    /// Lines to print.
    Say(Vec<String>),
    /// A request to send to the base URL `base`, whose response
    /// [`Setup::said`] reads.
    Send {
        /// The base URL it is sent to.
        base: String,
        /// The request.
        request: Request,
    },
}

/// How a destination that takes commands is set up, without I/O: what a
/// task reads, the steps it takes, and what the response to a request it
/// sends says.
pub trait Setup {
    /// What `task` reads, for the destination `name`.
    ///
    /// # Errors
    ///
    /// Where the destination is not set up as `task` asks, saying why.
    fn needs(&self, name: &str, task: SetupTask<'_>) -> Result<SetupNeeds<'_>, String>;

    /// The steps `task` takes for the destination `name`, its needs met with
    /// `made`.
    ///
    /// # Errors
    ///
    /// Where the destination is not set up as `task` asks, saying why.
    fn steps(&self, name: &str, task: SetupTask<'_>, made: Made) -> Result<Vec<SetupStep>, String>;

    /// What is said of `task` from the response to the request it sent.
    ///
    /// # Errors
    ///
    /// What the platform said where it did not do it.
    fn said(&self, task: SetupTask<'_>, response: &Response) -> Result<Vec<String>, String>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_task_a_kind_does_not_take_names_both() {
        let asked = [
            (SetupTask::Commands, "commands"),
            (
                SetupTask::Roles {
                    guild: "1",
                    create: None,
                },
                "roles",
            ),
            (SetupTask::Manifest { served: None }, "a manifest"),
            (
                SetupTask::Registration {
                    served: Served {
                        url: "https://herald.example",
                        base: "",
                    },
                },
                "a registration",
            ),
        ];
        for (task, named) in asked {
            assert_eq!(
                task.unasked("chat"),
                format!("a chat destination is not set up with {named}")
            );
        }
    }

    #[test]
    fn herald_is_served_under_its_base_at_a_sound_address() {
        let served = |url| Served {
            url,
            base: "/tools",
        };
        assert_eq!(
            served("https://herald.example/").address(),
            Ok("https://herald.example/tools".to_owned())
        );
        for unsound in [
            "ftp://x",
            "https://herald.example/a b",
            "https://herald.example/'",
            "https://herald.example/\"",
            "https://herald.example/\\",
        ] {
            assert_eq!(
                served(unsound).address(),
                Err(format!(
                    "\"{unsound}\" is not an address herald is served at; give one such as https://herald.example"
                ))
            );
        }
    }
}
