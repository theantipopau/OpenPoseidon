//! Drive the `check-names` sweep through the built binary against a local mod store.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, SystemTime};

use sha2::{Digest, Sha256};

/// A legacy-style catalog entry: `<root>/<mod_id>/mod.json` with no `current` object (the store
/// reads that as revision 1) beside the pre-revision `<mod_id>/<mod_id>.pbo.zst` a revision 1
/// download reads. `sha256` matches the artifact, so a clean entry exercises the hash comparison
/// and not only the existence check. Returns the artifact path so a case can break it.
fn write_entry(root: &Path, mod_id: &str, name: &str, folder_name: Option<&str>) -> PathBuf {
    let directory = root.join(mod_id);
    std::fs::create_dir_all(&directory).unwrap();
    let artifact = directory.join(format!("{mod_id}.pbo.zst"));
    let bytes = format!("artifact of {mod_id}").into_bytes();
    std::fs::write(&artifact, &bytes).unwrap();
    // `ModCatalogEntry`'s required fields (no serde default): the two optional URLs are present
    // and null, which is how a just-published entry serializes when it has none.
    let mut entry = serde_json::json!({
        "modId": mod_id,
        "name": name,
        "version": "1.0",
        "description": "",
        "homepageUrl": null,
        "downloadUrl": null,
        "sha256": format!("{:x}", Sha256::digest(&bytes)),
    });
    if let Some(folder) = folder_name {
        entry["folderName"] = serde_json::Value::String(folder.to_string());
    }
    std::fs::write(
        directory.join("mod.json"),
        serde_json::to_vec(&entry).unwrap(),
    )
    .unwrap();
    artifact
}

fn check_names_with(root: &Path, extra: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_papa-bear-master-service"))
        .arg("check-names")
        .args(extra)
        .args(["--mods-dir", root.to_str().unwrap()])
        // The sweep must read this directory, never a bucket a developer's shell exports.
        .env_remove("MODS_S3_BUCKET")
        .env_remove("MODS_S3_ENDPOINT")
        .env_remove("MODS_S3_ACCESS_KEY")
        .env_remove("MODS_S3_SECRET_KEY")
        .output()
        .unwrap()
}

fn check_names(root: &Path) -> Output {
    check_names_with(root, &[])
}

/// A claim object as a publish leaves it: `_claims/<hex of the folded folder>.json`, naming the
/// entry that took the folder. `mod_id` need not be an entry, which is what makes it a leftover.
fn write_claim(root: &Path, folder: &str, mod_id: &str) -> PathBuf {
    write_claim_replacing(root, folder, mod_id, None)
}

/// The claim a move writes: the same object, recording the folder the new revision leaves.
fn write_move_claim(root: &Path, folder: &str, mod_id: &str, replaces: &str) -> PathBuf {
    write_claim_replacing(root, folder, mod_id, Some(replaces))
}

fn write_claim_replacing(
    root: &Path,
    folder: &str,
    mod_id: &str,
    replaces: Option<&str>,
) -> PathBuf {
    let directory = root.join("_claims");
    std::fs::create_dir_all(&directory).unwrap();
    let mut name = String::new();
    for byte in folder.to_lowercase().bytes() {
        write!(name, "{byte:02x}").unwrap();
    }
    let path = directory.join(format!("{name}.json"));
    let mut body = serde_json::json!({ "modId": mod_id, "folder": folder });
    if let Some(replaces) = replaces {
        body["replaces"] = serde_json::Value::String(replaces.to_string());
    }
    std::fs::write(&path, body.to_string()).unwrap();
    path
}

/// Backdate a claim the way a crashed publish's leftover ages: the sweep reads the object's
/// modification time, which is also what its `--fix` grace period measures.
fn age_claim(path: &Path, age: Duration) {
    let file = std::fs::File::options().write(true).open(path).unwrap();
    file.set_modified(SystemTime::now() - age).unwrap();
}

#[test]
fn a_clean_catalog_exits_zero() {
    let dir = tempfile::tempdir().unwrap();
    write_entry(dir.path(), "plain-mod", "Plain Mod", None);
    write_entry(dir.path(), "folder-mod", "Folder Mod", Some("@folder-mod"));

    let output = check_names(dir.path());
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(
            "check-names: checked 2 catalog entries, no rejected names, no shared install folders, no unused folder claims, every artifact intact"
        ),
        "stdout: {stdout}"
    );
}

