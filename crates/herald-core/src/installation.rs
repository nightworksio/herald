//! Which installation of a GitHub App a token is minted for (ADR-0041).

/// Which installation of a GitHub App a token is minted for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Installation<'a> {
    /// The installation on the owner at this path, looked up.
    Owner(&'a str),
    /// The installation on the repository at this full path, looked up.
    Repository(&'a str),
    /// The installation of this id.
    Fixed(u64),
}
