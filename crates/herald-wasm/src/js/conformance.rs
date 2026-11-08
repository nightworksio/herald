//! The store conformance suite run over a host's own SQL database, as the
//! Durable Object store keeps its records: built only with the
//! `conformance` feature, for testing a host.

use send_wrapper::SendWrapper;
use wasm_bindgen::prelude::*;

use super::ports::JsSql;

/// Runs the conformance suite every store passes over `sql`, a database
/// offering `transaction(statements)`, as the Durable Object store keeps
/// its records.
///
/// # Errors
///
/// Why the store could not open, or the first case it fails.
#[wasm_bindgen]
pub async fn conformance(sql: JsValue) -> Result<(), JsError> {
    let store = herald_store_durable_object::store(JsSql(SendWrapper::new(sql.unchecked_into())))
        .await
        .map_err(|error| JsError::new(&error.to_string()))?;
    herald_testing::store_conformance(&store)
        .await
        .map_err(|failure| JsError::new(&failure.to_string()))
}
