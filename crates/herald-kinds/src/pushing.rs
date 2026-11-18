use herald_core::Waits;
use herald_ports::Secret;

/// Where a destination that pushes to a peer pushes, as its options name
/// it: the peer of `[peers]` and the herald source of the peer's taking
/// the pushes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pushing<'a> {
    /// The peer of `[peers]`.
    pub peer: &'a str,
    /// The peer's source whose inbox takes the pushes.
    pub inbox: &'a str,
}

/// The peer a destination pushes to, as `[peers]` has it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Peered {
    /// Where the peer is served: `https://`, its domain and the path its
    /// endpoints are under.
    pub url: String,
    /// Whether private events may go to it.
    pub private: bool,
}

/// A route's push to a peer, as the flows push through it: where the
/// envelopes go, the most bytes one takes, whether private events may go,
/// how long it waits, and the instance's key that proves each envelope and
/// signs each request.
#[derive(Debug, Clone, Copy)]
pub struct Pushed<'a> {
    /// The peer of `[peers]`.
    pub peer: &'a str,
    /// The URL of the peer's inbox.
    pub inbox: &'a str,
    /// The most bytes one envelope takes, its proof counted.
    pub envelope: usize,
    /// Whether private events may go to the peer.
    pub private: bool,
    /// How long it waits where the peer's inbox names no wait.
    pub waits: Waits,
    /// The instance's newest key.
    pub key: &'a Secret,
}
