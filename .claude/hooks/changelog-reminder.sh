#!/usr/bin/env bash
# Claude Code PreToolUse (Bash) hook — non-blocking CHANGELOG reminder.
#
# When Claude runs a `git commit` that stages Rust source (*.rs) but does NOT
# stage CHANGELOG.md, emit a reminder to update the [Unreleased] section.
# It never blocks the commit; it only injects a note Claude sees next turn
# plus a one-line message for the user.
#
# Limitation: only inspects the staged index (`git diff --cached`). A commit
# made with `git commit -a` (auto-stage) won't be seen here.
set -uo pipefail

input=$(cat)

# Only act on commands that actually commit (handles `git add x && git commit`).
cmd=$(printf '%s' "$input" | jq -r '.tool_input.command // ""' 2>/dev/null || true)
case "$cmd" in
  *"git commit"*) ;;
  *) exit 0 ;;
esac

staged=$(git diff --cached --name-only 2>/dev/null || true)

if printf '%s\n' "$staged" | grep -qE '\.rs$' \
   && ! printf '%s\n' "$staged" | grep -qx 'CHANGELOG.md'; then
  jq -n '{
    hookSpecificOutput: {
      hookEventName: "PreToolUse",
      additionalContext: "Staged .rs source changes but CHANGELOG.md is not staged. Add an entry under the [Unreleased] section of CHANGELOG.md, then: git add CHANGELOG.md && git commit --amend --no-edit."
    },
    systemMessage: "⚠️  CHANGELOG.md not updated for this commit — consider adding an [Unreleased] entry."
  }'
fi
