#!/usr/bin/env bash
#
# self_improve.sh — drive flashcode to improve its own source, verified by tests.
#
# Each iteration operates on a THROWAWAY COPY of this repo (never your real tree):
#
#   1. Run `cargo test`.
#      - If it fails, ask flashcode to fix the failure.
#      - If it passes, ask flashcode to make one concrete improvement.
#   2. Run `cargo test` again.
#      - Green  -> commit the round to the copy's git history.
#      - Red    -> revert the round (git checkout .), so nothing unverified survives.
#
# The build/test suite is the fitness function: only verified changes are kept.
#
# Usage:
#   DEEPSEEK_API_KEY=sk-... ./self_improve.sh [iterations]
#
# Env:
#   ITERATIONS      number of rounds (default 5; overridden by $1)
#   FLASHCODE_BIN   path to the flashcode binary (default: ./target/release/flashcode)
#   WORKDIR         where the throwaway copy lives (default: a fresh mktemp dir)

set -uo pipefail

ITERATIONS="${1:-${ITERATIONS:-5}}"
SRC_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
FLASHCODE_BIN="${FLASHCODE_BIN:-$SRC_DIR/target/release/flashcode}"
WORKDIR="${WORKDIR:-$(mktemp -d "${TMPDIR:-/tmp}/flashcode-selfimprove.XXXXXX")}"

if [[ -z "${DEEPSEEK_API_KEY:-}" ]]; then
    echo "error: DEEPSEEK_API_KEY is not set" >&2
    exit 1
fi

# Build the binary from the pristine source first, if missing.
if [[ ! -x "$FLASHCODE_BIN" ]]; then
    echo ">> building flashcode (release) from $SRC_DIR"
    ( cd "$SRC_DIR" && cargo build --release ) || { echo "initial build failed" >&2; exit 1; }
fi

echo ">> source:   $SRC_DIR"
echo ">> binary:   $FLASHCODE_BIN"
echo ">> workdir:  $WORKDIR"
echo ">> rounds:   $ITERATIONS"
echo

# Make the throwaway copy: track the source, exclude build artifacts and VCS.
mkdir -p "$WORKDIR/repo"
tar -C "$SRC_DIR" \
    --exclude='./target' --exclude='./.git' --exclude='./self_improve.sh' \
    -cf - . | tar -C "$WORKDIR/repo" -xf -

cd "$WORKDIR/repo" || exit 1

# Give the copy its own git so we can commit/revert per round.
git init -q
git add -A
git -c user.email=selfimprove@flashcode -c user.name=flashcode commit -qm "baseline: copy of source"

run_tests() {
    cargo test --quiet >"$WORKDIR/test.log" 2>&1
}

echo "== establishing baseline =="
if run_tests; then
    echo "   baseline: tests PASS"
    BASELINE_GREEN=1
else
    echo "   baseline: tests FAIL (loop will try to fix first)"
    BASELINE_GREEN=0
fi
echo

FIX_PROMPT='The Rust project in this directory has failing tests. Run `cargo test` to
see the failures, diagnose the root cause, and fix the source so all tests pass.
Do not weaken or delete tests to make them pass. Keep changes minimal and focused.'

IMPROVE_PROMPT='This is the flashcode coding-agent CLI (Rust). Make ONE concrete,
self-contained improvement to the codebase and verify it with `cargo test`.
Good improvements: fix a latent bug you can find, add a missing unit test for
existing behavior, tighten error handling, or simplify a rough edge. Rules:
- Keep the change small and focused (one improvement per run).
- Do NOT add new dependencies or new CLI features.
- All existing tests must still pass; if you add behavior, add a test for it.
- Do not delete or weaken existing tests.
Briefly state what you improved and why.'

for (( i=1; i<=ITERATIONS; i++ )); do
    echo "================ iteration $i / $ITERATIONS ================"

    # Choose the task based on current test state.
    if run_tests; then
        echo ">> tests green -> asking for an improvement"
        PROMPT="$IMPROVE_PROMPT"
    else
        echo ">> tests red -> asking for a fix"
        PROMPT="$FIX_PROMPT"
    fi

    # Drive flashcode against the copy. --yes so it can edit without prompts.
    DEEPSEEK_API_KEY="$DEEPSEEK_API_KEY" \
        "$FLASHCODE_BIN" --yes -p "$PROMPT" || echo "   (flashcode exited non-zero)"

    echo ">> verifying with cargo test"
    if run_tests; then
        if git diff --quiet && git diff --cached --quiet; then
            echo ">> no changes made this round (tests still green); stopping early"
            break
        fi
        git add -A
        git -c user.email=selfimprove@flashcode -c user.name=flashcode \
            commit -qm "self-improve round $i (tests pass)"
        echo ">> round $i KEPT (committed)"
    else
        echo ">> round $i REVERTED (tests failed):"
        sed 's/^/     /' "$WORKDIR/test.log" | tail -n 15
        git reset -q --hard HEAD
    fi
    echo
done

echo "================ summary ================"
echo "commits in the throwaway copy:"
git --no-pager log --oneline
echo
echo "The improved copy is at: $WORKDIR/repo"
echo "Review it, and if you like a round, cherry-pick or diff it back into $SRC_DIR:"
echo "   diff -ru \"$SRC_DIR/src\" \"$WORKDIR/repo/src\""
