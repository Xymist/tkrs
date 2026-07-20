#!/usr/bin/env python3
"""
Convert a tk ticket into a GitHub issue whose body is rendered the way GitHub
renders an issue-form submission (one `### <field label>` heading per field),
and open it in a repo chosen at call time.

The default field set matches a conventional maintenance-ticket form; when the
target repo has its own issue form, pass --fields-json so the body uses that
form's labels and parses as a submission of it.

Default field values:
  - "How easy is this to change..."   -> Unsure
  - "Which module(s)..."              -> Unsure/Other
  - "...hours of developer time..."   -> 1
  - assignee                          -> none (pass --assignee, e.g. @me)

After creation, the repo-level "Priority" issue field (GitHub's issue-fields
preview) is set from the ticket's tk priority: P0->Urgent, P1->High, P2->Medium,
P3->Low. Best-effort — skipped (with a note) if the repo has no such field, the
option name doesn't match, or the token can't set it. Disable with --no-priority.

Body fields populated from the ticket:
  - "Describe the change required"            <- ticket intro + implementation plan
                                                 (the plan is read from ## Implementation
                                                 Plan, or falls back to ## Notes for
                                                 tickets whose plan was added via
                                                 `tk add-note`)
  - "List the success/acceptance criteria"    <- ticket ## Acceptance Criteria
  - "What happens if we don't do this?"       <- a standard default (tk has no such field)

The rendered body mirrors what GitHub produces when an issue form is
submitted: one `### <field label>` heading per field, value beneath.

IMPORTANT: the tk ticket id is NEVER written into the GitHub issue (tk ids are
local-only). Instead, after the issue is created, the ticket is pinned with
`tk edit <id> --external-ref gh-<number>` (direct frontmatter write as a
fallback when tk is unavailable) so the link lives on the private side only.

Usage:
    tk_to_gh_issue.py <ticket-id> <owner/repo> [options]

Options:
    --assignee NAME     GitHub assignee, e.g. @me (default: unassigned)
    --title-prefix STR  Issue title prefix (default: "[Maintenance]: ")
    --fields-json PATH  JSON array replacing the default body fields; each
                        entry is {"label": ..., "value": ...} for a fixed
                        value or {"label": ..., "source": "describe" |
                        "acceptance"} for ticket-derived content
    --tickets-root DIR  Repo root containing the .tickets/ dir
                        (default: auto-detect from CWD upward)
    --dry-run           Print the title, body and gh command; create nothing
    --body-file PATH    Use this file's contents as the issue body instead of
                        rendering one from the ticket (title, pinning and
                        priority still come from the ticket). This is how the
                        skill's readability pass feeds the polished body in.
    --no-pin            Do not write external_ref back to the tk ticket
    --no-priority       Do not set the repo Priority issue field
    --priority-field N  Name of the single-select issue field to set
                        (default: Priority)
    --label LABEL       Add a label (repeatable); none by default
"""
import argparse
import json
import os
import re
import subprocess
import sys
import tempfile

# Default field labels, in form order. When targeting a repo with its own
# issue form, override the whole set with --fields-json so the labels match
# that form exactly (GitHub only parses the body as a form submission when
# every `###` heading matches a form field label byte-for-byte).
LBL_DESCRIBE = "Describe the change required"
LBL_SUCCESS = "List the success/acceptance criteria for this ticket"
LBL_HOURS = "How many hours of developer time has this lost us?"
LBL_COST = "What happens if we don't do this?"
LBL_EFFORT = "How easy is this to change or fix?"
LBL_MODULE = "Which module(s) would making this change affect?"

DEFAULT_COST = (
    "Tracked maintenance item. Deferring it leaves the change described "
    "above unaddressed; see the description and acceptance criteria for "
    "scope and impact."
)


def die(msg):
    print(f"error: {msg}", file=sys.stderr)
    sys.exit(1)


def find_ticket_file(ticket_id, tickets_root):
    cands = []
    if tickets_root:
        cands.append(os.path.join(tickets_root, ".tickets", f"{ticket_id}.md"))
        cands.append(os.path.join(tickets_root, f"{ticket_id}.md"))
    d = os.path.abspath(os.getcwd())
    while True:
        cands.append(os.path.join(d, ".tickets", f"{ticket_id}.md"))
        parent = os.path.dirname(d)
        if parent == d:
            break
        d = parent
    for c in cands:
        if os.path.isfile(c):
            return c
    return None


