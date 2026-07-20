//! `tk publish` — render a tk ticket as an external tracker item and file
//! it there. Currently ships one target, GitHub (`tk publish github`),
//! structured as a subcommand so further targets can be added alongside it
//! without reshaping the CLI surface.
//!
//! The body is rendered the way GitHub renders an issue-form submission:
//! one `### <field label>` heading per field, sourced from the ticket's own
//! parsed sections (no reparsing of the ticket file). tk ids are local-only
//! and must never reach GitHub, so the outbound title/body is checked for
//! leaks before anything is created. The in-process ticket store lock is
//! held across the whole check-create-pin sequence, and an OS-level
//! advisory file lock on `.publish.lock` inside the ticket store serializes
//! concurrent `tk publish` invocations across separate processes, since
//! each invocation is its own process with an independent copy of the
//! in-process lock.

use clap::Args;
use color_eyre::eyre::eyre;
use std::collections::{BTreeSet, HashSet};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::process::Command as OsCommand;
use std::sync::LazyLock;

use fd_lock::RwLock as FileLock;
use regex::Regex;
use time::OffsetDateTime;

use crate::Ticket;
use crate::fs::{lock_tickets, read_ticket, tickets_dir, write_ticket};
use crate::ids::resolve_partial_id;
use crate::resolve_ticket_path;

/// Name of the advisory lockfile held inside the ticket store for the
/// duration of a `tk publish` run. A leading dot and non-`.md` extension
/// keep it out of every ticket-directory scan (`read_all_tickets` filters
/// on `.md`; `resolve_ticket_path` requires `name.ends_with(".md")`).
const PUBLISH_LOCK_FILE: &str = ".publish.lock";

/// Opens (creating if absent) the advisory lockfile inside the ticket
/// store, ready for [`fd_lock::RwLock::try_write`]. The caller acquires and
/// holds the write guard itself, since the guard borrows from this value
/// and the two must live in the same scope.
fn open_publish_lock() -> color_eyre::Result<FileLock<std::fs::File>> {
    let path = tickets_dir()?.join(PUBLISH_LOCK_FILE);
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .map_err(|e| eyre!("failed to open publish lockfile {}: {e}", path.display()))?;
    Ok(FileLock::new(file))
}

/// `tk publish github <id> <owner/repo>`: renders a ticket as a GitHub
/// issue-form submission and creates it via `gh issue create`.
#[derive(Args, Debug)]
pub struct GithubPublishArgs {
    /// Ticket ID (or partial ID) to publish.
    id: String,

    /// Target GitHub repo, owner/name.
    repo: String,

    #[arg(
        long = "assignee",
        help = "GitHub assignee, e.g. @me (default: unassigned)"
    )]
    assignee: Option<String>,

    #[arg(
        long = "title-prefix",
        default_value = "[Maintenance]: ",
        help = "Issue title prefix"
    )]
    title_prefix: String,

    #[arg(
        long = "fields-json",
        value_name = "PATH",
        help = "JSON array replacing the default body fields; entries are {label, value} \
                or {label, source: describe|acceptance}"
    )]
    fields_json: Option<PathBuf>,

    #[arg(
        long = "dry-run",
        default_value_t = false,
        help = "Print the title, body and gh command; create nothing"
    )]
    dry_run: bool,

    #[arg(
        long = "body-file",
        value_name = "PATH",
        help = "Use this file's contents as the issue body instead of rendering one from \
                the ticket (title, pinning and priority still come from the ticket)"
    )]
    body_file: Option<PathBuf>,

    #[arg(
        long = "no-pin",
        default_value_t = false,
        help = "Do not write external_ref back to the tk ticket"
    )]
    no_pin: bool,

    #[arg(
        long = "re-file",
        default_value_t = false,
        help = "Create a new issue even though the ticket already has an external_ref"
    )]
    re_file: bool,

    #[arg(
        long = "no-priority",
        default_value_t = false,
        help = "Do not set the repo Priority issue field"
    )]
    no_priority: bool,

    #[arg(
        long = "priority-field",
        default_value = "Priority",
        help = "Name of the single-select issue field to set"
    )]
    priority_field: String,

    #[arg(long = "label", help = "Add a label (repeatable); none by default")]
    label: Vec<String>,
}

// Default field labels, in form order. When targeting a repo with its own
// issue form, override the whole set with --fields-json so the labels match
// that form exactly (GitHub only parses the body as a form submission when
// every `###` heading matches a form field label byte-for-byte).
const LBL_DESCRIBE: &str = "Describe the change required";
const LBL_SUCCESS: &str = "List the success/acceptance criteria for this ticket";
const LBL_HOURS: &str = "How many hours of developer time has this lost us?";
const LBL_COST: &str = "What happens if we don't do this?";
const LBL_EFFORT: &str = "How easy is this to change or fix?";
const LBL_MODULE: &str = "Which module(s) would making this change affect?";

const DEFAULT_COST: &str = "Tracked maintenance item. Deferring it leaves the change \
described above unaddressed; see the description and acceptance criteria for scope and \
impact.";

/// tk priority (frontmatter `priority: N`, 0..3 == P0..P3) to the repo
/// "Priority" issue-field option name; `None` for priorities the map
/// doesn't cover (e.g. P4).
fn priority_target(priority: u8) -> Option<&'static str> {
    match priority {
        0 => Some("Urgent"),
        1 => Some("High"),
        2 => Some("Medium"),
        3 => Some("Low"),
        _ => None,
    }
}

/// The content a [`FieldEntry`] renders under its heading: either a literal
/// value or content pulled from the ticket.
enum FieldSpec {
    /// A literal string, used as-is.
    Value(String),
    /// Content derived from the ticket via [`render_describe`] or
    /// [`render_acceptance`].
    Source(FieldSource),
}

#[derive(Clone, Copy)]
enum FieldSource {
    Describe,
    Acceptance,
}

/// A single issue-body field: a heading label paired with its content spec.
struct FieldEntry {
    label: String,
    spec: FieldSpec,
}

/// The maintenance-ticket issue-form field set used when `--fields-json` is
/// not given.
fn default_fields() -> Vec<FieldEntry> {
    vec![
        FieldEntry {
            label: LBL_DESCRIBE.to_string(),
            spec: FieldSpec::Source(FieldSource::Describe),
        },
        FieldEntry {
            label: LBL_SUCCESS.to_string(),
            spec: FieldSpec::Source(FieldSource::Acceptance),
        },
        FieldEntry {
            label: LBL_HOURS.to_string(),
            spec: FieldSpec::Value("1".to_string()),
        },
        FieldEntry {
            label: LBL_COST.to_string(),
            spec: FieldSpec::Value(DEFAULT_COST.to_string()),
        },
        FieldEntry {
            label: LBL_EFFORT.to_string(),
            spec: FieldSpec::Value("Unsure".to_string()),
        },
        FieldEntry {
            label: LBL_MODULE.to_string(),
            spec: FieldSpec::Value("Unsure/Other".to_string()),
        },
    ]
}

