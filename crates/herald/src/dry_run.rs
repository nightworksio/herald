//! A dry run: an `Http` that sends nothing and keeps each request, and the
//! requests it kept printed.

use std::future::Future;
use std::io::Write;
use std::sync::{Arc, Mutex, PoisonError};

use herald_core::status;
use herald_ports::{Http, HttpError, Request, Response};

use super::output::say;

/// An `Http` that keeps each request it is handed, sends none, and answers
/// each with a success, for a dry run, whose records are kept in memory.
#[derive(Clone, Default)]
pub(super) struct Dry {
    sent: Arc<Mutex<Vec<String>>>,
}

impl Dry {
    /// Each request kept, printed as the requests a first announcement
    /// sends.
    pub(super) fn print(&self, out: &mut dyn Write) {
        say(
            out,
            "A dry run sends nothing and keeps no record: these are the requests a first announcement sends.",
        );
        for request in self
            .sent
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
        {
            say(out, "");
            for line in request.lines() {
                say(out, line);
            }
        }
        say(out, "");
    }
}

impl Http for Dry {
    fn send(&self, request: Request) -> impl Future<Output = Result<Response, HttpError>> + Send {
        self.sent
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(request.with_body().to_string());
        std::future::ready(Ok(Response {
            status: status::OK,
            headers: Vec::new(),
            body: br#"{"id":"0"}"#.to_vec(),
        }))
    }
}
