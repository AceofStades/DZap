#!/usr/bin/env bash
# Boots the built DZap ISO with three throwaway virtual disks for the
# wipe + recovery project demo. Everything destructive in this session
# happens to qcow2 files under build/demo/ -- the host's real drives are
# never attached to this VM.
set -euo pipefail

REPO_ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
DEMO_DIR="$REPO_ROOT/build/demo"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ISO_FILE=${1:-}
HTTP_PORT=${DZAP_DEMO_HTTP_PORT:-8123}
# Plain -vga std lets the guest's VESA driver pick its own default (often a
# small, oddly-shaped mode like 1280x800), which then gets stretched to fit
# whatever size the QEMU window is -- that's the source of the blurry,
# scroll-needed dashboard. xres/yres on the VGA device pins the boot
# resolution directly, so the window opens crisp and correctly sized. This
# QEMU build has no virtio-gpu/qxl (checked via `-vga help`), so std VGA
# with an explicit mode is the reliable option.
DEMO_WIDTH=${DZAP_DEMO_WIDTH:-1920}
DEMO_HEIGHT=${DZAP_DEMO_HEIGHT:-1200}

if [[ -z $ISO_FILE ]]; then
    ISO_FILE=$(find "$REPO_ROOT/out" -maxdepth 1 -type f -name 'dzap-*.iso' -print 2>/dev/null | sort | tail -n 1)
fi
if [[ -z $ISO_FILE || ! -f $ISO_FILE ]]; then
    echo "DZap ISO not found. Run 'make build-iso' first." >&2
    exit 1
fi

if [[ ! -f "$DEMO_DIR/wipe-target.qcow2" || ! -f "$DEMO_DIR/recovery-source.qcow2" || ! -f "$DEMO_DIR/recovery-destination.qcow2" ]]; then
    echo "==> Demo disks missing, creating them first"
    "$SCRIPT_DIR/prepare-demo-drives.sh"
fi

acceleration=()
if [[ -r /dev/kvm && -w /dev/kvm ]]; then
    acceleration=(-enable-kvm)
fi

# Serve seed-recovery-source.sh to the guest over QEMU user-mode networking
# (guest reaches the host at 10.0.2.2). Loopback-only, killed on exit.
python3 -m http.server "$HTTP_PORT" --bind 127.0.0.1 --directory "$SCRIPT_DIR" >/dev/null 2>&1 &
HTTP_PID=$!
trap 'kill "$HTTP_PID" 2>/dev/null || true' EXIT

cat <<EOF
==================================================================
 DZap demo VM -- using $ISO_FILE
 Guest display: ${DEMO_WIDTH}x${DEMO_HEIGHT} (override with
 DZAP_DEMO_WIDTH / DZAP_DEMO_HEIGHT env vars if your screen differs).

 Three throwaway virtual disks; nothing on this host is touched:

   /dev/vda  WIPE-TARGET        secure-erase demo (starts blank)
   /dev/vdb  RECOVERY-SOURCE    seed it, then it gets "formatted"
   /dev/sda  RECOVERY-DEST      USB-like evidence/recovery target

 Console switching (useful if the host intercepts Ctrl+Alt+Fn):
   1. Ctrl+Alt+2   -> QEMU monitor
   2. type one of:
        sendkey ctrl-alt-f3   root shell   (seed demo files here)
        sendkey ctrl-alt-f1   kiosk dashboard
        sendkey ctrl-alt-f2   read-only TUI
   3. Ctrl+Alt+1   -> back to the guest display
   Ctrl+Alt+G releases keyboard/mouse grab.

 On tty3, log in as "root" (no password), then run:

   curl -s http://10.0.2.2:$HTTP_PORT/seed-recovery-source.sh | sh

 That writes notes.txt + photo.png to /dev/vdb, hashes them, then
 quick-formats /dev/vdb (simulating an accidental format) and formats
 /dev/sda so the dashboard can use it as a recovery destination.
 Then switch to tty1 for the dashboard.
==================================================================
EOF

# Not exec'd: the HTTP server cleanup trap above needs this shell to still
# be alive when QEMU exits.
qemu-system-x86_64 \
    "${acceleration[@]}" \
    -m 4096 \
    -smp 2 \
    -boot d \
    -cdrom "$ISO_FILE" \
    -netdev user,id=n0 \
    -device virtio-net-pci,netdev=n0 \
    -drive if=none,id=wipe0,format=qcow2,file="$DEMO_DIR/wipe-target.qcow2" \
    -device virtio-blk-pci,drive=wipe0,serial=DZAPWIPE1 \
    -drive if=none,id=src0,format=qcow2,file="$DEMO_DIR/recovery-source.qcow2" \
    -device virtio-blk-pci,drive=src0,serial=DZAPSRC1 \
    -device qemu-xhci,id=usb0 \
    -drive if=none,id=dst0,format=qcow2,file="$DEMO_DIR/recovery-destination.qcow2" \
    -device usb-storage,bus=usb0.0,drive=dst0,serial=DZAPDEST1,removable=on \
    -vga none \
    -device VGA,vgamem_mb=64,xres=$DEMO_WIDTH,yres=$DEMO_HEIGHT
