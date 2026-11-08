//! Prints the JSON Schema of an app's event, which `herald-event.schema.json`
//! holds.

fn main() {
    print!("{}", herald_sources_app::schema());
}
