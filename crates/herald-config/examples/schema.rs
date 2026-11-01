//! Prints the JSON Schema of `herald.toml`, which `herald.schema.json` holds.

fn main() {
    print!("{}", herald_config::schema());
}
