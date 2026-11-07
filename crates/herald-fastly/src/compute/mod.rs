//! The service: one request answered, then the work it left.

mod host;
mod http;
mod kv;
#[cfg(test)]
mod tests;

use fastly::http::Method;
use fastly::{Request, Response};
use herald_compose::{Asked, Composed, Hosting, ROUND_SECRET, plain, told};
use herald_config::load;
use herald_ports::Secret;
use host::{FastlyHost, STORE};

/// The Config Store item holding `herald.toml`.
const CONFIG: &str = "herald.toml";
/// The most of a request's body that is read.
const MOST: usize = 1 << 20;

fn said(lines: impl IntoIterator<Item = String>) {
    for line in lines {
        eprintln!("{line}");
    }
}

fn asked(mut request: Request) -> Option<Asked> {
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
    let body = request.take_body_bytes();
    (body.len() <= MOST).then_some(Asked {
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

fn send(status: u16, kind: &str, body: Vec<u8>) {
    Response::from_status(status)
        .with_content_type(kind.parse().unwrap_or(fastly::mime::TEXT_PLAIN))
        .with_body(body)
        .send_to_client();
}

/// Answers the client's request, then does the work it leaves.
pub(crate) fn serve() {
    let request = Request::from_client();
    let is_head = request.get_method() == Method::HEAD;
    let Some(asked) = asked(request) else {
        let reply = plain(413, "the body is too large");
        send(reply.status, reply.kind, reply.body);
        return;
    };
    let composed = match composed() {
        Ok(composed) => composed,
        Err(error) => {
            eprintln!("herald could not start: {error}");
            let reply = plain(500, "herald could not start");
            send(reply.status, reply.kind, reply.body);
            return;
        }
    };
    let secret: Option<Secret> =
        pollster::block_on(composed.host().secrets_for_round(ROUND_SECRET));
    let reached = pollster::block_on(composed.reached(&asked, secret.as_ref()));
    said(reached.said);
    let body = if is_head {
        Vec::new()
    } else {
        reached.reply.body
    };
    send(reached.reply.status, reached.reply.kind, body);
    if let Some(job) = reached.job {
        said(told::worked(&pollster::block_on(composed.work(job))));
    }
}
