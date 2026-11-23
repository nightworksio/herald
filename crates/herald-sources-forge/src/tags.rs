//! A repository's version tags on each forge, listed newest first and each
//! read in full where its listing leaves its time or message out
//! (ADR-0031).

use herald_ports::Response;
use herald_wire::SourceError;

use crate::limited::answered;
use jiff::Timestamp;
use serde::Deserialize;

use crate::Forge;
use crate::wire::ForgeTag;

/// A tag as a forge's listing names it: read in full, or what reading it
/// in full asks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Listed {
    /// The listing names all a tag's event takes.
    Read(ForgeTag),
    /// A GitHub tag, whose reference names what it points at.
    Referenced(String),
    /// A Forgejo annotated tag, the tag object `object`, with its message.
    Annotated {
        name: String,
        object: String,
        message: Option<String>,
    },
}

impl Listed {
    /// The tag's name.
    pub(crate) fn name(&self) -> &str {
        match self {
            Self::Read(tag) => &tag.name,
            Self::Referenced(name) | Self::Annotated { name, .. } => name,
        }
    }
}

/// What a tag reference points at, as GitHub names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Pointed {
    /// An annotated tag object.
    Tag(String),
    /// A commit, for a lightweight tag.
    Commit(String),
}

/// A tag, as GitHub lists it.
#[derive(Deserialize)]
struct GitHubTag {
    name: String,
}

/// A tag, as GitLab lists it.
#[derive(Deserialize)]
struct GitLabTag {
    name: String,
    message: Option<String>,
    created_at: Option<Timestamp>,
    commit: Option<GitLabCommit>,
}

#[derive(Deserialize)]
struct GitLabCommit {
    committed_date: Option<Timestamp>,
}

/// A tag, as Forgejo and Gitea list it.
#[derive(Deserialize)]
struct ForgejoTag {
    name: String,
    message: Option<String>,
    id: String,
    commit: ForgejoCommit,
}

#[derive(Deserialize)]
struct ForgejoCommit {
    sha: String,
    created: Option<Timestamp>,
}

/// A reference, as GitHub names it.
#[derive(Deserialize)]
struct Reference {
    object: Object,
}

#[derive(Deserialize)]
struct Object {
    #[serde(rename = "type")]
    kind: String,
    sha: String,
}

/// An annotated tag object, as GitHub and Forgejo name it.
#[derive(Deserialize)]
struct TagObject {
    tag: String,
    message: Option<String>,
    tagger: Option<Signature>,
}

/// A commit, as GitHub's git database names it.
#[derive(Deserialize)]
struct Commit {
    committer: Option<Signature>,
}

#[derive(Deserialize)]
struct Signature {
    date: Option<Timestamp>,
}

/// `message` where it holds more than white space.
fn worded(message: Option<String>) -> Option<String> {
    message.filter(|message| !message.trim().is_empty())
}

impl From<GitLabTag> for Listed {
    fn from(tag: GitLabTag) -> Self {
        Self::Read(ForgeTag {
            date: tag
                .created_at
                .or_else(|| tag.commit.and_then(|commit| commit.committed_date)),
            message: worded(tag.message),
            name: tag.name,
        })
    }
}

impl From<ForgejoTag> for Listed {
    fn from(tag: ForgejoTag) -> Self {
        if tag.id == tag.commit.sha {
            Self::Read(ForgeTag {
                name: tag.name,
                message: None,
                date: tag.commit.created,
            })
        } else {
            Self::Annotated {
                name: tag.name,
                object: tag.id,
                message: worded(tag.message),
            }
        }
    }
}

impl Forge {
    /// The tag `name` as the forge's answer to [`crate::Origin::tag_request`]
    /// names it; on GitHub, whose tag is read through its reference, the
    /// answer is not read.
    pub(crate) fn tag(self, name: &str, response: &Response) -> Result<Listed, SourceError> {
        let what = self.api().tag;
        match self {
            Self::GitHub => Ok(Listed::Referenced(name.to_owned())),
            Self::GitLab => {
                answered(self, what, response)?;
                crate::wire::read::<GitLabTag>(what, &response.body).map(Listed::from)
            }
            Self::Forgejo => {
                answered(self, what, response)?;
                crate::wire::read::<ForgejoTag>(what, &response.body).map(Listed::from)
            }
        }
    }

    /// The tags the forge's answer to [`crate::Origin::tags_request`] lists,
    /// newest first as the forge orders them, parsing no more than `limit`
    /// bytes.
    pub(crate) fn tags(
        self,
        response: &Response,
        limit: usize,
    ) -> Result<Vec<Listed>, SourceError> {
        let what = self.api().tags;
        answered(self, what, response)?;
        let body = &response.body;
        Ok(match self {
            Self::GitHub => herald_wire::read::<Vec<GitHubTag>>(what, limit, body)?
                .into_iter()
                .map(|tag| Listed::Referenced(tag.name))
                .collect(),
            Self::GitLab => herald_wire::read::<Vec<GitLabTag>>(what, limit, body)?
                .into_iter()
                .map(Listed::from)
                .collect(),
            Self::Forgejo => herald_wire::read::<Vec<ForgejoTag>>(what, limit, body)?
                .into_iter()
                .map(Listed::from)
                .collect(),
        })
    }

    /// What the tag reference in GitHub's answer to
    /// [`crate::Origin::reference_request`] points at.
    pub(crate) fn pointed(self, response: &Response) -> Result<Pointed, SourceError> {
        let what = self.api().tag;
        answered(self, what, response)?;
        let reference: Reference = crate::wire::read(what, &response.body)?;
        match reference.object.kind.as_str() {
            "tag" => Ok(Pointed::Tag(reference.object.sha)),
            "commit" => Ok(Pointed::Commit(reference.object.sha)),
            other => Err(SourceError::Unreadable {
                what,
                reason: format!("the tag points at a {other}, not a commit or a tag object"),
            }),
        }
    }

    /// The annotated tag in the forge's answer to
    /// [`crate::Origin::tag_object_request`], with `message` where the
    /// object holds none.
    pub(crate) fn tag_object(
        self,
        response: &Response,
        message: Option<String>,
    ) -> Result<ForgeTag, SourceError> {
        let what = self.api().tag;
        answered(self, what, response)?;
        let object: TagObject = crate::wire::read(what, &response.body)?;
        Ok(ForgeTag {
            name: object.tag,
            message: worded(object.message).or(message),
            date: object.tagger.and_then(|tagger| tagger.date),
        })
    }

    /// The lightweight tag `name`, at the commit in GitHub's answer to
    /// [`crate::Origin::commit_request`].
    pub(crate) fn committed(
        self,
        response: &Response,
        name: &str,
    ) -> Result<ForgeTag, SourceError> {
        let what = self.api().tag;
        answered(self, what, response)?;
        let commit: Commit = crate::wire::read(what, &response.body)?;
        Ok(ForgeTag {
            name: name.to_owned(),
            message: None,
            date: commit.committer.and_then(|committer| committer.date),
        })
    }

    /// The page of the tag `tag` of the repository whose page is `page`:
    /// on GitHub and Forgejo the page its Release has once there is one.
    pub(crate) fn tag_page(self, page: &str, tag: &str) -> String {
        let page = page.trim_end_matches('/');
        let tag = crate::forge::segments(tag);
        match self {
            Self::GitHub | Self::Forgejo => format!("{page}/releases/tag/{tag}"),
            Self::GitLab => format!("{page}/-/tags/{tag}"),
        }
    }
}

#[cfg(test)]
mod tests;
