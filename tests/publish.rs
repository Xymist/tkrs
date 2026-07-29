//! Integration tests for `tk publish github`.
//!
//! `gh` is shadowed by a small POSIX-sh script installed into a per-test
//! `fakebin/` directory that is prepended to `PATH`. The script records every
//! invocation's argv (one arg per line, terminated by a sentinel) to the file
//! named by `GH_ARGV_LOG`, and its behaviour is steered by env vars:
//!   GH_ISSUE_URL   — URL printed for `gh issue create` (defaults to /issues/999)
//!   GH_FAIL        — when set, `gh issue create` prints to stderr and exits 1
//!   GH_BODY_CAPTURE— when set, the `--body-file` contents are copied here
//! `gh api graphql` calls (the best-effort priority path) succeed with an
//! empty payload so they never fail the run.

use assert_cmd::{Command, cargo::cargo_bin_cmd};
use assert_fs::TempDir;
use predicates::prelude::*;
use std::fs;
use std::path::{Path, PathBuf};

const ISSUE_URL: &str = "https://github.com/example/repo/issues/999";

/// A non-dotted working directory under the temp root. assert_fs names its
/// temp dirs `.tmpXXXX`; running `tk` there would derive dotted ticket ids
/// (`.tm-...`) whose leading dot defeats the word-boundary leak checks. A
/// plain `work/` subdirectory yields ordinary ids.
fn work_dir(temp: &TempDir) -> PathBuf {
    let w = temp.path().join("work");
    fs::create_dir_all(&w).unwrap();
    w
}

fn tickets_dir(temp: &TempDir) -> PathBuf {
    work_dir(temp).join(".tickets")
}

fn tk_cmd(temp: &TempDir) -> Command {
    tk_cmd_in(&work_dir(temp))
}

fn tk_cmd_in(dir: &Path) -> Command {
    let mut cmd = cargo_bin_cmd!("tk");
    cmd.current_dir(dir);
    // Keep store resolution inside the fixture: a real HOME or TICKETS_DIR
    // would send writes to the developer's own store.
    cmd.env("HOME", dir.join("__home"));
    cmd.env_remove("TICKETS_DIR");
    cmd
}

fn create_ticket(temp: &TempDir, title: &str) -> String {
    create_ticket_in(&work_dir(temp), title)
}

fn create_ticket_in(dir: &Path, title: &str) -> String {
    let out = tk_cmd_in(dir).arg("create").arg(title).output().unwrap();
    assert!(out.status.success(), "create failed for {title}");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn create_ticket_with_desc(temp: &TempDir, title: &str, desc: &str) -> String {
    create_ticket_with_desc_in(&work_dir(temp), title, desc)
}

fn create_ticket_with_desc_in(dir: &Path, title: &str, desc: &str) -> String {
    let out = tk_cmd_in(dir)
        .arg("create")
        .arg(title)
        .arg("-d")
        .arg(desc)
        .output()
        .unwrap();
    assert!(out.status.success(), "create failed for {title}");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn ticket_json(temp: &TempDir, id: &str) -> serde_json::Value {
    let out = tk_cmd(temp)
        .arg("show")
        .arg(id)
        .arg("--json")
        .output()
        .unwrap();
    assert!(out.status.success(), "show --json failed for {id}");
    serde_json::from_slice(&out.stdout).unwrap()
}

/// Writes an executable `gh` shim into `<temp>/fakebin/` and returns that
/// directory so callers can prepend it to `PATH`.
fn install_fake_gh(temp: &TempDir) -> PathBuf {
    let bin = temp.path().join("fakebin");
    fs::create_dir_all(&bin).unwrap();
    let script = r#"#!/bin/sh
{
  for a in "$@"; do printf '%s\n' "$a"; done
  printf -- '---INVOCATION-END---\n'
} >> "$GH_ARGV_LOG"

if [ "$1" = "api" ]; then
  printf '{"data":{}}\n'
  exit 0
fi

if [ "$1" = "issue" ] && [ "$2" = "create" ]; then
  if [ -n "$GH_BODY_CAPTURE" ]; then
    prev=""
    for a in "$@"; do
      if [ "$prev" = "--body-file" ]; then
        cat "$a" > "$GH_BODY_CAPTURE"
      fi
      prev="$a"
    done
  fi
  if [ -n "$GH_FAIL" ]; then
    printf 'gh: HTTP 422 could not create issue\n' 1>&2
    exit 1
  fi
  if [ -n "$GH_ISSUE_URL" ]; then
    printf '%s\n' "$GH_ISSUE_URL"
  else
    printf '%s\n' "https://github.com/example/repo/issues/999"
  fi
  exit 0
fi
exit 0
"#;
    let gh = bin.join("gh");
    fs::write(&gh, script).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&gh, fs::Permissions::from_mode(0o755)).unwrap();
    }
    bin
}

