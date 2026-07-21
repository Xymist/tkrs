use assert_cmd::{Command, cargo::cargo_bin_cmd};
use assert_fs::TempDir;
use predicates::prelude::*;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::time::{Duration, SystemTime};

fn tk_cmd(dir: &TempDir) -> Command {
    let mut cmd = cargo_bin_cmd!("tk");
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

/// Writes a raw ticket file whose file name is independent of its
/// frontmatter `id:` value, so two distinct files can be made to claim the
/// same id.
fn write_ticket_at(dir: &TempDir, file_name: &str, id: &str) {
    let tickets_dir = dir.path().join(".tickets");
    fs::create_dir_all(&tickets_dir).unwrap();
    let path = tickets_dir.join(file_name);
    let mut file = fs::File::create(&path).unwrap();
    writeln!(file, "---").unwrap();
    writeln!(file, "id: {id}").unwrap();
    writeln!(file, "status: open").unwrap();
    writeln!(file, "deps: []").unwrap();
    writeln!(file, "links: []").unwrap();
    writeln!(file, "created: 2026-01-01T00:00:00Z").unwrap();
    writeln!(file, "type: task").unwrap();
    writeln!(file, "priority: 2").unwrap();
    writeln!(file, "---").unwrap();
    writeln!(file, "# {id}").unwrap();
}

#[allow(clippy::too_many_arguments)]
fn write_ticket_with_fields(
    dir: &TempDir,
    id: &str,
    _title: &str,
    status: &str,
    deps: &[&str],
    links: &[&str],
    tags: &[&str],
    parent: Option<&str>,
    body: &str,
) {
    let tickets_dir = dir.path().join(".tickets");
    fs::create_dir_all(&tickets_dir).unwrap();
    let path = dir.path().join(".tickets").join(format!("{id}.md"));
    let mut file = fs::File::create(&path).unwrap();
    writeln!(file, "---").unwrap();
    writeln!(file, "id: {id}").unwrap();
    writeln!(file, "status: {status}").unwrap();
    writeln!(file, "deps: [{}]", deps.join(", ")).unwrap();
    writeln!(file, "links: [{}]", links.join(", ")).unwrap();
    writeln!(file, "created: 2026-01-01T00:00:00Z").unwrap();
    writeln!(file, "type: task").unwrap();
    writeln!(file, "priority: 2").unwrap();
    if !tags.is_empty() {
        writeln!(file, "tags: [{}]", tags.join(", ")).unwrap();
    }
    if let Some(p) = parent {
        writeln!(file, "parent: {p}").unwrap();
    }
    writeln!(file, "---").unwrap();
    writeln!(file, "{body}").unwrap();
}

fn assert_links(path: impl AsRef<Path>, expected: &[&str]) {
    let contents = fs::read_to_string(path).unwrap();
    let actual = parse_links(&contents);
    let expected: Vec<String> = expected.iter().map(|s| s.to_string()).collect();
    assert_eq!(
        actual, expected,
        "expected links {expected:?} but found {actual:?}\n{contents}"
    );
}

/// Extract the `links` list from ticket frontmatter, tolerating either
/// flow-style (`links: [a, b]`) or block-style (`links:\n- a\n- b`) YAML.
fn parse_links(contents: &str) -> Vec<String> {
    parse_yaml_list(contents, "links:")
}

/// Extract the `deps` list from ticket frontmatter, tolerating either
/// flow-style (`deps: [a, b]`) or block-style (`deps:\n- a\n- b`) YAML.
fn parse_deps(contents: &str) -> Vec<String> {
    parse_yaml_list(contents, "deps:")
}

/// Extract a YAML list value identified by `key` (e.g. "deps:" or "links:"),
/// tolerating either flow-style (`key: [a, b]`) or block-style
/// (`key:\n- a\n- b`) YAML.
fn parse_yaml_list(contents: &str, key: &str) -> Vec<String> {
    let mut lines = contents.lines();
    while let Some(line) = lines.next() {
        let trimmed = line.trim();
        let Some(rest) = trimmed.strip_prefix(key) else {
            continue;
        };
        let rest = rest.trim();
        if let Some(inner) = rest.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            return inner
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect();
        }
        // block style: collect following `- item` lines
        let mut items = Vec::new();
        for item in lines.by_ref() {
            let item = item.trim();
            if let Some(value) = item.strip_prefix("- ") {
                items.push(value.trim().to_string());
            } else {
                break;
            }
        }
        return items;
    }
    Vec::new()
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
fn create_uses_parent_tickets_dir() {
    let temp = TempDir::new().unwrap();
    let tickets_root = temp.path().join(".tickets");
    fs::create_dir_all(&tickets_root).unwrap();

    let nested = temp.path().join("a/b/c");
    fs::create_dir_all(&nested).unwrap();

    let output = tk_cmd(&temp)
        .current_dir(&nested)
        .arg("create")
        .arg("From child")
        .output()
        .expect("create runs");
    assert!(output.status.success());
    let id = String::from_utf8_lossy(&output.stdout).trim().to_string();

    let ticket_path = tickets_root.join(format!("{id}.md"));
    assert!(
        ticket_path.exists(),
        "ticket written to parent .tickets dir"
    );
}

#[test]
fn create_respects_tickets_dir_env_override() {
    let temp = TempDir::new().unwrap();
    let override_dir = temp.path().join("custom");
    fs::create_dir_all(&override_dir).unwrap();

    let output = tk_cmd(&temp)
        .env("TICKETS_DIR", override_dir.to_str().unwrap())
        .arg("create")
        .arg("Env dir")
        .output()
        .expect("create runs");
    assert!(output.status.success());
    let id = String::from_utf8_lossy(&output.stdout).trim().to_string();

    let ticket_path = override_dir.join(format!("{id}.md"));
    assert!(ticket_path.exists(), "ticket written to env override dir");
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
fn create_parent_resolves_partial_and_errors_on_missing_or_ambiguous() {
    let temp = TempDir::new().unwrap();

    // Seed tickets to resolve against
    let parent_exact = {
        let out = tk_cmd(&temp)
            .arg("create")
            .arg("Parent One")
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    let parent_two = {
        let out = tk_cmd(&temp)
            .arg("create")
            .arg("Parent Two")
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };

    // Build shared prefix for ambiguity
    let mut ambiguous = String::new();
    for (a, b) in parent_exact.chars().zip(parent_two.chars()) {
        if a == b {
            ambiguous.push(a);
        } else {
            break;
        }
    }
    if ambiguous.is_empty() {
        ambiguous.push_str(&parent_exact[..1.min(parent_exact.len())]);
    }

    // Find a unique prefix for parent_exact that doesn't appear in parent_two
    let mut unique = String::new();
    for len in 1..=parent_exact.len() {
        let sub = &parent_exact[..len];
        if !parent_two.contains(sub) {
            unique = sub.to_string();
            break;
        }
    }
    if unique.is_empty() {
        unique = parent_exact.clone();
    }

    // Ambiguous partial should fail
    tk_cmd(&temp)
        .arg("create")
        .arg("Child Ambig")
        .arg("--parent")
        .arg(&ambiguous)
        .assert()
        .failure()
        .stderr(predicate::str::contains("ambiguous ID"));

    // Missing parent should fail
    tk_cmd(&temp)
        .arg("create")
        .arg("Child Missing")
        .arg("--parent")
        .arg("no-such")
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found"));

    // Valid partial resolves and writes canonical parent id
    let out = tk_cmd(&temp)
        .arg("create")
        .arg("Child Good")
        .arg("--parent")
        .arg(&unique)
        .output()
        .unwrap();
    assert!(out.status.success());
    let child_id = String::from_utf8_lossy(&out.stdout).trim().to_string();

    // Parent relationship is derived from reverse dependencies: the resolved
    // parent ticket should now list the new child in its deps.
    let parent_path = temp
        .path()
        .join(".tickets")
        .join(format!("{parent_exact}.md"));
    let parent_contents = fs::read_to_string(parent_path).unwrap();
    assert!(
        parse_deps(&parent_contents).contains(&child_id),
        "parent should list child as a dependency: {parent_contents}"
    );

    // Ensure parent_two untouched
    let p2_path = temp
        .path()
        .join(".tickets")
        .join(format!("{parent_two}.md"));
    assert!(p2_path.exists());
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
fn reopen_clears_closed_at_and_adds_optional_note() {
    let temp = TempDir::new().unwrap();

    let id = {
        let mut create = tk_cmd(&temp);
        let out = create.arg("create").arg("Reopen Me").output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };

    tk_cmd(&temp).arg("close").arg(&id).assert().success();

    tk_cmd(&temp)
        .arg("reopen")
        .arg(&id)
        .arg("--note")
        .arg("because reasons")
        .assert()
        .success();

    let path = temp.path().join(".tickets").join(format!("{id}.md"));
    let contents = fs::read_to_string(&path).unwrap();
    assert!(contents.contains("status: open"));
    assert!(!contents.contains("closed_at:"));
    assert!(contents.contains("because reasons"));

    // second reopen without note is idempotent
    tk_cmd(&temp).arg("reopen").arg(&id).assert().success();
    let contents2 = fs::read_to_string(&path).unwrap();
    assert!(contents2.contains("status: open"));
    assert!(!contents2.contains("closed_at:"));
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
fn status_rejects_invalid_value() {
    let temp = TempDir::new().unwrap();
    let id = {
        let out = tk_cmd(&temp).arg("create").arg("Oops").output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };

    tk_cmd(&temp)
        .arg("status")
        .arg(&id)
        .arg("done")
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid status"));
}

#[test]
fn status_appends_note_and_accepts_case_insensitive_value() {
    let temp = TempDir::new().unwrap();
    let id = {
        let out = tk_cmd(&temp).arg("create").arg("Case").output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };

    tk_cmd(&temp)
        .arg("status")
        .arg(&id)
        .arg("In-Progress")
        .arg("--note")
        .arg("moving")
        .assert()
        .success();

    let path = temp.path().join(".tickets").join(format!("{id}.md"));
    let contents = fs::read_to_string(&path).unwrap();
    assert!(contents.contains("status: in_progress"));
    assert!(contents.contains("## Notes"));
    assert!(contents.contains("moving"));
}

#[test]
fn status_is_idempotent_without_note_for_same_value() {
    let temp = TempDir::new().unwrap();
    let id = {
        let out = tk_cmd(&temp).arg("create").arg("Same").output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };

    tk_cmd(&temp)
        .arg("status")
        .arg(&id)
        .arg("closed")
        .assert()
        .success();

    let path = temp.path().join(".tickets").join(format!("{id}.md"));
    let contents = fs::read_to_string(&path).unwrap();
    assert_eq!(contents.matches("status: closed").count(), 1);
    assert_eq!(contents.matches("closed_at:").count(), 1);

    tk_cmd(&temp)
        .arg("status")
        .arg(&id)
        .arg("CLOSED")
        .assert()
        .success();

    let again = fs::read_to_string(&path).unwrap();
    assert_eq!(again.matches("status: closed").count(), 1);
    assert_eq!(again.matches("closed_at:").count(), 1);
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
    assert_eq!(parse_deps(&contents), vec![id2.clone()]);
}

#[test]
fn dep_add_detects_ambiguous_ids() {
    let temp = TempDir::new().unwrap();

    let one = tk_cmd(&temp).arg("create").arg("Alpha").output().unwrap();
    let two = tk_cmd(&temp).arg("create").arg("Alpine").output().unwrap();
    let target = tk_cmd(&temp).arg("create").arg("Target").output().unwrap();
    let target_id = String::from_utf8_lossy(&target.stdout).trim().to_string();

    let one_id = String::from_utf8_lossy(&one.stdout).trim().to_string();
    let two_id = String::from_utf8_lossy(&two.stdout).trim().to_string();

    // find a short substring common to both ids to trigger ambiguity
    let mut ambiguous = String::new();
    for (a, b) in one_id.chars().zip(two_id.chars()) {
        if a == b {
            ambiguous.push(a);
            if ambiguous.len() >= 2 {
                break;
            }
        } else {
            break;
        }
    }
    if ambiguous.is_empty() {
        ambiguous.push_str(&one_id[0..2.min(one_id.len())]);
    }

    tk_cmd(&temp)
        .arg("dep")
        .arg(&ambiguous)
        .arg(&target_id)
        .assert()
        .failure()
        .stderr(predicate::str::contains("ambiguous ID"));
}

#[test]
fn dep_add_check_cycle_reverts_on_cycle() {
    let temp = TempDir::new().unwrap();

    let root = {
        let out = tk_cmd(&temp).arg("create").arg("Root").output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    let dep = {
        let out = tk_cmd(&temp).arg("create").arg("Dep").output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };

    tk_cmd(&temp)
        .arg("dep")
        .arg(&root)
        .arg(&dep)
        .assert()
        .success();

    tk_cmd(&temp)
        .arg("dep")
        .arg(&dep)
        .arg(&root)
        .arg("--check-cycle")
        .assert()
        .failure()
        .stderr(predicate::str::contains("cycle detected"));

    let dep_path = temp.path().join(".tickets").join(format!("{dep}.md"));
    let contents = fs::read_to_string(dep_path).unwrap();
    assert!(
        contents.contains("deps: []"),
        "deps were reverted: {contents}"
    );
}

#[test]
fn dep_add_is_idempotent() {
    let temp = TempDir::new().unwrap();

    let root = {
        let out = tk_cmd(&temp).arg("create").arg("Root").output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    let dep = {
        let out = tk_cmd(&temp).arg("create").arg("Dep").output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };

    tk_cmd(&temp)
        .arg("dep")
        .arg(&root)
        .arg(&dep)
        .assert()
        .success();

    tk_cmd(&temp)
        .arg("dep")
        .arg(&root)
        .arg(&dep)
        .assert()
        .failure()
        .stderr(predicate::str::contains("already exists"));

    let root_path = temp.path().join(".tickets").join(format!("{root}.md"));
    let contents = fs::read_to_string(root_path).unwrap();
    assert_eq!(contents.matches(&dep).count(), 1, "dep listed once");
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
fn dep_tree_filters_by_status_and_only_open() {
    let temp = TempDir::new().unwrap();

    let root = {
        let mut create = tk_cmd(&temp);
        let out = create.arg("create").arg("Root").output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };

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

    let closed_dep = {
        let mut create = tk_cmd(&temp);
        let out = create.arg("create").arg("Closed Dep").output().unwrap();
        let id = String::from_utf8_lossy(&out.stdout).trim().to_string();
        tk_cmd(&temp).arg("close").arg(&id).assert().success();
        id
    };

    for dep in [&open_dep, &in_progress_dep, &closed_dep] {
        tk_cmd(&temp)
            .arg("dep")
            .arg(&root)
            .arg(dep)
            .assert()
            .success();
    }

    tk_cmd(&temp)
        .arg("dep")
        .arg("tree")
        .arg(&root)
        .arg("--status")
        .arg("closed")
        .assert()
        .success()
        .stdout(predicate::str::contains(&closed_dep))
        .stdout(predicate::str::contains(&open_dep).not())
        .stdout(predicate::str::contains(&in_progress_dep).not());

    tk_cmd(&temp)
        .arg("dep")
        .arg("tree")
        .arg(&root)
        .arg("--only-open")
        .assert()
        .success()
        .stdout(predicate::str::contains(&open_dep))
        .stdout(predicate::str::contains(&in_progress_dep))
        .stdout(predicate::str::contains(&closed_dep).not());
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

    tk_cmd(&temp)
        .arg("dep")
        .arg(&a)
        .arg(&b)
        .arg("--check-cycle")
        .arg("false")
        .assert()
        .success();
    tk_cmd(&temp)
        .arg("dep")
        .arg(&b)
        .arg(&c)
        .arg("--check-cycle")
        .arg("false")
        .assert()
        .success();
    tk_cmd(&temp)
        .arg("dep")
        .arg(&c)
        .arg(&a)
        .arg("--check-cycle")
        .arg("false")
        .assert()
        .success();

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
        .arg("--check-cycle")
        .arg("false")
        .assert()
        .success();
    tk_cmd(&temp)
        .arg("dep")
        .arg(&closed_b)
        .arg(&closed_a)
        .arg("--check-cycle")
        .arg("false")
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
fn ls_filters_by_parent_and_surfaces_parent_in_json() {
    let temp = TempDir::new().unwrap();

    let parent_id = {
        let out = tk_cmd(&temp).arg("create").arg("Epic").output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };

    let child_id = {
        let out = tk_cmd(&temp)
            .arg("create")
            .arg("Child")
            .arg("--parent")
            .arg(&parent_id)
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };

    let orphan_id = {
        let out = tk_cmd(&temp).arg("create").arg("Orphan").output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };

    // --parent filter returns only direct children
    tk_cmd(&temp)
        .arg("ls")
        .arg("--parent")
        .arg(&parent_id)
        .assert()
        .success()
        .stdout(predicate::str::contains(&child_id))
        .stdout(predicate::str::contains(&orphan_id).not())
        .stdout(predicate::str::contains(&parent_id).not());

    // --parent accepts the partial-ID form other commands use
    let suffix: String = parent_id
        .rsplit('-')
        .next()
        .expect("id has prefix")
        .chars()
        .take(3)
        .collect();
    tk_cmd(&temp)
        .arg("ls")
        .arg("--parent")
        .arg(&suffix)
        .assert()
        .success()
        .stdout(predicate::str::contains(&child_id));

    // JSON output surfaces derived parents unconditionally
    let out = tk_cmd(&temp).arg("ls").arg("--json").output().unwrap();
    assert!(out.status.success());
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let arr = json.as_array().expect("array");
    let child = arr.iter().find(|v| v["id"] == child_id).expect("child row");
    assert_eq!(child["parents"], serde_json::json!([parent_id.clone()]));
    let orphan = arr
        .iter()
        .find(|v| v["id"] == orphan_id)
        .expect("orphan row");
    assert_eq!(orphan["parents"], serde_json::json!([]));

    // unknown --parent surfaces a resolution error
    tk_cmd(&temp)
        .arg("ls")
        .arg("--parent")
        .arg("does-not-exist")
        .assert()
        .failure();
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
fn ready_respects_status_filter_and_includes_counts() {
    let temp = TempDir::new().unwrap();

    // closed dependency
    let dep_closed = {
        let mut create = tk_cmd(&temp);
        let out = create.arg("create").arg("Closed Dep").output().unwrap();
        let id = String::from_utf8_lossy(&out.stdout).trim().to_string();
        tk_cmd(&temp).arg("close").arg(&id).assert().success();
        id
    };

    // open ready ticket
    let ready_open = {
        let mut create = tk_cmd(&temp);
        let out = create.arg("create").arg("Ready Open").output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    tk_cmd(&temp)
        .arg("dep")
        .arg(&ready_open)
        .arg(&dep_closed)
        .assert()
        .success();

    // in_progress ready ticket
    let ready_in_progress = {
        let mut create = tk_cmd(&temp);
        let out = create
            .arg("create")
            .arg("Ready In Progress")
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    tk_cmd(&temp)
        .arg("dep")
        .arg(&ready_in_progress)
        .arg(&dep_closed)
        .assert()
        .success();
    tk_cmd(&temp)
        .arg("start")
        .arg(&ready_in_progress)
        .assert()
        .success();

    // closed ticket should be excluded even if deps closed
    let closed_ticket = {
        let mut create = tk_cmd(&temp);
        let out = create.arg("create").arg("Closed Ready").output().unwrap();
        let id = String::from_utf8_lossy(&out.stdout).trim().to_string();
        tk_cmd(&temp)
            .arg("dep")
            .arg(&id)
            .arg(&dep_closed)
            .assert()
            .success();
        tk_cmd(&temp).arg("close").arg(&id).assert().success();
        id
    };

    // default ready shows open and in_progress, excludes closed
    let default_out = tk_cmd(&temp).arg("ready").output().unwrap();
    assert!(default_out.status.success());
    let default_stdout = String::from_utf8_lossy(&default_out.stdout);
    assert!(default_stdout.contains(&ready_open));
    assert!(default_stdout.contains(&ready_in_progress));
    assert!(!default_stdout.contains(&closed_ticket));

    // status filter only closed
    let closed_out = tk_cmd(&temp)
        .arg("ready")
        .arg("--status")
        .arg("closed")
        .output()
        .unwrap();
    assert!(closed_out.status.success());
    let closed_stdout = String::from_utf8_lossy(&closed_out.stdout);
    assert!(closed_stdout.contains(&closed_ticket));
    assert!(!closed_stdout.contains(&ready_open));
    assert!(!closed_stdout.contains(&ready_in_progress));

    // show-deps prints dependency counts
    tk_cmd(&temp)
        .arg("ready")
        .arg("--show-deps")
        .assert()
        .success()
        .stdout(predicate::str::contains("(deps: 1)"));
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
    let mut deps = [
        open_dep.clone(),
        in_progress_dep.clone(),
        "missing-one".to_string(),
    ];
    deps.sort();
    // Rewrite the (block-style) `deps:` list to inject a dangling dependency.
    let mut out_lines: Vec<String> = Vec::new();
    let mut lines = contents.lines().peekable();
    while let Some(line) = lines.next() {
        if line.starts_with("deps:") {
            // Skip any existing block-style `- item` entries that follow.
            while let Some(peek) = lines.peek() {
                if peek.trim_start().starts_with("- ") {
                    lines.next();
                } else {
                    break;
                }
            }
            out_lines.push("deps:".to_string());
            for d in &deps {
                out_lines.push(format!("- {d}"));
            }
        } else {
            out_lines.push(line.to_string());
        }
    }
    let new_contents = out_lines.join("\n");
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
fn show_displays_resolved_metadata_and_body() {
    let temp = TempDir::new().unwrap();

    write_ticket_with_fields(
        &temp,
        "parent-1",
        "Parent Ticket",
        "open",
        &["child-1"],
        &[],
        &[],
        None,
        "# Parent Ticket\n\nParent body",
    );

    write_ticket_with_fields(
        &temp,
        "child-1",
        "Child Ticket",
        "open",
        &["parent-1"],
        &["parent-1"],
        &["feat", "backend"],
        None,
        "# Child Ticket\n\nChild description\n\n## Design\n\nDesign text\n\n## Acceptance Criteria\n\nDo the thing\n\n## Notes\n\nNote here",
    );

    tk_cmd(&temp)
        .arg("show")
        .arg("child-1")
        .assert()
        .success()
        .stdout(predicate::str::contains("child-1 [open] - Child Ticket"))
        .stdout(predicate::str::contains(
            "Parents: parent-1 (Parent Ticket)",
        ))
        .stdout(predicate::str::contains("Deps: parent-1 (Parent Ticket)"))
        .stdout(predicate::str::contains("Links: parent-1 (Parent Ticket)"))
        .stdout(predicate::str::contains("Tags: [feat, backend]"))
        .stdout(predicate::str::contains("## Implementation Plan"))
        .stdout(predicate::str::contains("## Acceptance Criteria"))
        .stdout(predicate::str::contains("## Notes"));
}

#[test]
fn show_json_includes_resolved_titles_and_body() {
    let temp = TempDir::new().unwrap();

    write_ticket_with_fields(
        &temp,
        "parent-1",
        "Parent Ticket",
        "open",
        &["child-1"],
        &[],
        &[],
        None,
        "# Parent Ticket\n\nParent body",
    );

    write_ticket_with_fields(
        &temp,
        "child-1",
        "Child Ticket",
        "open",
        &["parent-1"],
        &["parent-1"],
        &["feat", "backend"],
        None,
        "# Child Ticket\n\nChild description\n\n## Design\n\nDesign text\n\n## Acceptance Criteria\n\nDo the thing\n\n## Notes\n\nNote here",
    );

    let out = tk_cmd(&temp)
        .arg("show")
        .arg("child-1")
        .arg("--json")
        .output()
        .unwrap();
    assert!(out.status.success());
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();

    assert_eq!(json["id"], "child-1");
    assert_eq!(json["title"], "Child Ticket");
    assert_eq!(json["parents"][0]["id"], "parent-1");
    assert_eq!(json["parents"][0]["title"], "Parent Ticket");
    assert_eq!(json["deps"][0]["title"], "Parent Ticket");
    assert_eq!(json["links"][0]["title"], "Parent Ticket");
    assert_eq!(json["tags"], serde_json::json!(["feat", "backend"]));
    assert_eq!(json["body"]["description"], "Child description");
    assert_eq!(json["body"]["implementation_plan"], "Design text");
    assert_eq!(json["body"]["acceptance"], "Do the thing");
    assert_eq!(json["body"]["notes"], serde_json::json!(["Note here"]));
}

#[test]
fn edit_uses_editor_and_succeeds() {
    let temp = TempDir::new().unwrap();

    let id = {
        let mut create = tk_cmd(&temp);
        let out = create.arg("create").arg("Edit Me").output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };

    // Simulate editor that succeeds and writes nothing. The long-form
    // --interactive flag selects the editor path; --force launches it even
    // though stdout is not a TTY. VISUAL is cleared so the EDITOR fallback is
    // exercised deterministically regardless of the host environment.
    tk_cmd(&temp)
        .env_remove("VISUAL")
        .env("EDITOR", "true")
        .arg("edit")
        .arg(&id)
        .arg("--interactive")
        .arg("--force")
        .assert()
        .success();
}

#[test]
fn edit_respects_visual_then_editor_and_print_mode() {
    let temp = TempDir::new().unwrap();

    let id = {
        let out = tk_cmd(&temp).arg("create").arg("Edit Me").output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };

    // print mode
    let print_out = tk_cmd(&temp)
        .arg("edit")
        .arg(&id)
        .arg("--print")
        .output()
        .unwrap();
    assert!(print_out.status.success());
    let printed = String::from_utf8_lossy(&print_out.stdout);
    assert!(printed.contains(&id));

    // VISUAL takes precedence over EDITOR. --force launches the editor
    // even though stdout is not a TTY under the test harness; EDITOR=false
    // would fail if it were consulted.
    tk_cmd(&temp)
        .env("VISUAL", "printf")
        .env("EDITOR", "false")
        .arg("edit")
        .arg(&id)
        .arg("-i")
        .arg("--force")
        .assert()
        .success();
}

fn create_ticket(temp: &TempDir, title: &str) -> String {
    let out = tk_cmd(temp).arg("create").arg(title).output().unwrap();
    assert!(out.status.success());
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn ticket_json(temp: &TempDir, id: &str) -> serde_json::Value {
    let out = tk_cmd(temp)
        .arg("show")
        .arg(id)
        .arg("--json")
        .output()
        .unwrap();
    assert!(out.status.success());
    serde_json::from_slice(&out.stdout).unwrap()
}

fn ticket_contents(temp: &TempDir, id: &str) -> String {
    fs::read_to_string(temp.path().join(".tickets").join(format!("{id}.md"))).unwrap()
}

// MC/DC for cmd_edit's `has_updates` decision (src/cli.rs):
//   args.description.is_some()
//     || args.implementation_plan.is_some()
//     || args.acceptance.is_some()
//     || args.external_ref.is_some()
//     || args.body_from_file.is_some()
//   c1 = description.is_some()
//   c2 = implementation_plan.is_some()
//   c3 = acceptance.is_some()
//   c4 = external_ref.is_some()
//   c5 = body_from_file.is_some()
// A pure OR chain: the outcome is true iff at least one flag is passed;
// each single-true case is masked-independent against the all-false case.
// T0 (all false) = edit_print_alone_prints_path_without_mutation (only --print).
// Independence pairs (each single-true invocation vs T0):
//   c1: edit_description_empty_clears_it (its `-d` invocation) vs T0
//   c2: edit_implementation_plan_updates vs T0
//   c3: edit_description_updates_and_preserves_other_sections
//       (its `--acceptance "Must be green"` invocation) vs T0
//   c4: edit_external_ref_empty_removes_frontmatter_key
//       (its `--external-ref gh-99` invocation) vs T0
//   c5: edit_body_from_file_replaces_description vs T0
// Six cases cover five conditions -- the n+1 minimum.
//
// MC/DC for apply_edit_updates' set_description guard (src/cli.rs):
//   args.description.is_some() || args.body_from_file.is_some()
//   c1 = description.is_some()
//   c2 = body_from_file.is_some()
// This decision is only reached once has_updates is already true, so its
// (F,F) case is an invocation that passes some other section flag.
//   (F,F): edit_acceptance_and_external_ref_together (neither -d nor --body-from-file)
//   (T,F): edit_description_updates_and_preserves_other_sections (its `-d` invocation)
//   (F,T): edit_body_from_file_replaces_description
//   (T,T): edit_description_and_body_from_file_merge
// Independence pairs:
//   c1: (T,F) vs (F,F)
//   c2: (F,T) vs (F,F)
#[test]
fn edit_bare_requires_a_mode() {
    let temp = TempDir::new().unwrap();
    let id = create_ticket(&temp, "Needs a mode");

    tk_cmd(&temp)
        .arg("edit")
        .arg(&id)
        .assert()
        .code(2)
        .stderr(predicate::str::contains("--interactive"));
}

#[test]
fn edit_force_alone_requires_a_mode() {
    let temp = TempDir::new().unwrap();
    let id = create_ticket(&temp, "Force needs a mode");

    // --force does not satisfy the required edit-mode group on its own.
    tk_cmd(&temp)
        .arg("edit")
        .arg(&id)
        .arg("--force")
        .assert()
        .code(2)
        .stderr(predicate::str::contains("--interactive"));
}

#[test]
fn edit_print_alone_prints_path_without_mutation() {
    let temp = TempDir::new().unwrap();
    let id = create_ticket(&temp, "Print Only");

    let before = ticket_contents(&temp, &id);

    let out = tk_cmd(&temp)
        .arg("edit")
        .arg(&id)
        .arg("--print")
        .output()
        .unwrap();
    assert!(out.status.success());
    let printed = String::from_utf8_lossy(&out.stdout);
    assert!(printed.contains(&id), "path printed");
    assert!(!printed.contains("Updated"), "no update confirmation");

    let after = ticket_contents(&temp, &id);
    assert_eq!(before, after, "ticket file unchanged");
}

#[test]
fn edit_description_updates_and_preserves_other_sections() {
    let temp = TempDir::new().unwrap();
    let id = create_ticket(&temp, "Describe Me");

    tk_cmd(&temp)
        .arg("edit")
        .arg(&id)
        .arg("--acceptance")
        .arg("Must be green")
        .assert()
        .success();

    let out = tk_cmd(&temp)
        .arg("edit")
        .arg(&id)
        .arg("-d")
        .arg("Fresh description")
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains(&format!("Updated {id}")));

    let json = ticket_json(&temp, &id);
    assert_eq!(json["body"]["description"], "Fresh description");
    assert_eq!(json["body"]["acceptance"], "Must be green");

    let contents = ticket_contents(&temp, &id);
    assert!(contents.contains("## Implementation Plan"));
    assert!(contents.contains("## Acceptance Criteria"));
    assert!(contents.contains("## Notes"));
}

#[test]
fn edit_description_empty_clears_it() {
    let temp = TempDir::new().unwrap();
    let id = create_ticket(&temp, "Clear Me");

    tk_cmd(&temp)
        .arg("edit")
        .arg(&id)
        .arg("-d")
        .arg("Something to clear")
        .assert()
        .success();

    tk_cmd(&temp)
        .arg("edit")
        .arg(&id)
        .arg("-d")
        .arg("")
        .assert()
        .success();

    let json = ticket_json(&temp, &id);
    assert!(json["body"]["description"].is_null(), "description cleared");

    // The description occupies the untitled space before the first section
    // heading; when cleared it renders as the `-` placeholder there.
    let contents = ticket_contents(&temp, &id);
    let before_impl = contents.split("## Implementation Plan").next().unwrap();
    assert!(
        before_impl.trim_end().ends_with('-'),
        "cleared description renders the placeholder, got: {before_impl:?}"
    );
}

#[test]
fn edit_implementation_plan_updates() {
    let temp = TempDir::new().unwrap();
    let id = create_ticket(&temp, "Plan Me");

    let out = tk_cmd(&temp)
        .arg("edit")
        .arg(&id)
        .arg("--implementation-plan")
        .arg("Do step one, then step two")
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains(&format!("Updated {id}")));

    let json = ticket_json(&temp, &id);
    assert_eq!(
        json["body"]["implementation_plan"],
        "Do step one, then step two"
    );
    // The implementation-plan-only edit leaves every other section untouched.
    assert!(json["body"]["description"].is_null());
    assert!(json["body"]["acceptance"].is_null());
    assert_eq!(json["body"]["notes"], serde_json::json!([]));

    // All canonical headings remain present on disk so the ticket round-trips.
    let contents = ticket_contents(&temp, &id);
    assert!(contents.contains("## Implementation Plan"));
    assert!(contents.contains("## Acceptance Criteria"));
    assert!(contents.contains("## Notes"));
}

#[test]
fn edit_acceptance_and_external_ref_together() {
    let temp = TempDir::new().unwrap();
    let id = create_ticket(&temp, "Two Fields");

    tk_cmd(&temp)
        .arg("edit")
        .arg(&id)
        .arg("--acceptance")
        .arg("Ship it")
        .arg("--external-ref")
        .arg("gh-42")
        .assert()
        .success();

    let json = ticket_json(&temp, &id);
    assert_eq!(json["body"]["acceptance"], "Ship it");
    assert_eq!(json["external_ref"], "gh-42");
}

#[test]
fn edit_external_ref_empty_removes_frontmatter_key() {
    let temp = TempDir::new().unwrap();
    let id = create_ticket(&temp, "Ref Removal");

    tk_cmd(&temp)
        .arg("edit")
        .arg(&id)
        .arg("--external-ref")
        .arg("gh-99")
        .assert()
        .success();
    assert!(ticket_contents(&temp, &id).contains("external_ref: gh-99"));

    tk_cmd(&temp)
        .arg("edit")
        .arg(&id)
        .arg("--external-ref")
        .arg("")
        .assert()
        .success();

    let contents = ticket_contents(&temp, &id);
    let frontmatter = contents.split("---").nth(1).unwrap();
    assert!(
        !frontmatter.contains("external_ref"),
        "cleared external_ref key is omitted, not serialized as null"
    );

    // Regression: a freshly created ticket with no --external-ref also omits
    // the key entirely.
    let fresh_id = create_ticket(&temp, "No Ref");
    let fresh = ticket_contents(&temp, &fresh_id);
    let fresh_fm = fresh.split("---").nth(1).unwrap();
    assert!(!fresh_fm.contains("external_ref"));
}

#[test]
fn edit_body_from_file_replaces_description() {
    let temp = TempDir::new().unwrap();
    let id = create_ticket(&temp, "From File");

    let body_path = temp.path().join("body.txt");
    fs::write(&body_path, "  File-sourced body  \n").unwrap();

    tk_cmd(&temp)
        .arg("edit")
        .arg(&id)
        .arg("--body-from-file")
        .arg(&body_path)
        .assert()
        .success();

    let json = ticket_json(&temp, &id);
    assert_eq!(json["body"]["description"], "File-sourced body");
}

#[test]
fn edit_description_and_body_from_file_merge() {
    let temp = TempDir::new().unwrap();
    let id = create_ticket(&temp, "Merge Me");

    let body_path = temp.path().join("body.txt");
    fs::write(&body_path, "Body paragraph").unwrap();

    tk_cmd(&temp)
        .arg("edit")
        .arg(&id)
        .arg("-d")
        .arg("Intro")
        .arg("--body-from-file")
        .arg(&body_path)
        .assert()
        .success();

    let json = ticket_json(&temp, &id);
    assert_eq!(json["body"]["description"], "Intro\n\nBody paragraph");
}

#[test]
fn edit_body_from_file_empty_clears_description() {
    let temp = TempDir::new().unwrap();
    let id = create_ticket(&temp, "Empty File");

    tk_cmd(&temp)
        .arg("edit")
        .arg(&id)
        .arg("-d")
        .arg("Pre-existing")
        .assert()
        .success();

    let body_path = temp.path().join("empty.txt");
    fs::write(&body_path, "   \n").unwrap();

    tk_cmd(&temp)
        .arg("edit")
        .arg(&id)
        .arg("--body-from-file")
        .arg(&body_path)
        .assert()
        .success();

    let json = ticket_json(&temp, &id);
    assert!(
        json["body"]["description"].is_null(),
        "empty file clears description to None"
    );
}

#[test]
fn edit_interactive_with_update_applies_and_prints_path() {
    let temp = TempDir::new().unwrap();
    let id = create_ticket(&temp, "Apply Then Editor");

    // Non-TTY under the harness: the editor path prints the path instead of
    // launching. The update flag is still applied first. EDITOR=false would
    // fail the command if it were launched.
    let out = tk_cmd(&temp)
        .env("EDITOR", "false")
        .arg("edit")
        .arg(&id)
        .arg("-i")
        .arg("-d")
        .arg("Applied via interactive")
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains(&format!("Updated {id}")), "update applied");
    assert!(stdout.contains(&id), "path printed");

    let json = ticket_json(&temp, &id);
    assert_eq!(json["body"]["description"], "Applied via interactive");
}

#[test]
fn edit_interactive_print_prints_path_without_editor() {
    let temp = TempDir::new().unwrap();
    let id = create_ticket(&temp, "Interactive Print");

    // --print short-circuits the editor path; EDITOR=false would fail if run.
    let out = tk_cmd(&temp)
        .env("EDITOR", "false")
        .arg("edit")
        .arg(&id)
        .arg("-i")
        .arg("--print")
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stdout).contains(&id),
        "path printed"
    );
}

#[test]
fn create_edit_launches_editor_post_create() {
    let temp = TempDir::new().unwrap();

    // Non-TTY: create --edit reaches the editor path but prints the path
    // rather than launching. EDITOR=true keeps the exit successful.
    tk_cmd(&temp)
        .env("EDITOR", "true")
        .arg("create")
        .arg("Create And Edit")
        .arg("--edit")
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
fn add_note_avoids_duplicate_headers_and_supports_tag_and_newlines() {
    let temp = TempDir::new().unwrap();

    let id = {
        let mut create = tk_cmd(&temp);
        let out = create.arg("create").arg("Note Test").output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };

    // First note inserts header
    tk_cmd(&temp)
        .arg("add-note")
        .arg(&id)
        .arg("First note")
        .assert()
        .success();

    // Second note should not duplicate header, and supports tag with trailing newlines
    tk_cmd(&temp)
        .arg("add-note")
        .arg(&id)
        .arg("Second note with trailing\n\n")
        .arg("--tag")
        .arg("infra")
        .assert()
        .success();

    let path = temp.path().join(".tickets").join(format!("{id}.md"));
    let contents = fs::read_to_string(path).unwrap();
    let notes_sections: Vec<_> = contents.match_indices("## Notes").collect();
    assert_eq!(notes_sections.len(), 1, "expected single Notes header");
    assert!(contents.contains("First note"));
    assert!(contents.contains("Second note with trailing"));
    assert!(contents.contains("[infra]"));
    assert!(contents.ends_with('\n'));
}

#[test]
fn query_outputs_ndjson_and_pretty_and_filters_without_jq() {
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

    // Default: ndjson one object per line
    let out = tk_cmd(&temp).arg("query").output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<_> = stdout.lines().collect();
    assert_eq!(lines.len(), 2, "expected ndjson lines");
    for line in &lines {
        serde_json::from_str::<serde_json::Value>(line).expect("valid ndjson");
    }
    assert!(stdout.contains(&one));
    assert!(stdout.contains(&two));

    // Pretty array output
    let pretty = tk_cmd(&temp)
        .arg("query")
        .arg("--format")
        .arg("pretty")
        .output()
        .unwrap();
    assert!(pretty.status.success());
    let val: serde_json::Value = serde_json::from_slice(&pretty.stdout).unwrap();
    let arr = val.as_array().expect("array");
    assert_eq!(arr.len(), 2);

    // Built-in filtering: tag match and substring match
    tk_cmd(&temp)
        .arg("query")
        .arg("tags==x")
        .assert()
        .success()
        .stdout(predicate::str::contains(&one))
        .stdout(predicate::str::contains(&two).not());

    tk_cmd(&temp)
        .arg("query")
        .arg("title~Two")
        .assert()
        .success()
        .stdout(predicate::str::contains(&two))
        .stdout(predicate::str::contains(&one).not());
}

#[test]
fn query_handles_large_ticket_sets_without_quadratic_output() {
    let temp = TempDir::new().unwrap();

    let mut created = Vec::new();
    for idx in 0..200 {
        let id = format!("bulk-{idx:03}");
        write_ticket(&temp, &id, &[]);
        created.push(id);
    }

    let out = tk_cmd(&temp).arg("query").output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<_> = stdout.lines().collect();
    assert_eq!(lines.len(), created.len());
}

#[test]
fn generate_id_uses_three_char_prefix_for_single_segment_dirs() {
    let temp = TempDir::new().unwrap();
    // Create a single-segment dir name "plan" to check prefix
    let single = temp.path().join("plan");
    fs::create_dir_all(&single).unwrap();

    let mut cmd = tk_cmd(&temp);
    let out = cmd
        .current_dir(&single)
        .arg("create")
        .arg("Alpha")
        .output()
        .unwrap();
    assert!(out.status.success());
    let id = String::from_utf8_lossy(&out.stdout).trim().to_string();

    assert!(id.starts_with("pla-"), "expected 3-char prefix: {id}");

    // Multi-segment should still use first letters of segments
    let multi = temp.path().join("foo-bar");
    fs::create_dir_all(&multi).unwrap();
    let out2 = tk_cmd(&temp)
        .current_dir(&multi)
        .arg("create")
        .arg("Beta")
        .output()
        .unwrap();
    assert!(out2.status.success());
    let id2 = String::from_utf8_lossy(&out2.stdout).trim().to_string();
    assert!(id2.starts_with("fb-"), "expected segment initials: {id2}");
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
fn undep_resolves_partials_and_errors_when_missing() {
    let temp = TempDir::new().unwrap();

    // Create tickets with overlapping prefixes to exercise partial resolution
    let alpha = {
        let mut create = tk_cmd(&temp);
        let out = create.arg("create").arg("Alpha ticket").output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    let alps = {
        let mut create = tk_cmd(&temp);
        let out = create.arg("create").arg("Alps task").output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    let beta = {
        let mut create = tk_cmd(&temp);
        let out = create.arg("create").arg("Beta dep").output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };

    // Add dependency using full IDs
    tk_cmd(&temp)
        .arg("dep")
        .arg(&alpha)
        .arg(&beta)
        .assert()
        .success();

    // Ambiguous partial for ticket should error (use shared prefix of alpha/alps)
    let mut common = String::new();
    for (a, b) in alpha.chars().zip(alps.chars()) {
        if a == b {
            common.push(a);
        } else {
            break;
        }
    }
    if common.is_empty() {
        common.push_str(&alpha[..1.min(alpha.len())]);
    }
    tk_cmd(&temp)
        .arg("undep")
        .arg(&common)
        .arg(&beta)
        .assert()
        .failure()
        .stderr(predicate::str::contains("ambiguous ID"));

    // Missing dependency should error
    tk_cmd(&temp)
        .arg("undep")
        .arg(&alpha)
        .arg("nope")
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found"));

    // Valid partials succeed
    // Find a unique prefix for beta not shared with others
    let mut beta_partial = String::new();
    for len in 1..=beta.len() {
        let sub = &beta[..len];
        if !alpha.contains(sub) && !alps.contains(sub) {
            beta_partial = sub.to_string();
            break;
        }
    }
    if beta_partial.is_empty() {
        beta_partial = beta.clone();
    }
    tk_cmd(&temp)
        .arg("undep")
        .arg(&alpha)
        .arg(&beta_partial)
        .assert()
        .success()
        .stdout(predicate::str::contains("Removed dependency"));

    let ticket_path = temp.path().join(".tickets").join(format!("{alpha}.md"));
    let contents = fs::read_to_string(ticket_path).unwrap();
    assert!(contents.contains("deps: []"));

    // Ensure alps untouched
    let alps_path = temp.path().join(".tickets").join(format!("{alps}.md"));
    let alps_contents = fs::read_to_string(alps_path).unwrap();
    assert!(alps_contents.contains("deps: []"));
}

#[test]
fn undep_is_idempotent_and_normalizes_empty() {
    let temp = TempDir::new().unwrap();

    let dep = {
        let mut create = tk_cmd(&temp);
        let out = create.arg("create").arg("Dep").output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };

    let ticket = {
        let mut create = tk_cmd(&temp);
        let out = create.arg("create").arg("Root").output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };

    tk_cmd(&temp)
        .arg("dep")
        .arg(&ticket)
        .arg(&dep)
        .assert()
        .success();

    // first removal succeeds
    tk_cmd(&temp)
        .arg("undep")
        .arg(&ticket)
        .arg(&dep)
        .assert()
        .success()
        .stdout(predicate::str::contains("Removed dependency"));

    // second removal is idempotent and reports already removed
    tk_cmd(&temp)
        .arg("undep")
        .arg(&ticket)
        .arg(&dep)
        .assert()
        .success()
        .stdout(predicate::str::contains("Dependency not present"));

    // deps normalized to []
    let path = tk_cmd(&temp)
        .arg("edit")
        .arg(&ticket)
        .arg("--print")
        .output()
        .unwrap();
    let ticket_path = String::from_utf8_lossy(&path.stdout).trim().to_string();
    let contents = std::fs::read_to_string(ticket_path).unwrap();
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

/// Count the `.md` ticket files currently in the temp workspace's `.tickets`
/// directory, tolerating the directory not existing yet.
fn ticket_file_count(temp: &TempDir) -> usize {
    let dir = temp.path().join(".tickets");
    match fs::read_dir(&dir) {
        Ok(entries) => entries
            .flatten()
            .filter(|e| e.path().extension().map(|x| x == "md").unwrap_or(false))
            .count(),
        Err(_) => 0,
    }
}

#[test]
fn edit_acceptance_trims_whitespace() {
    let temp = TempDir::new().unwrap();
    let id = create_ticket(&temp, "Trim Me");

    tk_cmd(&temp)
        .arg("edit")
        .arg(&id)
        .arg("--acceptance")
        .arg("  x  ")
        .assert()
        .success();

    let json = ticket_json(&temp, &id);
    assert_eq!(json["body"]["acceptance"], "x");
}

#[test]
fn create_rejects_description_with_embedded_reserved_heading() {
    let temp = TempDir::new().unwrap();

    tk_cmd(&temp)
        .arg("create")
        .arg("Sneaky Notes")
        .arg("-d")
        .arg("Intro line\n## Notes\nsmuggled note")
        .assert()
        .failure()
        .stderr(predicate::str::contains("## Notes"));

    assert_eq!(
        ticket_file_count(&temp),
        0,
        "rejected create writes no ticket file"
    );
}

#[test]
fn create_rejects_acceptance_starting_with_reserved_heading() {
    let temp = TempDir::new().unwrap();

    tk_cmd(&temp)
        .arg("create")
        .arg("Design In Acceptance")
        .arg("--acceptance")
        .arg("## Design\nlots of detail")
        .assert()
        .failure()
        .stderr(predicate::str::contains("## Design"));

    assert_eq!(
        ticket_file_count(&temp),
        0,
        "rejected create writes no ticket file"
    );
}

#[test]
fn edit_rejects_implementation_plan_with_reserved_heading() {
    let temp = TempDir::new().unwrap();
    let id = create_ticket(&temp, "Plan Guard");

    let before = ticket_contents(&temp, &id);

    tk_cmd(&temp)
        .arg("edit")
        .arg(&id)
        .arg("--implementation-plan")
        .arg("Step one\n## Acceptance Criteria\nleaked criteria")
        .assert()
        .failure()
        .stderr(predicate::str::contains("## Acceptance Criteria"));

    assert_eq!(
        ticket_contents(&temp, &id),
        before,
        "rejected edit leaves the ticket file unchanged"
    );
}

#[test]
fn edit_rejects_body_from_file_with_reserved_heading() {
    let temp = TempDir::new().unwrap();
    let id = create_ticket(&temp, "File Guard");

    let before = ticket_contents(&temp, &id);

    let body_path = temp.path().join("body.txt");
    fs::write(&body_path, "Preamble\n## Notes\nnot allowed here\n").unwrap();

    tk_cmd(&temp)
        .arg("edit")
        .arg(&id)
        .arg("--body-from-file")
        .arg(&body_path)
        .assert()
        .failure()
        .stderr(predicate::str::contains("## Notes"));

    assert_eq!(
        ticket_contents(&temp, &id),
        before,
        "rejected body-from-file edit leaves the ticket file unchanged"
    );
}

#[test]
fn add_note_rejects_reserved_heading() {
    let temp = TempDir::new().unwrap();
    let id = create_ticket(&temp, "Note Guard");

    let before = ticket_contents(&temp, &id);

    tk_cmd(&temp)
        .arg("add-note")
        .arg(&id)
        .arg("## Notes\nnested heading")
        .assert()
        .failure()
        .stderr(predicate::str::contains("## Notes"));

    assert_eq!(
        ticket_contents(&temp, &id),
        before,
        "rejected note leaves the ticket file unchanged"
    );
}

#[test]
fn close_note_rejects_reserved_heading() {
    let temp = TempDir::new().unwrap();
    let id = create_ticket(&temp, "Close Guard");

    tk_cmd(&temp)
        .arg("close")
        .arg(&id)
        .arg("--note")
        .arg("## Design\nnot a note")
        .assert()
        .failure()
        .stderr(predicate::str::contains("## Design"));

    // The shared note path validates before persisting, so the status stays open.
    let contents = ticket_contents(&temp, &id);
    assert!(contents.contains("status: open"));
    assert!(!contents.contains("status: closed"));
}

#[test]
fn create_rejects_multiline_title() {
    let temp = TempDir::new().unwrap();

    tk_cmd(&temp)
        .arg("create")
        .arg("Title\n## Notes\ninjected")
        .assert()
        .failure()
        .stderr(predicate::str::contains("single line"));

    assert_eq!(
        ticket_file_count(&temp),
        0,
        "rejected create writes no ticket file"
    );
}

#[test]
fn add_note_rejects_multiline_tag() {
    let temp = TempDir::new().unwrap();
    let id = create_ticket(&temp, "Tag Guard");

    let before = ticket_contents(&temp, &id);

    tk_cmd(&temp)
        .arg("add-note")
        .arg(&id)
        .arg("harmless text")
        .arg("--tag")
        .arg("x\n## Acceptance Criteria")
        .assert()
        .failure()
        .stderr(predicate::str::contains("single line"));

    assert_eq!(
        ticket_contents(&temp, &id),
        before,
        "rejected tag leaves the ticket file unchanged"
    );
}

#[test]
fn create_rejects_literal_dash_acceptance() {
    let temp = TempDir::new().unwrap();

    tk_cmd(&temp)
        .arg("create")
        .arg("Dash Acceptance")
        .arg("--acceptance")
        .arg("-")
        .assert()
        .failure()
        .stderr(predicate::str::contains("'-'"));

    assert_eq!(
        ticket_file_count(&temp),
        0,
        "rejected create writes no ticket file"
    );
}

#[test]
fn edit_rejects_description_trimming_to_dash() {
    let temp = TempDir::new().unwrap();
    let id = create_ticket(&temp, "Dash Description");

    let before = ticket_contents(&temp, &id);

    tk_cmd(&temp)
        .arg("edit")
        .arg(&id)
        .arg("-d")
        .arg(" - ")
        .assert()
        .failure()
        .stderr(predicate::str::contains("'-'"));

    assert_eq!(
        ticket_contents(&temp, &id),
        before,
        "rejected edit leaves the ticket file unchanged"
    );
}

#[test]
fn create_allows_non_reserved_heading_lookalikes() {
    let temp = TempDir::new().unwrap();

    // `### Notes` (deeper level) and `Notes:` (no `## ` prefix) do not match a
    // reserved heading prefix, so they are stored verbatim.
    let out = tk_cmd(&temp)
        .arg("create")
        .arg("Lookalikes")
        .arg("-d")
        .arg("### Notes\nNotes: still fine")
        .output()
        .unwrap();
    assert!(out.status.success());
    let id = String::from_utf8_lossy(&out.stdout).trim().to_string();

    let json = ticket_json(&temp, &id);
    assert_eq!(json["body"]["description"], "### Notes\nNotes: still fine");
}

#[test]
fn update_help_describes_github_self_update() {
    let dir = TempDir::new().unwrap();
    tk_cmd(&dir)
        .args(["update", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Update tk to the latest release from GitHub",
        ));
}

#[test]
fn failed_write_preserves_original_ticket_contents() {
    let temp = TempDir::new().unwrap();
    let id = create_ticket(&temp, "Integrity Guard");
    let before = ticket_contents(&temp, &id);

    let tickets_dir = temp.path().join(".tickets");
    let mut perms = fs::metadata(&tickets_dir).unwrap().permissions();
    let original_perms = perms.clone();
    perms.set_readonly(true);
    fs::set_permissions(&tickets_dir, perms).unwrap();

    // Root (some CI environments) ignores directory write bits; skip there.
    let probe = tickets_dir.join(".write-probe");
    if fs::File::create(&probe).is_ok() {
        let _ = fs::remove_file(&probe);
        fs::set_permissions(&tickets_dir, original_perms).unwrap();
        return;
    }

    tk_cmd(&temp)
        .arg("edit")
        .arg(&id)
        .arg("-d")
        .arg("replacement text")
        .assert()
        .failure();

    fs::set_permissions(&tickets_dir, original_perms).unwrap();
    assert_eq!(
        ticket_contents(&temp, &id),
        before,
        "failed write leaves the ticket byte-for-byte intact"
    );
}

#[cfg(unix)]
#[test]
fn edit_preserves_restrictive_file_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let temp = TempDir::new().unwrap();
    let id = create_ticket(&temp, "Perms Guard");
    let path = temp.path().join(".tickets").join(format!("{id}.md"));
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();

    tk_cmd(&temp)
        .arg("edit")
        .arg(&id)
        .arg("-d")
        .arg("updated")
        .assert()
        .success();

    let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "0600 survives an edit");
}

#[cfg(unix)]
#[test]
fn edit_refuses_read_only_ticket_file() {
    use std::os::unix::fs::PermissionsExt;

    let temp = TempDir::new().unwrap();
    let id = create_ticket(&temp, "ReadOnly Guard");
    let path = temp.path().join(".tickets").join(format!("{id}.md"));
    let before = ticket_contents(&temp, &id);
    fs::set_permissions(&path, fs::Permissions::from_mode(0o444)).unwrap();

    tk_cmd(&temp)
        .arg("edit")
        .arg(&id)
        .arg("-d")
        .arg("updated")
        .assert()
        .failure()
        .stderr(predicate::str::contains("read-only"));

    let meta = fs::metadata(&path).unwrap();
    assert_eq!(meta.permissions().mode() & 0o777, 0o444, "mode unchanged");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(ticket_contents(&temp, &id), before, "bytes unchanged");
}

fn tree_create(dir: &TempDir, title: &str) -> String {
    let out = tk_cmd(dir).arg("create").arg(title).output().unwrap();
    assert!(out.status.success(), "create failed: {out:?}");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn tree_create_priority(dir: &TempDir, title: &str, priority: &str) -> String {
    let out = tk_cmd(dir)
        .arg("create")
        .arg(title)
        .arg("--priority")
        .arg(priority)
        .output()
        .unwrap();
    assert!(out.status.success(), "create failed: {out:?}");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// Shortest hash-tail substring of `id` that no other id in `others` contains,
/// so `resolve_partial_id` resolves it uniquely back to `id`.
fn unique_partial(id: &str, others: &[&str]) -> String {
    let hash = id.rsplit('-').next().unwrap_or(id);
    for len in 1..=hash.len() {
        let sub = &hash[..len];
        if !others.iter().any(|o| o.contains(sub)) {
            return sub.to_string();
        }
    }
    id.to_string()
}

/// The common leading substring of two ids (always their shared `prefix-`),
/// which `resolve_partial_id` treats as ambiguous because both ids contain it.
fn shared_prefix(a: &str, b: &str) -> String {
    let mut common = String::new();
    for (x, y) in a.chars().zip(b.chars()) {
        if x == y {
            common.push(x);
        } else {
            break;
        }
    }
    if common.is_empty() {
        common.push_str(&a[..1.min(a.len())]);
    }
    common
}

/// Sanitized Mermaid node id, mirroring `sanitize_mermaid_id` in
/// `src/tree.rs` -- keep the two in sync: every character outside
/// `[A-Za-z0-9_]` (the id's `-`, and the leading `.` of a temp-dir prefix)
/// becomes `_`, and the result is prefixed with `t_` (guards against a
/// Mermaid keyword collision, a leading digit, or an empty id).
fn mermaid_id(id: &str) -> String {
    let mut sanitized = String::from("t_");
    sanitized.extend(id.chars().map(|c| {
        if c.is_ascii_alphanumeric() || c == '_' {
            c
        } else {
            '_'
        }
    }));
    sanitized
}

const MERMAID_HEADER: &str = "%%{init: {\"flowchart\": {\"curve\": \"linear\"}}}%%\nflowchart TD\n";

#[test]
fn tree_root_restricts_to_subtree_full_and_partial_id() {
    let temp = TempDir::new().unwrap();
    let a = tree_create(&temp, "Epic A");
    let b = tree_create(&temp, "Mid B");
    let c = tree_create(&temp, "Leaf C");
    let d = tree_create(&temp, "Unrelated D");
    // Chain A -> B -> C; D stands alone.
    tk_cmd(&temp).arg("dep").arg(&a).arg(&b).assert().success();
    tk_cmd(&temp).arg("dep").arg(&b).arg(&c).assert().success();

    // Full id: rooting at B shows only B and its dependency C.
    tk_cmd(&temp)
        .arg("tree")
        .arg("--root")
        .arg(&b)
        .assert()
        .success()
        .stdout(predicate::str::contains(&b))
        .stdout(predicate::str::contains(&c))
        .stdout(predicate::str::contains(&a).not())
        .stdout(predicate::str::contains(&d).not());

    // Partial id resolves the same subtree.
    let partial = unique_partial(&b, &[&a, &c, &d]);
    tk_cmd(&temp)
        .arg("tree")
        .arg("--root")
        .arg(&partial)
        .assert()
        .success()
        .stdout(predicate::str::contains(&b))
        .stdout(predicate::str::contains(&c))
        .stdout(predicate::str::contains(&a).not())
        .stdout(predicate::str::contains(&d).not());
}

#[test]
fn tree_root_composes_with_inverted_and_status() {
    let temp = TempDir::new().unwrap();
    let l = tree_create(&temp, "Leaf");
    let m = tree_create(&temp, "Mid");
    let e = tree_create(&temp, "Epic");
    // Chain epic -> mid -> leaf.
    tk_cmd(&temp).arg("dep").arg(&m).arg(&l).assert().success();
    tk_cmd(&temp).arg("dep").arg(&e).arg(&m).assert().success();

    // --root fixes the scope to `m`'s own dependency closure ({m, l});
    // --inverted only changes how that fixed scope is presented (leaf-first).
    // `e` is never part of `m`'s own closure (it depends on `m`, not the
    // other way around), so it must not appear.
    tk_cmd(&temp)
        .arg("tree")
        .arg("--root")
        .arg(&m)
        .arg("--inverted")
        .assert()
        .success()
        .stdout(predicate::str::contains(&l))
        .stdout(predicate::str::contains(&m))
        .stdout(predicate::str::contains(&e).not());

    // Close the leaf: default open selection prunes it from the scope (and
    // so from the inverted rendering too), leaving only `m`; `--status all`
    // brings it back.
    tk_cmd(&temp).arg("close").arg(&l).assert().success();
    tk_cmd(&temp)
        .arg("tree")
        .arg("--root")
        .arg(&m)
        .arg("--inverted")
        .assert()
        .success()
        .stdout(predicate::str::contains(&m))
        .stdout(predicate::str::contains(&l).not());
    tk_cmd(&temp)
        .arg("tree")
        .arg("--root")
        .arg(&m)
        .arg("--inverted")
        .arg("--status")
        .arg("all")
        .assert()
        .success()
        .stdout(predicate::str::contains(&l));
}

#[test]
fn tree_root_epic_inverted_is_leaf_first_and_excludes_out_of_scope_dependant() {
    let temp = TempDir::new().unwrap();
    let l = tree_create(&temp, "Leaf");
    let m = tree_create(&temp, "Mid");
    let e = tree_create(&temp, "Epic");
    let z = tree_create(&temp, "Zeta");
    // Chain epic -> mid -> leaf; zeta depends on the epic (outside its scope).
    tk_cmd(&temp).arg("dep").arg(&m).arg(&l).assert().success();
    tk_cmd(&temp).arg("dep").arg(&e).arg(&m).assert().success();
    tk_cmd(&temp).arg("dep").arg(&z).arg(&e).assert().success();

    let out = tk_cmd(&temp)
        .arg("tree")
        .arg("--root")
        .arg(&e)
        .arg("--inverted")
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);

    // Leaf-first: the leaf nests above mid, which nests above the epic.
    let l_pos = stdout.find(l.as_str()).expect("leaf present");
    let m_pos = stdout.find(m.as_str()).expect("mid present");
    let e_pos = stdout.find(e.as_str()).expect("epic present");
    assert!(l_pos < m_pos && m_pos < e_pos);
    assert!(
        !stdout.contains(z.as_str()),
        "zeta depends on the epic rather than being depended on by it, so it is \
         out of the epic's own dependency closure: {stdout}"
    );
}

#[test]
fn tree_root_errors_on_unknown_and_ambiguous_id() {
    let temp = TempDir::new().unwrap();
    let alpha = tree_create(&temp, "Alpha");
    let alps = tree_create(&temp, "Alps");

    // Unknown id resolves to nothing, like every other resolve_partial_id path.
    tk_cmd(&temp)
        .arg("tree")
        .arg("--root")
        .arg("no-such-id")
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found"));

    // The shared prefix matches both tickets.
    let common = shared_prefix(&alpha, &alps);
    tk_cmd(&temp)
        .arg("tree")
        .arg("--root")
        .arg(&common)
        .assert()
        .failure()
        .stderr(predicate::str::contains("ambiguous ID"));
}

#[test]
fn tree_root_on_closed_ticket_prints_nothing_by_default() {
    let temp = TempDir::new().unwrap();
    let t = tree_create(&temp, "Solo");
    tk_cmd(&temp).arg("close").arg(&t).assert().success();

    // Default open selection filters the closed root out: empty output, not a
    // shallow tree.
    tk_cmd(&temp)
        .arg("tree")
        .arg("--root")
        .arg(&t)
        .assert()
        .success()
        .stdout(predicate::str::is_empty());
}

#[test]
fn graph_emits_directive_header_node_and_edge_blocks() {
    let temp = TempDir::new().unwrap();
    let root = tree_create(&temp, "Root");
    let child = tree_create(&temp, "Child");
    tk_cmd(&temp)
        .arg("dep")
        .arg(&root)
        .arg(&child)
        .assert()
        .success();

    let out = tk_cmd(&temp).arg("graph").output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);

    assert!(
        stdout.starts_with(MERMAID_HEADER),
        "graph must open with the init directive and flowchart header: {stdout}"
    );
    let root_s = mermaid_id(&root);
    let child_s = mermaid_id(&child);
    // Node block: `    <id>["<label>"]` per node.
    assert!(stdout.contains(&format!("{root_s}[\"[P2] {root}: Root\"]")));
    assert!(stdout.contains(&format!("{child_s}[\"[P2] {child}: Child\"]")));
    // Edge block: dependant --> dependency under the default normal orientation.
    assert!(stdout.contains(&format!("{root_s} --> {child_s}")));
}

#[test]
fn graph_root_partial_restricts_and_inverted_flips_edges() {
    let temp = TempDir::new().unwrap();
    let l = tree_create(&temp, "Leaf");
    let m = tree_create(&temp, "Mid");
    let e = tree_create(&temp, "Epic");
    let z = tree_create(&temp, "Zeta");
    // Chain epic -> mid -> leaf; zeta unrelated.
    tk_cmd(&temp).arg("dep").arg(&m).arg(&l).assert().success();
    tk_cmd(&temp).arg("dep").arg(&e).arg(&m).assert().success();

    let l_s = mermaid_id(&l);
    let m_s = mermaid_id(&m);
    let e_s = mermaid_id(&e);
    let z_s = mermaid_id(&z);

    // Normal orientation: dependant --> dependency.
    let normal = tk_cmd(&temp).arg("graph").output().unwrap();
    assert!(normal.status.success());
    let normal_out = String::from_utf8_lossy(&normal.stdout);
    assert!(normal_out.contains(&format!("{e_s} --> {m_s}")));
    assert!(normal_out.contains(&format!("{m_s} --> {l_s}")));
    assert!(!normal_out.contains(&format!("{m_s} --> {e_s}")));

    // Inverted orientation flips every edge to dependency --> dependant.
    let inverted = tk_cmd(&temp)
        .arg("graph")
        .arg("--inverted")
        .output()
        .unwrap();
    assert!(inverted.status.success());
    let inverted_out = String::from_utf8_lossy(&inverted.stdout);
    assert!(inverted_out.contains(&format!("{l_s} --> {m_s}")));
    assert!(inverted_out.contains(&format!("{m_s} --> {e_s}")));
    assert!(!inverted_out.contains(&format!("{e_s} --> {m_s}")));

    // Partial --root restricts to the mid subtree: only the mid -> leaf edge,
    // with the epic and the unrelated zeta absent.
    let partial = unique_partial(&m, &[&l, &e, &z]);
    let restricted = tk_cmd(&temp)
        .arg("graph")
        .arg("--root")
        .arg(&partial)
        .output()
        .unwrap();
    assert!(restricted.status.success());
    let restricted_out = String::from_utf8_lossy(&restricted.stdout);
    assert!(restricted_out.contains(&format!("{m_s} --> {l_s}")));
    assert!(!restricted_out.contains(&e_s));
    assert!(!restricted_out.contains(&z_s));
}

#[test]
fn graph_root_epic_inverted_emits_dependency_to_dependant_edges_within_scope() {
    let temp = TempDir::new().unwrap();
    let l = tree_create(&temp, "Leaf");
    let m = tree_create(&temp, "Mid");
    let e = tree_create(&temp, "Epic");
    let z = tree_create(&temp, "Zeta");
    // Chain epic -> mid -> leaf; zeta depends on the epic (outside its scope).
    tk_cmd(&temp).arg("dep").arg(&m).arg(&l).assert().success();
    tk_cmd(&temp).arg("dep").arg(&e).arg(&m).assert().success();
    tk_cmd(&temp).arg("dep").arg(&z).arg(&e).assert().success();

    let l_s = mermaid_id(&l);
    let m_s = mermaid_id(&m);
    let e_s = mermaid_id(&e);
    let z_s = mermaid_id(&z);

    // --root fixes the scope to the epic's own closure ({e, m, l}); --inverted
    // presents it dependency --> dependant, leaf-first, ending at the epic.
    let out = tk_cmd(&temp)
        .arg("graph")
        .arg("--root")
        .arg(&e)
        .arg("--inverted")
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains(&format!("{l_s} --> {m_s}")));
    assert!(stdout.contains(&format!("{m_s} --> {e_s}")));
    assert!(
        !stdout.contains(&z_s),
        "zeta is outside the epic's own dependency closure: {stdout}"
    );
}

#[test]
fn graph_root_on_closed_ticket_prints_directive_and_header_only() {
    let temp = TempDir::new().unwrap();
    let t = tree_create(&temp, "Solo");
    tk_cmd(&temp).arg("close").arg(&t).assert().success();

    // Default open selection drops the only root, so no nodes and no edge block
    // are rendered -- just the directive and header.
    tk_cmd(&temp)
        .arg("graph")
        .arg("--root")
        .arg(&t)
        .assert()
        .success()
        .stdout(predicate::eq(MERMAID_HEADER));
}

#[test]
fn graph_root_errors_on_unknown_and_ambiguous_id() {
    let temp = TempDir::new().unwrap();
    let alpha = tree_create(&temp, "Alpha");
    let alps = tree_create(&temp, "Alps");

    tk_cmd(&temp)
        .arg("graph")
        .arg("--root")
        .arg("no-such-id")
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found"));

    let common = shared_prefix(&alpha, &alps);
    tk_cmd(&temp)
        .arg("graph")
        .arg("--root")
        .arg(&common)
        .assert()
        .failure()
        .stderr(predicate::str::contains("ambiguous ID"));
}

#[test]
fn tree_on_empty_repo_prints_nothing() {
    let temp = TempDir::new().unwrap();
    tk_cmd(&temp)
        .arg("tree")
        .assert()
        .success()
        .stdout(predicate::str::is_empty());
}

#[test]
fn tree_renders_connectors_and_indentation() {
    let temp = TempDir::new().unwrap();
    let r = tree_create(&temp, "Root");
    let c1 = tree_create(&temp, "Child One");
    let c2 = tree_create(&temp, "Child Two");
    let g1 = tree_create(&temp, "Grand One");
    let g2 = tree_create(&temp, "Grand Two");
    // R depends on both children; each child owns one grandchild.
    tk_cmd(&temp).arg("dep").arg(&r).arg(&c1).assert().success();
    tk_cmd(&temp).arg("dep").arg(&r).arg(&c2).assert().success();
    tk_cmd(&temp)
        .arg("dep")
        .arg(&c1)
        .arg(&g1)
        .assert()
        .success();
    tk_cmd(&temp)
        .arg("dep")
        .arg(&c2)
        .arg(&g2)
        .assert()
        .success();

    // `dep_add` stores deps sorted by id, so order the two branches the same way
    // to build the expected rendering deterministically.
    let mut branches = [
        (c1.clone(), "Child One", g1.clone(), "Grand One"),
        (c2.clone(), "Child Two", g2.clone(), "Grand Two"),
    ];
    branches.sort_by(|a, b| a.0.cmp(&b.0));
    let (fid, ft, fg, fgt) = &branches[0];
    let (sid, st, sg, sgt) = &branches[1];

    // Non-last child: `├── ` with a `│   ` continuation into its subtree.
    // Last child: `└── ` with a four-space continuation. Root carries no glyph.
    let expected = format!(
        "[P2] {r}: Root\n├── [P2] {fid}: {ft}\n│   └── [P2] {fg}: {fgt}\n└── [P2] {sid}: {st}\n    └── [P2] {sg}: {sgt}\n"
    );
    tk_cmd(&temp)
        .arg("tree")
        .assert()
        .success()
        .stdout(predicate::eq(expected.as_str()));
}

#[test]
fn tree_prints_multiple_roots_at_column_zero() {
    let temp = TempDir::new().unwrap();
    let a = tree_create_priority(&temp, "Alpha", "0");
    let b = tree_create_priority(&temp, "Beta", "1");
    // Two independent roots, ordered by priority (0 before 1), both at column 0.
    let expected = format!("[P0] {a}: Alpha\n[P1] {b}: Beta\n");
    tk_cmd(&temp)
        .arg("tree")
        .arg("--status")
        .arg("all")
        .assert()
        .success()
        .stdout(predicate::eq(expected.as_str()));
}

#[test]
fn tree_default_hides_closed_tickets() {
    let temp = TempDir::new().unwrap();
    let open = tree_create(&temp, "Still Open");
    let closed = tree_create(&temp, "Done");
    tk_cmd(&temp).arg("close").arg(&closed).assert().success();

    tk_cmd(&temp)
        .arg("tree")
        .assert()
        .success()
        .stdout(predicate::str::contains(&open))
        .stdout(predicate::str::contains(&closed).not());
}

#[test]
fn tree_status_all_shows_open_and_closed() {
    let temp = TempDir::new().unwrap();
    let open = tree_create(&temp, "Still Open");
    let closed = tree_create(&temp, "Done");
    tk_cmd(&temp).arg("close").arg(&closed).assert().success();

    tk_cmd(&temp)
        .arg("tree")
        .arg("--status")
        .arg("all")
        .assert()
        .success()
        .stdout(predicate::str::contains(&open))
        .stdout(predicate::str::contains(&closed));
}

#[test]
fn tree_status_closed_shows_only_closed() {
    let temp = TempDir::new().unwrap();
    let open = tree_create(&temp, "Still Open");
    let closed = tree_create(&temp, "Done");
    tk_cmd(&temp).arg("close").arg(&closed).assert().success();

    tk_cmd(&temp)
        .arg("tree")
        .arg("--status")
        .arg("closed")
        .assert()
        .success()
        .stdout(predicate::str::contains(&closed))
        .stdout(predicate::str::contains(&open).not());
}

#[test]
fn tree_rejects_invalid_status_value() {
    let temp = TempDir::new().unwrap();
    tk_cmd(&temp)
        .arg("tree")
        .arg("--status")
        .arg("bogus")
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid value"));
}

#[test]
fn tree_inverted_makes_leaves_roots_with_epic_nested() {
    let temp = TempDir::new().unwrap();
    let a = tree_create(&temp, "Leaf A");
    let b = tree_create(&temp, "Leaf B");
    let e = tree_create(&temp, "Epic");
    // Epic depends on both leaves.
    tk_cmd(&temp).arg("dep").arg(&e).arg(&a).assert().success();
    tk_cmd(&temp).arg("dep").arg(&e).arg(&b).assert().success();

    // Roots and `dep_add`-sorted deps both order by id; sort the leaves to
    // build the expected rendering deterministically.
    let mut leaves = [(a.clone(), "Leaf A"), (b.clone(), "Leaf B")];
    leaves.sort_by(|x, y| x.0.cmp(&y.0));
    let (fid, ft) = &leaves[0];
    let (sid, st) = &leaves[1];

    // Default (normal): the epic is the root with both leaves nested under it.
    let normal_expected = format!("[P2] {e}: Epic\n├── [P2] {fid}: {ft}\n└── [P2] {sid}: {st}\n");
    tk_cmd(&temp)
        .arg("tree")
        .assert()
        .success()
        .stdout(predicate::eq(normal_expected.as_str()));

    // Inverted: each leaf is a column-0 root with the epic nested via `└── `,
    // roots ordered by priority then id (all equal priority here).
    let inverted_expected =
        format!("[P2] {fid}: {ft}\n└── [P2] {e}: Epic\n[P2] {sid}: {st}\n└── [P2] {e}: Epic\n");
    tk_cmd(&temp)
        .arg("tree")
        .arg("--inverted")
        .assert()
        .success()
        .stdout(predicate::eq(inverted_expected.as_str()));
}

#[test]
fn tree_inverted_composes_with_status_filter() {
    let temp = TempDir::new().unwrap();
    let l = tree_create(&temp, "Leaf");
    let e = tree_create(&temp, "Epic");
    tk_cmd(&temp).arg("dep").arg(&e).arg(&l).assert().success();
    tk_cmd(&temp).arg("close").arg(&e).assert().success();

    // Default open selection prunes the closed epic; the leaf stands alone.
    let open_expected = format!("[P2] {l}: Leaf\n");
    tk_cmd(&temp)
        .arg("tree")
        .arg("--inverted")
        .assert()
        .success()
        .stdout(predicate::eq(open_expected.as_str()));

    // `--status all` brings the closed epic back, nested under the leaf.
    let all_expected = format!("[P2] {l}: Leaf\n└── [P2] {e}: Epic\n");
    tk_cmd(&temp)
        .arg("tree")
        .arg("--inverted")
        .arg("--status")
        .arg("all")
        .assert()
        .success()
        .stdout(predicate::eq(all_expected.as_str()));
}

#[test]
fn tree_inverted_multi_level_glyphs_match_normal() {
    let temp = TempDir::new().unwrap();
    let l = tree_create(&temp, "Leaf");
    let m = tree_create(&temp, "Mid");
    let e = tree_create(&temp, "Epic");
    // Chain: epic -> mid -> leaf (mid depends on leaf, epic depends on mid).
    tk_cmd(&temp).arg("dep").arg(&m).arg(&l).assert().success();
    tk_cmd(&temp).arg("dep").arg(&e).arg(&m).assert().success();

    // Normal: epic root, single-child chain down to the leaf.
    let normal_expected = format!("[P2] {e}: Epic\n└── [P2] {m}: Mid\n    └── [P2] {l}: Leaf\n");
    tk_cmd(&temp)
        .arg("tree")
        .assert()
        .success()
        .stdout(predicate::eq(normal_expected.as_str()));

    // Inverted: leaf root, chain reversed up to the epic, identical glyphs.
    let inverted_expected = format!("[P2] {l}: Leaf\n└── [P2] {m}: Mid\n    └── [P2] {e}: Epic\n");
    tk_cmd(&temp)
        .arg("tree")
        .arg("--inverted")
        .assert()
        .success()
        .stdout(predicate::eq(inverted_expected.as_str()));
}

#[test]
fn duplicate_ticket_id_is_rejected_at_load_for_tree_ls_and_graph() {
    let temp = TempDir::new().unwrap();
    write_ticket_at(&temp, "a.md", "x-1234");
    write_ticket_at(&temp, "b.md", "x-1234");

    for subcommand in ["tree", "ls", "graph"] {
        tk_cmd(&temp).arg(subcommand).assert().failure().stderr(
            predicate::str::contains("duplicate ticket id 'x-1234'")
                .and(predicate::str::contains("a.md"))
                .and(predicate::str::contains("b.md")),
        );
    }
}

#[test]
fn multiple_duplicated_ticket_ids_are_all_listed_in_one_error() {
    let temp = TempDir::new().unwrap();
    write_ticket_at(&temp, "a.md", "x-1111");
    write_ticket_at(&temp, "b.md", "x-1111");
    write_ticket_at(&temp, "c.md", "y-2222");
    write_ticket_at(&temp, "d.md", "y-2222");

    tk_cmd(&temp).arg("ls").assert().failure().stderr(
        predicate::str::contains("duplicate ticket id 'x-1111'")
            .and(predicate::str::contains("duplicate ticket id 'y-2222'"))
            .and(predicate::str::contains("a.md"))
            .and(predicate::str::contains("b.md"))
            .and(predicate::str::contains("c.md"))
            .and(predicate::str::contains("d.md")),
    );
}
