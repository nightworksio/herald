use std::future::ready;

use herald_core::{Announcement, Chore, Delivery, Entry, Profile, Sent, Step};
use herald_kinds::{Probe, Reach, Signing};
use herald_ports::{Base, Boxed, DynHttp, DynMarkers, Erased, HttpError};

use crate::markers::MatrixMarkers;
use crate::matrix::Matrix;
use crate::request::cannot;

impl Reach for Matrix {
    fn profile(&self) -> Profile {
        Self::profile(self)
    }

    fn base(&self) -> Base<'_> {
        Base::At(Self::api(self))
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
            cannot("archive or lock a thread").to_string(),
        ))))
    }

    fn posts<'a>(&'a self, http: &'a dyn DynHttp) -> Box<dyn DynMarkers + 'a> {
        Box::new(MatrixMarkers::new(self, Erased(http)))
    }

    fn probe<'a>(&'a self, http: &'a dyn DynHttp, delivery: &'a Delivery) -> Boxed<'a, Vec<Probe>> {
        Box::pin(async move { crate::probe::probe(self, &Erased(http), delivery).await })
    }
}
