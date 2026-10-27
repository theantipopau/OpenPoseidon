//! Drive the built `papa` binary through a pack -> unpack round-trip.

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;

fn papa() -> Command {
    Command::new(env!("CARGO_BIN_EXE_papa"))
}

#[test]
fn pack_then_unpack_restores_the_tree() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("src");
    fs::create_dir_all(src.join("data")).unwrap();
    fs::write(src.join("config.cpp"), b"class CfgPatches {};\n").unwrap();
    fs::write(src.join("data/note.txt"), b"hello papa\n").unwrap();

    let pbo = dir.path().join("out.pbo");
    let status = papa()
        .args(["pack", src.to_str().unwrap(), "-o", pbo.to_str().unwrap()])
        .args(["--prefix", "demo\\addon"])
        .status()
        .unwrap();
    assert!(status.success(), "pack failed");
    assert!(pbo.is_file(), "pbo not created");

    let out = dir.path().join("out");
    let status = papa()
        .args(["unpack", pbo.to_str().unwrap(), "-o", out.to_str().unwrap()])
        .status()
        .unwrap();
    assert!(status.success(), "unpack failed");

    assert_eq!(
        fs::read(out.join("config.cpp")).unwrap(),
        b"class CfgPatches {};\n"
    );
    assert_eq!(
        fs::read(out.join("data/note.txt")).unwrap(),
        b"hello papa\n"
    );

    // `info` runs and reports both entries.
    let info = papa()
        .args(["info", pbo.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(info.status.success());
    let stdout = String::from_utf8_lossy(&info.stdout);
    assert!(stdout.contains("config.cpp"), "info output: {stdout}");
    assert!(stdout.contains("data/note.txt"), "info output: {stdout}");
    assert!(
        stdout.contains("prefix = demo\\addon"),
        "info output: {stdout}"
    );
}

// MSERVER-CLI-RULE-1: the publisher's `--mod-id` is held to the same one-component rule the
// master service applies (`papa_bear_client::catalog::is_single_path_component`), so a name the
// service would refuse never reaches the network. `Sth..else` used to be refused by the CLI's
// own `contains("..")` while the engine installs it — it now passes validation and stops at the
// (deliberately unreachable) master instead, which is the other half of the same parity.
//
// MSERVER-NAME-CASES-1: the cases are the shared fixture the C++ suite and both Rust unit tests
// read, not a third hand-typed list, so a name added there is exercised here as well.
fn catalog_name_cases(key: &str) -> Vec<String> {
    let fixture = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/mod_names/catalog_name_cases.json"
    ));
    let json: serde_json::Value =
        serde_json::from_str(fixture).expect("the catalog name cases fixture is valid JSON");
    json[key]
        .as_array()
        .unwrap_or_else(|| panic!("'{key}' is an array in the fixture"))
        .iter()
        .map(|case| {
            case.as_str()
                .unwrap_or_else(|| panic!("'{key}' holds strings"))
                .to_owned()
        })
        .collect()
}

#[test]
fn publish_validates_mod_id_before_any_network_call() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("config.cpp"), b"class CfgPatches {};\n").unwrap();

    let publish = |mod_id: &str, folder: Option<&str>| {
        let mut command = papa();
        command.args([
            "--master",
            "http://127.0.0.1:1",
            "publish",
            src.to_str().unwrap(),
            "--admin-key",
            "test-key",
            "--mod-id",
            mod_id,
        ]);
        if let Some(folder) = folder {
            command.args(["--folder-name", folder]);
        }
        command.output().unwrap()
    };

    let refused = catalog_name_cases("refused");
    let accepted = catalog_name_cases("accepted");
    // A list that parsed to nothing would pass vacuously, so both sides carry a floor.
    assert!(refused.len() >= 20, "refused cases: {refused:?}");
    assert!(accepted.len() >= 8, "accepted cases: {accepted:?}");

    for name in &refused {
        let output = publish(name, None);
        assert!(!output.status.success(), "mod id {name:?} published");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains(&format!("invalid mod id '{name}'")),
            "mod id {name:?} stderr: {stderr}"
        );
        assert!(
            !stderr.contains("catalog lookup failed"),
            "mod id {name:?} reached the network: {stderr}"
        );
    }

    // `--folder-name` is the same value after the service's trim: the publisher refuses it
    // before packing and uploading, and reports the normalized (trimmed) value like the
    // master service does.
    for name in &refused {
        let output = publish("good-id", Some(name));
        assert!(!output.status.success(), "folder {name:?} published");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains(&format!("invalid folder name '{}'", name.trim())),
            "folder {name:?} stderr: {stderr}"
        );
        assert!(
            !stderr.contains("catalog lookup failed"),
            "folder {name:?} reached the network: {stderr}"
        );
    }

    // One URL-safe accepted pair keeps the positive control: the fixture must still call both
    // names accepted, validation passes, and the request goes out — failing only because
    // 127.0.0.1:1 is closed. The rest of the accepted list is covered by the unit tests, which
    // apply the same file to both spellings without a two-second connection attempt each.
    for name in ["Sth..else", "ver2.0"] {
        assert!(
            accepted.iter().any(|case| case == name),
            "{name:?} left the accepted list"
        );
    }
    let reached_master = publish("Sth..else", Some("ver2.0"));
    assert!(
        !reached_master.status.success(),
        "unreachable master published"
    );
    let stderr = String::from_utf8_lossy(&reached_master.stderr);
    assert!(
        stderr.contains("catalog lookup failed"),
        "expected a catalog failure past validation, got: {stderr}"
    );
}