/// Prepends `bin` to `PATH` and points the shim at `argv_log`.
fn with_fake_gh(cmd: &mut Command, bin: &Path, argv_log: &Path) {
    let existing = std::env::var("PATH").unwrap_or_default();
    cmd.env("PATH", format!("{}:{}", bin.display(), existing));
    cmd.env("GH_ARGV_LOG", argv_log);
}

// -- default dry-run rendering ---------------------------------------------

#[test]
fn dry_run_lists_all_maintenance_headings_in_order() {
    let temp = TempDir::new().unwrap();
    let id = create_ticket_with_desc(&temp, "Improve widget rendering", "The widget flickers");

    let out = tk_cmd(&temp)
        .arg("publish")
        .arg("github")
        .arg(&id)
        .arg("acme/widgets")
        .arg("--dry-run")
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();

    assert!(
        stdout.contains("TITLE: [Maintenance]: Improve widget rendering"),
        "title prefixed and printed: {stdout}"
    );

    let headings = [
        "### Describe the change required",
        "### List the success/acceptance criteria for this ticket",
        "### How many hours of developer time has this lost us?",
        "### What happens if we don't do this?",
        "### How easy is this to change or fix?",
        "### Which module(s) would making this change affect?",
    ];
    let mut last = 0usize;
    for heading in headings {
        let at = stdout
            .find(heading)
            .unwrap_or_else(|| panic!("missing heading '{heading}' in:\n{stdout}"));
        assert!(at >= last, "heading '{heading}' out of order in:\n{stdout}");
        last = at;
    }
}

#[test]
fn dry_run_creates_nothing_and_needs_no_gh() {
    let temp = TempDir::new().unwrap();
    let id = create_ticket(&temp, "Nothing created");

    tk_cmd(&temp)
        .arg("publish")
        .arg("github")
        .arg(&id)
        .arg("acme/widgets")
        .arg("--dry-run")
        .assert()
        .success();

    // The ticket is untouched: no external_ref pinned by a dry run.
    assert!(ticket_json(&temp, &id)["external_ref"].is_null());
}

// -- custom fields spec -----------------------------------------------------

#[test]
fn dry_run_fields_json_uses_custom_headings_and_title_prefix() {
    let temp = TempDir::new().unwrap();
    let id = create_ticket_with_desc(&temp, "Custom form", "Body describing the change");

    let fields = temp.path().join("fields.json");
    fs::write(
        &fields,
        r#"[
            {"label": "Overview", "source": "describe"},
            {"label": "Extra detail", "value": "static text"}
        ]"#,
    )
    .unwrap();

    let out = tk_cmd(&temp)
        .arg("publish")
        .arg("github")
        .arg(&id)
        .arg("acme/widgets")
        .arg("--dry-run")
        .arg("--fields-json")
        .arg(&fields)
        .arg("--title-prefix")
        .arg("[Bug]: ")
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();

    assert!(stdout.contains("TITLE: [Bug]: Custom form"), "{stdout}");
    assert!(stdout.contains("### Overview"), "{stdout}");
    assert!(stdout.contains("### Extra detail"), "{stdout}");
    assert!(stdout.contains("static text"), "{stdout}");
    assert!(
        !stdout.contains("### Describe the change required"),
        "default headings must be gone: {stdout}"
    );
}

