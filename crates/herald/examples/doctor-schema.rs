//! Prints the JSON Schema of what `herald doctor --format json` prints,
//! which `herald-doctor.schema.json` holds.

fn main() {
    print!("{}", herald::doctor_schema());
}