/// A stub master for the requests `papa publish` makes before it streams the artifact: the
/// catalog lookup (`GET /v1/mods/<id>`), the install-folder reservation, and the abort or commit
/// that ends it. It answers in order and records each request line, so a test can tell how far
/// the publisher got. `stop` ends the responder when the publisher made fewer requests than
/// expected, instead of blocking the join.
fn spawn_master(
    responses: Vec<(u16, String)>,
    stop: Arc<AtomicBool>,
) -> (String, JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let handle = std::thread::spawn(move || {
        let mut seen = Vec::new();
        let mut served = 0;
        while served < responses.len() && !stop.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream.set_nonblocking(false).unwrap();
                    let mut reader = BufReader::new(stream.try_clone().unwrap());
                    let mut request_line = String::new();
                    reader.read_line(&mut request_line).unwrap();
                    // Drain the headers, then any body (a reservation and its end are JSON POSTs)
                    // so the next request starts at a request line.
                    let mut content_length = 0usize;
                    loop {
                        let mut line = String::new();
                        if reader.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                            break;
                        }
                        if let Some(value) =
                            line.to_ascii_lowercase().strip_prefix("content-length:")
                        {
                            content_length = value.trim().parse().unwrap_or(0);
                        }
                    }
                    if content_length > 0 {
                        let mut body = vec![0u8; content_length];
                        reader.read_exact(&mut body).unwrap();
                    }
                    seen.push(request_line.trim().to_string());
                    let (status, body) = &responses[served];
                    write!(
                        &mut stream,
                        "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .unwrap();
                    stream.flush().unwrap();
                    served += 1;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                Err(error) => panic!("stub master accept failed: {error}"),
            }
        }
        seen
    });
    (format!("http://{address}"), handle)
}

/// Run `papa publish` against the stub for a source path that is neither a folder nor a PBO, so
/// the error text says whether packing was reached. Returns stderr and the stub's request lines.
fn publish_against_stub(responses: Vec<(u16, String)>, folder_name: &str) -> (String, Vec<String>) {
    let stop = Arc::new(AtomicBool::new(false));
    let (master, handle) = spawn_master(responses, stop.clone());
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("not-a-folder");
    let output = papa()
        .args([
            "--master",
            &master,
            "publish",
            source.to_str().unwrap(),
            "--admin-key",
            "test-key",
            "--mod-id",
            "beta",
            "--name",
            "Beta",
            "--folder-name",
            folder_name,
        ])
        .output()
        .unwrap();
    stop.store(true, Ordering::SeqCst);
    let seen = handle.join().unwrap();
    assert!(
        !output.status.success(),
        "the publish unexpectedly succeeded"
    );
    (String::from_utf8_lossy(&output.stderr).to_string(), seen)
}

// MSERVER-PREFLIGHT-1 / MSERVER-CLAIM-RESERVE-1: `papa publish` reserves its install folder
// before it packs the source and before it uploads the artifact. The source here packs into an
// error, so the error that comes back names the step that ran first.
#[test]
fn publish_refuses_a_claimed_folder_before_packing_or_uploading() {
    let refusal = "folder 'SHARED' is already claimed by catalog entry 'alpha' as 'shared' (one folder on a case-insensitive filesystem); publish with a different folderName";
    let (stderr, seen) = publish_against_stub(
        vec![(404, String::new()), (409, refusal.to_string())],
        "SHARED",
    );

    assert!(
        stderr.contains(&format!("publish refused before uploading: {refusal}")),
        "stderr: {stderr}"
    );
    assert!(
        !stderr.contains("is neither a folder nor a file"),
        "the source was packed before the claim was checked: {stderr}"
    );
    assert_eq!(seen.len(), 2, "requests: {seen:?}");
    assert!(seen[0].starts_with("GET /v1/mods/beta "), "{seen:?}");
    assert!(
        seen[1].starts_with("POST /v1/catalog/folder-claim "),
        "{seen:?}"
    );
}

