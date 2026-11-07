//! herald on Fastly Compute (ADR-0017), on Fastly's own ABI. It compiles for
//! `wasm32-wasip1` alone. Its config and template files are items of the
//! Config Store `herald`, and its secrets items of the Secret Store `herald`.

#[cfg(all(target_arch = "wasm32", target_os = "wasi"))]
mod compute;

#[cfg(all(target_arch = "wasm32", target_os = "wasi"))]
fn main() {
    compute::serve();
}

#[cfg(not(all(target_arch = "wasm32", target_os = "wasi")))]
fn main() {}
