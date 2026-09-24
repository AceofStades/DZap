#!/usr/bin/env bash
# Creates the throwaway qcow2 disks used by the DZap project demo. These are
# plain files under build/demo/ (already git-ignored via build/) -- nothing
# on the host's real drives is created, mounted, or touched by this script.
set -euo pipefail

REPO_ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
DEMO_DIR="$REPO_ROOT/build/demo"
FORCE=0
[[ "${1:-}" == "--force" ]] && FORCE=1

mkdir -p "$DEMO_DIR"

create_disk() {
    local path=$1 size=$2 label=$3
    if [[ -f $path && $FORCE -eq 0 ]]; then
        echo "==> $label already exists at $path (use --force to recreate)"
        return
    fi
    rm -f "$path"
    qemu-img create -f qcow2 "$path" "$size" >/dev/null
    echo "==> Created $label: $path ($size)"
}

create_disk "$DEMO_DIR/wipe-target.qcow2" 64M \
    "wipe-target      (secure-erase demo drive, stays blank)"
create_disk "$DEMO_DIR/recovery-source.qcow2" 128M \
    "recovery-source   (gets seeded with files, then 'accidentally' formatted)"
create_disk "$DEMO_DIR/recovery-destination.qcow2" 512M \
    "recovery-destination (USB-like target for the ddrescue image + recovered files)"

cat <<EOF

Demo disks ready under $DEMO_DIR
These are ordinary qcow2 files; nothing on this machine's real storage
was created, mounted, or modified.

Next: scripts/demo/run-demo.sh
EOF
