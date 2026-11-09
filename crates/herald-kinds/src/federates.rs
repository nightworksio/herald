use herald_core::Sent;
use herald_ports::{Boxed, CryptoError, DynHttp, DynSigner, HttpError};
use jiff::Timestamp;

/// What an activity delivered to an inbox herald serves asks of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Activity {
    /// A follow of the actor `target`, by the activity's signer, with the
    /// follow's own id.
    Follow {
        /// The actor followed.
        target: String,
        /// The follow's id.
        id: String,
    },
    /// An undone follow of the actor `target` by `follower`.
    Undo {
        /// Who stops following.
        follower: String,
        /// The actor they stop following.
        target: String,
    },
    /// Nothing herald takes, and why.
    Nothing(String),
}

/// Where a follower takes what is delivered to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inboxes {
    /// Its own inbox, where an answer to its follow goes.
    pub personal: String,
    /// The inbox announcements are delivered to: its server's shared one,
    /// where it names one, else its own.
    pub delivered: String,
}

/// A destination herald serves as one of its own actors rather than posting
/// to: what it publishes, whom it lets follow, and how it answers them.
pub trait Federates: Send + Sync {
    /// The actor's name.
    fn actor(&self) -> &str;

    /// The domain its handle is on.
    fn domain(&self) -> &str;

    /// The actor's id.
    fn id(&self) -> String;

    /// Its WebFinger document.
    fn finger(&self) -> String;

    /// Its actor document, with the public half of its key read through
    /// `signer`.
    fn document<'a>(&'a self, signer: &'a dyn DynSigner) -> Boxed<'a, Result<String, CryptoError>>;

    /// Whether `note`, kept as the announcement `key`, is its own.
    fn owns(&self, note: &str, key: &str) -> bool;

    /// The most notes a page of its outbox lists.
    fn outbox_most(&self) -> usize;

    /// The name of its outbox's first page.
    fn first_page(&self) -> &'static str;

    /// Its outbox, holding `total` notes.
    fn outbox(&self, total: usize) -> String;

    /// The page `page` of its outbox, listing `notes`, before `next`.
    fn outbox_page(&self, page: &str, notes: &[String], next: Option<&str>) -> String;

    /// Its followers collection, counting `admitted` and never naming one.
    fn followers(&self, admitted: usize) -> String;

    /// Whether `follower` may follow it.
    fn admits(&self, follower: &str) -> bool;

    /// Whether there is room for `follower` beside `following`.
    fn room(&self, follower: &str, following: &[&str]) -> bool;

    /// Whether each follow waits for the operator.
    fn manual(&self) -> bool;

    /// What `body`, signed by `signer`, asks of it.
    fn read(&self, body: &[u8], signer: &str) -> Activity;

    /// Where `follower`, whose actor document is `document`, takes what is
    /// delivered to it.
    fn inboxes(&self, document: &[u8], follower: &str) -> Option<Inboxes>;

    /// The answer accepting, or else rejecting, the follow `id` by
    /// `follower`.
    fn answer(&self, id: &str, follower: &str, accept: bool) -> String;

    /// Posts `answer` to `inbox` through `http`, signed at `now` through
    /// `signer`, and reads the inbox's answer.
    fn post<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        signer: &'a dyn DynSigner,
        now: Timestamp,
        inbox: &'a str,
        answer: &'a str,
    ) -> Boxed<'a, Result<Sent, HttpError>>;
}
