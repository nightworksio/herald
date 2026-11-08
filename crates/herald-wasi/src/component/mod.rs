//! The component: what a request asks of herald, and the ports the host gives.

#[cfg(feature = "conformance")]
mod conformance;
mod handler;
mod host;
mod http;
mod served;
#[cfg(feature = "spin")]
mod spin;
#[cfg(test)]
mod tests;

use wasip2::exports::http::incoming_handler::{Guest, IncomingRequest, ResponseOutparam};

struct Component;

impl Guest for Component {
    fn handle(request: IncomingRequest, out: ResponseOutparam) {
        wstd::runtime::block_on(handler::handled(request, out));
    }
}

wasip2::http::proxy::export!(Component);