#[test]
fn two_entries_that_install_into_one_folder_are_reported() {
    let dir = tempfile::tempdir().unwrap();
    write_entry(dir.path(), "first-mod", "First Mod", Some("shared"));
    write_entry(dir.path(), "second-mod", "Second Mod", Some("shared"));

    let output = check_names(dir.path());
    assert!(!output.status.success(), "sweep passed unexpectedly");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(
            "install collision: 'first-mod' and 'second-mod' install into the same folder 'shared'"
        ),
        "stdout: {stdout}"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("check-names: 1 install collision in 2 catalog entries"),
        "stderr: {stderr}"
    );
}

// The catalog's own fallback is a folder name too: an entry with no `folderName` installs into
// `@<modId>`, which another entry can claim outright.
#[test]
fn a_folder_name_that_shadows_the_fallback_is_a_collision() {
    let dir = tempfile::tempdir().unwrap();
    write_entry(dir.path(), "alpha", "Alpha", None);
    write_entry(dir.path(), "beta", "Beta", Some("@alpha"));

    let output = check_names(dir.path());
    assert!(!output.status.success(), "sweep passed unexpectedly");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(
            "install collision: 'alpha' and 'beta' install into the same folder '@alpha'"
        ),
        "stdout: {stdout}"
    );
}

// The installer runs on Windows, where two spellings are one directory even though they are two
// store keys. The row keeps both spellings so the operator can see what the catalog says.
#[test]
fn case_only_install_folders_are_a_collision() {
    let dir = tempfile::tempdir().unwrap();
    write_entry(dir.path(), "lower-mod", "Lower Mod", Some("csla"));
    write_entry(dir.path(), "upper-mod", "Upper Mod", Some("CSLA"));

    let output = check_names(dir.path());
    assert!(!output.status.success(), "sweep passed unexpectedly");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(
            "install collision: 'lower-mod' ('csla') and 'upper-mod' ('CSLA') name one folder on a case-insensitive filesystem"
        ),
        "stdout: {stdout}"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("check-names: 1 install collision in 2 catalog entries"),
        "stderr: {stderr}"
    );
}

#[test]
fn a_rejected_folder_name_is_reported_and_fails_the_run() {
    let dir = tempfile::tempdir().unwrap();
    write_entry(
        dir.path(),
        "bad-folder",
        "Bad Folder",
        Some("..\\..\\Documents"),
    );

    let output = check_names(dir.path());
    assert!(!output.status.success(), "sweep passed unexpectedly");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("bad-folder: folderName '..\\..\\Documents' is not one path component"),
        "stdout: {stdout}"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("check-names: 1 rejected name in 1 catalog entry"),
        "stderr: {stderr}"
    );
}

#[test]
fn a_missing_artifact_is_reported_and_fails_the_run() {
    let dir = tempfile::tempdir().unwrap();
    let artifact = write_entry(dir.path(), "broken-mod", "Broken Mod", None);
    std::fs::remove_file(&artifact).unwrap();

    let output = check_names(dir.path());
    assert!(!output.status.success(), "sweep passed unexpectedly");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("broken-mod: revision 1 artifact is missing"),
        "stdout: {stdout}"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("check-names: 1 damaged artifact in 1 catalog entry"),
        "stderr: {stderr}"
    );
}

#[test]
fn an_artifact_that_no_longer_matches_its_sha256_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let artifact = write_entry(dir.path(), "tampered-mod", "Tampered Mod", None);
    let expected = format!("{:x}", Sha256::digest(b"artifact of tampered-mod"));
    let actual = format!("{:x}", Sha256::digest(b"replaced bytes"));
    std::fs::write(&artifact, b"replaced bytes").unwrap();

    let output = check_names(dir.path());
    assert!(!output.status.success(), "sweep passed unexpectedly");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(&format!(
            "tampered-mod: revision 1 artifact sha256 is '{expected}' but the object hashes to '{actual}'"
        )),
        "stdout: {stdout}"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("check-names: 1 damaged artifact in 1 catalog entry"),
        "stderr: {stderr}"
    );
}

#[test]
fn a_claim_that_names_no_catalog_entry_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    write_entry(dir.path(), "alpha", "Alpha", None);
    write_claim(dir.path(), "shared", "ghost");

    let output = check_names(dir.path());
    assert!(!output.status.success(), "sweep passed unexpectedly");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(
            "orphan claim: 'shared' is reserved by 'ghost', which is not a catalog entry (claimed "
        ),
        "stdout: {stdout}"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("check-names: 1 unused claim in 1 catalog entry"),
        "stderr: {stderr}"
    );
}