/// Loads and validates a `--fields-json` spec: a non-empty JSON array of
/// `{"label": str, "value": str}` or `{"label": str, "source": "describe" |
/// "acceptance"}` entries. Labels must be non-empty, single-line, and
/// unique; each entry needs exactly one of `value`/`source`.
fn load_fields_spec(path: &Path) -> color_eyre::Result<Vec<FieldEntry>> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| eyre!("could not read --fields-json {}: {e}", path.display()))?;
    let value: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|e| eyre!("could not parse --fields-json {}: {e}", path.display()))?;
    let entries = value
        .as_array()
        .filter(|a| !a.is_empty())
        .ok_or_else(|| eyre!("--fields-json must be a non-empty JSON array"))?;

    let mut seen_labels: HashSet<String> = HashSet::new();
    let mut fields = Vec::with_capacity(entries.len());

    for (i, entry) in entries.iter().enumerate() {
        let obj = entry
            .as_object()
            .ok_or_else(|| eyre!("--fields-json entry {i} must be an object"))?;

        let label = obj
            .get("label")
            .and_then(|v| v.as_str())
            .ok_or_else(|| eyre!("--fields-json entry {i} needs a string 'label'"))?;
        if label.trim().is_empty() || label.contains(['\n', '\r']) {
            return Err(eyre!(
                "--fields-json entry {i}: 'label' must be a non-empty single-line string"
            ));
        }
        if !seen_labels.insert(label.to_string()) {
            return Err(eyre!("--fields-json: duplicate label '{label}'"));
        }

        let has_value = obj.contains_key("value");
        let has_source = obj.contains_key("source");
        if has_value == has_source {
            return Err(eyre!(
                "--fields-json entry {i} ('{label}') needs exactly one of 'source' or 'value'"
            ));
        }

        let spec = if has_value {
            let value = obj.get("value").and_then(|v| v.as_str()).ok_or_else(|| {
                eyre!("--fields-json entry {i} ('{label}'): 'value' must be a string")
            })?;
            FieldSpec::Value(value.to_string())
        } else {
            match obj.get("source").and_then(|v| v.as_str()).unwrap_or("") {
                "describe" => FieldSpec::Source(FieldSource::Describe),
                "acceptance" => FieldSpec::Source(FieldSource::Acceptance),
                other => {
                    return Err(eyre!(
                        "--fields-json entry {i}: unknown source '{other}' (use 'describe' \
                         or 'acceptance')"
                    ));
                }
            }
        };

        fields.push(FieldEntry {
            label: label.to_string(),
            spec,
        });
    }

    Ok(fields)
}

/// True when `value` is `None` or, once trimmed, is one of the placeholder
/// strings ticket authors commonly leave in an unfilled section.
fn is_blank(value: Option<&str>) -> bool {
    match value {
        None => true,
        Some(s) => matches!(s.trim(), "" | "-" | "_" | "TBD" | "N/A"),
    }
}

/// Appends `heading` + `content` to `base`, separated by a blank line;
/// starts fresh (no leading separator) when `base` is empty.
fn append_section(base: String, heading: &str, content: &str) -> String {
    if base.is_empty() {
        format!("{heading}\n\n{content}")
    } else {
        format!("{base}\n\n{heading}\n\n{content}")
    }
}

/// Renders the "describe" source: the ticket's intro text plus its
/// implementation plan, or its notes when the plan is empty (tickets whose
/// plan was recorded via `tk add-note` carry it there instead). Falls back
/// to a placeholder when nothing usable is found.
fn render_describe(ticket: &Ticket) -> String {
    let mut describe = ticket.description().unwrap_or("").to_string();

    if !is_blank(ticket.implementation_plan()) {
        describe = append_section(
            describe,
            "#### Implementation plan",
            ticket.implementation_plan().unwrap_or_default(),
        );
    } else {
        let notes_joined = (!ticket.notes().is_empty()).then(|| ticket.notes().join("\n\n"));
        if !is_blank(notes_joined.as_deref()) {
            describe = append_section(
                describe,
                "#### Details",
                notes_joined.as_deref().unwrap_or_default(),
            );
        }
    }

    if is_blank(Some(describe.as_str())) {
        describe = "_(No description in source ticket.)_".to_string();
    }

    describe
}

/// Renders the "acceptance" source: the ticket's acceptance criteria, or a
/// pointer back to the description when none are recorded.
fn render_acceptance(ticket: &Ticket) -> String {
    if is_blank(ticket.acceptance()) {
        "See the description / implementation plan above.".to_string()
    } else {
        ticket.acceptance().unwrap_or_default().to_string()
    }
}

/// Renders the issue body: one `### <label>` heading per field, mirroring
/// what GitHub produces when an issue form is submitted.
fn build_body(ticket: &Ticket, fields: &[FieldEntry]) -> String {
    let describe = render_describe(ticket);
    let acceptance = render_acceptance(ticket);

    fields
        .iter()
        .map(|f| {
            let value = match &f.spec {
                FieldSpec::Value(v) => v.as_str(),
                FieldSpec::Source(FieldSource::Describe) => describe.as_str(),
                FieldSpec::Source(FieldSource::Acceptance) => acceptance.as_str(),
            };
            format!("### {}\n\n{}", f.label, value)
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

// Generic tk-id shape: short lowercase prefix + hex. The denylist drops
// common technical terms that match the pattern (sha-256 etc.).
static STRAY_ID_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b([a-z]{1,4})-([0-9a-f]{3,})\b").expect("static regex pattern is valid")
});
static ISSUE_NUMBER_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"/issues/(\d+)\b").expect("static regex pattern is valid"));

const STRAY_ID_DENYLIST: &[&str] = &["sha", "utf", "rfc", "iso", "crc", "cve"];

/// True for characters that, adjacent to a candidate id match, mean the
/// match is really the interior or tail of a longer token rather than a
/// genuine id boundary.
fn is_id_boundary_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-'
}

