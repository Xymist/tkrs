use assert_cmd::{Command, cargo::cargo_bin_cmd};
use assert_fs::TempDir;
use predicates::prelude::*;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::time::{Duration, SystemTime};

fn tk_cmd(dir: &TempDir) -> Command {
    let mut cmd = cargo_bin_cmd!("ticket");
    cmd.current_dir(dir.path());
    cmd
}

fn write_ticket(dir: &TempDir, id: &str, links: &[&str]) {
    let tickets_dir = dir.path().join(".tickets");
    fs::create_dir_all(&tickets_dir).unwrap();
    let path = dir.path().join(".tickets").join(format!("{id}.md"));
    let mut file = fs::File::create(&path).unwrap();
    writeln!(file, "---").unwrap();
    writeln!(file, "id: {id}").unwrap();
    writeln!(file, "status: open").unwrap();
    writeln!(file, "deps: []").unwrap();
    writeln!(file, "links: [{}]", links.join(", ")).unwrap();
    writeln!(file, "created: 2026-01-01T00:00:00Z").unwrap();
    writeln!(file, "type: task").unwrap();
    writeln!(file, "priority: 2").unwrap();
    writeln!(file, "---").unwrap();
    writeln!(file, "# {id}").unwrap();
}

fn assert_links(path: impl AsRef<Path>, expected: &[&str]) {
    let contents = fs::read_to_string(path).unwrap();
    let needle = format!("links: [{}]", expected.join(", "));
    assert!(
        contents.contains(&needle),
        "expected links line with {needle}\n{contents}"
    );
}

#[test]
fn help_prints_usage_and_commands() {
    let temp = TempDir::new().unwrap();

    tk_cmd(&temp)
        .arg("help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage: tk"))
        .stdout(predicate::str::contains("create"))
        .stdout(predicate::str::contains("Tickets stored as markdown"));
}

#[test]
fn create_writes_ticket_with_defaults() {
    let temp = TempDir::new().unwrap();
    let mut cmd = tk_cmd(&temp);

    let assert = cmd.arg("create").arg("My title").assert().success();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout)
        .trim()
        .to_string();
    assert!(!stdout.is_empty(), "id printed");

    let ticket_path = temp.path().join(".tickets").join(format!("{stdout}.md"));
    assert!(ticket_path.exists(), "ticket file created");

    let contents = fs::read_to_string(ticket_path).unwrap();
    assert!(contents.contains("status: open"));
    assert!(contents.contains("# My title"));
}

#[test]
fn create_requires_title_argument() {
    let temp = TempDir::new().unwrap();

    tk_cmd(&temp)
        .arg("create")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Usage: tk create <TITLE>"));
}

#[test]
fn create_rejects_empty_title() {
    let temp = TempDir::new().unwrap();

    tk_cmd(&temp)
        .arg("create")
        .arg("   ")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Title is required"));
}

#[test]
fn create_rejects_invalid_tag_characters() {
    let temp = TempDir::new().unwrap();

    tk_cmd(&temp)
        .arg("create")
        .arg("Bad tags")
        .arg("--tags")
        .arg("alpha [bracket]")
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid tag"));
}

#[test]
fn create_supports_body_from_file() {
    let temp = TempDir::new().unwrap();
    let body_path = temp.path().join("body.txt");
    fs::write(&body_path, "Custom body\nLine 2").unwrap();

    let out = tk_cmd(&temp)
        .arg("create")
        .arg("From file")
        .arg("--body-from-file")
        .arg(&body_path)
        .output()
        .unwrap();
    assert!(out.status.success());
    let id = String::from_utf8_lossy(&out.stdout).trim().to_string();

    let contents =
        fs::read_to_string(temp.path().join(".tickets").join(format!("{id}.md"))).unwrap();
    assert!(contents.contains("# From file"));
    assert!(contents.contains("Custom body"));
    assert!(contents.contains("Line 2"));
}

#[test]
fn create_supports_template_substitution() {
    let temp = TempDir::new().unwrap();
    let template_path = temp.path().join("tpl.md");
    let mut tpl = fs::File::create(&template_path).unwrap();
    writeln!(tpl, "# {{title}}\nCreated: {{created}}\nID: {{id}}\n").unwrap();

    let out = tk_cmd(&temp)
        .arg("create")
        .arg("Tpl")
        .arg("--template")
        .arg(&template_path)
        .output()
        .unwrap();
    assert!(out.status.success());
    let id = String::from_utf8_lossy(&out.stdout).trim().to_string();

    let contents =
        fs::read_to_string(temp.path().join(".tickets").join(format!("{id}.md"))).unwrap();
    assert!(contents.contains(&format!("ID: {id}")));
    assert!(contents.contains("Created:"));
    assert!(contents.contains("# Tpl"));
}