// The claim is how a live entry holds its folder, so only a claim naming no directory is a
// leftover; a catalog whose claims all name their entries must stay clean.
#[test]
fn a_claim_held_by_a_catalog_entry_is_not_reported() {
    let dir = tempfile::tempdir().unwrap();
    write_entry(dir.path(), "alpha", "Alpha", Some("shared"));
    write_claim(dir.path(), "shared", "alpha");

    let output = check_names(dir.path());
    assert!(
        output.status.success(),
        "stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("no unused folder claims"),
        "stdout: {stdout}"
    );
}

#[test]
fn fixing_removes_an_orphaned_claim_and_passes() {
    let dir = tempfile::tempdir().unwrap();
    write_entry(dir.path(), "alpha", "Alpha", None);
    let claim = write_claim(dir.path(), "shared", "ghost");
    age_claim(&claim, Duration::from_secs(600));

    let output = check_names_with(dir.path(), &["--fix"]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("; removed)"), "stdout: {stdout}");
    assert!(
        stdout.contains("check-names: removed 1 unused install-folder claim"),
        "stdout: {stdout}"
    );
    assert!(!claim.exists(), "the claim object survived --fix");

    // The sweep is clean afterwards, which is the point of the flag.
    assert!(check_names(dir.path()).status.success());
}

#[test]
fn fixing_keeps_a_claim_a_publish_may_still_be_finishing() {
    let dir = tempfile::tempdir().unwrap();
    write_entry(dir.path(), "alpha", "Alpha", None);
    let claim = write_claim(dir.path(), "shared", "ghost");

    let output = check_names_with(dir.path(), &["--fix"]);
    assert!(
        !output.status.success(),
        "a fresh orphan cannot count as fixed"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("; kept, a publish may still be finishing)"),
        "stdout: {stdout}"
    );
    assert!(
        claim.exists(),
        "--fix removed a claim that may belong to a running publish"
    );
}

// MSERVER-CLAIM-LEASE-1: a reservation is taken before the source is packed and held while the
// artifact is compressed and streamed, which can take far longer than the age floor. Its lease is
// the publisher's own deadline, so `--fix` must leave the claim while the lease is still running —
// the fold here is over an hour old, which on age alone would be a leftover.
#[test]
fn fixing_keeps_a_claim_whose_lease_is_still_running() {
    let dir = tempfile::tempdir().unwrap();
    write_entry(dir.path(), "alpha", "Alpha", None);
    let claim = write_claim(dir.path(), "shared", "publish-in-flight");
    let lease_until = i64::try_from(
        SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap()
        + 10 * 60 * 1000;
    let mut body: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&claim).unwrap()).unwrap();
    body["lease_until"] = serde_json::json!(lease_until);
    std::fs::write(&claim, body.to_string()).unwrap();
    age_claim(&claim, Duration::from_secs(60 * 60));

    let output = check_names_with(dir.path(), &["--fix"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("; kept, its reservation is still being renewed)"),
        "stdout: {stdout}"
    );
    assert!(
        claim.exists(),
        "--fix removed a claim a publisher is still renewing"
    );
}

// MSERVER-CLAIM-KEEP-1: a revision that keeps its folder reserves it too, so a publisher that
// reserved and then stopped renewing leaves a token on the claim of the folder the entry installs
// into. The catalog holds the folder without that claim, and the deadline says the publisher is
// gone, so `--fix` can remove it at once — while a live lease is still a publish that must not be
// touched.
#[test]
fn fixing_removes_a_lapsed_reservation_on_an_installed_folder() {
    let dir = tempfile::tempdir().unwrap();
    write_entry(dir.path(), "alpha", "Alpha", Some("alpha-dir"));
    let claim = write_claim(dir.path(), "alpha-dir", "alpha");
    let mut body: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&claim).unwrap()).unwrap();
    body["token"] = serde_json::json!("f0e1d2c3");
    let expired = i64::try_from(
        SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap()
        - 1;
    body["lease_until"] = serde_json::json!(expired);
    std::fs::write(&claim, body.to_string()).unwrap();

    let output = check_names(dir.path());
    assert!(!output.status.success(), "sweep passed unexpectedly");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("lapsed reservation: 'alpha-dir' is reserved for 'alpha', whose reservation stopped renewing (claimed "),
        "stdout: {stdout}"
    );
    assert!(claim.exists(), "a report-only sweep deleted the claim");

    let output = check_names_with(dir.path(), &["--fix"]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("; removed)"), "stdout: {stdout}");
    assert!(
        stdout.contains("check-names: removed 1 unused install-folder claim"),
        "stdout: {stdout}"
    );
    assert!(!claim.exists(), "the lapsed reservation survived --fix");
    assert!(check_names(dir.path()).status.success());
}