#[test]
fn fields_json_invalid_spec_fails_with_message() {
    let temp = TempDir::new().unwrap();
    let id = create_ticket(&temp, "Bad spec");

    let fields = temp.path().join("fields.json");
    fs::write(&fields, "[]").unwrap();

    tk_cmd(&temp)
        .arg("publish")
        .arg("github")
        .arg(&id)
        .arg("acme/widgets")
        .arg("--dry-run")
        .arg("--fields-json")
        .arg(&fields)
        .assert()
        .failure()
        .stderr(predicate::str::contains("non-empty JSON array"));
}

// -- full create flow -------------------------------------------------------

#[test]
fn create_flow_pins_external_ref_and_passes_expected_gh_args() {
    let temp = TempDir::new().unwrap();
    let id = create_ticket_with_desc(&temp, "Fix the flaky test", "The suite is flaky");

    let bin = install_fake_gh(&temp);
    let argv_log = temp.path().join("gh_argv.log");

    let mut cmd = tk_cmd(&temp);
    with_fake_gh(&mut cmd, &bin, &argv_log);
    let out = cmd
        .arg("publish")
        .arg("github")
        .arg(&id)
        .arg("acme/widgets")
        .arg("--assignee")
        .arg("@me")
        .arg("--label")
        .arg("bug")
        .arg("--label")
        .arg("chore")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(stdout.contains(ISSUE_URL), "URL echoed: {stdout}");
    assert!(stdout.contains("pinned"), "pin confirmed: {stdout}");
    assert!(stdout.contains("gh-999"), "ref printed: {stdout}");

    assert_eq!(ticket_json(&temp, &id)["external_ref"], "gh-999");

    let log = fs::read_to_string(&argv_log).unwrap();
    for token in [
        "issue",
        "create",
        "--repo",
        "acme/widgets",
        "--title",
        "[Maintenance]: Fix the flaky test",
        "--assignee",
        "@me",
        "--label",
        "bug",
        "chore",
    ] {
        assert!(log.contains(token), "gh argv missing '{token}':\n{log}");
    }
    // The best-effort priority path shells out to `gh api graphql`.
    assert!(log.contains("graphql"), "priority attempt logged:\n{log}");
}

// -- pinned-ticket idempotency ---------------------------------------------

#[test]
fn pinned_ticket_refuses_without_reflag_before_gh_runs() {
    let temp = TempDir::new().unwrap();
    let id = create_ticket(&temp, "Already filed");
    tk_cmd(&temp)
        .arg("edit")
        .arg(&id)
        .arg("--external-ref")
        .arg("gh-100")
        .assert()
        .success();

    let bin = install_fake_gh(&temp);
    let argv_log = temp.path().join("gh_argv.log");

    let mut cmd = tk_cmd(&temp);
    with_fake_gh(&mut cmd, &bin, &argv_log);
    cmd.arg("publish")
        .arg("github")
        .arg(&id)
        .arg("acme/widgets")
        .assert()
        .failure()
        .stderr(predicate::str::contains("already pinned"))
        .stderr(predicate::str::contains("--re-file"));

    assert!(!argv_log.exists(), "gh must not have been invoked");
    assert_eq!(ticket_json(&temp, &id)["external_ref"], "gh-100");
}

