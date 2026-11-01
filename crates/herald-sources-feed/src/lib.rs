//! herald's feed source (ADR-0003): each entry of an RSS 2.0 or Atom 1.0
//! feed becomes an [`Event`](herald_core::Event), parsed without I/O, its
//! HTML converted to Markdown.
//!
//! A feed is polled: each poll is described as a request the flow sends,
//! and the answer it hands back is read into the newest entries.

mod entity;
mod feed;
mod html;
mod options;

pub use feed::Feed;
pub use html::markdown;
pub use options::FeedOptions;
