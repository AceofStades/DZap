#!/usr/bin/env bash
# End-to-end test: boot a throwaway QEMU VM from the Alpine live ISO with a
# scratch virtual disk, run the DZap backend inside it, and wipe the VIRTUAL
# disk (/dev/vda, a qcow2 file). Nothing on the host is ever wiped — the
# destructive code paths only exist inside the ephemeral VM.
#
# Usage: server/scripts/e2e-qemu.sh
# Requires: qemu-system-x86_64, qemu-img, curl, python3 (+ pexpect, installed
#           into an ephemeral venv automatically)
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
WORK=/tmp/dzap-e2e
ALPINE_VERSION=3.21.2
ALPINE_URL="https://dl-cdn.alpinelinux.org/alpine/v3.21/releases/x86_64/alpine-virt-${ALPINE_VERSION}-x86_64.iso"
ISO="$WORK/alpine-virt-${ALPINE_VERSION}-x86_64.iso"
SERVE_DIR="$WORK/serve"

mkdir -p "$WORK" "$SERVE_DIR"

echo "==> [1/5] Building static musl binary"
cargo build --release --target x86_64-unknown-linux-musl \
    --manifest-path "$REPO_ROOT/server/Cargo.toml"
cp "$REPO_ROOT/server/target/x86_64-unknown-linux-musl/release/server" "$SERVE_DIR/server"
cp "$REPO_ROOT/server/scripts/guest-test.sh" "$SERVE_DIR/guest-test.sh"

echo "==> [2/5] Fetching Alpine virt ISO (cached in $WORK)"
if [ ! -f "$ISO" ]; then
    curl -sS -o "$ISO" "$ALPINE_URL"
fi

echo "==> [3/5] Preparing scratch disk and driver"
rm -f "$WORK/scratch.qcow2" "$WORK/serial.log"
qemu-img create -f qcow2 "$WORK/scratch.qcow2" 64M >/dev/null

VENV="$WORK/venv"
if [ ! -x "$VENV/bin/python" ]; then
    python3 -m venv "$VENV"
    "$VENV/bin/pip" -q install pexpect
fi

echo "==> [4/5] Booting VM and running tests (serial log: $WORK/serial.log)"
python3 -m http.server 8123 --bind 127.0.0.1 --directory "$SERVE_DIR" >/dev/null 2>&1 &
HTTP_PID=$!
trap 'kill $HTTP_PID 2>/dev/null || true' EXIT

"$VENV/bin/python" "$REPO_ROOT/server/scripts/e2e-driver.py" "$ISO" "$WORK/scratch.qcow2" "$WORK/serial.log"

echo "==> [5/5] Results"
grep "^E2E:" "$WORK/serial.log" || true
if grep -q "E2E: ALL TESTS PASSED" "$WORK/serial.log"; then
    echo "==> e2e-qemu: PASS"
else
    echo "==> e2e-qemu: FAIL (full log: $WORK/serial.log)" >&2
    exit 1
fi
