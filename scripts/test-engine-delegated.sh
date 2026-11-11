#!/usr/bin/env bash
# Run Engine tests with a private cgroup on a Linux host with a user systemd manager.
set -euo pipefail
repo=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)
test -x /usr/bin/bwrap
export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"

exec timeout 300s systemd-run --user --wait --pipe --collect --quiet \
    --expand-environment=no --working-directory="$repo" --setenv="PATH=$PATH" \
    -p RuntimeMaxSec=290s -p 'Delegate=cpu memory pids' -p DelegateSubgroup=runner \
    /usr/bin/bash -euc '
        scope=$(sed -n "s/^0:://p" /proc/self/cgroup)
        case "$scope" in
            /*.service/runner) ;;
            *) echo "Expected an isolated systemd service subgroup, got $scope" >&2; exit 1 ;;
        esac
        export PULSAR_CGROUP_ROOT="/sys/fs/cgroup${scope%/*}"
        printf "%s\n" "+cpu +memory +pids" > "$PULSAR_CGROUP_ROOT/cgroup.subtree_control"
        exec env RUST_MIN_STACK=16777216 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_DEV_OPT_LEVEL=0 \
            CARGO_PROFILE_TEST_DEBUG=0 CARGO_PROFILE_TEST_OPT_LEVEL=0 \
            priority-build cargo test -p pulsar-engine --lib --locked "$@"
    ' bash "$@"
