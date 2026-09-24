# DZap demo video script (~3 minutes)

Target runtime: 2:45-3:15. Times below are guidelines, not hard cues --
pace to what's actually on screen. Everything happens inside one QEMU
window (`make demo`); nothing here touches your host's real drives, and
you don't need to say that more than once. Order: wipe first, recovery
second.

## Before you hit record

1. `make build-iso` (skip if `out/dzap-*.iso` is already current).
2. `make demo-drives` -- creates the three throwaway virtual disks.
3. `make demo` -- boot once, do a **dry run** of the whole script below
   so you know the exact click paths and timings on your machine, then
   re-seed with `scripts/demo/prepare-demo-drives.sh --force` before
   the real take.
4. Have `build/demo/` cleaned to a fresh state right before recording
   (see "Redoing a take" in `scripts/demo/README.md`).
5. Start your screen recorder on the QEMU window only (or full screen --
   your call), then `make demo`.

## 0:00 - 0:20 -- Cold open

**Say:** "This is DZap -- a bootable Linux USB that securely erases
drives and recovers data off them. I'm running the actual live image
inside a QEMU virtual machine with three throwaway virtual disks, so
nothing I do in the next three minutes touches my real laptop."

**Show:** The QEMU window booting, DZap's kiosk dashboard coming up on
tty1.

## 0:20 - 0:40 -- The setup

**Say:** "Before we started, I stamped one virtual drive with some
placeholder confidential data, and dropped a note and a photo onto a
second drive, then quick-formatted that one -- the way you would if you
accidentally reformatted a real disk. First we'll securely wipe the
first drive, then recover the files off the second."

**Show:** Quick cut to tty3 output from `seed-recovery-source.sh`
(recorded ahead of time, or sped up 4x) showing the marker text being
written, the two files being written and hashed, and the "accidental"
reformat happening. ~10-15s of screen time is enough -- viewers just
need to see it happened.

## 0:40 - 1:40 -- Secure wipe walkthrough

**Say:** "Let's start with erasing a drive you're done with."

**Show:** In the dashboard, select `WIPE-TARGET` (serial `DZAPWIPE1`),
open wipe methods.

**Say:** "DZap runs an identity-bound preflight first -- it checks the
drive isn't mounted, isn't the system disk, isn't backing an active
LVM or LUKS volume, and it re-confirms the exact device identity right
before it authorizes anything destructive."

**Show:** Run preflight, let the checks resolve green, authorize the
`overwrite_1_pass` method (the method a virtual disk actually
advertises -- call out in narration that real ATA Secure Erase / NVMe
Sanitize paths need real hardware and are the same code path, just
gated on firmware capability).

**Say:** "Once it's authorized, it overwrites the entire device and
then reads every byte back to verify the pattern -- that's not just
'trust me,' the tool actually checks."

**Show:** Wipe progress to completion, verification result.

**Say:** "And it signs a certificate of erasure -- job ID, method,
verification strategy, hash-chained evidence -- exportable as JSON or
PDF."

**Show:** Generate/open the certificate (PDF view looks best on
camera).

**Say (independent proof):** "Let's not just take the dashboard's word
for it -- here's the same drive from a plain root shell, byte for
byte."

**Show:** Cut to tty3. Run:
```bash
od -A x -t x1z /dev/vda | head -5
```
This was readable placeholder text before the wipe; show it's now all
`00`s. Optionally follow with `cmp /dev/vda /dev/zero` to show the
*entire* 64 MiB checks out as zero, not just the first few lines.

## 1:40 - 2:55 -- Recovery walkthrough

**Say (selecting the source):** "Now the recovery side. Here's the
drive that got accidentally formatted. DZap assesses it read-only
first -- it never touches the source until you explicitly choose a
destination and start imaging."

**Show:** Select `RECOVERY-SOURCE` (serial `DZAPSRC1`), run the
assessment, point out the read-only / content-state result on screen.

**Say (destination + imaging):** "I'll point it at our third virtual
drive as the recovery destination -- DZap only accepts a *different*
physical drive, so you can't accidentally image a disk onto itself."

**Show:** Pick `RECOVERY-DEST` (serial `DZAPDEST1`), start the
ddrescue image job, let the progress bar run to completion (it's a
small virtual disk, this takes a few seconds).

**Say (carving):** "The filesystem metadata is gone after that format,
so a plain file copy won't find anything. This is where PhotoRec comes
in -- it scans the raw image for file signatures instead of trusting
the filesystem."

**Show:** Run PhotoRec against the completed image, show the recovered
file list appear (photo.png, ideally notes.txt too).

**Say (proof):** "And here's the proof -- the SHA-256 of the recovered
photo matches the hash we recorded before the format. Byte for byte,
nothing changed."

**Show:** Open/compare the recovered file's hash against
`/root/dzap-demo-original-hashes.txt` (split screen or quick cut to a
terminal showing both hashes side by side).

## 2:55 - 3:15 -- Close

**Say:** "Prove what you erased, recover what you can -- all from one
bootable image, and in this demo, entirely inside a disposable VM.
That's DZap."

**Show:** Dashboard home / drive list one more time, then fade out or
cut.

## Notes for editing

- If the ddrescue/wipe progress bars take longer on your machine than
  they did in rehearsal, speed up that section 2-4x in post rather than
  waiting on camera -- the interesting part is the before/after, not
  the wait.
- Keep the tty-switching (`Ctrl+Alt+2` -> `sendkey` -> `Ctrl+Alt+1`)
  out of the final cut if it's fiddly; jump-cut past it.
- The three strongest "proof" beats, don't rush any of them: the
  certificate PDF, the tty3 hexdump before/after the wipe, and the
  recovered-file hash comparison.
