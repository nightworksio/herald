//! Each record store over its backend, through the conformance suite every store passes.

#[cfg(test)]
mod kept;
#[cfg(test)]
mod kv;
#[cfg(test)]
mod memory;
#[cfg(test)]
mod sql;