#[test]
fn start_sets_status_in_progress() {
    let temp = TempDir::new().unwrap();
    let mut create = tk_cmd(&temp);
    let output = create
        .arg("create")
        .arg("My title")
        .output()
        .expect("create runs");
    assert!(output.status.success());
    let id = String::from_utf8_lossy(&output.stdout).trim().to_string();

    let mut start = tk_cmd(&temp);
    start.arg("start").arg(&id).assert().success();

    let ticket_path = temp.path().join(".tickets").join(format!("{id}.md"));
    let contents = fs::read_to_string(ticket_path).unwrap();
    assert!(contents.contains("status: in_progress"));
}

#[test]
fn close_sets_status_closed() {
    let temp = TempDir::new().unwrap();
    let mut create = tk_cmd(&temp);
    let output = create
        .arg("create")
        .arg("My title")
        .output()
        .expect("create runs");
    assert!(output.status.success());
    let id = String::from_utf8_lossy(&output.stdout).trim().to_string();

    let mut close = tk_cmd(&temp);
    close.arg("close").arg(&id).assert().success();

    let ticket_path = temp.path().join(".tickets").join(format!("{id}.md"));
    let contents = fs::read_to_string(ticket_path).unwrap();
    assert!(contents.contains("status: closed"));
}

#[test]
fn close_sets_closed_at_and_appends_note_once() {
    let temp = TempDir::new().unwrap();

    let id = {
        let mut create = tk_cmd(&temp);
        let out = create.arg("create").arg("Close Me").output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };

    // First close with note
    tk_cmd(&temp)
        .arg("close")
        .arg(&id)
        .arg("--note")
        .arg("done")
        .assert()
        .success();

    let path = temp.path().join(".tickets").join(format!("{id}.md"));
    let contents = fs::read_to_string(&path).unwrap();
    assert!(contents.contains("status: closed"));
    assert!(contents.contains("closed_at:"));
    assert!(contents.contains("## Notes"));
    assert!(contents.contains("done"));

    // Second close without note should be idempotent (no duplicate status or closed_at)
    tk_cmd(&temp).arg("close").arg(&id).assert().success();
    let again = fs::read_to_string(&path).unwrap();
    assert_eq!(again.matches("status: closed").count(), 1);
    assert_eq!(again.matches("closed_at:").count(), 1);
}

#[test]
fn reopen_sets_status_open() {
    let temp = TempDir::new().unwrap();
    let mut create = tk_cmd(&temp);
    let output = create
        .arg("create")
        .arg("My title")
        .output()
        .expect("create runs");
    assert!(output.status.success());
    let id = String::from_utf8_lossy(&output.stdout).trim().to_string();

    // close first
    let mut close = tk_cmd(&temp);
    close.arg("close").arg(&id).assert().success();

    let mut reopen = tk_cmd(&temp);
    reopen.arg("reopen").arg(&id).assert().success();

    let ticket_path = temp.path().join(".tickets").join(format!("{id}.md"));
    let contents = fs::read_to_string(ticket_path).unwrap();
    assert!(contents.contains("status: open"));
}

#[test]
fn status_sets_requested_value() {
    let temp = TempDir::new().unwrap();
    let mut create = tk_cmd(&temp);
    let output = create
        .arg("create")
        .arg("My title")
        .output()
        .expect("create runs");
    assert!(output.status.success());
    let id = String::from_utf8_lossy(&output.stdout).trim().to_string();

    let mut status = tk_cmd(&temp);
    status
        .arg("status")
        .arg(&id)
        .arg("in_progress")
        .assert()
        .success();

    let ticket_path = temp.path().join(".tickets").join(format!("{id}.md"));
    let contents = fs::read_to_string(ticket_path).unwrap();
    assert!(contents.contains("status: in_progress"));
}

#[test]
fn dep_add_appends_dependency() {
    let temp = TempDir::new().unwrap();

    let id1 = {
        let mut create = tk_cmd(&temp);
        let out = create.arg("create").arg("One").output().unwrap();
        assert!(out.status.success());
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };

    let id2 = {
        let mut create = tk_cmd(&temp);
        let out = create.arg("create").arg("Two").output().unwrap();
        assert!(out.status.success());
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };

    let mut dep = tk_cmd(&temp);
    dep.arg("dep").arg(&id1).arg(&id2).assert().success();

    let ticket_path = temp.path().join(".tickets").join(format!("{id1}.md"));
    let contents = fs::read_to_string(ticket_path).unwrap();
    assert!(contents.contains(&format!("deps: [{id2}]")));
}

