//! The service: one request answered, then the work it left.

mod host;
mod http;
mod kv;
#[cfg(test)]
mod tests;

use fastly::http::Method;
use fastly::{Request, Response};
use std::io::Read as _;

use herald_compose::{Asked, Composed, Hosting, ROUND_SECRET, told};
use herald_config::load;
use herald_flows::{Answered, Reply, refusal};
use herald_ports::Secret;
use host::{FastlyHost, STORE};

/// The Config Store item holding `herald.toml`.
const CONFIG: &str = "herald.toml";

fn said(lines: impl IntoIterator<Item = String>) {
    for line in lines {
        eprintln!("{line}");
    }
}

/// What `request` asks, its body read no further than one byte past `most`.
fn asked(mut request: Request, most: usize) -> std::io::Result<Asked> {
    let method = request.get_method_str().to_owned();
    let target = request.get_url().path().to_owned()
        + &request
            .get_query_str()
            .map_or_else(String::new, |query| format!("?{query}"));
    let headers = request
        .get_headers()
        .filter_map(|(name, value)| {
            Some((name.as_str().to_owned(), value.to_str().ok()?.to_owned()))
        })
        .collect();
    let mut body = Vec::new();
    let limit = u64::try_from(most).unwrap_or(u64::MAX).saturating_add(1);
    request.take_body().take(limit).read_to_end(&mut body)?;
    Ok(Asked {
        method,
        target,
        headers,
        body,
    })
}

fn composed() -> Result<Composed<FastlyHost>, String> {
    let host = FastlyHost::new();
    let text = host
        .item(CONFIG)
        .ok_or_else(|| format!("{CONFIG} is not in the Config Store {STORE}"))?;
    let read = |name: &str| {
        host.item(name)
            .ok_or_else(|| format!("{name} is not in the Config Store {STORE}"))
    };
    let config = load(CONFIG, &text, &read).map_err(|errors| errors.to_string())?;
    let hosting = Hosting::Fastly;
    pollster::block_on(Composed::new(
        host,
        config,
        hosting.stores(),
        hosting.name(),
    ))
}

fn send(reply: Reply, head: bool) {
    let mut response = Response::from_status(reply.status)
        .with_content_type(reply.kind.parse().unwrap_or(fastly::mime::TEXT_PLAIN));
    if let Some(after) = reply.retry_after {
        response.set_header("retry-after", after.to_string());
    }
    if !head {
        response.set_body(reply.body);
    }
    response.send_to_client();
}

/// Sends the refusal `answered`, and logs its reason.
fn refused(answered: Answered) {
    said(answered.refused.as_deref().map(told::refused));
    send(answered.reply, false);
}

/// Answers the client's request, then does the work it leaves.
pub(crate) fn serve() {
    let request = Request::from_client();
    let is_head = request.get_method() == Method::HEAD;
    let composed = match composed() {
        Ok(composed) => composed,
        Err(error) => {
            refused(refusal(500, format!("herald could not start: {error}")));
            return;
        }
    };
    let asked = match asked(request, composed.config().server.body) {
        Ok(asked) => asked,
        Err(error) => {
            refused(refusal(400, format!("its body could not be read: {error}")));
            return;
        }
    };
    let secret: Option<Secret> =
        pollster::block_on(composed.host().secrets_for_round(ROUND_SECRET));
    let reached = pollster::block_on(composed.reached(&asked, secret.as_ref()));
    said(reached.said);
    send(reached.reply, is_head);
    if let Some(left) = reached.left {
        said(told::worked(&pollster::block_on(composed.work(left))));
    }
}