#[test]
fn pinned_ticket_dry_run_prints_note_instead_of_refusing() {
    let temp = TempDir::new().unwrap();
    let id = create_ticket(&temp, "Pinned dry run");
    tk_cmd(&temp)
        .arg("edit")
        .arg(&id)
        .arg("--external-ref")
        .arg("gh-100")
        .assert()
        .success();

    let out = tk_cmd(&temp)
        .arg("publish")
        .arg("github")
        .arg(&id)
        .arg("acme/widgets")
        .arg("--dry-run")
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(stdout.contains("already pinned to gh-100"), "{stdout}");
    assert!(stdout.contains("--re-file"), "{stdout}");

    assert_eq!(ticket_json(&temp, &id)["external_ref"], "gh-100");
}

#[test]
fn re_file_creates_and_repins() {
    let temp = TempDir::new().unwrap();
    let id = create_ticket(&temp, "Refile me");
    tk_cmd(&temp)
        .arg("edit")
        .arg(&id)
        .arg("--external-ref")
        .arg("gh-100")
        .assert()
        .success();

    let bin = install_fake_gh(&temp);
    let argv_log = temp.path().join("gh_argv.log");

    let mut cmd = tk_cmd(&temp);
    with_fake_gh(&mut cmd, &bin, &argv_log);
    cmd.arg("publish")
        .arg("github")
        .arg(&id)
        .arg("acme/widgets")
        .arg("--re-file")
        .assert()
        .success();

    assert!(argv_log.exists(), "gh should have been invoked");
    assert_eq!(ticket_json(&temp, &id)["external_ref"], "gh-999");
}

#[test]
fn re_file_with_no_pin_keeps_old_ref() {
    let temp = TempDir::new().unwrap();
    let id = create_ticket(&temp, "Refile no pin");
    tk_cmd(&temp)
        .arg("edit")
        .arg(&id)
        .arg("--external-ref")
        .arg("gh-200")
        .assert()
        .success();

    let bin = install_fake_gh(&temp);
    let argv_log = temp.path().join("gh_argv.log");

    let mut cmd = tk_cmd(&temp);
    with_fake_gh(&mut cmd, &bin, &argv_log);
    cmd.arg("publish")
        .arg("github")
        .arg(&id)
        .arg("acme/widgets")
        .arg("--re-file")
        .arg("--no-pin")
        .assert()
        .success();

    assert!(argv_log.exists(), "gh should have been invoked");
    assert_eq!(
        ticket_json(&temp, &id)["external_ref"],
        "gh-200",
        "pin must be untouched with --no-pin"
    );
}

// -- leak enforcement -------------------------------------------------------

#[test]
fn body_referencing_another_store_id_hard_fails_without_gh() {
    let temp = TempDir::new().unwrap();
    let other = create_ticket(&temp, "Other ticket");
    let id = create_ticket_with_desc(&temp, "Main ticket", &format!("Depends on {other} landing"));

    let bin = install_fake_gh(&temp);
    let argv_log = temp.path().join("gh_argv.log");

    let mut cmd = tk_cmd(&temp);
    with_fake_gh(&mut cmd, &bin, &argv_log);
    cmd.arg("publish")
        .arg("github")
        .arg(&id)
        .arg("acme/widgets")
        .assert()
        .failure()
        .stderr(predicate::str::contains("references local tk ids"))
        .stderr(predicate::str::contains(other.as_str()));

    assert!(!argv_log.exists(), "gh must not have been invoked");
    assert!(ticket_json(&temp, &id)["external_ref"].is_null());
}

#[test]
fn body_file_containing_source_id_hard_fails_without_gh() {
    let temp = TempDir::new().unwrap();
    let id = create_ticket(&temp, "Body file leak");

    let body = temp.path().join("body.md");
    fs::write(
        &body,
        format!("This body mentions {id} directly, which leaks."),
    )
    .unwrap();

    let bin = install_fake_gh(&temp);
    let argv_log = temp.path().join("gh_argv.log");

    let mut cmd = tk_cmd(&temp);
    with_fake_gh(&mut cmd, &bin, &argv_log);
    cmd.arg("publish")
        .arg("github")
        .arg(&id)
        .arg("acme/widgets")
        .arg("--body-file")
        .arg(&body)
        .assert()
        .failure()
        .stderr(predicate::str::contains("tk ids must never appear"));

    assert!(!argv_log.exists(), "gh must not have been invoked");
    assert!(ticket_json(&temp, &id)["external_ref"].is_null());
}

