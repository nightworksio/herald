//! Prints the JSON Schema of an app's event, which `herald-event.schema.json`
//! holds.

#[expect(clippy::print_stdout, reason = "the schema is this tool's output")]
fn main() {
    print!("{}", herald_sources_app::schema());
}
