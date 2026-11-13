use herald_core::Address;

/// The DNS suffix of each partition whose regions are named with a prefix
/// of their own, as AWS's partitions name them.
const PARTITIONS: [(&str, &str); 6] = [
    ("cn-", "amazonaws.com.cn"),
    ("us-iso-", "c2s.ic.gov"),
    ("us-isob-", "sc2s.sgov.gov"),
    ("us-isof-", "csp.hci.ic.gov"),
    ("eu-isoe-", "cloud.adc-e.uk"),
    ("eusc-", "amazonaws.eu"),
];

/// The DNS suffix of the commercial partition and of the `aws-us-gov`
/// partition.
const COMMERCIAL: &str = "amazonaws.com";

/// The variable naming every service's endpoint, and before a service's
/// identifier, that service's alone.
const ENDPOINT_URL: &str = "AWS_ENDPOINT_URL";

/// The variable that, set to `true`, has the endpoints the variables name
/// ignored.
const IGNORE_ENDPOINT_URLS: &str = "AWS_IGNORE_CONFIGURED_ENDPOINT_URLS";

/// The DNS suffix of the partition `region` is in.
fn suffix(region: &str) -> &'static str {
    PARTITIONS
        .iter()
        .find(|(prefix, _)| region.starts_with(prefix))
        .map_or(COMMERCIAL, |(_, suffix)| suffix)
}

/// The base URL of `service`'s own endpoint in `region`, in the region's
/// partition.
#[must_use]
pub fn endpoint(service: &str, region: &str) -> String {
    format!("https://{service}.{region}.{}", suffix(region))
}

/// The endpoint the variables `var` reads name for `service`, as the AWS
/// SDKs read them: the service's own variable, then every service's, and
/// none where endpoints so named are ignored.
#[must_use]
pub fn configured<'a>(service: &str, var: impl Fn(&str) -> Option<&'a str>) -> Option<&'a str> {
    if var(IGNORE_ENDPOINT_URLS).is_some_and(|ignored| ignored.eq_ignore_ascii_case("true")) {
        return None;
    }
    let own = format!("{ENDPOINT_URL}_{}", service.to_ascii_uppercase());
    [own.as_str(), ENDPOINT_URL]
        .into_iter()
        .find_map(|name| var(name).filter(|url| !url.is_empty()))
}

/// Where a service is reached, and the host and path its requests are
/// signed for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    url: String,
    host: String,
    path: String,
}

impl Endpoint {
    /// `service` in `region`, at the base URL `given` where one is, or at
    /// the region's own endpoint.
    #[must_use]
    pub fn new(service: &str, region: &str, given: Option<&str>) -> Self {
        let url = given.map_or_else(
            || endpoint(service, region),
            |given| given.trim_end_matches('/').to_owned(),
        );
        let address = Address::parse(&url);
        let host = address.as_ref().map(Address::authority).unwrap_or_default();
        let path = address
            .as_ref()
            .and_then(|address| address.target().split('?').next())
            .unwrap_or_default()
            .trim_end_matches('/')
            .to_owned();
        Self { url, host, path }
    }

