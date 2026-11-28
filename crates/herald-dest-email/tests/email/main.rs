//! The email message as written: the addresses herald takes, every field and
//! part read back by a strict reader, the encodings at their limits, and
//! every attack on a field through the text an announcement carries; the
//! destination's table and routes as the config reads them, and each message
//! sent through Amazon SES and Mailgun.

#[cfg(test)]
mod address;
#[cfg(test)]
mod config;
#[cfg(test)]
mod injection;
#[cfg(test)]
mod judge;
#[cfg(test)]
mod mailgun;
#[cfg(test)]
mod message;
#[cfg(test)]
mod property;
#[cfg(test)]
mod read;
#[cfg(test)]
mod ses;
