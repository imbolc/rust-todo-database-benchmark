#!/usr/bin/env sh

set -eu

# Worktrees share hooks. Offer setup only when a hook is missing and input
# is interactive, so automated checks never wait for an installation answer.
HOOK_PATH=$(git rev-parse --git-path hooks/pre-commit)

if [ ! -f "$HOOK_PATH" ] && [ -t 0 ]; then
    # Install tools
    cargo clippy --version >/dev/null 2>&1 || rustup component add clippy
    cargo shear --version >/dev/null 2>&1 || cargo install --locked cargo-shear
    cargo sort --version >/dev/null 2>&1 || cargo install --locked cargo-sort
    typos --version >/dev/null 2>&1 || cargo install --locked typos-cli
    rumdl --version >/dev/null 2>&1 || cargo install --locked rumdl

    rustup toolchain list | grep -q 'nightly' || rustup toolchain install nightly
    cargo +nightly fmt --version >/dev/null 2>&1 || rustup component add rustfmt --toolchain nightly

    printf "Install the shared pre-commit hook to avoid further manual running? (y/N): "
    read -r link_hook
    case "$link_hook" in
    [Yy])
        [ ! -L "$HOOK_PATH" ] || unlink "$HOOK_PATH"
        cat >"$HOOK_PATH" <<'EOF'
#!/bin/sh
exec "$(git rev-parse --show-toplevel)/.pre-commit.sh" "$@"
EOF
        chmod +x "$HOOK_PATH"
        ;;
    esac
fi

check_log=$(mktemp)
trap 'unlink "$check_log"' 0

run_quiet() {
    name=$1
    shift
    if "$@" >"$check_log" 2>&1; then
        return 0
    else
        status=$?
        printf '%s: failed (exit %s)\n' "$name" "$status"
        cat "$check_log"
        return "$status"
    fi
}

# Checks
typos .
rumdl check -q . || { printf "Run:\nrumdl fmt .\n" && exit 1; }
cargo +nightly fmt --all -- --check || { printf "Run:\ncargo +nightly fmt --all\n" && exit 1; }
run_quiet "cargo sort" cargo sort -c || { printf "Run:\ncargo sort --workspace\n" && exit 1; }
run_quiet "tests" cargo test -q --all-features --all-targets
cargo clippy -q --all-features --all-targets -- -D warnings
run_quiet "cargo shear" cargo shear || { printf "Run:\ncargo shear --fix\n" && exit 1; }
