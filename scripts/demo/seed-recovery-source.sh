#!/bin/sh
# Runs INSIDE the DZap guest (as root, e.g. on tty3) -- never on the host.
# Formats /dev/vdb, writes two known files, hashes them, then re-formats
# /dev/vdb to simulate an accidental quick-format. Also formats /dev/sda so
# it is ready to be picked as a recovery destination in the dashboard.
# Stamps /dev/vda (the wipe target) with a readable marker so the wipe demo
# has a visible before/after.
#
# Fetch and run with:
#   curl -s http://10.0.2.2:8123/seed-recovery-source.sh | sh
set -e

echo "== formatting /dev/vdb and writing demo files =="
mkfs.ext4 -qF -L SOURCE /dev/vdb
mkdir -p /mnt/dzap-demo-src
mount /dev/vdb /mnt/dzap-demo-src

cat >/mnt/dzap-demo-src/notes.txt <<'TXT'
Q3 board notes -- please do not lose this file.
TXT
base64 -d >/mnt/dzap-demo-src/photo.png <<'PNG'
iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=
PNG

echo "== original file hashes (compare against these after recovery) =="
sha256sum /mnt/dzap-demo-src/notes.txt /mnt/dzap-demo-src/photo.png | tee /root/dzap-demo-original-hashes.txt
sync
umount /mnt/dzap-demo-src

echo "== simulating an accidental quick-format of /dev/vdb =="
mkfs.ext4 -qF -L SOURCE /dev/vdb

echo "== preparing /dev/sda as the recovery destination filesystem =="
mkfs.ext4 -qF -L EVIDENCE /dev/sda

echo "== stamping /dev/vda (wipe target) with a readable marker =="
yes 'DZAP-DEMO-CONFIDENTIAL-RECORD-2026 ' | head -c 64M >/dev/vda
sync
echo "== before state (first bytes of /dev/vda) =="
od -A x -t x1z /dev/vda | head -5

echo "== done. Switch to tty1 (Ctrl+Alt+2, 'sendkey ctrl-alt-f1', Ctrl+Alt+1) for the dashboard. =="
echo "== after wiping /dev/vda in the dashboard, come back here and run: =="
echo "==   od -A x -t x1z /dev/vda | head -5      (should now be all zeros)"
echo "==   cmp /dev/vda /dev/zero                  (confirms the whole 64 MiB is zero)"