#[test]
fn publish_reaches_the_source_once_the_claim_is_free() {
    let (stderr, seen) = publish_against_stub(
        vec![
            (404, String::new()),
            (
                200,
                r#"{"modId":"beta","folder":"Free Folder","token":"tok"}"#.to_string(),
            ),
            (204, String::new()),
        ],
        "Free Folder",
    );

    assert!(
        stderr.contains("is neither a folder nor a file"),
        "a free claim did not reach packing: {stderr}"
    );
    assert_eq!(seen.len(), 3, "requests: {seen:?}");
    assert!(
        seen[1].starts_with("POST /v1/catalog/folder-claim "),
        "{seen:?}"
    );
    // The failed publish gives the folder back instead of leaving it to the sweep's age floor.
    assert!(
        seen[2].starts_with("POST /v1/catalog/folder-claim/abort "),
        "{seen:?}"
    );
}

// MSERVER-CLAIM-LEASE-1: a reservation is taken before the source is packed, and the master's
// lease is shorter than a large mod's pack plus transfer. The publisher renews it while it works,
// so `check-names --fix` cannot read a live publish as an abandoned claim. The first renewal goes
// out before packing, which is why this stub — whose source cannot be packed — still sees it.
// MSERVER-CLAIM-KEEP-1: a revision that keeps its folder is reserved too, so `papa publish` on an
// existing entry renews that reservation while it packs, exactly as a new entry does. The stub's
// catalog lookup answers 200, which is the only difference from the new-entry case.
#[test]
fn publish_reserves_a_revision_that_keeps_its_folder() {
    let (stderr, seen) = publish_against_stub(
        vec![
            (
                200,
                r#"{"modId":"beta","name":"Beta","folderName":"Free Folder"}"#.to_string(),
            ),
            (
                200,
                r#"{"modId":"beta","folder":"Free Folder","token":"tok","renewAfterMs":200000}"#
                    .to_string(),
            ),
            (204, String::new()),
            (204, String::new()),
        ],
        "Free Folder",
    );

    assert!(
        stderr.contains("is neither a folder nor a file"),
        "a keeps-folder reservation did not reach packing: {stderr}"
    );
    assert_eq!(seen.len(), 4, "requests: {seen:?}");
    assert!(seen[0].starts_with("GET /v1/mods/beta "), "{seen:?}");
    assert!(
        seen[1].starts_with("POST /v1/catalog/folder-claim "),
        "{seen:?}"
    );
    assert!(
        seen[2].starts_with("POST /v1/catalog/folder-claim/renew "),
        "{seen:?}"
    );
    assert!(
        seen[3].starts_with("POST /v1/catalog/folder-claim/abort "),
        "{seen:?}"
    );
}

// A master that predates keeps-folder reservations answers no token for one. The publisher still
// uploads, with no lease to renew, exactly as it did before this route reserved those revisions.
#[test]
fn publish_uploads_when_an_older_master_reserves_nothing_for_a_kept_folder() {
    let (stderr, seen) = publish_against_stub(
        vec![
            (200, r#"{"modId":"beta","name":"Beta"}"#.to_string()),
            (
                200,
                r#"{"modId":"beta","folder":"Free Folder"}"#.to_string(),
            ),
            (204, String::new()),
        ],
        "Free Folder",
    );

    assert!(
        stderr.contains("is neither a folder nor a file"),
        "a token-less reservation did not reach packing: {stderr}"
    );
    // Nothing was reserved, so there is no lease to renew and nothing to give back.
    assert_eq!(seen.len(), 2, "requests: {seen:?}");
    assert!(
        !seen
            .iter()
            .any(|line| line.starts_with("POST /v1/catalog/folder-claim/")),
        "a token-less reservation was renewed or released: {seen:?}"
    );
}

#[test]
fn publish_renews_its_reservation_while_it_works() {
    let (stderr, seen) = publish_against_stub(
        vec![
            (404, String::new()),
            (
                200,
                r#"{"modId":"beta","folder":"Free Folder","token":"tok","renewAfterMs":200000}"#
                    .to_string(),
            ),
            (204, String::new()),
            (204, String::new()),
        ],
        "Free Folder",
    );

    assert!(
        stderr.contains("is neither a folder nor a file"),
        "a leased claim did not reach packing: {stderr}"
    );
    assert_eq!(seen.len(), 4, "requests: {seen:?}");
    assert!(
        seen[2].starts_with("POST /v1/catalog/folder-claim/renew "),
        "{seen:?}"
    );
    // Renewing stops before the folder is given back, so no renewal can re-create the claim the
    // abort removes.
    assert!(
        seen[3].starts_with("POST /v1/catalog/folder-claim/abort "),
        "{seen:?}"
    );
}
