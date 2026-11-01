/// Whether `version` has a pre-release part, as semantic versioning writes
/// one: a numeric version, a hyphen, and an identifier, as in `v1.2.0-rc.1`.
/// A leading `v` and any build metadata after `+` are ignored.
#[must_use]
pub fn prerelease_version(version: &str) -> bool {
    let version = version.strip_prefix(['v', 'V']).unwrap_or(version);
    let version = version.split('+').next().unwrap_or_default();
    let Some((number, pre)) = version.split_once('-') else {
        return false;
    };
    let numeric = number
        .split('.')
        .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()));
    numeric && !pre.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_version_with_a_pre_release_part_is_a_pre_release() {
        for version in ["1.0.0-rc.1", "v2.0.0-beta", "V0.1-alpha+build.5", "3-dev"] {
            assert!(prerelease_version(version), "{version}");
        }
    }

    #[test]
    fn anything_else_is_not() {
        for version in [
            "1.0.0",
            "v1.2.3+build-7",
            "nightly",
            "release-2026",
            "1.2.3-",
            "1..2-rc",
            "-rc",
            "",
        ] {
            assert!(!prerelease_version(version), "{version}");
        }
    }
}
