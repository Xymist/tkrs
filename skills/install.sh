#!/usr/bin/env bash

set -euo pipefail

skills_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)
install_failed=0

link_skills() {
  local source_dir=$1
  local target_dir=$2
  local skill_source
  local skill_name
  local skill_target

  if [[ ! -d "$target_dir" ]]; then
    printf 'Skipping missing directory: %s\n' "$target_dir"
    return
  fi

  for skill_source in "$source_dir"/*; do
    [[ -f "$skill_source/SKILL.md" ]] || continue
    skill_name=${skill_source##*/}
    skill_target="$target_dir/$skill_name"

    if [[ -L "$skill_target" ]] &&
      [[ "$(readlink "$skill_target")" == "$skill_source" ]]; then
      printf 'Already linked: %s\n' "$skill_target"
    elif [[ -e "$skill_target" || -L "$skill_target" ]]; then
      printf 'Refusing to overwrite: %s\n' "$skill_target" >&2
      install_failed=1
    else
      ln -s "$skill_source" "$skill_target"
      printf 'Linked: %s -> %s\n' "$skill_target" "$skill_source"
    fi
  done
}

link_skills "$skills_dir/claude" "$HOME/.claude/skills"
link_skills "$skills_dir/codex" "$HOME/.codex/skills"

exit "$install_failed"
