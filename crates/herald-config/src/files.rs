//! The files a config names beside itself, as the host reads them.

/// How a host reads the files a config names: a template's or a member
/// list's file through `read`, and a local preset through `read_inside`.
pub trait Files {
    /// The text of the file at `path`, as the config names it.
    ///
    /// # Errors
    ///
    /// Why the file could not be read.
    fn read(&self, path: &str) -> Result<String, String>;

    /// The text of the file at `path` where, every link followed, it is a
    /// file inside the config's directory (ADR-0092 decision 4).
    ///
    /// # Errors
    ///
    /// Why the file could not be read, or where it leads outside the
    /// config's directory.
    fn read_inside(&self, path: &str) -> Result<String, String>;
}

/// What a host that reads files through one function says of a local
/// preset, whose links it cannot follow.
pub const UNCONTAINED: &str = "this host cannot tell where a file's links lead, so it reads no local preset; run herald's native binary, or write the preset's tables into herald.toml";

impl<F: Fn(&str) -> Result<String, String>> Files for F {
    fn read(&self, path: &str) -> Result<String, String> {
        self(path)
    }

    fn read_inside(&self, _: &str) -> Result<String, String> {
        Err(UNCONTAINED.to_owned())
    }
}
