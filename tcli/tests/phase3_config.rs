//! config.toml + env precedence for `TCLI_AUTH_BASE`.

use std::fs;
use std::path::PathBuf;

use tcli::config::{self, config_path};
use tcli::config_file;

use serial_test::serial;

#[test]
#[serial]
fn tcli_auth_base_overrides_file() {
    let tmp = tempfile::tempdir().unwrap();
    let home: PathBuf = tmp.path().join("h");
    fs::create_dir_all(&home).unwrap();

    let p = config_path(&home);
    fs::write(
        &p,
        r#"
[auth]
base = "http://ignored.example"
client_id = "from-file"
"#,
    )
    .unwrap();

    std::env::set_var("TCLI_AUTH_BASE", "http://override.example:9999");

    let cfg = config_file::load(&p).unwrap();
    let r = config::resolve(&cfg).unwrap();
    assert_eq!(r.base.as_str(), "http://override.example:9999/");
    assert_eq!(r.client_id, "from-file");

    std::env::remove_var("TCLI_AUTH_BASE");
}