/// Case-insensitive substring search with hand-rolled boundary checks
/// instead of a regex `\b`. A regex word boundary requires one side of the
/// match to be a "word" character ([A-Za-z0-9_]); when a needle *begins*
/// with a non-word character (e.g. a leading `.`, as in an id derived from
/// a dotted working-directory name like `.tm-1a2b3`) and is itself preceded
/// by another non-word character (whitespace, punctuation), neither side of
/// that position is a word character, so `\b` never matches there and the
/// leak slips through undetected. Here a match counts when the character
/// immediately before it (if any) is not alphanumeric or `-`, and the
/// character immediately after it (if any) is not alphanumeric — the same
/// intent as a word boundary, without that blind spot.
fn contains_word(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    let haystack_chars: Vec<char> = haystack.to_lowercase().chars().collect();
    let needle_chars: Vec<char> = needle.to_lowercase().chars().collect();
    let needle_len = needle_chars.len();
    if needle_len == 0 || haystack_chars.len() < needle_len {
        return false;
    }

    haystack_chars
        .windows(needle_len)
        .enumerate()
        .any(|(start, window)| {
            if window != needle_chars.as_slice() {
                return false;
            }
            let before_ok = start
                .checked_sub(1)
                .map(|i| !is_id_boundary_char(haystack_chars[i]))
                .unwrap_or(true);
            let after_ok = haystack_chars
                .get(start + needle_len)
                .map(|c| !c.is_ascii_alphanumeric())
                .unwrap_or(true);
            before_ok && after_ok
        })
}

/// Finds substrings shaped like a tk id (`prefix-hex`) in `haystack`, minus
/// common technical terms (sha-256 etc.) that match the same pattern.
fn find_stray_ids(haystack: &str) -> Vec<String> {
    let mut found: BTreeSet<String> = BTreeSet::new();
    for cap in STRAY_ID_RE.captures_iter(haystack) {
        let prefix = &cap[1];
        if STRAY_ID_DENYLIST.contains(&prefix.to_lowercase().as_str()) {
            continue;
        }
        found.insert(format!("{}-{}", &cap[1], &cap[2]));
    }
    found.into_iter().collect()
}

/// A completed leak scan: which hard-fail conditions fired, and which
/// tk-id-shaped text was merely suspicious.
struct LeakReport {
    /// The outbound text contains the ticket's resolved id or the raw
    /// CLI-supplied id, at a boundary.
    own_id_leak: bool,
    /// Other local store ids referenced in the outbound text (sorted,
    /// deduplicated, excluding the resolved id itself).
    store_id_leaks: Vec<String>,
    /// Text shaped like a tk id but not a known local id.
    stray_ids: Vec<String>,
}

/// Scans `outbound` for tk-id leaks against the resolved/raw-input id and
/// every id in the local store. Pure: callers decide whether a hard-fail
/// condition is an error (a real run) or a warning (`--dry-run`).
fn scan_leaks(
    outbound: &str,
    resolved_id: &str,
    raw_input_id: &str,
    all_ids: &[String],
) -> LeakReport {
    let own_id_leak = contains_word(outbound, resolved_id) || contains_word(outbound, raw_input_id);

    let mut store_id_leaks: Vec<String> = all_ids
        .iter()
        .map(String::as_str)
        .filter(|id| *id != resolved_id)
        .filter(|id| contains_word(outbound, id))
        .map(str::to_string)
        .collect();
    store_id_leaks.sort_unstable();
    store_id_leaks.dedup();

    LeakReport {
        own_id_leak,
        store_id_leaks,
        stray_ids: find_stray_ids(outbound),
    }
}

/// Enforces that tk ids never reach GitHub: on a real run, hard-fails when
/// the outbound title+body contains the source ticket's resolved or
/// CLI-supplied id, or any id present in the local ticket store. Under
/// `--dry-run` — the documented readability-pass entry point whose purpose
/// includes finding and removing exactly these references — the same
/// conditions are downgraded to warnings so the full rendered output stays
/// visible for editing; a real run still refuses. Text that merely looks
/// like a tk id (not a known local id) is always a warning, never a
/// failure.
fn check_leaks(
    outbound: &str,
    resolved_id: &str,
    raw_input_id: &str,
    all_ids: &[String],
    dry_run: bool,
) -> color_eyre::Result<()> {
    let report = scan_leaks(outbound, resolved_id, raw_input_id, all_ids);

    if report.own_id_leak {
        let message = format!(
            "issue body/title contains the tk id '{resolved_id}'; tk ids must never appear \
             in GitHub issues — reword and retry"
        );
        if dry_run {
            eprintln!("warning: error-on-create: {message}");
        } else {
            return Err(eyre!(message));
        }
    }

    if !report.store_id_leaks.is_empty() {
        let message = format!(
            "issue body/title references local tk ids: {}; replace them with their gh-<n> \
             external refs (or describe the tickets in words) and retry",
            report.store_id_leaks.join(", ")
        );
        if dry_run {
            eprintln!("warning: error-on-create: {message}");
        } else {
            return Err(eyre!(message));
        }
    }

    if !report.stray_ids.is_empty() {
        eprintln!(
            "warning: body contains tk-id-like references: {} — replace with gh-<n> refs or \
             reword",
            report.stray_ids.join(", ")
        );
    }

    Ok(())
}

fn extract_issue_number(url: &str) -> Option<u64> {
    ISSUE_NUMBER_RE.captures(url)?.get(1)?.as_str().parse().ok()
}

/// A body file written under the OS temp directory for `gh issue create
/// --body-file`; removed on drop so a failed or successful run never leaves
/// stray files behind.
struct TempBodyFile(PathBuf);

impl TempBodyFile {
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempBodyFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Creates the temporary body file with `O_CREAT|O_EXCL` semantics (via
/// `create_new`) so a pre-existing file or symlink at the chosen path is
/// rejected rather than followed or overwritten, and restricts it to
/// owner-only read/write on Unix so the ticket body it carries (which may
/// include sensitive maintenance detail) is never world-readable.
fn write_temp_body(body: &str) -> color_eyre::Result<TempBodyFile> {
    let mut path = std::env::temp_dir();
    let unique = format!(
        "tk-publish-{}-{}.md",
        std::process::id(),
        OffsetDateTime::now_utc().unix_timestamp_nanos()
    );
    path.push(unique);

    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }

    let mut file = options.open(&path).map_err(|e| {
        eyre!(
            "failed to create temporary body file {}: {e}",
            path.display()
        )
    })?;
    file.write_all(body.as_bytes()).map_err(|e| {
        eyre!(
            "failed to write temporary body file {}: {e}",
            path.display()
        )
    })?;

    Ok(TempBodyFile(path))
}

/// Reduces a label to comparable letters: "🌋 Urgent" -> "urgent".
fn norm(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_lowercase())
        .collect()
}

