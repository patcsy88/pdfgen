#!/usr/bin/env bash
# Fails unless the Go module has no third-party dependencies. With none, there is no third-party license to
# check; the Go standard library (BSD-3-Clause) is the only other code a built binary contains.
# Run from go/: ./license-check.sh
set -euo pipefail

mods=$(go list -m all)
main=$(go list -m)
extra=$(grep -vxF "$main" <<<"$mods" || true)

if [ -n "$extra" ]; then
	echo "FAIL third-party modules found; check each license against the allowlist before using them:" >&2
	echo "$extra" >&2
	exit 1
fi
if grep -qE '^\s*require\b|^\s*replace\b' go.mod; then
	echo "FAIL go.mod has require or replace directives" >&2
	exit 1
fi
echo "ok   $main  (no third-party modules; go.sum $( [ -e go.sum ] && echo present || echo absent ))"