// -- body-file handling -----------------------------------------------------

#[test]
fn body_file_empty_is_rejected() {
    let temp = TempDir::new().unwrap();
    let id = create_ticket(&temp, "Empty body file");

    let body = temp.path().join("body.md");
    fs::write(&body, "   \n\t\n").unwrap();

    tk_cmd(&temp)
        .arg("publish")
        .arg("github")
        .arg(&id)
        .arg("acme/widgets")
        .arg("--body-file")
        .arg(&body)
        .assert()
        .failure()
        .stderr(predicate::str::contains("is empty"));
}

#[test]
fn body_file_used_verbatim_in_dry_run() {
    let temp = TempDir::new().unwrap();
    let id = create_ticket(&temp, "Verbatim dry run");

    let body = temp.path().join("body.md");
    fs::write(&body, "Custom verbatim body\n\nSecond paragraph").unwrap();

    let out = tk_cmd(&temp)
        .arg("publish")
        .arg("github")
        .arg(&id)
        .arg("acme/widgets")
        .arg("--body-file")
        .arg(&body)
        .arg("--dry-run")
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(stdout.contains("Custom verbatim body"), "{stdout}");
    assert!(stdout.contains("Second paragraph"), "{stdout}");
    assert!(
        !stdout.contains("### Describe the change required"),
        "rendered fields must be replaced by the body file: {stdout}"
    );
}

#[test]
fn body_file_passed_to_gh_verbatim() {
    let temp = TempDir::new().unwrap();
    let id = create_ticket(&temp, "Verbatim to gh");

    let body = temp.path().join("body.md");
    fs::write(&body, "Verbatim body for the gh capture").unwrap();

    let bin = install_fake_gh(&temp);
    let argv_log = temp.path().join("gh_argv.log");
    let capture = temp.path().join("captured_body.md");

    let mut cmd = tk_cmd(&temp);
    with_fake_gh(&mut cmd, &bin, &argv_log);
    cmd.env("GH_BODY_CAPTURE", &capture)
        .arg("publish")
        .arg("github")
        .arg(&id)
        .arg("acme/widgets")
        .arg("--body-file")
        .arg(&body)
        .assert()
        .success();

    let captured = fs::read_to_string(&capture).unwrap();
    assert_eq!(captured.trim(), "Verbatim body for the gh capture");
}

// -- lock behaviour ---------------------------------------------------------

#[test]
fn publish_lockfile_never_appears_in_listings() {
    let temp = TempDir::new().unwrap();
    let id = create_ticket(&temp, "Visible ticket");

    // Simulate the lockfile the publish path leaves behind.
    fs::write(tickets_dir(&temp).join(".publish.lock"), "").unwrap();

    tk_cmd(&temp)
        .arg("ls")
        .assert()
        .success()
        .stdout(predicate::str::contains(&id))
        .stdout(predicate::str::contains("publish.lock").not());

    tk_cmd(&temp)
        .arg("query")
        .assert()
        .success()
        .stdout(predicate::str::contains("publish.lock").not());
}

