use std::fmt::Write as _;

use herald_core::hex;
use herald_ports::{Request, Secret, Text};
use hmac::{Hmac, KeyInit, Mac};
use jiff::Timestamp;
use sha2::{Digest, Sha256};

/// The service DynamoDB's requests are signed for.
const SERVICE: &str = "dynamodb";
/// The signature's algorithm, as AWS names it.
const ALGORITHM: &str = "AWS4-HMAC-SHA256";

/// What a request is signed with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Signing<'a> {
    pub(crate) access_key_id: &'a Secret,
    pub(crate) secret_access_key: &'a Secret,
    pub(crate) session_token: Option<&'a Secret>,
    pub(crate) region: &'a str,
    pub(crate) host: &'a str,
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

/// The key a day's requests in `region` are signed with, derived from
/// `secret`.
pub(crate) fn signing_key(secret: &str, day: &str, region: &str, service: &str) -> Vec<u8> {
    let dated = hmac(format!("AWS4{secret}").as_bytes(), day.as_bytes());
    let regional = hmac(&dated, region.as_bytes());
    let serviced = hmac(&regional, service.as_bytes());
    hmac(&serviced, b"aws4_request")
}

/// `request`, a POST to `/` whose `X-Amz-Target` is `target`, signed at
/// `now` with AWS Signature Version 4.
pub(crate) fn signed(
    request: Request,
    target: &str,
    signing: &Signing<'_>,
    now: Timestamp,
) -> Request {
    let stamp = now.strftime("%Y%m%dT%H%M%SZ").to_string();
    let day = now.strftime("%Y%m%d").to_string();
    let mut headers = vec![
        ("content-type", "application/x-amz-json-1.0".to_owned()),
        ("host", signing.host.to_owned()),
        ("x-amz-date", stamp.clone()),
    ];
    if let Some(token) = signing.session_token {
        headers.push(("x-amz-security-token", token.expose().to_owned()));
    }
    headers.push(("x-amz-target", target.to_owned()));
    let canonical_headers = headers
        .iter()
        .fold(String::new(), |mut lines, (name, value)| {
            let _ = writeln!(lines, "{name}:{value}");
            lines
        });
    let signed_headers = headers
        .iter()
        .map(|(name, _)| *name)
        .collect::<Vec<_>>()
        .join(";");
    let canonical = format!(
        "POST\n/\n\n{canonical_headers}\n{signed_headers}\n{}",
        hashed(&request.body)
    );
    let scope = format!("{day}/{}/{SERVICE}/aws4_request", signing.region);
    let to_sign = format!(
        "{ALGORITHM}\n{stamp}\n{scope}\n{}",
        hashed(canonical.as_bytes())
    );
    let key = signing_key(
        signing.secret_access_key.expose(),
        &day,
        signing.region,
        SERVICE,
    );
    let signature = hex(&hmac(&key, to_sign.as_bytes()));
    let mut request = request
        .header("Content-Type", Text::plain("application/x-amz-json-1.0"))
        .header("X-Amz-Date", Text::plain(stamp))
        .header("X-Amz-Target", Text::plain(target))
        .header(
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
                "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY",
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
            host: "dynamodb.eu-west-1.amazonaws.com",
        };
        let plain = signed(request(), "DynamoDB_20120810.GetItem", &signing, now);
        assert_eq!(header(&plain, "X-Amz-Date"), "20261105T230000Z");
        assert_eq!(header(&plain, "X-Amz-Target"), "DynamoDB_20120810.GetItem");
        let authorization = header(&plain, "Authorization");
        assert!(authorization.starts_with(
            "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20261105/eu-west-1/dynamodb/aws4_request, SignedHeaders=content-type;host;x-amz-date;x-amz-target, Signature="
        ), "{authorization}");
        assert_eq!(header(&plain, "X-Amz-Security-Token"), "");
        signing.session_token = Some(&token);
        let tokened = signed(request(), "DynamoDB_20120810.GetItem", &signing, now);
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
            "DynamoDB_20120810.GetItem",
            &signing,
            now,
        );
        assert_ne!(header(&other, "Authorization"), authorization);
    }
}
