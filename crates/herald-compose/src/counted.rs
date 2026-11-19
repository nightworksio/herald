use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use herald_ports::{
    Binding, DynInfer, Http, HttpError, Infer, InferError, Inference, Request, Response,
};

use crate::host::Host;

/// An `Http` that counts each request it sends on a count it shares with
/// every other connection herald makes, so a host's limit on outbound
/// requests is kept.
#[derive(Debug, Clone)]
pub struct Counted<H> {
    http: H,
    made: Arc<AtomicUsize>,
}

impl<H> Counted<H> {
    /// `http`, counting each request on `made`.
    #[must_use]
    pub(crate) const fn new(http: H, made: Arc<AtomicUsize>) -> Self {
        Self { http, made }
    }
}

impl<H: Http> Http for Counted<H> {
    async fn send(&self, request: Request) -> Result<Response, HttpError> {
        self.made.fetch_add(1, Ordering::Relaxed);
        self.http.send(request).await
    }
}

/// What reaches each model `host` runs beside herald, by the model's name
/// and binding, each inference counted on `made` (ADR-0059).
pub(crate) fn binding<'h, H: Host>(
    host: &'h H,
    made: &'h Arc<AtomicUsize>,
) -> impl FnMut(&str, Binding<'_>) -> Result<Arc<dyn DynInfer>, String> + 'h {
    move |name, binding| bound(host.inference(binding), name, made)
}

/// `infer`, the model `name` its host runs, each inference counted on
/// `made`; or why its host runs no such model (ADR-0059).
pub(crate) fn bound(
    infer: Result<Arc<dyn DynInfer>, String>,
    name: &str,
    made: &Arc<AtomicUsize>,
) -> Result<Arc<dyn DynInfer>, String> {
    let infer = infer.map_err(|why| format!("the model {name} is not run here: {why}"))?;
    Ok(Arc::new(Counted::new(infer, Arc::clone(made))))
}

impl Infer for Counted<Arc<dyn DynInfer>> {
    async fn infer(&self, inference: Inference) -> Result<Vec<u8>, InferError> {
        self.made.fetch_add(1, Ordering::Relaxed);
        self.http.infer_boxed(inference).await
    }
}

#[cfg(test)]
mod tests {
    use herald_ports::{Method, SecretText};

    use super::*;

    struct Answering;

    impl Http for Answering {
        fn send(&self, _: Request) -> impl Future<Output = Result<Response, HttpError>> + Send {
            std::future::ready(Ok(Response {
                status: 204,
                headers: Vec::new(),
                body: Vec::new(),
            }))
        }
    }

    #[test]
    fn each_request_sent_is_counted_on_the_shared_count() {
        let made = Arc::new(AtomicUsize::new(0));
        let one = Counted::new(Answering, Arc::clone(&made));
        let other = Counted::new(Answering, Arc::clone(&made));
        let request = || Request::new(Method::Get, SecretText::plain("https://forge.example/"));
        let sent = pollster::block_on(async {
            [
                one.send(request()).await,
                other.send(request()).await,
                one.send(request()).await,
            ]
        });
        assert!(
            sent.iter()
                .all(|sent| sent.as_ref().is_ok_and(|response| response.status == 204))
        );
        assert_eq!(made.load(Ordering::Relaxed), 3);
    }

    #[derive(Debug)]
    struct Running;

    impl Infer for Running {
        fn infer(
            &self,
            inference: Inference,
        ) -> impl Future<Output = Result<Vec<u8>, InferError>> + Send {
            std::future::ready(Ok(inference.model.into_bytes()))
        }
    }

    #[test]
    fn each_inference_of_a_bound_model_is_counted_and_an_unbound_one_refused() {
        let made = Arc::new(AtomicUsize::new(1));
        let running: Arc<dyn DynInfer> = Arc::new(Running);
        let infer = bound(Ok(running), "ai", &made).unwrap();
        let asked = Inference {
            model: "m".to_owned(),
            inputs: Vec::new(),
            tokens: 1,
            most: 1,
        };
        let answered = pollster::block_on(infer.infer_boxed(asked));
        assert_eq!(answered, Ok(b"m".to_vec()));
        assert_eq!(made.load(Ordering::Relaxed), 2);
        assert_eq!(
            bound(Err("no AI".to_owned()), "ai", &made).err().as_deref(),
            Some("the model ai is not run here: no AI")
        );
    }
}
