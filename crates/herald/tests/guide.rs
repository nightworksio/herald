//! The README's quick start shows herald's own `herald.toml`, which herald's
//! releases announce themselves with (ADR-0010): the guide's copy is the
//! file, and the file is one `herald doctor` passes.

#[cfg(test)]
mod guide {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use herald::{DONE, run};

    fn root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    fn read(name: &str) -> String {
        std::fs::read_to_string(root().join(name)).unwrap()
    }

    #[test]
    fn the_quick_start_shows_herald_s_own_config() {
        let readme = read("README.md");
        let start = readme.find("## Quick start").unwrap();
        let open = readme[start..].find("```toml\n").unwrap() + start + "```toml\n".len();
        let close = readme[open..].find("```\n").unwrap() + open;
        assert_eq!(&readme[open..close], read("herald.toml"));
    }

    #[test]
    fn herald_s_own_config_passes_the_doctor() {
        let config = root().join("herald.toml").display().to_string();
        let args = [
            "herald", "--config", &config, "doctor", "--layer", "actions",
        ]
        .map(ToOwned::to_owned);
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = pollster::block_on(run(
            &args,
            &BTreeMap::new(),
            &|_| Err(String::new()),
            &mut out,
            &mut err,
        ));
        assert_eq!(code, DONE, "{}", String::from_utf8_lossy(&err));
    }
}