def split_frontmatter(text):
    """Return (frontmatter_str, body_str). Frontmatter is the block between the
    first two '---' fences."""
    if text.lstrip().startswith("---"):
        m = re.match(r"^\s*---\n(.*?)\n---\n?(.*)$", text, re.DOTALL)
        if m:
            return m.group(1), m.group(2)
    return "", text


# tk's parser switches sections on a case-sensitive *prefix* match of these
# exact strings (read_ticket in src/fs.rs), including the lowercase plan
# variant and the legacy Design alias; mirror it so both sides slice the same
# content. `###` headings inside a section body don't match any prefix, so
# they stay content, not section boundaries.
HEADING_PREFIXES = [
    ("## Implementation Plan", "Implementation Plan"),
    ("## Implementation plan", "Implementation Plan"),
    ("## Design", "Implementation Plan"),
    ("## Acceptance Criteria", "Acceptance Criteria"),
    ("## Notes", "Notes"),
]


def parse_body(body):
    """Return (title, intro_text, {canonical_section: content}).

    Sections are sliced between the *first* occurrence of each canonical
    heading, in document order, so embedded headings never create spurious
    sections."""
    lines = body.splitlines()
    title = None
    title_idx = -1
    for i, line in enumerate(lines):
        if line.startswith("# ") and not line.startswith("## "):
            title = line[2:].strip()
            title_idx = i
            break

    anchors = []  # (line_index, canonical_name)
    seen = set()
    for i, line in enumerate(lines):
        if i <= title_idx:
            continue
        canon = next((c for p, c in HEADING_PREFIXES if line.startswith(p)),
                     None)
        if canon and canon not in seen:
            anchors.append((i, canon))
            seen.add(canon)
    anchors.sort()

    first_section = anchors[0][0] if anchors else len(lines)
    intro_text = "\n".join(lines[title_idx + 1:first_section]).strip()

    sec = {}
    for j, (idx, name) in enumerate(anchors):
        end = anchors[j + 1][0] if j + 1 < len(anchors) else len(lines)
        sec[name] = "\n".join(lines[idx + 1:end]).strip()
    return title, intro_text, sec


def is_blank(s):
    return not s or s.strip() in ("", "-", "_", "TBD", "N/A")


DEFAULT_FIELDS = [
    {"label": LBL_DESCRIBE, "source": "describe"},
    {"label": LBL_SUCCESS, "source": "acceptance"},
    {"label": LBL_HOURS, "value": "1"},
    {"label": LBL_COST, "value": DEFAULT_COST},
    {"label": LBL_EFFORT, "value": "Unsure"},
    {"label": LBL_MODULE, "value": "Unsure/Other"},
]


def load_fields_spec(path):
    try:
        with open(path) as f:
            spec = json.load(f)
    except (OSError, json.JSONDecodeError) as e:
        die(f"could not read --fields-json: {e}")
    if not isinstance(spec, list) or not spec:
        die("--fields-json must be a non-empty JSON array")
    labels = set()
    for i, entry in enumerate(spec):
        if not isinstance(entry, dict) or "label" not in entry:
            die(f"--fields-json entry {i} needs a 'label'")
        label = entry["label"]
        if (not isinstance(label, str) or not label.strip()
                or "\n" in label or "\r" in label):
            die(f"--fields-json entry {i}: 'label' must be a non-empty "
                f"single-line string")
        if label in labels:
            die(f"--fields-json: duplicate label '{label}'")
        labels.add(label)
        if ("source" in entry) == ("value" in entry):
            die(f"--fields-json entry {i} ('{label}') needs exactly "
                f"one of 'source' or 'value'")
        if "value" in entry and not isinstance(entry["value"], str):
            die(f"--fields-json entry {i} ('{label}'): 'value' must be "
                f"a string")
        if entry.get("source") not in (None, "describe", "acceptance"):
            die(f"--fields-json entry {i}: unknown source "
                f"'{entry['source']}' (use 'describe' or 'acceptance')")
    return spec


def build_body(intro, sec, fields):
    impl = sec.get("Implementation Plan", "")
    notes = sec.get("Notes", "")
    acceptance = sec.get("Acceptance Criteria", "")

    # "describe" = intro + the plan, wherever it lives. Some tickets carry the
    # plan under ## Implementation Plan; others (added via `tk add-note`)
    # carry it under ## Notes. Prefer the former; fall back to the latter so
    # the plan always reaches the issue.
    describe = intro
    if not is_blank(impl):
        describe = (describe + "\n\n" if describe else "") + \
            "#### Implementation plan\n\n" + impl
    elif not is_blank(notes):
        describe = (describe + "\n\n" if describe else "") + \
            "#### Details\n\n" + notes
    if is_blank(describe):
        describe = "_(No description in source ticket.)_"

    if is_blank(acceptance):
        acceptance = "See the description / implementation plan above."

    sources = {"describe": describe, "acceptance": acceptance}
    return "\n\n".join(
        f"### {f['label']}\n\n" + (f["value"] if "value" in f
                                   else sources[f["source"]])
        for f in fields)


