//! The files a config names, as the host reads them, and where they may lie:
//! beside the config, or in a directory `[files] allowed` names (ADR-0092
//! decision 8).

use std::path::{Component, Path};

/// The directory a file the config names must lie in, every link followed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Within<'a> {
    /// The config's own directory.
    Beside,
    /// A directory `[files] allowed` names, absolute.
    Allowed(&'a str),
}

impl Within<'_> {
    /// Where `path`, as the config names it, may be read from: beside the
    /// config where it is a path of names, or the directory of `allowed`
    /// that holds it where it is absolute; none where it steps up with `..`
    /// or lies in no such directory.
    #[must_use]
    pub fn of<'a>(path: &str, allowed: &'a [String]) -> Option<Within<'a>> {
        let named = Path::new(path);
        if named.components().any(|step| step == Component::ParentDir) {
            return None;
        }
        if named.is_absolute() {
            return allowed
                .iter()
                .find(|directory| named.starts_with(directory.as_str()))
                .map(|directory| Within::Allowed(directory.as_str()));
        }
        let mut steps = named.components().peekable();
        (steps.peek().is_some()
            && steps.all(|step| matches!(step, Component::Normal(_) | Component::CurDir)))
        .then_some(Within::Beside)
    }
}

/// Why a host read no file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unread {
    /// Its links lead outside the directory it must lie in.
    Outside,
    /// It could not be read, and why.
    Failed(String),
}

/// How a host reads the files a config names: a template's, a member
/// list's, a block list's and a CA file through `read`, and a local preset
/// through `read_inside`.
pub trait Files {
    /// The text of the file at `path`, as the config names it, where it is
    /// a file inside `within`.
    ///
    /// # Errors
    ///
    /// Where its links lead outside `within`, or why it could not be read.
    fn read(&self, path: &str, within: Within<'_>) -> Result<String, Unread>;

    /// The text of the file at `path` where, every link followed, it is a
    /// file inside the config's directory (ADR-0092 decision 4).
    ///
    /// # Errors
    ///
    /// Why the file could not be read, or where it leads outside the
    /// config's directory.
    fn read_inside(&self, path: &str) -> Result<String, String>;
}

/// How the config `config` reads each file it names: through `files`
/// where it lies beside the config or in a directory of `allowed`, else
/// refused, every link followed, saying why and never a word of what it
/// holds.
pub(crate) fn contained<'a>(
    files: &'a dyn Files,
    config: &'a str,
    allowed: Vec<String>,
) -> impl Fn(&str) -> Result<String, String> + 'a {
    move |path| {
        let outside = || {
            format!(
                "it lies outside the directory of {config} and every directory [files] allowed names; move it beside {config}, or name its directory in [files] allowed"
            )
        };
        let within = Within::of(path, &allowed).ok_or_else(outside)?;
        files.read(path, within).map_err(|unread| match unread {
            Unread::Outside => outside(),
            Unread::Failed(why) => why,
        })
    }
}

/// What a host that reads files through one function says of a local
/// preset, whose links it cannot follow.
pub const UNCONTAINED: &str = "this host cannot tell where a file's links lead, so it reads no local preset; run herald's native binary, or write the preset's tables into herald.toml";

impl<F: Fn(&str) -> Result<String, String>> Files for F {
    fn read(&self, path: &str, _: Within<'_>) -> Result<String, Unread> {
        self(path).map_err(Unread::Failed)
    }

    fn read_inside(&self, _: &str) -> Result<String, String> {
        Err(UNCONTAINED.to_owned())
    }
}

#[cfg(test)]
mod tests;