/// Runs a `gh api graphql` query/mutation. String variables are passed via
/// `-f`, integer variables via `-F`. Returns the `data` payload, or an error
/// string on transport failure or a GraphQL `errors` payload.
fn gh_graphql(
    query: &str,
    string_vars: &[(&str, &str)],
    int_vars: &[(&str, i64)],
) -> Result<serde_json::Value, String> {
    let mut cmd = OsCommand::new("gh");
    cmd.arg("api")
        .arg("graphql")
        .arg("-f")
        .arg(format!("query={query}"));
    for (k, v) in string_vars {
        cmd.arg("-f").arg(format!("{k}={v}"));
    }
    for (k, v) in int_vars {
        cmd.arg("-F").arg(format!("{k}={v}"));
    }

    let output = cmd.output().map_err(|e| format!("failed to run gh: {e}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let data: serde_json::Value = if stdout.trim().is_empty() {
        serde_json::json!({})
    } else {
        serde_json::from_str(&stdout).unwrap_or_else(|_| serde_json::json!({}))
    };

    if !output.status.success() || data.get("errors").is_some() {
        let message = data
            .get("errors")
            .map(|errors| errors.to_string())
            .unwrap_or_else(|| {
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                if !stderr.is_empty() {
                    stderr
                } else {
                    stdout.trim().to_string()
                }
            });
        return Err(message);
    }

    Ok(data
        .get("data")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({})))
}

/// Best-effort: sets the repo's single-select `field_name` issue field on
/// `issue_number` to the option mapped from `priority`. Never fails the
/// run — every miss is printed as a note or warning.
fn set_issue_priority(repo: &str, issue_number: u64, priority: u8, field_name: &str) {
    let Some(target) = priority_target(priority) else {
        println!("note: priority {priority} has no mapping; '{field_name}' left unset");
        return;
    };
    let Some((owner, name)) = repo.split_once('/') else {
        eprintln!("warning: cannot parse repo '{repo}'; priority not set");
        return;
    };

    let query = r#"
    query($owner:String!, $name:String!, $number:Int!) {
      repository(owner:$owner, name:$name) {
        issue(number:$number) { id viewerCanSetFields }
        issueFields(first:50) {
          nodes { __typename
            ... on IssueFieldSingleSelect { id name options { id name } } }
        }
      }
    }"#;

    let data = match gh_graphql(
        query,
        &[("owner", owner), ("name", name)],
        &[("number", issue_number as i64)],
    ) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("warning: could not read issue fields ({e}); priority not set");
            return;
        }
    };

    let empty_obj = serde_json::json!({});
    let repository = data.get("repository").unwrap_or(&empty_obj);
    let issue = repository.get("issue").unwrap_or(&empty_obj);
    let nodes = repository
        .get("issueFields")
        .and_then(|f| f.get("nodes"))
        .and_then(|n| n.as_array())
        .cloned()
        .unwrap_or_default();

    let Some(field) = nodes.iter().find(|n| {
        n.get("__typename").and_then(|t| t.as_str()) == Some("IssueFieldSingleSelect")
            && n.get("name")
                .and_then(|v| v.as_str())
                .is_some_and(|s| s.eq_ignore_ascii_case(field_name))
    }) else {
        println!("note: repo {repo} has no '{field_name}' issue field; priority not set");
        return;
    };

    let options = field
        .get("options")
        .and_then(|o| o.as_array())
        .cloned()
        .unwrap_or_default();
    let Some(opt) = options.iter().find(|o| {
        o.get("name")
            .and_then(|n| n.as_str())
            .is_some_and(|n| norm(n) == norm(target))
    }) else {
        let have: Vec<&str> = options
            .iter()
            .filter_map(|o| o.get("name").and_then(|n| n.as_str()))
            .collect();
        eprintln!(
            "warning: '{field_name}' has no '{target}' option (have: {}); priority not set",
            have.join(", ")
        );
        return;
    };

    if issue.get("viewerCanSetFields").and_then(|v| v.as_bool()) == Some(false) {
        eprintln!("warning: no permission to set fields on #{issue_number}; priority not set");
        return;
    }

    let mutation = r#"
    mutation($issueId:ID!, $fieldId:ID!, $optionId:ID!) {
      setIssueFieldValue(input:{
        issueId:$issueId,
        issueFields:[{ fieldId:$fieldId, singleSelectOptionId:$optionId }]
      }) { clientMutationId }
    }"#;

    let issue_id = issue.get("id").and_then(|v| v.as_str()).unwrap_or_default();
    let field_id = field.get("id").and_then(|v| v.as_str()).unwrap_or_default();
    let option_id = opt.get("id").and_then(|v| v.as_str()).unwrap_or_default();

    if let Err(e) = gh_graphql(
        mutation,
        &[
            ("issueId", issue_id),
            ("fieldId", field_id),
            ("optionId", option_id),
        ],
        &[],
    ) {
        eprintln!("warning: setting priority failed ({e})");
        return;
    }

    let opt_name = opt.get("name").and_then(|v| v.as_str()).unwrap_or(target);
    println!("set {field_name} = {opt_name} (P{priority})");
}

/// Re-reads `id` fresh from disk via [`resolve_ticket_path`] and
/// [`read_ticket`], bypassing any in-memory snapshot. Used both for the
/// pre-create idempotency check and, separately, immediately before
/// pinning, so neither decision is made on a copy of the ticket that may
/// have gone stale while this process was blocked on the publish lock or
/// waiting on `gh`.
fn reread_ticket(id: &str) -> color_eyre::Result<Ticket> {
    let path = resolve_ticket_path(id)?;
    read_ticket(&path)?.ok_or_else(|| eyre!("Error: ticket '{id}' not found"))
}