#[test]
fn concurrent_publish_lock_is_held_across_processes() {
    use fd_lock::RwLock;

    let temp = TempDir::new().unwrap();
    let id = create_ticket_with_desc(&temp, "Locked out", "body");

    let bin = install_fake_gh(&temp);
    let argv_log = temp.path().join("gh_argv.log");

    let lock_path = tickets_dir(&temp).join(".publish.lock");
    let file = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&lock_path)
        .unwrap();
    let mut lock = RwLock::new(file);
    let guard = lock.try_write().expect("test acquires the publish lock");

    // A separate `tk publish` process must fail closed while we hold the lock.
    let mut blocked = tk_cmd(&temp);
    with_fake_gh(&mut blocked, &bin, &argv_log);
    blocked
        .arg("publish")
        .arg("github")
        .arg(&id)
        .arg("acme/widgets")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "another tk publish is in progress",
        ));
    assert!(
        !argv_log.exists(),
        "gh must not run while lock is contended"
    );

    drop(guard);

    // With the lock released, the retry proceeds and creates the issue.
    let mut retry = tk_cmd(&temp);
    with_fake_gh(&mut retry, &bin, &argv_log);
    retry
        .arg("publish")
        .arg("github")
        .arg(&id)
        .arg("acme/widgets")
        .assert()
        .success();
    assert_eq!(ticket_json(&temp, &id)["external_ref"], "gh-999");
}

#[test]
fn lock_is_released_after_gh_failure() {
    let temp = TempDir::new().unwrap();
    let id = create_ticket_with_desc(&temp, "Fail then retry", "body");

    let bin = install_fake_gh(&temp);
    let argv_log = temp.path().join("gh_argv.log");

    // First run: gh create fails; the advisory lock must be released on the
    // error path so a subsequent run is not blocked.
    let mut failing = tk_cmd(&temp);
    with_fake_gh(&mut failing, &bin, &argv_log);
    failing
        .env("GH_FAIL", "1")
        .arg("publish")
        .arg("github")
        .arg(&id)
        .arg("acme/widgets")
        .assert()
        .failure();

    let mut retry = tk_cmd(&temp);
    with_fake_gh(&mut retry, &bin, &argv_log);
    retry
        .arg("publish")
        .arg("github")
        .arg(&id)
        .arg("acme/widgets")
        .assert()
        .success()
        .stderr(predicate::str::contains("another tk publish is in progress").not());

    assert_eq!(ticket_json(&temp, &id)["external_ref"], "gh-999");
}

// -- gh failure propagation -------------------------------------------------

#[test]
fn gh_create_failure_is_surfaced_and_ticket_unmodified() {
    let temp = TempDir::new().unwrap();
    let id = create_ticket_with_desc(&temp, "Surface failure", "body");

    let bin = install_fake_gh(&temp);
    let argv_log = temp.path().join("gh_argv.log");

    let mut cmd = tk_cmd(&temp);
    with_fake_gh(&mut cmd, &bin, &argv_log);
    cmd.env("GH_FAIL", "1")
        .arg("publish")
        .arg("github")
        .arg(&id)
        .arg("acme/widgets")
        .assert()
        .failure()
        .stderr(predicate::str::contains("gh issue create failed"))
        .stderr(predicate::str::contains("HTTP 422"));

    assert!(ticket_json(&temp, &id)["external_ref"].is_null());
}

// -- dry-run leak downgrade --------------------------------------------------

#[test]
fn dry_run_downgrades_leak_to_warning_and_still_renders_body() {
    let temp = TempDir::new().unwrap();
    let other = create_ticket(&temp, "Other ticket");
    let id = create_ticket_with_desc(
        &temp,
        "Leaky ticket",
        &format!("References {other} directly"),
    );

    let out = tk_cmd(&temp)
        .arg("publish")
        .arg("github")
        .arg(&id)
        .arg("acme/widgets")
        .arg("--dry-run")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(
        out.status.success(),
        "dry-run must still exit 0 despite the leak: stderr={stderr}"
    );
    assert!(
        stdout.contains("TITLE:") && stdout.contains(other.as_str()),
        "the full body, including the leaking reference, must still render: {stdout}"
    );
    assert!(
        stderr.contains("error-on-create") && stderr.contains(other.as_str()),
        "the would-be hard failure must be downgraded to a warning: {stderr}"
    );
}