# tk priority (frontmatter `priority: N`, 0..3 == P0..P3) -> the repo "Priority"
# issue-field option name. Matching against the live option list is fuzzy
# (case/emoji-insensitive), so "Urgent" still matches an option like "🌋 Urgent".
PRIORITY_MAP = {0: "Urgent", 1: "High", 2: "Medium", 3: "Low"}


def parse_priority(fm):
    m = re.search(r"^priority:\s*(\d+)\s*$", fm, re.MULTILINE)
    return int(m.group(1)) if m else None


def parse_external_ref(fm):
    m = re.search(r"^external_ref:\s*(\S+)\s*$", fm, re.MULTILINE)
    return m.group(1).strip("'\"") if m else None


def _norm(s):
    """Reduce a label to comparable letters: '🌋 Urgent' -> 'urgent'."""
    return re.sub(r"[^a-z]", "", (s or "").lower())


def gh_graphql(query, **variables):
    """Run a GraphQL query/mutation via `gh api graphql`. Ints are passed as
    typed variables (-F), everything else as strings (-f). Raises RuntimeError
    on transport error or a GraphQL `errors` payload."""
    cmd = ["gh", "api", "graphql", "-f", "query=" + query]
    for k, v in variables.items():
        flag = "-F" if isinstance(v, int) and not isinstance(v, bool) else "-f"
        cmd += [flag, f"{k}={v}"]
    out = subprocess.run(cmd, capture_output=True, text=True)
    try:
        data = json.loads(out.stdout) if out.stdout.strip() else {}
    except json.JSONDecodeError:
        data = {}
    if out.returncode != 0 or data.get("errors"):
        raise RuntimeError(data.get("errors") or out.stderr.strip() or out.stdout.strip())
    return data.get("data") or {}


def set_issue_priority(gh_repo, issue_number, priority_num, field_name):
    """Best-effort: set the repo's single-select `field_name` issue field on the
    issue to the option mapped from the tk priority. Never raises — prints a
    note/warning and returns on any miss."""
    if priority_num is None:
        print(f"note: ticket has no priority; '{field_name}' left unset", file=sys.stderr)
        return
    target = PRIORITY_MAP.get(priority_num)
    if target is None:
        print(f"note: priority {priority_num} has no mapping; '{field_name}' left unset",
              file=sys.stderr)
        return
    if "/" not in gh_repo:
        print(f"warning: cannot parse repo '{gh_repo}'; priority not set", file=sys.stderr)
        return
    owner, name = gh_repo.split("/", 1)

    q = """
    query($owner:String!, $name:String!, $number:Int!) {
      repository(owner:$owner, name:$name) {
        issue(number:$number) { id viewerCanSetFields }
        issueFields(first:50) {
          nodes { __typename
            ... on IssueFieldSingleSelect { id name options { id name } } }
        }
      }
    }"""
    try:
        data = gh_graphql(q, owner=owner, name=name, number=issue_number)
    except RuntimeError as e:
        print(f"warning: could not read issue fields ({e}); priority not set", file=sys.stderr)
        return

    repo = data.get("repository") or {}
    issue = repo.get("issue") or {}
    field = next((n for n in (repo.get("issueFields") or {}).get("nodes", [])
                  if n.get("__typename") == "IssueFieldSingleSelect"
                  and n.get("name", "").lower() == field_name.lower()), None)
    if not field:
        print(f"note: repo {gh_repo} has no '{field_name}' issue field; priority not set",
              file=sys.stderr)
        return
    opt = next((o for o in field["options"] if _norm(o["name"]) == _norm(target)), None)
    if not opt:
        have = ", ".join(o["name"] for o in field["options"])
        print(f"warning: '{field_name}' has no '{target}' option (have: {have}); "
              f"priority not set", file=sys.stderr)
        return
    if issue.get("viewerCanSetFields") is False:
        print(f"warning: no permission to set fields on #{issue_number}; priority not set",
              file=sys.stderr)
        return

    mut = """
    mutation($issueId:ID!, $fieldId:ID!, $optionId:ID!) {
      setIssueFieldValue(input:{
        issueId:$issueId,
        issueFields:[{ fieldId:$fieldId, singleSelectOptionId:$optionId }]
      }) { clientMutationId }
    }"""
    try:
        gh_graphql(mut, issueId=issue["id"], fieldId=field["id"], optionId=opt["id"])
    except RuntimeError as e:
        print(f"warning: setting priority failed ({e})", file=sys.stderr)
        return
    print(f"set {field_name} = {opt['name']} (P{priority_num})")


