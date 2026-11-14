//! The conformance suites run over a host's own databases and key-value
//! stores: built only with the `conformance` feature, for testing a host.

use herald_ports::Parameters;
use herald_records::{Dialect, SqlRecords};
use send_wrapper::SendWrapper;
use wasm_bindgen::prelude::*;

use super::ports::{JsKv, JsSql};

fn failed(failure: &impl ToString) -> JsError {
    JsError::new(&failure.to_string())
}

/// Runs the conformance suite every store passes over `sql`, a database
/// offering `transaction(statements)`, as the Durable Object store keeps
/// its records.
///
/// # Errors
///
/// Why the store could not open, or the first case it fails.
#[wasm_bindgen]
pub async fn conformance(sql: JsValue) -> Result<(), JsError> {
    let store = SqlRecords::kept(
        JsSql(SendWrapper::new(sql.unchecked_into())),
        Dialect::Sqlite,
        herald_ports::RecordOptions::default(),
    )
    .await
    .map_err(|error| failed(&error))?;
    herald_testing::store_conformance(&store)
        .await
        .map_err(|failure| failed(&failure))
}

/// Runs the conformance suite every database passes over `sql`, a database
/// offering `transaction(statements)` whose statements name their
/// parameters as `dialect`, `sqlite` or `postgres`, writes them.
///
/// # Errors
///
/// A dialect herald does not speak, or the first case the database fails.
#[wasm_bindgen(js_name = sqlConformance)]
pub async fn sql_conformance(sql: JsValue, dialect: String) -> Result<(), JsError> {
    let parameters = match dialect.as_str() {
        "sqlite" => Parameters::Question,
        "postgres" => Parameters::Dollar,
        other => return Err(JsError::new(&format!("herald speaks no {other} dialect"))),
    };
    herald_testing::sql_conformance(&JsSql(SendWrapper::new(sql.unchecked_into())), parameters)
        .await
        .map_err(|failure| failed(&failure))
}

/// Runs the conformance suite every key-value store passes over `kv`, a
/// store offering `get`, `setIf`, `deleteIf` and `list`.
///
/// # Errors
///
/// The first case the store fails.
#[wasm_bindgen(js_name = kvConformance)]
pub async fn kv_conformance(kv: JsValue) -> Result<(), JsError> {
    herald_testing::kv_conformance(&JsKv(SendWrapper::new(kv.unchecked_into())))
        .await
        .map_err(|failure| failed(&failure))
}
