#!/usr/bin/env bash
# scripts/coverage.sh
#
# Generate and view workspace coverage.
# Requires: cargo install cargo-llvm-cov
#
# Usage:
#   ./scripts/coverage.sh          → summary + lcov report
#   ./scripts/coverage.sh --html   → open HTML report in browser
#   ./scripts/coverage.sh --check  → fail if coverage < 85% (CI mode)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKSPACE="$(cd "$SCRIPT_DIR/.." && pwd)"

cd "$WORKSPACE"

MODE="${1:-}"

case "$MODE" in
  --html)
    echo "Generating HTML coverage report…"
    cargo llvm-cov --workspace --html --output-dir target/coverage/html
    echo "Report written to target/coverage/html/index.html"
    if command -v xdg-open &>/dev/null; then
      xdg-open target/coverage/html/index.html
    elif command -v open &>/dev/null; then
      open target/coverage/html/index.html
    fi
    ;;
  --check)
    echo "Checking coverage threshold (≥85% lines and regions)…"
    cargo llvm-cov --workspace --summary-only --fail-under-lines 85 --fail-under-regions 85
    ;;
  *)
    echo "Generating LCOV coverage report…"
    mkdir -p target/coverage
    cargo llvm-cov --workspace --lcov --output-path target/coverage/lcov.info
    echo "LCOV report: target/coverage/lcov.info"
    echo ""
    echo "Summary:"
    cargo llvm-cov --workspace --summary-only
    ;;
esac
