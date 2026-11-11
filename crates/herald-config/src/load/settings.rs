//! `[http]`, `[server]`, `[render]` and `[announce]`: the bounds, waits and
//! limits herald keeps.

use std::num::NonZeroUsize;
use std::ops::Range;

use herald_core::{Duration, Inbound, RecordKind, Sandbox, Size};
use herald_kinds::Store;
use herald_ports::{Outbound, Proxy, Roots};
use jiff::SignedDuration;
use toml::Spanned;

use super::{Loader, Value};
use crate::config::{Announce, KeyCache, Reconnecting, Server};
use crate::file::{
    AnnounceFile, HttpFile, InboundFile, KeyCacheFile, RenderFile, ServerFile, SessionsFile,
};

impl Loader<'_> {
    /// `[http]` as `file` gives it, its CA file read through `read`.
    pub(super) fn http(
        &mut self,
        file: HttpFile,
        read: &dyn Fn(&str) -> Result<String, String>,
    ) -> Outbound {
        let defaults = Outbound::default();
        let total = file.total.map_or(defaults.total, |total| total.0);
        let connect = file.connect.map_or(defaults.connect, |connect| {
            if connect.get_ref().0 > total {
                self.error(
                    &connect.span(),
                    "http.connect",
                    "a connection may take no longer than the whole request; make connect at most total",
                );
            }
            connect.into_inner().0
        });
        let proxy = file.proxy.map_or(Proxy::Environment, |proxy| {
            match proxy.get_ref().as_str() {
                "environment" => Proxy::Environment,
                "none" => Proxy::None,
                url if url.starts_with("http://") || url.starts_with("https://") => {
                    Proxy::At(url.to_owned())
                }
                other => {
                    self.error(
                        &proxy.span(),
                        "http.proxy",
                        format!(
                            "\"{other}\" is no proxy; write \"environment\", \"none\" or a proxy's http or https URL"
                        ),
                    );
                    Proxy::None
                }
            }
        });
        let roots = file
            .ca_file
            .and_then(|ca_file| match read(ca_file.get_ref()) {
                Ok(pem) if pem.contains("-----BEGIN CERTIFICATE-----") => Some(Roots {
                    file: ca_file.get_ref().clone(),
                    pem,
                }),
                Ok(_) => {
                    self.error(
                        &ca_file.span(),
                        "http.ca_file",
                        format!("{} holds no PEM certificate", ca_file.get_ref()),
                    );
                    None
                }
                Err(error) => {
                    self.error(&ca_file.span(), "http.ca_file", error);
                    None
                }
            });
        let body = self.most(
            file.body.map(|body| (body.span(), body.into_inner().0)),
            defaults.body,
            Outbound::BODY_MOST,
            "http.body",
            &Size::written(Outbound::BODY_MOST),
        );
        Outbound {
            connect,
            total,
            proxy,
            roots,
            body,
        }
    }

    /// `[server]` as `file` gives it, over the store `store`.
    pub(super) fn server(&mut self, file: ServerFile, store: Option<&Store>) -> Server {
        let defaults = Server::default();
        let listen = file.listen.map_or(defaults.listen, |listen| {
            if listen.get_ref().parse::<std::net::SocketAddr>().is_err() {
                self.error(
                    &listen.span(),
                    "server.listen",
                    format!(
                        "\"{}\" is not an address and a port; write one such as \"{}\"",
                        listen.get_ref(),
                        Server::LISTEN
                    ),
                );
            }
            listen.into_inner()
        });
        let base = file.base.map_or_else(String::new, |base| {
            if !base
                .get_ref()
                .strip_prefix('/')
                .is_some_and(|rest| herald_core::path_of_names(rest, 1..=usize::MAX))
            {
                self.error(
                    &base.span(),
                    "server.base",
                    format!(
                        "\"{}\" is not a path to serve under; write one such as \"/herald\", starting with / and ending without one",
                        base.get_ref()
                    ),
                );
            }
            base.into_inner()
        });
        let keys = file.keys.cache.map_or(KeyCache::Memory, |cache| {
            if *cache.get_ref() == KeyCacheFile::Memory {
                return KeyCache::Memory;
            }
            if let Some(store) = store
                && !store.keeps(RecordKind::Keys)
            {
                let hint = store
                    .kept_by(RecordKind::Keys)
                    .unwrap_or_else(|| "give a store that keeps records".to_owned());
                self.error(
                    &cache.span(),
                    "server.keys.cache",
                    format!(
                        "the {} store keeps no keys; keep them in \"memory\", or {hint}",
                        store.kind()
                    ),
                );
            }
            KeyCache::Store
        });
        let count = |given: Option<NonZeroUsize>, default| given.map_or(default, NonZeroUsize::get);
        Server {
            listen,
            base,
            jobs: count(file.jobs, defaults.jobs),
            queue: count(file.queue, count(file.jobs, defaults.jobs)),
            connections: count(file.connections, defaults.connections),
            body: self.most(
                file.body.map(|body| (body.span(), body.into_inner().0)),
                defaults.body,
                Server::BODY_MOST,
                "server.body",
                &Size::written(Server::BODY_MOST),
            ),
            fetches: count(file.fetches, defaults.fetches),
            completions: count(file.completions, defaults.completions),
            silence: file.silence.map_or(defaults.silence, |silence| silence.0),
            keys,
            keys_refreshed: file
                .keys
                .refresh_after
                .map_or(defaults.keys_refreshed, |after| after.0),
            keys_reread: file
                .keys
                .reread_after
                .map_or(defaults.keys_reread, |after| after.0),
            keys_most: count(file.keys.most, defaults.keys_most),
            inbound: self.inbound(file.inbound, file.keys.document),
            busy_retry: file.busy_retry.map_or(Server::BUSY_RETRY, |after| after.0),
            read: file.read.map_or(Server::READ, |read| read.0),
            held: self.held(file.held, file.read),
            accept_pause: file
                .accept_pause
                .map_or(Server::ACCEPT_PAUSE, |pause| pause.0),
            sessions: self.reconnecting(file.sessions),
        }
    }

    /// `given`, where it is no more than `most`; a value past it is named
    /// as a mistake by `path`, with `most` written as `shown`.
    fn most<T: PartialOrd + Copy>(
        &mut self,
        given: Option<(Range<usize>, T)>,
        default: T,
        most: T,
        path: &str,
        shown: &str,
    ) -> T {
        let Some((span, given)) = given else {
            return default;
        };
        if given > most {
            self.error(
                &span,
                path,
                format!(
                    "make it at most {shown}, four times its default; a wider bound loosens what herald holds a request or a template to"
                ),
            );
        }
        given
    }

    fn inbound(&mut self, file: InboundFile, document: Option<Spanned<Size>>) -> Inbound {
        let defaults = Inbound::DEFAULT;
        let most = Inbound::MOST;
        let covered = file
            .covered
            .map(|covered| (covered.span(), covered.into_inner().get()));
        if let Some((span, given)) = &covered
            && *given < Inbound::LEAST_COVERED
        {
            self.error(
                span,
                "server.inbound.covered",
                format!(
                    "an ActivityPub signature covers at least {} headers; make it at least that",
                    Inbound::LEAST_COVERED
                ),
            );
        }
        let covered = self.most(
            covered,
            defaults.covered,
            most.covered,
            "server.inbound.covered",
            &most.covered.to_string(),
        );
        Inbound {
            replay_window: self.most(
                file.replay_window
                    .map(|window| (window.span(), window.into_inner().0)),
                defaults.replay_window,
                most.replay_window,
                "server.inbound.replay_window",
                &format!("\"{}m\"", most.replay_window.as_mins()),
            ),
            signatures: self.most(
                file.signatures
                    .map(|signatures| (signatures.span(), signatures.into_inner().get())),
                defaults.signatures,
                most.signatures,
                "server.inbound.signatures",
                &most.signatures.to_string(),
            ),
            covered,
            document: self.most(
                document.map(|document| (document.span(), document.into_inner().0)),
                defaults.document,
                most.document,
                "server.keys.document",
                &Size::written(most.document),
            ),
        }
    }

    /// `[server] held`, where a connection is held at least as long as it
    /// may take to send its headers.
    fn held(&mut self, held: Option<Spanned<Duration>>, read: Option<Duration>) -> SignedDuration {
        let read = read.map_or(Server::READ, |read| read.0);
        let Some(held) = held else {
            return Server::HELD.max(read);
        };
        if held.get_ref().0 < read {
            self.error(
                &held.span(),
                "server.held",
                "a connection is held at least as long as it may take to send its headers; make held at least read",
            );
        }
        held.into_inner().0
    }

    fn reconnecting(&mut self, file: SessionsFile) -> Reconnecting {
        let first_wait = file
            .first_wait
            .map_or(Reconnecting::FIRST_WAIT, |first| first.0);
        let longest_wait = file.longest_wait.map_or_else(
            || Reconnecting::LONGEST_WAIT.max(first_wait),
            |longest| {
                if longest.get_ref().0 < first_wait {
                    self.error(
                        &longest.span(),
                        "server.sessions.longest_wait",
                        "the longest wait is at least the first; make longest_wait at least first_wait",
                    );
                }
                longest.into_inner().0
            },
        );
        Reconnecting {
            first_wait,
            longest_wait,
            jitter: file.jitter.unwrap_or(true),
        }
    }

    pub(super) fn announce(&mut self, value: Option<Value<'_>>) -> Announce {
        value
            .and_then(|value| self.typed::<AnnounceFile>(value, "announce"))
            .map_or_else(Announce::default, |file| Announce {
                lease: file.lease.map_or(Announce::LEASE, |lease| lease.0),
                latest_attempts: file
                    .latest_attempts
                    .map_or(Announce::LATEST_ATTEMPTS, NonZeroUsize::get),
            })
    }

    pub(super) fn render(&mut self, value: Option<Value<'_>>) -> Sandbox {
        value
            .and_then(|value| self.typed::<RenderFile>(value, "render"))
            .map_or(Sandbox::DEFAULT, |file| self.sandbox(file))
    }

    fn sandbox(&mut self, file: RenderFile) -> Sandbox {
        let defaults = Sandbox::DEFAULT;
        let most = Sandbox::MOST;
        Sandbox {
            fuel: self.most(
                file.fuel.map(|fuel| (fuel.span(), fuel.into_inner().get())),
                defaults.fuel,
                most.fuel,
                "render.fuel",
                &most.fuel.to_string(),
            ),
            recursion: self.most(
                file.recursion
                    .map(|recursion| (recursion.span(), recursion.into_inner().get())),
                defaults.recursion,
                most.recursion,
                "render.recursion",
                &most.recursion.to_string(),
            ),
            output: self.most(
                file.output
                    .map(|output| (output.span(), output.into_inner().0)),
                defaults.output,
                most.output,
                "render.output",
                &Size::written(most.output),
            ),
        }
    }
}
