//! `herald health`: whether a `herald server` answers its health, as a
//! container's health check asks it.

use std::io::Write;
use std::time::Duration;

use herald_config::Server;
use herald_core::{Address, status};
use herald_flows::HEALTH;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use super::{STOPPED, say};

/// The most of an answer read.
const MOST: u64 = 16_384;

/// The URL `listen` is reached at from this machine: an address that listens
/// everywhere is asked on loopback.
pub(crate) fn url_of(listen: &str) -> String {
    let local = if let Some(port) = listen.strip_prefix("0.0.0.0:") {
        format!("127.0.0.1:{port}")
    } else if let Some(port) = listen.strip_prefix("[::]:") {
        format!("[::1]:{port}")
    } else {
        listen.to_owned()
    };
    format!("http://{local}{HEALTH}")
}

/// The URL to ask: `asked` where given, and otherwise `/healthz` on the
/// address `listen` names where it names one, the config's, or the server's
/// own default.
pub(crate) fn url(
    asked: Option<String>,
    listen: Option<&str>,
    configured: impl FnOnce() -> Option<String>,
) -> String {
    asked
        .or_else(|| listen.filter(|listen| !listen.is_empty()).map(url_of))
        .or_else(|| configured().map(|listen| url_of(&listen)))
        .unwrap_or_else(|| url_of(Server::LISTEN))
}

/// Where a plain HTTP `url` is reached, the `Host` it is asked as, and the
/// target it asks for.
fn parts(url: &str) -> Option<(String, String, String)> {
    let address = Address::parse(url).filter(|address| address.scheme() == "http")?;
    let reached = format!("{}:{}", address.host(), address.port()?);
    Some((reached, address.authority(), address.target().to_owned()))
}

/// Why `url` did not answer healthy, or nothing where it did.
async fn asked(url: &str, within: Duration) -> Result<(), String> {
    let (reached, authority, path) =
        parts(url).ok_or_else(|| format!("{url} is not an http:// URL naming a host"))?;
    let exchange = async {
        let mut stream = TcpStream::connect(reached).await?;
        let request = format!(
            "GET {path} HTTP/1.1\r\nHost: {authority}\r\nUser-Agent: herald\r\nConnection: close\r\n\r\n"
        );
        stream.write_all(request.as_bytes()).await?;
        let mut answer = Vec::new();
        stream.take(MOST).read_to_end(&mut answer).await?;
        Ok::<_, std::io::Error>(answer)
    };
    let answer = tokio::time::timeout(within, exchange)
        .await
        .map_err(|_| format!("no answer within {} ms", within.as_millis()))?
        .map_err(|error| error.to_string())?;
    let line = String::from_utf8_lossy(&answer);
    let first = line.lines().next().unwrap_or_default();
    let healthy = first.split(' ').nth(1).and_then(|code| code.parse().ok()) == Some(status::OK);
    if healthy {
        Ok(())
    } else if first.is_empty() {
        Err("it closed without answering".to_owned())
    } else {
        Err(format!("it answered {first}"))
    }
}

/// Whether the server at `url` answers healthy `within`: 0 where it does,
/// and [`STOPPED`] where it does not, saying why.
pub(crate) async fn health(
    url: &str,
    within: Duration,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    match asked(url, within).await {
        Ok(()) => {
            say(out, &format!("herald answers at {url}"));
            0
        }
        Err(why) => {
            say(err, &format!("herald does not answer at {url}: {why}"));
            STOPPED
        }
    }
}

#[cfg(test)]
mod tests {
    use tokio::net::TcpListener;

    use super::*;

    const SOON: Duration = Duration::from_secs(5);

    fn on_tokio<T>(future: impl Future<Output = T>) -> T {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(future)
    }

