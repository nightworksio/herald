//! The files a config names, as the host reads them, and where they may lie:
//! beside the config, or in a directory `[files] allowed` names inside what
//! the host lets a config reach (ADR-0092 decision 8).

use std::path::{Component, Path};

/// The variable listing the directories, each absolute and joined as the
/// host's `PATH` joins its own, that a config's `[files] allowed` may name
/// on this host.
pub const FILES_ALLOWED: &str = "HERALD_FILES_ALLOWED";

/// What a host lets a config's `[files] allowed` name: the directories,
/// every link followed, a directory it lists must lie inside, and where the
/// host took them from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reach {
    /// The directories [`FILES_ALLOWED`] lists.
    Listed(Vec<String>),
    /// The config's own directory alone, where [`FILES_ALLOWED`] is not set.
    Beside(String),
    /// None: the host reads files by name, not by path.
    Named,
}

impl Reach {
    /// Whether `directory`, every link followed, lies inside a directory of
    /// the reach, step by step: `/workspace` is not inside `/work`.
    #[must_use]
    pub fn holds(&self, directory: &str) -> bool {
        let directory = Path::new(directory);
        let inside = |bound: &String| {
            let bound = Path::new(bound);
            bound.is_absolute() && directory.starts_with(bound)
        };
        match self {
            Self::Listed(listed) => listed.iter().any(inside),
            Self::Beside(own) => inside(own),
            Self::Named => false,
        }
    }

    /// Why a directory that does not lie inside the reach is refused,
    /// naming no directory.
    #[must_use]
    pub fn refusal(&self) -> String {
        match self {
            Self::Listed(_) => {
                format!("lies inside no directory {FILES_ALLOWED} lists on this host")
            }
            Self::Beside(_) => format!(
                "lies outside the config's own directory, and {FILES_ALLOWED}, which lists more, is not set on this host"
            ),
            Self::Named => "cannot be reached: this host reads files by name".to_owned(),
        }
    }

    /// What the reach is and where the host took it from.
    #[must_use]
    pub fn said(&self) -> String {
        match self {
            Self::Listed(listed) => format!(
                "[files] allowed may name directories inside those {FILES_ALLOWED} lists on this host: {}",
                listed.join(", ")
            ),
            Self::Beside(own) => format!(
                "[files] allowed may name directories inside the config's own, {own}, alone: {FILES_ALLOWED} is not set on this host"
            ),
            Self::Named => {
                "[files] allowed may name no directory: this host reads files by name".to_owned()
            }
        }
    }
}

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

    /// What this host lets `[files] allowed` name.
    fn reach(&self) -> Reach {
        Reach::Named
    }

    /// The directory at the path given, every link followed, where it is
    /// one.
    fn resolved(&self, _directory: &str) -> Option<String> {
        None
    }
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
