//! Prints the JSON Schema of `herald.toml`, which `herald.schema.json` holds.

#[expect(clippy::print_stdout, reason = "the schema is this tool's output")]
fn main() {
    print!("{}", herald_config::schema(&herald_compose::kinds()));
}