    /// A server on loopback answering one request with `answer`, and the
    /// address it listens on.
    async fn answering(answer: &'static [u8]) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap().to_string();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut asked = [0; 1024];
            let _ = stream.read(&mut asked).await;
            let _ = stream.write_all(answer).await;
        });
        address
    }

    fn checked(url: &str) -> (u8, String, String) {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = on_tokio(health(url, SOON, &mut out, &mut err));
        (
            code,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    #[test]
    fn an_address_listening_everywhere_is_asked_on_loopback() {
        assert_eq!(url_of("0.0.0.0:8080"), "http://127.0.0.1:8080/healthz");
        assert_eq!(url_of("[::]:9000"), "http://[::1]:9000/healthz");
        assert_eq!(url_of("127.0.0.1:8080"), "http://127.0.0.1:8080/healthz");
        assert_eq!(url_of("herald:8080"), "http://herald:8080/healthz");
    }

    #[test]
    fn the_url_asked_comes_first_then_the_listen_variable_then_the_config() {
        let configured = || Some("10.0.0.1:7000".to_owned());
        assert_eq!(
            url(
                Some("http://a/b".to_owned()),
                Some("0.0.0.0:9000"),
                configured
            ),
            "http://a/b"
        );
        assert_eq!(
            url(None, Some("0.0.0.0:9000"), configured),
            "http://127.0.0.1:9000/healthz"
        );
        assert_eq!(
            url(None, Some(""), configured),
            "http://10.0.0.1:7000/healthz"
        );
        assert_eq!(url(None, None, configured), "http://10.0.0.1:7000/healthz");
        assert_eq!(url(None, None, || None), "http://127.0.0.1:8080/healthz");
    }

    #[test]
    fn a_url_is_read_as_its_authority_and_path() {
        let parted = |reached: &str, authority: &str, target: &str| {
            Some((reached.to_owned(), authority.to_owned(), target.to_owned()))
        };
        assert_eq!(parts("http://a:1/x/y?z"), parted("a:1", "a:1", "/x/y?z"));
        assert_eq!(parts("http://a:1"), parted("a:1", "a:1", "/"));
        assert_eq!(parts("http://u@a/x"), parted("a:80", "a", "/x"));
        assert_eq!(
            parts("http://[::1]:8080/"),
            parted("[::1]:8080", "[::1]:8080", "/")
        );
        assert_eq!(parts("http://"), None);
        assert_eq!(parts("https://a:1/x"), None);
    }

    #[test]
    fn a_server_answering_200_is_healthy() {
        let address = on_tokio(async {
            let address =
                answering(b"HTTP/1.1 200 OK\r\ncontent-length: 12\r\n\r\nherald is up").await;
            let url = format!("http://{address}/healthz");
            let (mut out, mut err) = (Vec::new(), Vec::new());
            let code = health(&url, SOON, &mut out, &mut err).await;
            (code, String::from_utf8(out).unwrap(), err.is_empty(), url)
        });
        let (code, out, quiet, url) = address;
        assert_eq!(code, 0);
        assert_eq!(out.trim_end(), format!("herald answers at {url}"));
        assert!(quiet);
    }

    #[test]
    fn a_server_answering_anything_else_is_not_healthy_and_says_what_it_answered() {
        for (answer, why) in [
            (
                &b"HTTP/1.1 503 Service Unavailable\r\n\r\n"[..],
                "it answered HTTP/1.1 503 Service Unavailable",
            ),
            (&b""[..], "it closed without answering"),
        ] {
            let (code, out, err) = on_tokio(async move {
                let address = answering(answer).await;
                let url = format!("http://{address}/healthz");
                let (mut out, mut err) = (Vec::new(), Vec::new());
                let code = health(&url, SOON, &mut out, &mut err).await;
                (
                    code,
                    String::from_utf8(out).unwrap(),
                    String::from_utf8(err).unwrap(),
                )
            });
            assert_eq!(code, STOPPED);
            assert_eq!(out, "");
            assert!(err.trim_end().ends_with(why), "{err}");
        }
    }

    #[test]
    fn a_server_that_never_answers_is_not_healthy_once_the_time_is_up() {
        let (code, err) = on_tokio(async {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}/healthz", listener.local_addr().unwrap());
            let (mut out, mut err) = (Vec::new(), Vec::new());
            let code = health(&url, Duration::from_millis(50), &mut out, &mut err).await;
            drop(listener);
            (code, String::from_utf8(err).unwrap())
        });
        assert_eq!(code, STOPPED);
        assert!(err.trim_end().ends_with("no answer within 50 ms"), "{err}");
    }

    #[test]
    fn nothing_listening_or_a_url_that_is_not_http_is_not_healthy() {
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let (code, _, err) = checked(&format!("http://127.0.0.1:{port}/healthz"));
        assert_eq!(code, STOPPED);
        assert!(err.starts_with(&format!(
            "herald does not answer at http://127.0.0.1:{port}/healthz: "
        )));
        let (code, _, err) = checked("ftp://herald/healthz");
        assert_eq!(code, STOPPED);
        assert_eq!(
            err.trim_end(),
            "herald does not answer at ftp://herald/healthz: ftp://herald/healthz is not an http:// URL naming a host"
        );
    }
}
