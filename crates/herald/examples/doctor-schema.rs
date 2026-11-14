//! Prints the JSON Schema of what `herald doctor --format json` prints,
//! which `herald-doctor.schema.json` holds.

#[expect(clippy::print_stdout, reason = "the schema is this tool's output")]
fn main() {
    print!("{}", herald::doctor_schema());
}
