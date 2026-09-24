# DZap project demo

A self-contained, disposable QEMU environment for demoing the wipe and
recovery flows. Everything destructive happens inside the VM, to qcow2
files under `build/demo/` (already git-ignored). Your host's real drives
are never attached to this VM, so there is nothing to accidentally wipe.

## One-time setup

```bash
make build-iso        # builds out/dzap-*.iso (skip if you already have one)
make demo-drives       # creates build/demo/*.qcow2
```

## Run it

```bash
make demo
# or: scripts/demo/run-demo.sh [path-to-iso]
```

This boots the newest `out/dzap-*.iso` with three virtual disks:

| Guest path | Role | Starting state |
| --- | --- | --- |
| `/dev/vda` | `WIPE-TARGET` | blank -- the drive you securely erase live |
| `/dev/vdb` | `RECOVERY-SOURCE` | empty until seeded -- gets "accidentally" formatted |
| `/dev/sda` | `RECOVERY-DEST` | empty until seeded -- USB-like evidence/recovery target |

## Seed both drives (do this once per take)

Switch to tty3 (`Ctrl+Alt+2` for the QEMU monitor, then
`sendkey ctrl-alt-f3`, then `Ctrl+Alt+1`), log in as `root` (no
password), and run:

```bash
curl -s http://10.0.2.2:8123/seed-recovery-source.sh | sh
```

This does three things:
- Stamps `/dev/vda` (the wipe target) with a readable repeating marker
  string, so the wipe demo has a visible before/after -- a freshly
  created virtual disk starts out all-zero already, so without this
  there is nothing to prove was erased.
- Writes `notes.txt` and `photo.png` to `/dev/vdb`, hashes them,
  quick-formats `/dev/vdb` (simulating the accidental format), and
  formats `/dev/sda` so the dashboard can select it as a recovery
  destination.

It's idempotent -- rerun it as many times as you like while rehearsing.

Then switch to tty1 (`Ctrl+Alt+2`, `sendkey ctrl-alt-f1`, `Ctrl+Alt+1`)
for the dashboard.

## Walkthrough once you're in the dashboard

**Wipe (WIPE-TARGET):**
1. Select the `WIPE-TARGET` drive (serial `DZAPWIPE1`).
2. Run preflight, confirm the identity-bound checks pass, and authorize
   `overwrite_1_pass` (the only method a virtio test disk advertises --
   real ATA/NVMe secure-erase paths need real hardware).
3. Watch the wipe run and verify.
4. Generate the signed certificate (JSON or PDF) as evidence.
5. **Prove it, outside the dashboard's own claim:** switch back to tty3
   and run:
   ```bash
   od -A x -t x1z /dev/vda | head -5      # was readable text, now all 00s
   cmp /dev/vda /dev/zero                 # confirms the whole 64 MiB is zero
   ```
   Do this on the SAME tty3 session you used to seed the drives -- don't
   try to hexdump `build/demo/wipe-target.qcow2` from the host instead;
   qcow2 is a sparse/compressed container, not raw bytes, so a host-side
   hexdump of the file won't look like a clean zero-fill even when the
   wipe worked, and QEMU holds the file locked while the VM is running
   anyway.

**Recovery (RECOVERY-SOURCE -> RECOVERY-DEST):**
1. Select the `RECOVERY-SOURCE` drive (serial `DZAPSRC1`) and run the
   read-only assessment.
2. Pick the `RECOVERY-DEST` drive (serial `DZAPDEST1`) as the
   destination and start the ddrescue image.
3. Once imaged, run PhotoRec against the image -- it carves `photo.png`
   back out by its file signature even though the filesystem metadata
   that pointed at it is long gone.
4. Open the recovered file and compare its SHA-256 against
   `/root/dzap-demo-original-hashes.txt` (printed during seeding) to
   prove it's byte-for-byte the same file.

## Redoing a take

Rerunning `curl ... | sh` on tty3 re-seeds `RECOVERY-SOURCE` from
scratch. To fully reset every demo disk (e.g. after actually wiping
`WIPE-TARGET`):

```bash
scripts/demo/prepare-demo-drives.sh --force
```

That only touches files under `build/demo/`.