#[test]
fn dep_tree_prints_tree() {
    let temp = TempDir::new().unwrap();

    let root = {
        let mut create = tk_cmd(&temp);
        let out = create.arg("create").arg("Root").output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    let child = {
        let mut create = tk_cmd(&temp);
        let out = create.arg("create").arg("Child").output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };

    tk_cmd(&temp)
        .arg("dep")
        .arg(&root)
        .arg(&child)
        .assert()
        .success();

    tk_cmd(&temp)
        .arg("dep")
        .arg("tree")
        .arg(&root)
        .assert()
        .success()
        .stdout(predicate::str::contains(&root))
        .stdout(predicate::str::contains(&child));
}

#[test]
fn dep_cycle_reports_cycle() {
    let temp = TempDir::new().unwrap();

    let a = {
        let mut create = tk_cmd(&temp);
        let out = create.arg("create").arg("A").output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    let b = {
        let mut create = tk_cmd(&temp);
        let out = create.arg("create").arg("B").output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    let c = {
        let mut create = tk_cmd(&temp);
        let out = create.arg("create").arg("C").output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };

    tk_cmd(&temp).arg("dep").arg(&a).arg(&b).assert().success();
    tk_cmd(&temp).arg("dep").arg(&b).arg(&c).assert().success();
    tk_cmd(&temp).arg("dep").arg(&c).arg(&a).assert().success();

    let mut canonical = vec![a.clone(), b.clone(), c.clone()];
    let (min_idx, _) = canonical
        .iter()
        .enumerate()
        .min_by(|(_, left), (_, right)| left.cmp(right))
        .unwrap();
    canonical.rotate_left(min_idx);
    let first = canonical.first().unwrap().clone();
    canonical.push(first);
    let expected_line = canonical.join(" -> ");

    tk_cmd(&temp)
        .arg("dep")
        .arg("cycle")
        .assert()
        .success()
        .stdout(predicate::str::contains("Dependency cycles:"))
        .stdout(predicate::str::contains(&expected_line));
}

#[test]
fn dep_cycle_include_closed_flag() {
    let temp = TempDir::new().unwrap();

    let closed_a = {
        let mut create = tk_cmd(&temp);
        let out = create.arg("create").arg("Closed A").output().unwrap();
        let id = String::from_utf8_lossy(&out.stdout).trim().to_string();
        tk_cmd(&temp).arg("close").arg(&id).assert().success();
        id
    };
    let closed_b = {
        let mut create = tk_cmd(&temp);
        let out = create.arg("create").arg("Closed B").output().unwrap();
        let id = String::from_utf8_lossy(&out.stdout).trim().to_string();
        tk_cmd(&temp).arg("close").arg(&id).assert().success();
        id
    };

    tk_cmd(&temp)
        .arg("dep")
        .arg(&closed_a)
        .arg(&closed_b)
        .assert()
        .success();
    tk_cmd(&temp)
        .arg("dep")
        .arg(&closed_b)
        .arg(&closed_a)
        .assert()
        .success();

    tk_cmd(&temp)
        .arg("dep")
        .arg("cycle")
        .assert()
        .success()
        .stdout(predicate::str::contains("No dependency cycles found"));

    tk_cmd(&temp)
        .arg("dep")
        .arg("cycle")
        .arg("--include-closed")
        .assert()
        .success()
        .stdout(predicate::str::contains("Dependency cycles:"))
        .stdout(predicate::str::contains(&closed_a))
        .stdout(predicate::str::contains(&closed_b));
}

#[test]
fn start_adds_note_and_is_idempotent() {
    let temp = TempDir::new().unwrap();

    let id = {
        let mut create = tk_cmd(&temp);
        let out = create.arg("create").arg("Start Me").output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };

    tk_cmd(&temp)
        .arg("start")
        .arg(&id)
        .arg("--note")
        .arg("Beginning work")
        .assert()
        .success();

    let path = temp.path().join(".tickets").join(format!("{id}.md"));
    let contents = fs::read_to_string(&path).unwrap();
    assert!(contents.contains("status: in_progress"));
    assert!(contents.contains("## Notes"));
    assert!(contents.contains("Beginning work"));

    // Second start should not duplicate status
    tk_cmd(&temp).arg("start").arg(&id).assert().success();
    let contents_again = fs::read_to_string(&path).unwrap();
    assert_eq!(contents_again.matches("status: in_progress").count(), 1);
}

#[test]
fn ls_filters_by_status_and_tag() {
    let temp = TempDir::new().unwrap();

    let open_id = {
        let mut create = tk_cmd(&temp);
        let out = create
            .arg("create")
            .arg("Open item")
            .arg("--tags")
            .arg("alpha,beta")
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };

    let closed_id = {
        let mut create = tk_cmd(&temp);
        let out = create
            .arg("create")
            .arg("Closed item")
            .arg("--tags")
            .arg("beta")
            .output()
            .unwrap();
        let id = String::from_utf8_lossy(&out.stdout).trim().to_string();
        tk_cmd(&temp).arg("close").arg(&id).assert().success();
        id
    };

    tk_cmd(&temp)
        .arg("ls")
        .arg("--status=open")
        .arg("-T")
        .arg("alpha")
        .assert()
        .success()
        .stdout(predicate::str::contains(&open_id))
        .stdout(predicate::str::contains("open"))
        .stdout(predicate::str::contains(&closed_id).not());
}

#[test]
fn ls_supports_columns_json_and_stable_ordering() {
    let temp = TempDir::new().unwrap();

    let low = {
        let mut create = tk_cmd(&temp);
        let out = create
            .arg("create")
            .arg("Low")
            .arg("--priority")
            .arg("3")
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };

    let high = {
        let mut create = tk_cmd(&temp);
        let out = create
            .arg("create")
            .arg("High")
            .arg("--priority")
            .arg("1")
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };

    // columns selection
    tk_cmd(&temp)
        .arg("ls")
        .arg("--columns")
        .arg("id,status,priority")
        .assert()
        .success()
        .stdout(predicate::str::contains(&high))
        .stdout(predicate::str::contains("[open]"))
        .stdout(predicate::str::contains("P1"));

    // json output shape
    let out = tk_cmd(&temp).arg("ls").arg("--json").output().unwrap();
    assert!(out.status.success());
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let arr = json.as_array().expect("array");
    assert!(arr.iter().any(|v| v["id"] == high));
    assert!(arr.iter().any(|v| v["id"] == low));

    // stable ordering: high priority should appear before low
    let stdout = String::from_utf8_lossy(&out.stdout);
    let high_idx = stdout.find(&high).unwrap();
    let low_idx = stdout.find(&low).unwrap();
    assert!(high_idx < low_idx, "expected high before low: {stdout}");
}

#[test]
fn ready_lists_when_dependencies_closed() {
    let temp = TempDir::new().unwrap();

    let dep_closed = {
        let mut create = tk_cmd(&temp);
        let out = create.arg("create").arg("Dep Closed").output().unwrap();
        let id = String::from_utf8_lossy(&out.stdout).trim().to_string();
        tk_cmd(&temp).arg("close").arg(&id).assert().success();
        id
    };

    let ready_id = {
        let mut create = tk_cmd(&temp);
        let out = create.arg("create").arg("Ready Root").output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    tk_cmd(&temp)
        .arg("dep")
        .arg(&ready_id)
        .arg(&dep_closed)
        .assert()
        .success();

    let blocked_id = {
        let mut create = tk_cmd(&temp);
        let out = create.arg("create").arg("Blocked Root").output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    let open_dep = {
        let mut create = tk_cmd(&temp);
        let out = create.arg("create").arg("Open Dep").output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    tk_cmd(&temp)
        .arg("dep")
        .arg(&blocked_id)
        .arg(&open_dep)
        .assert()
        .success();

    tk_cmd(&temp)
        .arg("ready")
        .assert()
        .success()
        .stdout(predicate::str::contains(&ready_id))
        .stdout(predicate::str::contains("Ready Root"))
        .stdout(predicate::str::contains(&blocked_id).not());
}

#[test]
fn ready_includes_titles() {
    let temp = TempDir::new().unwrap();

    let dep_closed = {
        let mut create = tk_cmd(&temp);
        let out = create.arg("create").arg("Dependency").output().unwrap();
        let id = String::from_utf8_lossy(&out.stdout).trim().to_string();
        tk_cmd(&temp).arg("close").arg(&id).assert().success();
        id
    };

    let ready_id = {
        let mut create = tk_cmd(&temp);
        let out = create
            .arg("create")
            .arg("Ready With Title")
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };

    tk_cmd(&temp)
        .arg("dep")
        .arg(&ready_id)
        .arg(&dep_closed)
        .assert()
        .success();

    tk_cmd(&temp)
        .arg("ready")
        .assert()
        .success()
        .stdout(predicate::str::contains(&ready_id))
        .stdout(predicate::str::contains("Ready With Title"));
}

#[test]
fn blocked_lists_when_dependencies_open() {
    let temp = TempDir::new().unwrap();

    let open_dep = {
        let mut create = tk_cmd(&temp);
        let out = create.arg("create").arg("Open Dep").output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };

    let blocked_root = {
        let mut create = tk_cmd(&temp);
        let out = create.arg("create").arg("Blocked Root").output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    tk_cmd(&temp)
        .arg("dep")
        .arg(&blocked_root)
        .arg(&open_dep)
        .assert()
        .success();

    let closed_dep = {
        let mut create = tk_cmd(&temp);
        let out = create.arg("create").arg("Closed Dep").output().unwrap();
        let id = String::from_utf8_lossy(&out.stdout).trim().to_string();
        tk_cmd(&temp).arg("close").arg(&id).assert().success();
        id
    };
    let ok_root = {
        let mut create = tk_cmd(&temp);
        let out = create.arg("create").arg("Ok Root").output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    tk_cmd(&temp)
        .arg("dep")
        .arg(&ok_root)
        .arg(&closed_dep)
        .assert()
        .success();

    tk_cmd(&temp)
        .arg("blocked")
        .assert()
        .success()
        .stdout(predicate::str::contains(&blocked_root))
        .stdout(predicate::str::contains(&open_dep))
        .stdout(predicate::str::contains(&ok_root).not());
}

#[test]
fn blocked_only_open_and_missing_dep_handling() {
    let temp = TempDir::new().unwrap();

    // Ticket with open and in-progress blockers
    let open_dep = {
        let mut create = tk_cmd(&temp);
        let out = create.arg("create").arg("Open Dep").output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    let in_progress_dep = {
        let mut create = tk_cmd(&temp);
        let out = create
            .arg("create")
            .arg("In Progress Dep")
            .output()
            .unwrap();
        let id = String::from_utf8_lossy(&out.stdout).trim().to_string();
        tk_cmd(&temp).arg("start").arg(&id).assert().success();
        id
    };

    let root = {
        let mut create = tk_cmd(&temp);
        let out = create.arg("create").arg("Root").output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };

    // Add deps: open and in-progress via CLI, then inject a missing dep directly
    tk_cmd(&temp)
        .arg("dep")
        .arg(&root)
        .arg(&open_dep)
        .assert()
        .success();
    tk_cmd(&temp)
        .arg("dep")
        .arg(&root)
        .arg(&in_progress_dep)
        .assert()
        .success();

    let root_path = temp.path().join(".tickets").join(format!("{root}.md"));
    let contents = fs::read_to_string(&root_path).unwrap();
    let mut deps = vec![
        open_dep.clone(),
        in_progress_dep.clone(),
        "missing-one".to_string(),
    ];
    deps.sort();
    let new_contents = contents
        .lines()
        .map(|line| {
            if line.starts_with("deps:") {
                format!("deps: [{}]", deps.join(", "))
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&root_path, new_contents).unwrap();

    // Default blocked shows open + in-progress + missing
    tk_cmd(&temp)
        .arg("blocked")
        .assert()
        .success()
        .stdout(predicate::str::contains(&root))
        .stdout(predicate::str::contains(&open_dep))
        .stdout(predicate::str::contains(&in_progress_dep))
        .stdout(predicate::str::contains("missing-one"));

    // only-open filters out in-progress blockers but keeps missing
    tk_cmd(&temp)
        .arg("blocked")
        .arg("--only-open")
        .assert()
        .success()
        .stdout(predicate::str::contains(&root))
        .stdout(predicate::str::contains(&open_dep))
        .stdout(predicate::str::contains(&in_progress_dep).not())
        .stdout(predicate::str::contains("missing-one"));
}

#[test]
fn closed_lists_recent_closed_with_filters() {
    let temp = TempDir::new().unwrap();

    // Old closed ticket (touch mtime older)
    let old_closed = {
        let mut create = tk_cmd(&temp);
        let out = create.arg("create").arg("Old Closed").output().unwrap();
        let id = String::from_utf8_lossy(&out.stdout).trim().to_string();
        tk_cmd(&temp).arg("close").arg(&id).assert().success();
        // adjust mtime to be older
        let path = temp.path().join(".tickets").join(format!("{id}.md"));
        filetime::set_file_mtime(path, filetime::FileTime::from_unix_time(0, 0)).unwrap();
        id
    };

    let recent_closed = {
        let mut create = tk_cmd(&temp);
        let out = create
            .arg("create")
            .arg("Recent Closed")
            .arg("--tags")
            .arg("foo")
            .output()
            .unwrap();
        let id = String::from_utf8_lossy(&out.stdout).trim().to_string();
        tk_cmd(&temp).arg("close").arg(&id).assert().success();
        id
    };

    tk_cmd(&temp)
        .arg("closed")
        .arg("--limit=1")
        .arg("-T")
        .arg("foo")
        .assert()
        .success()
        .stdout(predicate::str::contains(&recent_closed))
        .stdout(predicate::str::contains(&old_closed).not());
}

#[test]
fn closed_supports_since_and_fallback_ordering() {
    let temp = TempDir::new().unwrap();

    let no_mtime_id = {
        let mut create = tk_cmd(&temp);
        let out = create.arg("create").arg("No Mtime").output().unwrap();
        let id = String::from_utf8_lossy(&out.stdout).trim().to_string();
        tk_cmd(&temp).arg("close").arg(&id).assert().success();
        let path = temp.path().join(".tickets").join(format!("{id}.md"));
        filetime::set_file_mtime(
            &path,
            filetime::FileTime::from_system_time(SystemTime::UNIX_EPOCH),
        )
        .unwrap();
        id
    };

    let recent_id = {
        let mut create = tk_cmd(&temp);
        let out = create.arg("create").arg("Recent").output().unwrap();
        let id = String::from_utf8_lossy(&out.stdout).trim().to_string();
        tk_cmd(&temp).arg("close").arg(&id).assert().success();
        id
    };

    let future = SystemTime::now() + Duration::from_secs(10);
    let since = time::OffsetDateTime::from(future)
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap();

    // since in the future should produce no output
    tk_cmd(&temp)
        .arg("closed")
        .arg("--since")
        .arg(&since)
        .assert()
        .success()
        .stdout(predicate::str::is_empty());

    // fallback ordering: missing mtime should sort by id after real mtimes
    tk_cmd(&temp)
        .arg("closed")
        .assert()
        .success()
        .stdout(predicate::str::contains(&recent_id))
        .stdout(predicate::str::contains(&no_mtime_id));
}

#[test]
fn show_prints_ticket_contents() {
    let temp = TempDir::new().unwrap();

    let id = {
        let mut create = tk_cmd(&temp);
        let out = create.arg("create").arg("My Show Ticket").output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };

    tk_cmd(&temp)
        .arg("show")
        .arg(&id)
        .assert()
        .success()
        .stdout(predicate::str::contains(&id))
        .stdout(predicate::str::contains("My Show Ticket"));
}

#[test]
fn edit_uses_editor_and_succeeds() {
    let temp = TempDir::new().unwrap();

    let id = {
        let mut create = tk_cmd(&temp);
        let out = create.arg("create").arg("Edit Me").output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };

    // Simulate editor that succeeds and writes nothing
    tk_cmd(&temp)
        .env("EDITOR", "true")
        .arg("edit")
        .arg(&id)
        .assert()
        .success();
}

#[test]
fn add_note_appends_notes_section_and_text() {
    let temp = TempDir::new().unwrap();

    let id = {
        let mut create = tk_cmd(&temp);
        let out = create.arg("create").arg("Note Test").output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };

    tk_cmd(&temp)
        .arg("add-note")
        .arg(&id)
        .arg("First note")
        .assert()
        .success();

    let path = temp.path().join(".tickets").join(format!("{id}.md"));
    let contents = fs::read_to_string(path).unwrap();
    assert!(contents.contains("## Notes"));
    assert!(contents.contains("First note"));
}

#[test]
fn query_outputs_json_and_filters_with_jq() {
    let temp = TempDir::new().unwrap();

    let one = {
        let mut create = tk_cmd(&temp);
        let out = create
            .arg("create")
            .arg("One")
            .arg("--tags")
            .arg("x")
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };

    let two = {
        let mut create = tk_cmd(&temp);
        let out = create
            .arg("create")
            .arg("Two")
            .arg("--tags")
            .arg("y")
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };

    tk_cmd(&temp)
        .arg("query")
        .assert()
        .success()
        .stdout(predicate::str::contains(&one))
        .stdout(predicate::str::contains(&two));

    tk_cmd(&temp)
        .arg("query")
        .arg(".[] | select(.tags[]? == \"x\")")
        .assert()
        .success()
        .stdout(predicate::str::contains(&one))
        .stdout(predicate::str::contains(&two).not());
}

#[test]
fn migrate_beads_creates_tickets() {
    let temp = TempDir::new().unwrap();

    let beads_dir = temp.path().join(".beads");
    fs::create_dir_all(&beads_dir).unwrap();
    let jsonl = beads_dir.join("issues.jsonl");
    fs::write(
        &jsonl,
        r#"{ "id": "a1", "title": "Beads", "dependencies": [{"type":"blocks","depends_on_id":"b1"}], "status":"open", "priority":1 }
{ "id": "b1", "title": "Dep", "status":"closed" }
"#,
    )
    .unwrap();

    tk_cmd(&temp)
        .arg("migrate-beads")
        .assert()
        .success()
        .stdout(predicate::str::contains("Migrated 2 tickets"));

    let a1 = temp.path().join(".tickets/a1.md");
    let b1 = temp.path().join(".tickets/b1.md");
    assert!(a1.exists());
    assert!(b1.exists());
    let a1_contents = fs::read_to_string(a1).unwrap();
    assert!(a1_contents.contains("deps: [b1]"));
    assert!(a1_contents.contains("priority: 1"));
}

#[test]
fn migrate_beads_preserves_blocks_links_parent_and_notes() {
    let temp = TempDir::new().unwrap();

    let beads_dir = temp.path().join(".beads");
    fs::create_dir_all(&beads_dir).unwrap();
    let jsonl = beads_dir.join("issues.jsonl");
    fs::write(
        &jsonl,
        r#"{"id":"x1","title":"Parent","status":"open"}
{"id":"c1","title":"Child","status":"closed","dependencies":[{"type":"blocks","depends_on_id":"b1"},{"type":"related","depends_on_id":"r2"},{"type":"parent-child","depends_on_id":"x1"}],"notes":"First line\nSecond line"}
"#,
    )
    .unwrap();

    let assert = tk_cmd(&temp).arg("migrate-beads").assert().success();
    let output = String::from_utf8_lossy(&assert.get_output().stdout);
    assert!(output.contains("Migrated 2 tickets"), "stdout: {output}");

    let child = temp.path().join(".tickets/c1.md");
    let contents = fs::read_to_string(&child).unwrap();
    assert!(contents.contains("deps: [b1]"));
    assert!(contents.contains("links: [r2]"));
    assert!(contents.contains("parent: x1"));
    assert!(contents.contains("## Notes"));
    assert!(contents.contains("First line"));
    assert!(contents.contains("Second line"));
}

#[test]
fn migrate_beads_skips_malformed_records_with_warning() {
    let temp = TempDir::new().unwrap();

    let beads_dir = temp.path().join(".beads");
    fs::create_dir_all(&beads_dir).unwrap();
    let jsonl = beads_dir.join("issues.jsonl");
    fs::write(
        &jsonl,
        r#"{ "id": "good", "title": "Good", "status":"open" }
{ "id": "bad", "title": "", "status":"open" }
not-json
"#,
    )
    .unwrap();

    let assert = tk_cmd(&temp).arg("migrate-beads").assert().success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    assert!(stdout.contains("Migrated 1 tickets"), "stdout: {stdout}");
    assert!(stdout.contains("skipped 2"), "stdout: {stdout}");
    assert!(
        String::from_utf8_lossy(&assert.get_output().stderr).contains("Warning:"),
        "stderr: {}",
        String::from_utf8_lossy(&assert.get_output().stderr)
    );

    let good = temp.path().join(".tickets/good.md");
    assert!(good.exists());
    let bad = temp.path().join(".tickets/bad.md");
    assert!(!bad.exists());
}

#[test]
fn undep_removes_dependency() {
    let temp = TempDir::new().unwrap();

    let parent = {
        let mut create = tk_cmd(&temp);
        let out = create.arg("create").arg("Parent").output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };

    let child = {
        let mut create = tk_cmd(&temp);
        let out = create.arg("create").arg("Child").output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };

    tk_cmd(&temp)
        .arg("dep")
        .arg(&parent)
        .arg(&child)
        .assert()
        .success();

    tk_cmd(&temp)
        .arg("undep")
        .arg(&parent)
        .arg(&child)
        .assert()
        .success();

    let ticket_path = temp.path().join(".tickets").join(format!("{parent}.md"));
    let contents = fs::read_to_string(ticket_path).unwrap();
    assert!(contents.contains("deps: []"));
}

#[test]
fn link_adds_links_bidirectionally() {
    let temp = TempDir::new().unwrap();

    let first = {
        let mut create = tk_cmd(&temp);
        let out = create.arg("create").arg("First").output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };

    let second = {
        let mut create = tk_cmd(&temp);
        let out = create.arg("create").arg("Second").output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };

    tk_cmd(&temp)
        .arg("link")
        .arg(&first)
        .arg(&second)
        .assert()
        .success();

    let first_path = temp.path().join(".tickets").join(format!("{first}.md"));
    let second_path = temp.path().join(".tickets").join(format!("{second}.md"));

    assert_links(&first_path, &[second.as_str()]);
    assert_links(&second_path, &[first.as_str()]);
}

#[test]
fn link_dry_run_reports_without_writing() {
    let temp = TempDir::new().unwrap();

    write_ticket(&temp, "a-1", &[]);
    write_ticket(&temp, "b-2", &[]);

    tk_cmd(&temp)
        .arg("link")
        .arg("a-1")
        .arg("b-2")
        .arg("--dry-run")
        .assert()
        .success()
        .stdout(predicate::str::contains("a-1: [] -> [b-2]"))
        .stdout(predicate::str::contains("b-2: [] -> [a-1]"));

    let a_path = temp.path().join(".tickets").join("a-1.md");
    let b_path = temp.path().join(".tickets").join("b-2.md");
    assert_links(&a_path, &[]);
    assert_links(&b_path, &[]);
}

#[test]
fn link_skips_when_already_symmetric() {
    let temp = TempDir::new().unwrap();

    write_ticket(&temp, "p-1", &["t-2"]);
    write_ticket(&temp, "t-2", &["p-1"]);

    tk_cmd(&temp)
        .arg("link")
        .arg("p-1")
        .arg("t-2")
        .assert()
        .success()
        .stdout(predicate::str::contains("Links already up to date"));

    let p_path = temp.path().join(".tickets").join("p-1.md");
    let t_path = temp.path().join(".tickets").join("t-2.md");
    assert_links(&p_path, &["t-2"]);
    assert_links(&t_path, &["p-1"]);
}

#[test]
fn unlink_removes_links_bidirectionally() {
    let temp = TempDir::new().unwrap();

    let one = {
        let mut create = tk_cmd(&temp);
        let out = create.arg("create").arg("One").output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };

    let two = {
        let mut create = tk_cmd(&temp);
        let out = create.arg("create").arg("Two").output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };

    tk_cmd(&temp)
        .arg("link")
        .arg(&one)
        .arg(&two)
        .assert()
        .success();

    tk_cmd(&temp)
        .arg("unlink")
        .arg(&one)
        .arg(&two)
        .assert()
        .success();

    let one_path = temp.path().join(".tickets").join(format!("{one}.md"));
    let two_path = temp.path().join(".tickets").join(format!("{two}.md"));

    let one_contents = fs::read_to_string(one_path).unwrap();
    let two_contents = fs::read_to_string(two_path).unwrap();

    assert!(one_contents.contains("links: []"));
    assert!(two_contents.contains("links: []"));
}

#[test]
fn unlink_warns_when_missing_and_is_idempotent() {
    let temp = TempDir::new().unwrap();

    let one = {
        let mut create = tk_cmd(&temp);
        let out = create.arg("create").arg("One").output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };

    let two = {
        let mut create = tk_cmd(&temp);
        let out = create.arg("create").arg("Two").output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };

    // Link first to set up symmetry
    tk_cmd(&temp)
        .arg("link")
        .arg(&one)
        .arg(&two)
        .assert()
        .success();

    let one_path = temp.path().join(".tickets").join(format!("{one}.md"));
    let two_path = temp.path().join(".tickets").join(format!("{two}.md"));

    // First unlink succeeds and writes once per side
    tk_cmd(&temp)
        .arg("unlink")
        .arg(&one)
        .arg(&two)
        .assert()
        .success()
        .stdout(predicate::str::contains("Unlinked"));

    let one_after = fs::read_to_string(&one_path).unwrap();
    let two_after = fs::read_to_string(&two_path).unwrap();
    assert!(one_after.contains("links: []"));
    assert!(two_after.contains("links: []"));

    // Second unlink is idempotent, warns when requested, and does not rewrite contents
    let before_again_one = fs::read_to_string(&one_path).unwrap();
    let before_again_two = fs::read_to_string(&two_path).unwrap();

    tk_cmd(&temp)
        .arg("unlink")
        .arg(&one)
        .arg(&two)
        .arg("--warn-missing")
        .assert()
        .success()
        .stdout(predicate::str::contains("Warning: link"));

    let after_again_one = fs::read_to_string(&one_path).unwrap();
    let after_again_two = fs::read_to_string(&two_path).unwrap();
    assert_eq!(before_again_one, after_again_one);
    assert_eq!(before_again_two, after_again_two);
}
