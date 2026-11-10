use std::fmt::Write as _;

use herald_core::hex;
use herald_ports::{Request, Secret, Text};
use hmac::{Hmac, KeyInit, Mac};
use jiff::Timestamp;
use sha2::{Digest, Sha256};

/// The signature's algorithm, as AWS names it.
const ALGORITHM: &str = "AWS4-HMAC-SHA256";

/// What a request is signed with, and for which service where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signing<'a> {
    /// The access key's id.
    pub access_key_id: &'a Secret,
    /// The secret access key.
    pub secret_access_key: &'a Secret,
    /// A session token, where the credentials are temporary.
    pub session_token: Option<&'a Secret>,
    /// The region, such as `eu-west-1`.
    pub region: &'a str,
    /// The service, as AWS names it in a signature's scope, such as
    /// `dynamodb` or `lambda`.
    pub service: &'a str,
    /// The host the request is sent to.
    pub host: &'a str,
}

fn hmac(key: &[u8], message: &[u8]) -> Vec<u8> {
    <Hmac<Sha256> as KeyInit>::new_from_slice(key)
        .map(|mut mac| {
            mac.update(message);
            mac.finalize().into_bytes().to_vec()
        })
        .unwrap_or_default()
}

fn hashed(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

/// The key a day's requests to `service` in `region` are signed with,
/// derived from `secret`.
#[must_use]
pub fn signing_key(secret: &str, day: &str, region: &str, service: &str) -> Vec<u8> {
    let dated = hmac(format!("AWS4{secret}").as_bytes(), day.as_bytes());
    let regional = hmac(&dated, region.as_bytes());
    let serviced = hmac(&regional, service.as_bytes());
    hmac(&serviced, b"aws4_request")
}

/// `request`, a POST of `kind` to `path` with the `x-amz-` headers `amz`
/// beside its date and token, signed at `now` with AWS Signature Version 4.
/// `path` is written as AWS's canonical URI writes it.
#[must_use]
pub fn signed(
    request: Request,
    path: &str,
    kind: &str,
    amz: &[(&str, &str)],
    signing: &Signing<'_>,
    now: Timestamp,
) -> Request {
    let stamp = now.strftime("%Y%m%dT%H%M%SZ").to_string();
    let day = now.strftime("%Y%m%d").to_string();
    let mut headers: Vec<(String, String)> = vec![
        ("content-type".to_owned(), kind.to_owned()),
        ("host".to_owned(), signing.host.to_owned()),
        ("x-amz-date".to_owned(), stamp.clone()),
    ];
    if let Some(token) = signing.session_token {
        headers.push(("x-amz-security-token".to_owned(), token.expose().to_owned()));
    }
    for (name, value) in amz {
        headers.push((
            format!("x-amz-{}", name.to_lowercase()),
            (*value).to_owned(),
        ));
    }
    headers.sort();
    let canonical_headers = headers
        .iter()
        .fold(String::new(), |mut lines, (name, value)| {
            let _ = writeln!(lines, "{name}:{value}");
            lines
        });
    let signed_headers = headers
        .iter()
        .map(|(name, _)| name.as_str())
        .collect::<Vec<_>>()
        .join(";");
    let canonical = format!(
        "POST\n{path}\n\n{canonical_headers}\n{signed_headers}\n{}",
        hashed(&request.body)
    );
    let scope = format!("{day}/{}/{}/aws4_request", signing.region, signing.service);
    let to_sign = format!(
        "{ALGORITHM}\n{stamp}\n{scope}\n{}",
        hashed(canonical.as_bytes())
    );
    let key = signing_key(
        signing.secret_access_key.expose(),
        &day,
        signing.region,
        signing.service,
    );
    let signature = hex(&hmac(&key, to_sign.as_bytes()));
    let mut request = request
        .header("Content-Type", Text::plain(kind))
        .header("X-Amz-Date", Text::plain(stamp));
    for (name, value) in amz {
        request = request.header(format!("X-Amz-{name}"), Text::plain(*value));
    }
    request = request.header(
        "Authorization",
        Text::plain(format!("{ALGORITHM} Credential="))
            .then_secret(signing.access_key_id.clone())
            .then(format!(
                "/{scope}, SignedHeaders={signed_headers}, Signature={signature}"
            )),
    );
    if let Some(token) = signing.session_token {
        request = request.header(
            "X-Amz-Security-Token",
            Text::plain("").then_secret(token.clone()),
        );
    }
    request
}

#[cfg(test)]
mod tests {
    use herald_ports::Method;

    use super::*;

    #[test]
    fn the_signing_key_is_the_one_aws_documents() {
        assert_eq!(
            hex(&signing_key(
                include_str!("../fixtures/aws-documented-secret.test.txt").trim(),
                "20120215",
                "us-east-1",
                "iam"
            )),
            "f4780e2d9f65fa895f9c67b32ce1baf0b0d8a43505a000a1a9e090d414db404d"
        );
    }

    fn header(request: &Request, name: &str) -> String {
        request
            .headers
            .iter()
            .find(|(header, _)| header == name)
            .map(|(_, value)| value.expose())
            .unwrap_or_default()
    }

    #[test]
    fn a_request_carries_the_signature_aws_s_own_test_suite_gives() {
        let id = Secret::new("AWS_ACCESS_KEY_ID", "AKIDEXAMPLE");
        let key = Secret::new(
            "AWS_SECRET_ACCESS_KEY",
            include_str!("../fixtures/aws-documented-secret.test.txt").trim(),
        );
        let request = Request::new(Method::Post, Text::plain("https://example.amazonaws.com/"))
            .body(b"Param1=value1".to_vec());
        let signing = Signing {
            access_key_id: &id,
            secret_access_key: &key,
            session_token: None,
            region: "us-east-1",
            service: "service",
            host: "example.amazonaws.com",
        };
        let signed = signed(
            request,
            "/",
            "application/x-www-form-urlencoded",
            &[],
            &signing,
            "2015-08-30T12:36:00Z".parse().unwrap(),
        );
        assert_eq!(
            header(&signed, "Authorization"),
            "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20150830/us-east-1/service/aws4_request, SignedHeaders=content-type;host;x-amz-date, Signature=ff11897932ad3f4e8b18135d722051e5ac45fc38421b1da7b9d196a0fe09473a"
        );
    }

    #[test]
    fn a_request_is_signed_over_its_headers_and_body_with_its_token_where_it_has_one() {
        let id = Secret::new("AWS_ACCESS_KEY_ID", "AKIDEXAMPLE");
        let key = Secret::new(
            "AWS_SECRET_ACCESS_KEY",
            "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY",
        );
        let token = Secret::new("AWS_SESSION_TOKEN", "session");
        let now: Timestamp = "2026-11-05T23:00:00Z".parse().unwrap();
        let request = || {
            Request::new(
                Method::Post,
                Text::plain("https://dynamodb.eu-west-1.amazonaws.com/"),
            )
            .body(br#"{"TableName":"herald"}"#.to_vec())
        };
        let mut signing = Signing {
            access_key_id: &id,
            secret_access_key: &key,
            session_token: None,
            region: "eu-west-1",
            service: "dynamodb",
            host: "dynamodb.eu-west-1.amazonaws.com",
        };
        let json = "application/x-amz-json-1.0";
        let target = [("Target", "DynamoDB_20120810.GetItem")];
        let plain = signed(request(), "/", json, &target, &signing, now);
        assert_eq!(header(&plain, "X-Amz-Date"), "20261105T230000Z");
        assert_eq!(header(&plain, "X-Amz-Target"), "DynamoDB_20120810.GetItem");
        let authorization = header(&plain, "Authorization");
        assert_eq!(
            authorization,
            "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20261105/eu-west-1/dynamodb/aws4_request, SignedHeaders=content-type;host;x-amz-date;x-amz-target, Signature=815c67390ae7dfcff4ba13a9df909f854e22dc65cb561ed53256a9a8aec02658"
        );
        assert_eq!(header(&plain, "X-Amz-Security-Token"), "");
        signing.session_token = Some(&token);
        let tokened = signed(request(), "/", json, &target, &signing, now);
        assert_eq!(header(&tokened, "X-Amz-Security-Token"), "session");
        let authorization = header(&tokened, "Authorization");
        assert!(
            authorization.contains(
                "SignedHeaders=content-type;host;x-amz-date;x-amz-security-token;x-amz-target"
            ),
            "{authorization}"
        );
        assert!(!format!("{tokened:?}").contains("AKIDEXAMPLE"));
        let other = signed(
            request().body(b"{}".to_vec()),
            "/",
            json,
            &target,
            &signing,
            now,
        );
        assert_ne!(header(&other, "Authorization"), authorization);
        let invoked = signed(
            request(),
            "/2015-03-31/functions/herald/invocations",
            "application/json",
            &[("Invocation-Type", "Event")],
            &Signing {
                service: "lambda",
                ..signing
            },
            now,
        );
        assert_eq!(header(&invoked, "X-Amz-Invocation-Type"), "Event");
        let authorization = header(&invoked, "Authorization");
        assert!(
            authorization.contains("/lambda/aws4_request, SignedHeaders=content-type;host;x-amz-date;x-amz-invocation-type;x-amz-security-token, "),
            "{authorization}"
        );
    }
}
