//! herald as one wasm module behind a JavaScript host (ADR-0017): a
//! Cloudflare Worker, Vercel, Netlify, Deno, Node or Bun gives it `fetch`,
//! its secrets and variables, and the databases and key-value stores it
//! offers, and a thin shim in `deploy/` hands it each request and scheduled
//! round. It signs and verifies with the host's Web Crypto.
//!
//! What decides is plain Rust, tested natively; what speaks to JavaScript
//! is built for `wasm32` alone, and tested under Node.

pub mod keys;
pub mod numbers;
pub mod reaching;

#[cfg(target_arch = "wasm32")]
mod js;

#[cfg(target_arch = "wasm32")]
pub use js::{Herald, HostObject};