    /// Its base URL.
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }

    /// The host its requests are signed for: the URL's, with its port where
    /// that is not its scheme's.
    #[must_use]
    pub fn host(&self) -> &str {
        &self.host
    }

    /// The URL of `path` under it.
    #[must_use]
    pub fn at(&self, path: &str) -> String {
        format!("{}{path}", self.url)
    }

    /// `path` under it, as a signature's canonical URI names it.
    #[must_use]
    pub fn canonical(&self, path: &str) -> String {
        format!("{}{path}", self.path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_service_s_own_endpoint_is_in_its_region_s_partition() {
        let cases = [
            ("eu-west-1", "https://lambda.eu-west-1.amazonaws.com"),
            (
                "us-gov-west-1",
                "https://lambda.us-gov-west-1.amazonaws.com",
            ),
            ("cn-north-1", "https://lambda.cn-north-1.amazonaws.com.cn"),
            ("us-iso-east-1", "https://lambda.us-iso-east-1.c2s.ic.gov"),
            (
                "us-isob-east-1",
                "https://lambda.us-isob-east-1.sc2s.sgov.gov",
            ),
            (
                "us-isof-south-1",
                "https://lambda.us-isof-south-1.csp.hci.ic.gov",
            ),
            (
                "eu-isoe-west-1",
                "https://lambda.eu-isoe-west-1.cloud.adc-e.uk",
            ),
            (
                "eusc-de-east-1",
                "https://lambda.eusc-de-east-1.amazonaws.eu",
            ),
        ];
        for (region, url) in cases {
            assert_eq!(endpoint("lambda", region), url, "{region}");
        }
    }

    #[test]
    fn the_service_s_own_variable_names_its_endpoint_before_every_service_s() {
        const BOTH: [(&str, &str); 2] = [
            ("AWS_ENDPOINT_URL_LAMBDA", "http://lambda.local"),
            ("AWS_ENDPOINT_URL", "http://aws.local"),
        ];
        const EMPTY: [(&str, &str); 2] = [
            ("AWS_ENDPOINT_URL_LAMBDA", ""),
            ("AWS_ENDPOINT_URL", "http://aws.local"),
        ];
        const IGNORED: [(&str, &str); 2] = [
            ("AWS_IGNORE_CONFIGURED_ENDPOINT_URLS", "TRUE"),
            ("AWS_ENDPOINT_URL", "http://aws.local"),
        ];
        const HEEDED: [(&str, &str); 2] = [
            ("AWS_IGNORE_CONFIGURED_ENDPOINT_URLS", "false"),
            ("AWS_ENDPOINT_URL", "http://aws.local"),
        ];
        let vars = |pairs: &'static [(&'static str, &'static str)]| {
            move |name: &str| {
                pairs
                    .iter()
                    .find(|(key, _)| *key == name)
                    .map(|(_, value)| *value)
            }
        };
        let aws = Some("http://aws.local");
        assert_eq!(
            configured("lambda", vars(&BOTH)),
            Some("http://lambda.local")
        );
        assert_eq!(configured("dynamodb", vars(&BOTH)), aws);
        assert_eq!(configured("lambda", vars(&EMPTY)), aws);
        assert_eq!(configured("lambda", vars(&[])), None);
        assert_eq!(configured("lambda", vars(&IGNORED)), None);
        assert_eq!(configured("lambda", vars(&HEEDED)), aws);
    }

    #[test]
    fn an_endpoint_is_the_region_s_own_where_none_is_given() {
        let own = Endpoint::new("dynamodb", "cn-north-1", None);
        assert_eq!(own.url(), "https://dynamodb.cn-north-1.amazonaws.com.cn");
        assert_eq!(own.host(), "dynamodb.cn-north-1.amazonaws.com.cn");
        assert_eq!(own.at("/"), "https://dynamodb.cn-north-1.amazonaws.com.cn/");
        assert_eq!(own.canonical("/"), "/");
    }

    #[test]
    fn a_given_endpoint_is_signed_for_its_own_host_and_path() {
        let given = Endpoint::new(
            "lambda",
            "eu-west-1",
            Some("http://localhost:4566/aws/?x=1"),
        );
        assert_eq!(given.url(), "http://localhost:4566/aws/?x=1");
        let given = Endpoint::new("lambda", "eu-west-1", Some("http://localhost:4566/aws/"));
        assert_eq!(given.url(), "http://localhost:4566/aws");
        assert_eq!(given.host(), "localhost:4566");
        assert_eq!(given.at("/2015"), "http://localhost:4566/aws/2015");
        assert_eq!(given.canonical("/2015"), "/aws/2015");
        let bare = Endpoint::new("lambda", "eu-west-1", Some("https://Lambda.Example:443"));
        assert_eq!(bare.host(), "lambda.example");
        assert_eq!(bare.canonical("/"), "/");
        let unreadable = Endpoint::new("lambda", "eu-west-1", Some("localhost:4566"));
        assert_eq!(unreadable.host(), "");
        assert_eq!(unreadable.canonical("/"), "/");
    }
}
