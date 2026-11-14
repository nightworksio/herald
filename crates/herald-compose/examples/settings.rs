//! Prints the settings reference `.docs/settings.md` holds.

#[expect(clippy::print_stdout, reason = "the reference is this tool's output")]
fn main() {
    print!(
        "{}",
        herald_config::reference(&herald_config::schema(&herald_compose::kinds()))
    );
}
