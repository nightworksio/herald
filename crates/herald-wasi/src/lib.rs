//! herald as a `wasi:http` component (ADR-0017): Spin, `wasmtime serve`,
//! wasmCloud and any other `wasi:http` host. It compiles for `wasm32-wasip2`
//! alone, and reads the rest of herald through `herald-compose`.

#[cfg(all(target_arch = "wasm32", target_os = "wasi"))]
mod component;
