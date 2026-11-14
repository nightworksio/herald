//! What herald says of itself: its version and where it lives.

/// herald's version, as a literal `concat!` takes.
#[macro_export]
macro_rules! version {
    () => {
        env!("CARGO_PKG_VERSION")
    };
}

/// herald's repository, as a literal `concat!` takes.
#[macro_export]
macro_rules! repository {
    () => {
        "https://github.com/nightworksio/herald"
    };
}

/// herald's version.
pub const VERSION: &str = version!();