#[test]
fn real_run_still_hard_fails_on_the_same_leak() {
    let temp = TempDir::new().unwrap();
    let other = create_ticket(&temp, "Other ticket");
    let id = create_ticket_with_desc(
        &temp,
        "Leaky ticket",
        &format!("References {other} directly"),
    );

    let bin = install_fake_gh(&temp);
    let argv_log = temp.path().join("gh_argv.log");

    let mut cmd = tk_cmd(&temp);
    with_fake_gh(&mut cmd, &bin, &argv_log);
    cmd.arg("publish")
        .arg("github")
        .arg(&id)
        .arg("acme/widgets")
        .assert()
        .failure()
        .stderr(predicate::str::contains("references local tk ids"));

    assert!(!argv_log.exists(), "gh must not run on a real-run leak");
}

// -- fresh re-read before creating/pinning -----------------------------------

#[test]
fn publish_refuses_when_external_ref_written_directly_to_disk() {
    let temp = TempDir::new().unwrap();
    let id = create_ticket_with_desc(&temp, "Pinned behind our back", "body");

    // Simulate the ticket gaining a pin through a path that never goes
    // through `tk` at all (e.g. a concurrent actor), so the only way
    // publish can see it is by re-reading the ticket file fresh rather
    // than trusting an earlier in-memory snapshot.
    let path = tickets_dir(&temp).join(format!("{id}.md"));
    let contents = fs::read_to_string(&path).unwrap();
    assert!(
        !contents.contains("external_ref"),
        "ticket must start unpinned: {contents}"
    );
    let mut parts = contents.splitn(3, "---\n");
    let _leading = parts.next().unwrap();
    let frontmatter = parts.next().unwrap();
    let rest = parts.next().unwrap();
    let patched = format!("---\n{frontmatter}external_ref: gh-777\n---\n{rest}");
    fs::write(&path, patched).unwrap();

    let bin = install_fake_gh(&temp);
    let argv_log = temp.path().join("gh_argv.log");

    let mut cmd = tk_cmd(&temp);
    with_fake_gh(&mut cmd, &bin, &argv_log);
    cmd.arg("publish")
        .arg("github")
        .arg(&id)
        .arg("acme/widgets")
        .assert()
        .failure()
        .stderr(predicate::str::contains("already pinned to gh-777"))
        .stderr(predicate::str::contains("--re-file"));

    assert!(
        !argv_log.exists(),
        "gh must not run: the fresh re-read must catch the pin before create"
    );
}

// -- punctuation-prefixed ids -------------------------------------------------

#[test]
fn dotted_working_dir_id_leak_is_caught_by_boundary_scan() {
    // A dotted, hyphen-free directory name makes `generate_id` derive a
    // leading-dot id (e.g. `.do-1a2b3`, from the first three characters of
    // the directory name). A regex `\b` boundary can never match
    // immediately before a non-word character like `.` when it is itself
    // preceded by another non-word character (ordinary prose whitespace),
    // so the old word-boundary leak check would silently miss ids shaped
    // like this; the neighbour-character scan must not.
    let temp = TempDir::new().unwrap();
    let dotted = temp.path().join(".dotted");
    fs::create_dir_all(&dotted).unwrap();

    let leaking_id = create_ticket_in(&dotted, "Dotted id ticket");
    assert!(
        leaking_id.starts_with('.'),
        "expected a dotted id, got {leaking_id}"
    );

    let referencing_id = create_ticket_with_desc_in(
        &dotted,
        "References the dotted ticket",
        &format!("Depends on {leaking_id} landing first"),
    );

    let bin = install_fake_gh(&temp);
    let argv_log = temp.path().join("gh_argv.log");

    let mut cmd = tk_cmd_in(&dotted);
    with_fake_gh(&mut cmd, &bin, &argv_log);
    cmd.arg("publish")
        .arg("github")
        .arg(&referencing_id)
        .arg("acme/widgets")
        .assert()
        .failure()
        .stderr(predicate::str::contains("references local tk ids"))
        .stderr(predicate::str::contains(leaking_id.as_str()));

    assert!(!argv_log.exists(), "gh must not run on a leak");
}
