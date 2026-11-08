//! The service: one request answered, then the work it left.

mod host;
mod http;
mod kv;
#[cfg(test)]
mod tests;

use fastly::http::Method;
use fastly::{Request, Response};
use std::io::Read as _;

use herald_compose::{Asked, Composed, Hosting, told};
use herald_config::{Config, load};
use herald_flows::{Answered, Reply, refusal, uncomposed};
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

/// The service's host and the config its Config Store holds.
fn configured() -> Result<(FastlyHost, Config), String> {
    let host = FastlyHost::new();
    let text = host
        .item(CONFIG)
        .ok_or_else(|| format!("{CONFIG} is not in the Config Store {STORE}"))?;
    let read = |name: &str| {
        host.item(name)
            .ok_or_else(|| format!("{name} is not in the Config Store {STORE}"))
    };
    let config = load(CONFIG, &text, &read).map_err(|errors| errors.to_string())?;
    Ok((host, config))
}

fn composed(host: FastlyHost, config: Config) -> Result<Composed<FastlyHost>, String> {
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
    let started = configured().and_then(|(host, config)| {
        let target = request.get_url().path();
        match uncomposed(request.get_method_str(), target) {
            Some(reply) => Ok(Err(reply)),
            None => composed(host, config).map(Ok),
        }
    });
    let composed = match started {
        Ok(Ok(composed)) => composed,
        Ok(Err(reply)) => {
            send(reply, is_head);
            return;
        }
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
    let answer = pollster::block_on(composed.reached(&asked));
    said(answer.refused.as_deref().map(told::refused));
    said(answer.said);
    send(answer.reply, is_head);
    if let Some(left) = answer.left {
        said(told::worked(&pollster::block_on(composed.work(left))));
    }
}
