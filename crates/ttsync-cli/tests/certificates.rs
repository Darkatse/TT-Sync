use std::path::Path;
use std::process::{Command, Output};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ttsync_contract::pair::PairUri;
use ttsync_http::tls::{SelfManagedTls, TlsProvider};

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_tt-sync"))
        .arg("--no-color")
        .arg("--state-dir")
        .arg(root.join("state"))
        .arg("--config-file")
        .arg(root.join("config.toml"))
        .args(args)
        .output()
        .expect("run tt-sync")
}

fn pair_pin(root: &Path) -> String {
    let output = run(root, &["pair", "open", "--json"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let pair = PairUri::parse_uri_string(json["pair_uri"].as_str().unwrap()).unwrap();
    assert_eq!(json["spki_sha256"], pair.spki_sha256);
    pair.spki_sha256
}

#[test]
fn pairing_loads_external_pem_and_prefers_explicit_public_pin() {
    let root = std::env::temp_dir().join(format!("tt-sync-certs-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(root.join("certs")).unwrap();
    let config_path = root.join("config.toml");
    let base = "workspace_path = '.'\npublic_url = 'https://sync.example.com'\n";
    std::fs::write(&config_path, base).unwrap();

    let default_pin = pair_pin(&root);
    let local = SelfManagedTls::load_or_create(&root.join("state")).unwrap();
    assert_eq!(default_pin, local.spki_sha256());

    let external = rcgen::generate_simple_self_signed(["external.example.com".to_owned()]).unwrap();
    let cert_path = root.join("certs/fullchain.pem");
    let key_path = root.join("certs/key.pem");
    let cert_pem = external.cert.pem();
    let key_pem = external.key_pair.serialize_pem();
    std::fs::write(&cert_path, &cert_pem).unwrap();
    std::fs::write(&key_path, &key_pem).unwrap();
    let external_pin = SelfManagedTls::from_pem_files(&cert_path, &key_path)
        .unwrap()
        .spki_sha256()
        .to_owned();
    let files = "[tls]\ncert_file = 'certs/fullchain.pem'\nkey_file = 'certs/key.pem'\n";
    std::fs::write(&config_path, format!("{base}{files}")).unwrap();
    assert_eq!(pair_pin(&root), external_pin);
    assert_ne!(external_pin, default_pin);

    let public_pin = URL_SAFE_NO_PAD.encode([9u8; 32]);
    let pinned = format!("{base}public_spki_sha256 = '{public_pin}'\n");
    std::fs::write(&config_path, &pinned).unwrap();
    assert_eq!(pair_pin(&root), public_pin);
    std::fs::write(&config_path, format!("{pinned}{files}")).unwrap();
    assert_eq!(pair_pin(&root), public_pin);

    assert!(!run(&root, &["cert", "rotate-leaf"]).status.success());
    assert_eq!(std::fs::read_to_string(&cert_path).unwrap(), cert_pem);
    assert_eq!(std::fs::read_to_string(&key_path).unwrap(), key_pem);

    // A public pin accidentally placed inside [tls] must not be silently ignored.
    std::fs::write(
        &config_path,
        format!("{base}{files}public_spki_sha256 = '{public_pin}'\n"),
    )
    .unwrap();
    assert!(!run(&root, &["pair", "open", "--json"]).status.success());

    for pin in [
        "".to_owned(),
        "AA:BB:CC".to_owned(),
        URL_SAFE_NO_PAD.encode([1u8; 31]),
    ] {
        std::fs::write(
            &config_path,
            format!("{base}public_spki_sha256 = '{pin}'\n{files}"),
        )
        .unwrap();
        assert!(!run(&root, &["pair", "open", "--json"]).status.success());
    }
    std::fs::write(
        &config_path,
        format!("{base}[tls]\ncert_file = 'certs/fullchain.pem'\n"),
    )
    .unwrap();
    assert!(!run(&root, &["pair", "open", "--json"]).status.success());

    std::fs::write(&config_path, format!("{pinned}{files}")).unwrap();
    let wrong_key = rcgen::KeyPair::generate().unwrap().serialize_pem();
    std::fs::write(&key_path, &wrong_key).unwrap();
    assert!(!run(&root, &["pair", "open", "--json"]).status.success());
    assert_eq!(std::fs::read_to_string(&key_path).unwrap(), wrong_key);
    assert_eq!(std::fs::read_to_string(&cert_path).unwrap(), cert_pem);
    std::fs::remove_file(&cert_path).unwrap();
    assert!(!run(&root, &["pair", "open", "--json"]).status.success());
    assert!(!cert_path.exists());
    assert_eq!(std::fs::read_to_string(&key_path).unwrap(), wrong_key);
    std::fs::remove_dir_all(root).unwrap();
}
