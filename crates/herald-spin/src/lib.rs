//! Bindings to Spin's `sqlite` and `postgres` interfaces. They are generated,
//! and generated bindings call the component ABI through `unsafe`, so they sit
//! apart from the rest of herald, which forbids it. They carry no logic.

#[cfg(all(target_arch = "wasm32", target_os = "wasi"))]
#[allow(clippy::all, clippy::pedantic)]
mod bindings {
    wit_bindgen::generate!({
        world: "stores",
        path: "wit",
        generate_all,
    });
}

#[cfg(all(target_arch = "wasm32", target_os = "wasi"))]
pub use bindings::spin::postgres::postgres;
#[cfg(all(target_arch = "wasm32", target_os = "wasi"))]
pub use bindings::spin::sqlite::sqlite;