// A revision that moves an entry takes the new folder's claim before the revision is current and
// releases the old one only after it; a release that failed leaves the old folder held by an entry
// that now installs elsewhere. The orphan rule cannot see it — the entry is there — so it has its
// own row.
#[test]
fn a_claim_on_the_folder_an_entry_moved_off_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    write_entry(dir.path(), "alpha", "Alpha", Some("alpha-new"));
    write_claim(dir.path(), "alpha-dir", "alpha");

    let output = check_names(dir.path());
    assert!(!output.status.success(), "sweep passed unexpectedly");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(
            "stale claim: 'alpha-dir' is reserved by 'alpha', which now installs into 'alpha-new' (claimed "
        ),
        "stdout: {stdout}"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("check-names: 1 unused claim in 1 catalog entry"),
        "stderr: {stderr}"
    );
}

#[test]
fn fixing_removes_a_claim_on_the_folder_the_entry_left() {
    // The claim records no move onto `alpha-dir`, so no publish can still need it: the catalog
    // already installs into `alpha-new`. That is a decision, not a guess about age, so the claim
    // is fresh here and `--fix` removes it anyway.
    let dir = tempfile::tempdir().unwrap();
    write_entry(dir.path(), "alpha", "Alpha", Some("alpha-new"));
    let stale = write_claim(dir.path(), "alpha-dir", "alpha");

    let output = check_names_with(dir.path(), &["--fix"]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("; removed)"), "stdout: {stdout}");
    assert!(
        stdout.contains("check-names: removed 1 unused install-folder claim"),
        "stdout: {stdout}"
    );
    assert!(!stale.exists(), "the stale claim survived --fix");
    assert!(check_names(dir.path()).status.success());
}

// The other half of a move: the claim on the folder it is moving to, written before the new
// revision is current. It names the entry's current folder as the one it replaces, which says a
// publish is still working — the one leftover whose age has to decide.
#[test]
fn fixing_keeps_a_fresh_move_that_has_not_landed() {
    let dir = tempfile::tempdir().unwrap();
    write_entry(dir.path(), "alpha", "Alpha", Some("alpha-dir"));
    let pending = write_move_claim(dir.path(), "alpha-new", "alpha", "alpha-dir");

    let output = check_names_with(dir.path(), &["--fix"]);
    assert!(
        !output.status.success(),
        "a move in flight cannot count as fixed"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(
            "pending move: 'alpha-new' is reserved for 'alpha', which still installs into 'alpha-dir' (claimed "
        ),
        "stdout: {stdout}"
    );
    assert!(
        stdout.contains("; kept, a publish may still be finishing)"),
        "stdout: {stdout}"
    );
    assert!(
        pending.exists(),
        "--fix removed the destination of a move that has not landed"
    );
}

#[test]
fn the_summary_counts_rejected_names_and_damaged_artifacts_together() {
    let dir = tempfile::tempdir().unwrap();
    write_entry(dir.path(), "bad-folder", "Bad Folder", Some("a*b"));
    let artifact = write_entry(dir.path(), "broken-mod", "Broken Mod", None);
    std::fs::remove_file(&artifact).unwrap();

    let output = check_names(dir.path());
    assert!(!output.status.success(), "sweep passed unexpectedly");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("check-names: 1 rejected name, 1 damaged artifact in 2 catalog entries"),
        "stderr: {stderr}"
    );
}

// A directory whose name the rule rejects cannot exist on Windows (`*`, a trailing dot and a
// trailing space are all illegal there), so this case runs where the layout can be produced.
#[cfg(not(windows))]
#[test]
fn a_rejected_directory_name_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    write_entry(dir.path(), "a*b", "Bad Id", None);

    let output = check_names(dir.path());
    assert!(!output.status.success(), "sweep passed unexpectedly");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("a*b: mod_id 'a*b' is not one path component"),
        "stdout: {stdout}"
    );
}