/// Implements `tk publish github <id> <owner/repo>`. `--dry-run` creates
/// nothing and does not take the cross-process lock, so it can never
/// fail-close a concurrent real publish; every other run holds the
/// in-process ticket store lock (id resolution and leak enumeration only)
/// and the cross-process `.publish.lock` advisory file lock across the
/// whole check -> `gh issue create` -> pin sequence, failing closed
/// immediately (never blocking) if another `tk publish` already holds it.
/// The ticket itself is re-read fresh from disk after the file lock is
/// acquired, and again immediately before pinning, rather than relying on
/// the in-memory snapshot taken before the lock.
pub fn cmd_publish_github(args: GithubPublishArgs) -> color_eyre::Result<()> {
    let tickets = lock_tickets()?;
    let resolved_id = resolve_partial_id(&tickets, &args.id)?;
    let all_ids: Vec<String> = tickets.iter().map(|t| t.id().to_string()).collect();

    let mut publish_lock: Option<FileLock<std::fs::File>> = if args.dry_run {
        None
    } else {
        Some(open_publish_lock()?)
    };
    let _publish_guard = match publish_lock.as_mut() {
        Some(lock) => Some(lock.try_write().map_err(|e| match e.kind() {
            ErrorKind::WouldBlock => eyre!(
                "another tk publish is in progress for this ticket store; wait for it to \
                 finish and retry"
            ),
            _ => eyre!("failed to acquire the publish lock: {e}"),
        })?),
        None => None,
    };

    let ticket = reread_ticket(&resolved_id)?;

    let issue_title = format!("{}{}", args.title_prefix, ticket.title());

    let issue_body = match &args.body_file {
        Some(path) => {
            let content = std::fs::read_to_string(path)
                .map_err(|e| eyre!("could not read --body-file {}: {e}", path.display()))?;
            let trimmed = content.trim().to_string();
            if trimmed.is_empty() {
                return Err(eyre!("--body-file {} is empty", path.display()));
            }
            trimmed
        }
        None => {
            let fields = match &args.fields_json {
                Some(path) => load_fields_spec(path)?,
                None => default_fields(),
            };
            build_body(&ticket, &fields)
        }
    };

    let priority = ticket.priority();
    let existing_ref = ticket.external_ref().map(str::to_string);

    // Idempotency: a pinned ticket already has a public issue. Creating
    // another would orphan the first and silently repoint the pin, so a
    // rerun fails closed unless the operator explicitly asks to re-file.
    // `ticket` was re-read after the publish lock was acquired (see
    // `reread_ticket`), so this observes any pin written by a publish that
    // completed just before this one took the lock.
    if existing_ref.is_some() && !args.re_file && !args.dry_run {
        return Err(eyre!(
            "ticket is already pinned to {}; pass --re-file to deliberately create another \
             issue (add --no-pin to keep the existing pin)",
            existing_ref.as_deref().unwrap_or_default()
        ));
    }

    let outbound = format!("{issue_title}\n{issue_body}");
    check_leaks(&outbound, &resolved_id, &args.id, &all_ids, args.dry_run)?;

    let mut gh_cmd_display = vec![
        "gh".to_string(),
        "issue".to_string(),
        "create".to_string(),
        "--repo".to_string(),
        args.repo.clone(),
        "--title".to_string(),
        issue_title.clone(),
    ];
    if let Some(assignee) = &args.assignee {
        gh_cmd_display.push("--assignee".to_string());
        gh_cmd_display.push(assignee.clone());
    }
    for label in &args.label {
        gh_cmd_display.push("--label".to_string());
        gh_cmd_display.push(label.clone());
    }

    if args.dry_run {
        if let Some(r) = existing_ref.as_deref()
            && !args.re_file
        {
            println!(
                "# note: ticket is already pinned to {r}; a real run will refuse without \
                 --re-file"
            );
        }
        println!(
            "# would create in {} (assignee: {})\n",
            args.repo,
            args.assignee.as_deref().unwrap_or("none")
        );
        println!("TITLE: {issue_title}\n");
        println!("{issue_body}");
        println!(
            "\n# gh command: {} --body-file <tmp>",
            gh_cmd_display.join(" ")
        );
        if !args.no_priority
            && let Some(target) = priority_target(priority)
        {
            println!(
                "# would set {} = {} (P{})",
                args.priority_field, target, priority
            );
        }
        return Ok(());
    }

    let temp_file = write_temp_body(&issue_body)?;

    let mut cmd = OsCommand::new("gh");
    cmd.arg("issue")
        .arg("create")
        .arg("--repo")
        .arg(&args.repo)
        .arg("--title")
        .arg(&issue_title);
    if let Some(assignee) = &args.assignee {
        cmd.arg("--assignee").arg(assignee);
    }
    for label in &args.label {
        cmd.arg("--label").arg(label);
    }
    cmd.arg("--body-file").arg(temp_file.path());

    let run_result = cmd.output();
    drop(temp_file);
    let output = run_result.map_err(|e| eyre!("failed to run gh: {e}"))?;

    if !output.status.success() {
        let message = if !output.stderr.is_empty() {
            String::from_utf8_lossy(&output.stderr).to_string()
        } else {
            String::from_utf8_lossy(&output.stdout).to_string()
        };
        return Err(eyre!("gh issue create failed:\n{message}"));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let url = stdout.trim().lines().next_back().unwrap_or("").to_string();
    if url.is_empty() {
        println!("(issue created; no URL returned)");
    } else {
        println!("{url}");
    }
    // Re-echoed in every warning below, since stdout (the URL above) and
    // stderr (the warning) may not stay adjacent once captured or piped.
    let url_hint = if url.is_empty() {
        "no URL was returned by gh".to_string()
    } else {
        url.clone()
    };

    let issue_number = extract_issue_number(&url);

    // The issue already exists at this point regardless of what follows, so
    // pin/priority problems are surfaced but never cause an orphaned issue
    // to go unmentioned; `pin_error` is what turns into a non-zero exit
    // below once every best-effort step has had its turn.
    let mut pin_error: Option<String> = None;

    if !args.no_pin {
        match issue_number {
            Some(n) => {
                let ref_value = format!("gh-{n}");
                // Re-read right before writing: this process may have been
                // blocked on the publish lock or the `gh` call above for a
                // while, and another actor (a `tk edit`, for instance,
                // which does not take this lock) could have touched the
                // ticket meanwhile. Mutate only external_ref on the fresh
                // copy rather than writing back the snapshot taken earlier.
                match reread_ticket(&resolved_id) {
                    Ok(mut fresh) => {
                        fresh.set_external_ref(Some(ref_value.clone()));
                        match write_ticket(&fresh) {
                            Ok(()) => {
                                println!("pinned {resolved_id} -> external_ref: {ref_value}")
                            }
                            Err(e) => {
                                eprintln!(
                                    "warning: could not write external_ref ({e}); pin \
                                     manually with: tk edit {resolved_id} --external-ref \
                                     {ref_value}"
                                );
                                pin_error = Some(format!(
                                    "issue created ({url_hint}) but pinning failed: {e}"
                                ));
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!(
                            "warning: could not re-read ticket '{resolved_id}' for pinning \
                             ({e}); pin manually with: tk edit {resolved_id} --external-ref \
                             {ref_value}"
                        );
                        pin_error = Some(format!(
                            "issue created ({url_hint}) but the ticket could not be re-read \
                             to pin it: {e}"
                        ));
                    }
                }
            }
            None => {
                eprintln!(
                    "warning: could not parse the issue number from the gh URL \
                     ({url_hint}); external_ref not set — pin manually with: tk edit \
                     {resolved_id} --external-ref gh-<number-from-URL>"
                );
                pin_error = Some(format!(
                    "issue created ({url_hint}) but the issue number could not be parsed \
                     from it, so external_ref was not pinned"
                ));
            }
        }
    }

    if !args.no_priority {
        match issue_number {
            Some(n) => set_issue_priority(&args.repo, n, priority, &args.priority_field),
            None => eprintln!(
                "warning: could not parse the issue number from the gh URL ({url_hint}); \
                 priority not set"
            ),
        }
    }

    if let Some(reason) = pin_error {
        return Err(eyre!(reason));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::StatusValue;
    use crate::{TicketBody, TicketFrontmatter};
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    /// Writes `content` to a uniquely named file under the OS temp directory,
    /// runs [`load_fields_spec`] against it, then removes the file. nextest
    /// runs each test in its own process, so the pid keeps names unique across
    /// tests; the counter keeps them unique within a single test.
    fn load_json(content: &str) -> color_eyre::Result<Vec<FieldEntry>> {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "tk-fields-spec-{}-{}.json",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(&path, content).unwrap();
        let result = load_fields_spec(&path);
        let _ = std::fs::remove_file(&path);
        result
    }

    /// The error message from a rejected `--fields-json` spec. Uses `.err()`
    /// rather than `unwrap_err()` so the success type (`Vec<FieldEntry>`, which
    /// intentionally carries no `Debug`) need not be printable.
    fn load_err(content: &str) -> String {
        load_json(content)
            .err()
            .expect("expected a rejected spec")
            .to_string()
    }

    fn ticket_with(
        id: &str,
        description: Option<&str>,
        implementation_plan: Option<&str>,
        acceptance: Option<&str>,
        notes: Vec<String>,
    ) -> Ticket {
        Ticket {
            title: "Test Ticket".to_string(),
            frontmatter: TicketFrontmatter {
                id: id.to_string(),
                r#type: None,
                status: StatusValue::Open,
                deps: Vec::new(),
                links: Vec::new(),
                priority: 2,
                assignee: None,
                tags: Vec::new(),
                created: None,
                closed_at: None,
                external_ref: None,
            },
            body: TicketBody {
                description: description.map(str::to_string),
                implementation_plan: implementation_plan.map(str::to_string),
                acceptance: acceptance.map(str::to_string),
                notes,
            },
            path: PathBuf::from(format!("{id}.md")),
        }
    }

    // -- load_fields_spec ----------------------------------------------------

    #[test]
    fn load_fields_spec_happy_path_preserves_order_and_variants() {
        let fields = load_json(
            r#"[
                {"label": "Summary", "source": "describe"},
                {"label": "Criteria", "source": "acceptance"},
                {"label": "Effort", "value": "Low"}
            ]"#,
        )
        .expect("valid spec loads");

        assert_eq!(fields.len(), 3);
        assert_eq!(fields[0].label, "Summary");
        assert!(matches!(
            fields[0].spec,
            FieldSpec::Source(FieldSource::Describe)
        ));
        assert!(matches!(
            fields[1].spec,
            FieldSpec::Source(FieldSource::Acceptance)
        ));
        match &fields[2].spec {
            FieldSpec::Value(v) => assert_eq!(v, "Low"),
            _ => panic!("expected a literal value spec"),
        }
    }

    #[test]
    fn load_fields_spec_rejects_non_array() {
        let err = load_err(r#"{"label": "x", "value": "y"}"#);
        assert!(err.contains("non-empty JSON array"), "got: {err}");
    }

    #[test]
    fn load_fields_spec_rejects_empty_array() {
        let err = load_err("[]");
        assert!(err.contains("non-empty JSON array"), "got: {err}");
    }

    #[test]
    fn load_fields_spec_rejects_non_object_entry() {
        let err = load_err("[42]");
        assert!(err.contains("must be an object"), "got: {err}");
    }

    #[test]
    fn load_fields_spec_rejects_missing_label() {
        let err = load_err(r#"[{"value": "y"}]"#);
        assert!(err.contains("needs a string 'label'"), "got: {err}");
    }

    // MC/DC for load_fields_spec's label-shape guard:
    //   `label.trim().is_empty() || label.contains(['\n', '\r'])`
    //   c1 = label.trim().is_empty()
    //   c2 = label contains '\n' or '\r'
    // Outcome true => error. A pure OR: each single-true case is
    // masked-independent against the all-false case.
    //   T1 (F,F): load_fields_spec_happy_path_preserves_order_and_variants
    //             (labels "Summary"/"Criteria"/"Effort") => Ok
    //   T2 (T,F): load_fields_spec_rejects_whitespace_label ("   ") => error
    //   T3 (F,T): load_fields_spec_rejects_multiline_label ("a\nb") => error
    // Independence pairs:
    //   c1: T2 vs T1
    //   c2: T3 vs T1
    #[test]
    fn load_fields_spec_rejects_whitespace_label() {
        let err = load_err(r#"[{"label": "   ", "value": "y"}]"#);
        assert!(err.contains("non-empty single-line string"), "got: {err}");
    }

    #[test]
    fn load_fields_spec_rejects_multiline_label() {
        let err = load_err("[{\"label\": \"a\\nb\", \"value\": \"y\"}]");
        assert!(err.contains("non-empty single-line string"), "got: {err}");
    }

    #[test]
    fn load_fields_spec_rejects_duplicate_label() {
        let err = load_err(r#"[{"label": "Dup", "value": "1"}, {"label": "Dup", "value": "2"}]"#);
        assert!(err.contains("duplicate label 'Dup'"), "got: {err}");
    }

    // MC/DC for load_fields_spec's source/value guard: `has_value == has_source`
    //   c1 = has_value  (entry contains a "value" key)
    //   c2 = has_source (entry contains a "source" key)
    // The `==` is a boolean equivalence (error when both equal):
    //   T_both (T,T): load_fields_spec_rejects_both_value_and_source => error
    //   T_none (F,F): load_fields_spec_rejects_neither_value_nor_source => error
    //   T_val  (T,F): load_fields_spec_happy_path... ("Effort" entry) => Ok
    //   T_src  (F,T): load_fields_spec_happy_path... ("Summary" entry) => Ok
    // Independence pairs:
    //   c1: T_val vs T_none  (c2 held F, outcome Ok vs error)
    //   c2: T_src vs T_none  (c1 held F, outcome Ok vs error)
    #[test]
    fn load_fields_spec_rejects_both_value_and_source() {
        let err = load_err(r#"[{"label": "A", "value": "y", "source": "describe"}]"#);
        assert!(
            err.contains("exactly one of 'source' or 'value'"),
            "got: {err}"
        );
    }

    #[test]
    fn load_fields_spec_rejects_neither_value_nor_source() {
        let err = load_err(r#"[{"label": "A"}]"#);
        assert!(
            err.contains("exactly one of 'source' or 'value'"),
            "got: {err}"
        );
    }

    #[test]
    fn load_fields_spec_rejects_non_string_value() {
        let err = load_err(r#"[{"label": "A", "value": 42}]"#);
        assert!(err.contains("'value' must be a string"), "got: {err}");
    }

    #[test]
    fn load_fields_spec_rejects_unknown_source() {
        let err = load_err(r#"[{"label": "A", "source": "bogus"}]"#);
        assert!(err.contains("unknown source 'bogus'"), "got: {err}");
    }

    // -- is_blank ------------------------------------------------------------

    // MC/DC for is_blank's decision:
    //   `matches!(s.trim(), "" | "-" | "_" | "TBD" | "N/A")` (multi-matcher).
    // Each matcher is an alternative (OR-like). One example isolates each
    // matcher (true) plus the None case (true) and a non-matching case
    // (false); the false case is the shared "no matcher fires" pair partner.
    #[test]
    fn is_blank_none_is_blank() {
        assert!(is_blank(None));
    }

    #[test]
    fn is_blank_placeholder_variants_are_blank() {
        assert!(is_blank(Some("")));
        assert!(is_blank(Some("-")));
        assert!(is_blank(Some("_")));
        assert!(is_blank(Some("TBD")));
        assert!(is_blank(Some("N/A")));
    }

    #[test]
    fn is_blank_trims_before_matching() {
        assert!(is_blank(Some("   ")));
        assert!(is_blank(Some("  -  ")));
        assert!(is_blank(Some("\n TBD \n")));
    }

    #[test]
    fn is_blank_real_content_is_not_blank() {
        assert!(!is_blank(Some("Real description")));
    }

    #[test]
    fn is_blank_leading_placeholder_with_content_survives() {
        // "TBD" as leading text with real content after it: the trimmed
        // composite is not one of the placeholder tokens, so it is NOT blank.
        assert!(!is_blank(Some("TBD - but here is the real plan")));
        assert!(!is_blank(Some("N/A yet, will fill in later")));
    }

    // -- render_describe -----------------------------------------------------

    #[test]
    fn render_describe_includes_plan_when_present() {
        let t = ticket_with("t-1", Some("Intro text"), Some("Do step one"), None, vec![]);
        let out = render_describe(&t);
        assert!(out.contains("Intro text"));
        assert!(out.contains("#### Implementation plan"));
        assert!(out.contains("Do step one"));
        assert!(!out.contains("#### Details"));
    }

    #[test]
    fn render_describe_falls_back_to_notes_when_plan_blank() {
        let t = ticket_with(
            "t-1",
            Some("Intro text"),
            None,
            None,
            vec!["First note".to_string(), "Second note".to_string()],
        );
        let out = render_describe(&t);
        assert!(out.contains("Intro text"));
        assert!(out.contains("#### Details"));
        assert!(out.contains("First note"));
        assert!(out.contains("Second note"));
        assert!(!out.contains("#### Implementation plan"));
    }

    #[test]
    fn render_describe_placeholder_plan_is_treated_as_blank() {
        // A plan whose sole content is a placeholder token falls through to
        // the notes fallback rather than being rendered as the plan.
        let t = ticket_with(
            "t-1",
            Some("Intro"),
            Some("TBD"),
            None,
            vec!["A note".to_string()],
        );
        let out = render_describe(&t);
        assert!(out.contains("#### Details"));
        assert!(!out.contains("#### Implementation plan"));
    }

    #[test]
    fn render_describe_leading_placeholder_plan_survives() {
        // Leading "TBD" followed by real content is a non-blank composite and
        // renders as the implementation plan.
        let t = ticket_with(
            "t-1",
            Some("Intro"),
            Some("TBD - actual plan text follows"),
            None,
            vec![],
        );
        let out = render_describe(&t);
        assert!(out.contains("#### Implementation plan"));
        assert!(out.contains("actual plan text follows"));
    }

    #[test]
    fn render_describe_description_only_when_no_plan_or_notes() {
        let t = ticket_with("t-1", Some("Only the description"), None, None, vec![]);
        let out = render_describe(&t);
        assert_eq!(out, "Only the description");
    }

    #[test]
    fn render_describe_everything_blank_uses_fallback() {
        let t = ticket_with("t-1", None, None, None, vec![]);
        let out = render_describe(&t);
        assert_eq!(out, "_(No description in source ticket.)_");
    }

    // -- render_acceptance ---------------------------------------------------

    #[test]
    fn render_acceptance_returns_criteria_when_present() {
        let t = ticket_with("t-1", None, None, Some("Must pass CI"), vec![]);
        assert_eq!(render_acceptance(&t), "Must pass CI");
    }

    #[test]
    fn render_acceptance_falls_back_when_absent() {
        let t = ticket_with("t-1", None, None, None, vec![]);
        assert_eq!(
            render_acceptance(&t),
            "See the description / implementation plan above."
        );
    }

    #[test]
    fn render_acceptance_placeholder_criteria_falls_back() {
        let t = ticket_with("t-1", None, None, Some("N/A"), vec![]);
        assert_eq!(
            render_acceptance(&t),
            "See the description / implementation plan above."
        );
    }

    // -- build_body ----------------------------------------------------------

    #[test]
    fn build_body_maps_sources_and_values_to_headings() {
        let t = ticket_with("t-1", Some("Desc body"), None, Some("Accept body"), vec![]);
        let fields = vec![
            FieldEntry {
                label: "Describe".to_string(),
                spec: FieldSpec::Source(FieldSource::Describe),
            },
            FieldEntry {
                label: "Accept".to_string(),
                spec: FieldSpec::Source(FieldSource::Acceptance),
            },
            FieldEntry {
                label: "Fixed".to_string(),
                spec: FieldSpec::Value("literal".to_string()),
            },
        ];
        let body = build_body(&t, &fields);
        assert_eq!(
            body,
            "### Describe\n\nDesc body\n\n### Accept\n\nAccept body\n\n### Fixed\n\nliteral"
        );
    }

    // -- contains_word --------------------------------------------------------

    #[test]
    fn contains_word_matches_on_word_boundary_only() {
        assert!(contains_word("see tic-1a2b now", "tic-1a2b"));
        assert!(contains_word("SEE TIC-1A2B NOW", "tic-1a2b"));
        assert!(!contains_word("prefixtic-1a2bsuffix", "tic-1a2b"));
    }

    #[test]
    fn contains_word_empty_needle_is_false() {
        assert!(!contains_word("anything at all", ""));
    }

    #[test]
    fn contains_word_matches_punctuation_prefixed_needle() {
        // A leading `.` is not a regex `\b` word character, and neither is
        // the space that usually precedes it in prose, so a `\b`-based
        // implementation can never find a boundary here at all. The
        // neighbour-character scan has no such blind spot.
        assert!(contains_word("Depends on .tm-1a2b3 landing", ".tm-1a2b3"));
        assert!(contains_word(".tm-1a2b3 leads the sentence", ".tm-1a2b3"));
        assert!(!contains_word("prefix.tm-1a2b3suffix", ".tm-1a2b3"));
    }

    #[test]
    fn contains_word_rejects_mid_token_match() {
        // A short needle like "abc" must not match inside an unrelated
        // longer token ("xabcy"); this is the false-positive the raw
        // CLI-partial check used to have when it did a plain substring
        // search instead of a boundary-aware one.
        assert!(!contains_word("see xabcy over there", "abc"));
        assert!(contains_word("the abc report", "abc"));
    }

    // -- find_stray_ids ------------------------------------------------------

    #[test]
    fn find_stray_ids_finds_id_shaped_tokens() {
        let found = find_stray_ids("blocked by abc-1234 and def-99a here");
        assert!(found.contains(&"abc-1234".to_string()));
        assert!(found.contains(&"def-99a".to_string()));
    }

    #[test]
    fn find_stray_ids_excludes_denylisted_prefixes() {
        assert!(find_stray_ids("sha-256 utf-8 rfc-822 crc-32 cve-2024").is_empty());
    }

    #[test]
    fn find_stray_ids_empty_when_no_id_shapes() {
        assert!(find_stray_ids("just some ordinary prose here").is_empty());
    }

    // -- check_leaks ---------------------------------------------------------

    // MC/DC for check_leaks' own-id guard:
    //   `contains_word(outbound, resolved_id) || contains_word(outbound, raw_input_id)`
    //   c1 = outbound contains the resolved id at a boundary
    //   c2 = outbound contains the raw CLI-supplied id at a boundary
    // Outcome true => hard error on a real run. A pure OR: each single-true
    // case is masked-independent against the all-false case.
    //   T1 (F,F): check_leaks_passes_clean_body => Ok
    //   T2 (T,F): check_leaks_blocks_own_resolved_id_word_bounded => error
    //   T3 (F,T): check_leaks_blocks_raw_partial_id_at_word_boundary => error
    // Independence pairs:
    //   c1: T2 vs T1
    //   c2: T3 vs T1
    #[test]
    fn check_leaks_passes_clean_body() {
        let all_ids = vec!["tic-abc12".to_string()];
        assert!(
            check_leaks(
                "Clean title\nbody about widgets and gears",
                "tic-abc12",
                "tic-abc12",
                &all_ids,
                false,
            )
            .is_ok()
        );
    }

    #[test]
    fn check_leaks_blocks_own_resolved_id_word_bounded() {
        let all_ids = vec!["tic-abc12".to_string()];
        // raw id "zzz" is absent, isolating the resolved-id (c1) branch.
        let err = check_leaks(
            "See tic-abc12 for the rationale",
            "tic-abc12",
            "zzz",
            &all_ids,
            false,
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("tk ids must never appear"),
            "got: {err}"
        );
    }

    #[test]
    fn check_leaks_blocks_raw_partial_id_at_word_boundary() {
        let all_ids = vec!["tic-abc12".to_string()];
        // The full resolved id "tic-abc12" is not present verbatim, but the
        // raw CLI-supplied partial "abc12" appears as its own token (a
        // boundary on both sides), isolating the c2 branch.
        let err = check_leaks(
            "See abc12 in the log for details",
            "tic-abc12",
            "abc12",
            &all_ids,
            false,
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("tk ids must never appear"),
            "got: {err}"
        );
    }

    #[test]
    fn check_leaks_raw_partial_mid_token_is_not_a_leak() {
        let all_ids = vec!["tic-abc12".to_string()];
        // "abc12" embedded inside a longer token ("xabc12y") is not a leak:
        // matching the raw partial now requires the same boundary as any
        // other id, so a short fragment can't false-positive on an
        // unrelated word that happens to contain it.
        assert!(
            check_leaks(
                "Refs xabc12y in passing",
                "tic-abc12",
                "abc12",
                &all_ids,
                false,
            )
            .is_ok()
        );
    }

    #[test]
    fn check_leaks_blocks_punctuation_prefixed_store_id() {
        // Ids derived from a dotted working-directory name (e.g.
        // `.tm-1a2b3`) begin with a non-word character. A regex `\b` word
        // boundary never fires when neither side of a position is a word
        // character (as happens right before a leading `.` preceded by
        // whitespace), so the old \b-based check let ids like this slip
        // through undetected.
        let all_ids = vec![".tm-1a2b3".to_string(), "tic-abc12".to_string()];
        let err = check_leaks(
            "Depends on .tm-1a2b3 landing first",
            "tic-abc12",
            "tic-abc12",
            &all_ids,
            false,
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("references local tk ids"),
            "got: {err}"
        );
        assert!(err.to_string().contains(".tm-1a2b3"), "got: {err}");
    }

    #[test]
    fn check_leaks_blocks_another_store_id() {
        let all_ids = vec!["tic-abc12".to_string(), "tic-other9".to_string()];
        let err = check_leaks(
            "This depends on tic-other9 landing first",
            "tic-abc12",
            "tic-abc12",
            &all_ids,
            false,
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("references local tk ids"),
            "got: {err}"
        );
        assert!(err.to_string().contains("tic-other9"), "got: {err}");
    }

    #[test]
    fn check_leaks_allows_denylisted_hash_shapes() {
        let all_ids = vec!["tic-abc12".to_string()];
        assert!(
            check_leaks(
                "We hash with sha-256 and validate utf-8",
                "tic-abc12",
                "tic-abc12",
                &all_ids,
                false,
            )
            .is_ok()
        );
    }

    #[test]
    fn check_leaks_warns_but_passes_on_foreign_id_shape() {
        let all_ids = vec!["tic-abc12".to_string()];
        // "abc-1234" is not a store id and not denylisted: a stray-shape
        // warning, not a hard failure.
        assert!(
            check_leaks(
                "Related to abc-1234 upstream",
                "tic-abc12",
                "tic-abc12",
                &all_ids,
                false,
            )
            .is_ok()
        );
    }

    // `--dry-run` is the documented readability-pass entry point, whose
    // purpose includes finding and removing tk-id cross-references, so both
    // hard-fail conditions above must downgrade to a warning (still `Ok`)
    // rather than block the very output the pass needs to see.
    #[test]
    fn check_leaks_dry_run_downgrades_own_id_leak_to_warning() {
        let all_ids = vec!["tic-abc12".to_string()];
        assert!(
            check_leaks(
                "See tic-abc12 for the rationale",
                "tic-abc12",
                "tic-abc12",
                &all_ids,
                true,
            )
            .is_ok()
        );
    }

    #[test]
    fn check_leaks_dry_run_downgrades_store_id_leak_to_warning() {
        let all_ids = vec!["tic-abc12".to_string(), "tic-other9".to_string()];
        assert!(
            check_leaks(
                "This depends on tic-other9 landing first",
                "tic-abc12",
                "tic-abc12",
                &all_ids,
                true,
            )
            .is_ok()
        );
    }

    // -- priority_target / extract_issue_number ------------------------------

    #[test]
    fn priority_target_maps_known_priorities() {
        assert_eq!(priority_target(0), Some("Urgent"));
        assert_eq!(priority_target(1), Some("High"));
        assert_eq!(priority_target(2), Some("Medium"));
        assert_eq!(priority_target(3), Some("Low"));
        assert_eq!(priority_target(4), None);
    }

    #[test]
    fn extract_issue_number_parses_trailing_number() {
        assert_eq!(
            extract_issue_number("https://github.com/example/repo/issues/999"),
            Some(999)
        );
        assert_eq!(extract_issue_number("no issue url here"), None);
    }
}
