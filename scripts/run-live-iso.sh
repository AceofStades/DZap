#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
ISO_FILE=${1:-}
VM_DISK=${DZAP_VM_DISK:-"$REPO_ROOT/build/archiso/test-disk.qcow2"}

if [[ -z $ISO_FILE ]]; then
    ISO_FILE=$(find "$REPO_ROOT/out" -maxdepth 1 -type f -name 'dzap-*.iso' -print 2>/dev/null | sort | tail -n 1)
fi
if [[ -z $ISO_FILE || ! -f $ISO_FILE ]]; then
    echo "DZap ISO not found. Run scripts/build-live-iso.sh first." >&2
    exit 1
fi

mkdir -p "$(dirname "$VM_DISK")"
if [[ ! -f $VM_DISK ]]; then
    qemu-img create -f qcow2 "$VM_DISK" 4G
fi

acceleration=()
if [[ -r /dev/kvm && -w /dev/kvm ]]; then
    acceleration=(-enable-kvm)
fi

cat <<'EOF'
QEMU guest console switching (safe when the host intercepts Ctrl+Alt+Fn):
  1. Press Ctrl+Alt+2 to open the QEMU monitor.
  2. Type `sendkey ctrl-alt-f2` for the Rust TUI, or `sendkey ctrl-alt-f1` for the kiosk.
  3. Press Ctrl+Alt+1 to return to the guest display.
  Ctrl+Alt+G releases QEMU's keyboard and mouse grab.
EOF

exec qemu-system-x86_64 \
    "${acceleration[@]}" \
    -m 4096 \
    -smp 2 \
    -boot d \
    -cdrom "$ISO_FILE" \
    -drive "if=virtio,format=qcow2,file=$VM_DISK" \
    -vga std