def pin_external_ref(path, issue_number, ticket_id):
    """Set external_ref: gh-<n> on the ticket (best-effort); returns the ref
    on success, None otherwise. Uses `tk edit --external-ref`, which
    round-trips the file through tk's own parser and locking. A direct
    frontmatter rewrite happens only when tk is not installed at all — a
    *failing* tk is never second-guessed with a file write, since the
    failure may be a lock or concurrent edit. The fallback re-reads the file
    so it never writes content from before the issue was created."""
    ref = f"gh-{issue_number}"
    env = dict(os.environ, TICKETS_DIR=os.path.dirname(path))
    try:
        out = subprocess.run(["tk", "edit", ticket_id, "--external-ref", ref],
                             capture_output=True, text=True, env=env)
        if out.returncode == 0:
            return ref
        print(f"warning: tk edit failed "
              f"({(out.stderr or out.stdout).strip()}); external_ref not "
              f"set — pin manually with: tk edit {ticket_id} "
              f"--external-ref {ref}", file=sys.stderr)
        return None
    except FileNotFoundError:
        print("warning: tk not on PATH; writing frontmatter directly",
              file=sys.stderr)
    try:
        with open(path) as f:
            full_text = f.read()
    except OSError as e:
        print(f"warning: could not re-read ticket ({e}); external_ref "
              f"not set", file=sys.stderr)
        return None
    fm, _ = split_frontmatter(full_text)
    if not fm:
        print(f"warning: no frontmatter in {path}; external_ref not set",
              file=sys.stderr)
        return None
    if re.search(r"^external_ref:.*$", fm, re.MULTILINE):
        new_fm = re.sub(r"^external_ref:.*$", f"external_ref: {ref}", fm,
                        count=1, flags=re.MULTILINE)
    else:
        new_fm = fm.rstrip("\n") + f"\nexternal_ref: {ref}"
    new_text = full_text.replace(fm, new_fm, 1)
    with open(path, "w") as f:
        f.write(new_text)
    return ref


