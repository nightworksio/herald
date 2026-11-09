use std::any::Any;
use std::future::ready;

use herald_core::{Announcement, Chore, Delivery, Entry, Profile, Sent, Step};
use herald_kinds::{Reach, Signing, same};
use herald_ports::{Boxed, DynHttp, DynMarkers, Erased, HttpError};

use crate::markers::MatrixMarkers;
use crate::matrix::Matrix;
use crate::request::Refusal;

impl Reach for Matrix {
    fn profile(&self) -> Profile {
        Self::profile(self)
    }

    fn api(&self) -> &str {
        Self::api(self)
    }

    fn send<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        _: Signing<'a>,
        step: &'a Step,
        announcement: &'a Announcement,
        _: &'a Delivery,
        entry: &'a Entry,
    ) -> Boxed<'a, Result<Sent, HttpError>> {
        Box::pin(async move { Self::send(self, &Erased(http), step, announcement, entry).await })
    }

    fn housekeep<'a>(
        &'a self,
        _: &'a dyn DynHttp,
        _: Chore,
        _: &'a str,
    ) -> Boxed<'a, Result<Sent, HttpError>> {
        Box::pin(ready(Ok(Sent::Refused(
            Refusal::Cannot("archive or lock a thread").to_string(),
        ))))
    }

    fn posts<'a>(&'a self, http: &'a dyn DynHttp) -> Box<dyn DynMarkers + 'a> {
        Box::new(MatrixMarkers::new(self, Erased(http)))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn same(&self, other: &dyn Reach) -> bool {
        same(self, other)
    }
}