def main():
    ap = argparse.ArgumentParser(description="tk ticket -> GitHub maintenance issue")
    ap.add_argument("ticket_id")
    ap.add_argument("gh_repo", help="target GitHub repo, owner/name")
    ap.add_argument("--assignee", default=None,
                    help="GitHub assignee, e.g. @me (default: unassigned)")
    ap.add_argument("--title-prefix", default="[Maintenance]: ",
                    help="issue title prefix (default: '[Maintenance]: ')")
    ap.add_argument("--fields-json", default=None,
                    help="JSON array replacing the default body fields; "
                         "entries are {label, value} or {label, source: "
                         "describe|acceptance}")
    ap.add_argument("--tickets-root", default=None)
    ap.add_argument("--dry-run", action="store_true")
    ap.add_argument("--body-file", default=None,
                    help="use this file's contents as the issue body instead "
                         "of rendering one from the ticket")
    ap.add_argument("--no-pin", action="store_true")
    ap.add_argument("--re-file", action="store_true",
                    help="create a new issue even though the ticket already "
                         "has an external_ref (the pin is then repointed at "
                         "the new issue unless --no-pin is also given)")
    ap.add_argument("--no-priority", action="store_true",
                    help="do not set the repo Priority issue field")
    ap.add_argument("--priority-field", default="Priority",
                    help="name of the single-select issue field to set (default: Priority)")
    ap.add_argument("--label", action="append", default=[])
    args = ap.parse_args()

    path = find_ticket_file(args.ticket_id, args.tickets_root)
    if not path:
        die(f"could not locate .tickets/{args.ticket_id}.md "
            f"(pass --tickets-root <repo-root>)")

    with open(path) as f:
        full_text = f.read()
    fm, body = split_frontmatter(full_text)
    title, intro, sec = parse_body(body)
    if not title:
        die(f"no '# Title' heading found in {path}")

    issue_title = f"{args.title_prefix}{title}"
    if args.body_file:
        try:
            with open(args.body_file) as bf:
                issue_body = bf.read().strip()
        except OSError as e:
            die(f"could not read --body-file: {e}")
        if not issue_body:
            die(f"--body-file {args.body_file} is empty")
    else:
        fields = (load_fields_spec(args.fields_json) if args.fields_json
                  else DEFAULT_FIELDS)
        issue_body = build_body(intro, sec, fields)
    priority_num = parse_priority(fm)

    # Idempotency: a pinned ticket already has a public issue. Creating
    # another would orphan the first and silently repoint the pin, so a
    # rerun fails closed unless the operator explicitly asks to re-file.
    existing_ref = parse_external_ref(fm)
    if existing_ref and not args.re_file and not args.dry_run:
        die(f"ticket is already pinned to {existing_ref}; pass --re-file to "
            f"deliberately create another issue (add --no-pin to keep the "
            f"existing pin)")

    # tk ids are local-only and must never reach GitHub. The source ticket's
    # id and any id that exists in the same local store are hard failures —
    # those are unambiguous leaks. Anything else merely *shaped* like a tk id
    # (e.g. an id from another repo's store surviving in prose) is a warning,
    # since the pattern can false-positive on ordinary hyphenated words.
    outbound = issue_title + "\n" + issue_body
    src_id = os.path.splitext(os.path.basename(path))[0]
    if (re.search(rf"\b{re.escape(src_id)}\b", outbound, re.IGNORECASE)
            or re.search(re.escape(args.ticket_id), outbound, re.IGNORECASE)):
        die(f"issue body/title contains the tk id '{src_id}'; "
            f"tk ids must never appear in GitHub issues — reword and retry")
    known = {os.path.splitext(f)[0]
             for f in os.listdir(os.path.dirname(path)) if f.endswith(".md")}
    known.discard(src_id)
    leaked = sorted(k for k in known
                    if re.search(rf"\b{re.escape(k)}\b", outbound,
                                 re.IGNORECASE))
    if leaked:
        die(f"issue body/title references local tk ids: "
            f"{', '.join(leaked)}; replace them with their gh-<n> external "
            f"refs (or describe the tickets in words) and retry")
    # Generic tk-id shape: short lowercase prefix + hex. The denylist drops
    # common technical terms that match the pattern (sha-256 etc.).
    stray_pat = re.findall(r"\b([a-z]{1,4})-([0-9a-f]{3,})\b", outbound,
                           re.IGNORECASE)
    not_ids = {"sha", "utf", "rfc", "iso", "crc", "cve"}
    strays = sorted({f"{p}-{h}" for p, h in stray_pat
                     if p.lower() not in not_ids})
    if strays:
        print(f"warning: body contains tk-id-like references: "
              f"{', '.join(strays)} — replace with gh-<n> refs or reword",
              file=sys.stderr)

    cmd = ["gh", "issue", "create", "--repo", args.gh_repo,
           "--title", issue_title]
    if args.assignee:
        cmd += ["--assignee", args.assignee]
    for lab in args.label:
        cmd += ["--label", lab]

    if args.dry_run:
        if existing_ref and not args.re_file:
            print(f"# note: ticket is already pinned to {existing_ref}; "
                  f"a real run will refuse without --re-file")
        print(f"# would create in {args.gh_repo} "
              f"(assignee: {args.assignee or 'none'})\n")
        print(f"TITLE: {issue_title}\n")
        print(issue_body)
        print("\n# gh command: " + " ".join(cmd) + " --body-file <tmp>")
        if not args.no_priority and priority_num in PRIORITY_MAP:
            print(f"# would set {args.priority_field} = "
                  f"{PRIORITY_MAP[priority_num]} (P{priority_num})")
        return

    with tempfile.NamedTemporaryFile("w", suffix=".md", delete=False) as tf:
        tf.write(issue_body)
        body_file = tf.name
    cmd += ["--body-file", body_file]
    try:
        out = subprocess.run(cmd, capture_output=True, text=True)
    finally:
        os.unlink(body_file)
    if out.returncode != 0:
        die("gh issue create failed:\n" + (out.stderr or out.stdout))

    url = out.stdout.strip().splitlines()[-1] if out.stdout.strip() else ""
    print(url or "(issue created; no URL returned)")

    m = re.search(r"/issues/(\d+)\b", url)
    if m and not args.no_pin:
        ref = pin_external_ref(path, m.group(1), args.ticket_id)
        if ref:
            print(f"pinned {args.ticket_id} -> external_ref: {ref}")
    elif not m and not args.no_pin:
        print("warning: could not parse issue number from URL; "
              "external_ref not set", file=sys.stderr)

    if m and not args.no_priority:
        set_issue_priority(args.gh_repo, int(m.group(1)), priority_num,
                           args.priority_field)
    elif not m and not args.no_priority:
        print("warning: could not parse issue number from URL; "
              "priority not set", file=sys.stderr)


if __name__ == "__main__":
    main()
