# Implementation history

This history starts at the Rust migration because that is the foundation of the current live-USB product. Older commits contain the original Go/Electron experiments and UI iterations; they are useful archaeology but no longer describe the intended deployment model.

## Rust takeover

### `8e7930c` — The great rust takeover

The project direction moved from the earlier backend implementation toward Rust.

### `208d9df` — Track Cargo.lock

The Rust dependency resolution became part of source control. Keeping `Cargo.lock` is important for an application and for a repeatable live-image build.

### `b78d7d1` — Port backend from Go to Rust

The core service was ported to an Axum/Tokio Rust backend. The port established the basic HTTP routes, device discovery, wipe execution, health inspection, progress broadcast, and certificate generation used by later hardening work.

## Safety and regression baseline

### `4151582` — `fix(core): enforce wipe safety boundaries`

The first Rust hardening pass established strict method/device boundaries and removed unsafe assumptions inherited from the earlier implementation. The backend began treating unsupported operations as errors instead of attempting a best-effort destructive action.

### `fb08d20` — `test(server): cover API and WebSocket flows`

Integration tests were added under `server/tests/` to drive the actual Axum router through HTTP and WebSocket connections. These tests cover request parsing, route behavior, response shapes, origin handling, and failure cases without touching real disks.

### `e5d7b79` — `test(e2e): add isolated virtual-disk wipe`

A destructive end-to-end test was added. It boots an Alpine VM, downloads the statically linked Rust backend into the guest, fills a throwaway QEMU disk with random data, wipes it through the API, reads the whole disk back, and checks both JSON and PDF certificates. The destructive target exists only as a temporary qcow2 image.

## Two-phase wipe authorization

### `4c76158` — `feat(safety): require wipe preflight approval`

The wipe flow changed from a single destructive request to a two-stage handshake:

1. The UI requests a read-only preflight.
2. The backend returns the detected device identity and individual safety checks.
3. The operator confirms the selected device.
4. The UI sends the approved identity with the wipe request.
5. The backend detects the device again and requires an exact identity match.

This protects against a device path such as `/dev/sdb` being reused for different hardware between inspection and execution.

### `30116ca` — `fix(safety): block unsafe storage topology`

Preflight began rejecting:

- Mounted devices or mounted descendants.
- The running operating-system drive.
- Drives backing active RAID, LVM, encrypted, or device-mapper descendants.
- ATA devices with unverifiable or active Host Protected Areas (HPA).
- ATA devices with unverifiable or active Device Configuration Overlays (DCO).
- Frozen SATA SSDs when the selected operation requires ATA Security Erase.

The device identity was expanded to include model, serial, WWN, byte size, transport, and Linux major/minor number.

## Server-owned evidence

### `e43e056` — `feat(evidence): bind certificates to wipe jobs`

The backend became the authority for wipe results. It creates a random job identifier, stores immutable identity and method data, records state transitions, and uses those records as the sole certificate input.

Evidence events are linked through SHA-256 hashes. Persistent stores use temporary-file writes, `fsync`, atomic rename, directory `fsync`, and restrictive Unix permissions. On startup, malformed or tampered records stop the backend instead of being silently ignored.

### `cfb87e8` — `feat(frontend): use server-owned wipe records`

The dashboard began loading jobs and certificates from the backend rather than constructing authoritative completion data in the browser. Progress views now use server job IDs, and certificate requests contain only a job ID.

## Verification before certification

### `814de68` — `feat(verification): require verified wipe evidence`

A successful sanitization command stopped being sufficient for certification. Jobs now transition through `running`, `verifying`, and `verified`, or end at `failed`.

Host overwrites receive full pattern readback. ATA and NVMe firmware operations receive firmware-status validation plus deterministic beginning/middle/end samples. Device identity is checked again before verification.

### `ee7fe9a` — `feat(frontend): show wipe verification evidence`

The dashboard began displaying the verification strategy, bytes checked, readback SHA-256, firmware-status SHA-256 where present, identity-revalidation result, and final evidence hash.

## Firmware erase hardening

### `e8dea4c` — `feat(nvme): add capability-aware sanitize`

NVMe handling began reading the controller `sanicap` field and exposing only advertised sanitize actions:

- Cryptographic erase.
- Block erase.
- Overwrite sanitize.

Commands run against the derived controller path with `--wait`. When the controller advertises purge reporting, DZap requests it and verifies the sanitize log's completion, action, progress, and purge bits.

### `64f8b72` — `fix(ata): harden secure erase execution`

ATA Security Erase was split into normal and enhanced modes based on advertised capability. DZap sets a temporary user password, issues the selected erase, and attempts password disable as an independent recovery phase if setup or erase fails. Abort state cannot suppress that cleanup attempt.

### `9eccfae` — `fix(safety): protect local wipe operations`

The destructive boundary was tightened further:

- Only one wipe/verification reservation may own a device path at a time.
- Child-process stdout and stderr are drained concurrently with bounded capture, avoiding pipe deadlocks and unbounded diagnostic memory.
- CORS and WebSocket origins are restricted to the local dashboard.
- Unmount requests re-read block topology and refuse to unmount system or live media.

## Bootable live USB

### `b564e90` — `feat(live): add bootable USB image`

The application became a bootable appliance:

- The Rust backend serves the static Next.js export and API from the same loopback server.
- An ArchISO overlay defines the root backend service, `dzap` kiosk user, tty1 autologin, Xorg/Openbox session, and Chromium kiosk startup.
- The builder produces a statically linked musl backend and a hybrid BIOS/UEFI ISO.
- The boot-media detector treats `/run/archiso` and its descendants as protected system mounts.
- A QEMU smoke test boots the generated live filesystem, checks the backend and frontend, verifies kiosk-user ownership, and proves that the attached live image is identified as mounted OS media.

The first verified image was approximately 1.8 GiB because it inherited the complete ArchISO `releng` rescue package set. Reducing that package set is planned work rather than part of this milestone.

## Image-first data recovery

Recovery progressed from read-only assessment and destination planning into a complete image-first workflow. The backend now creates sparse, resumable `ddrescue` images and map files on a separately identified removable destination. Recovery jobs persist their source and destination identities, artifact paths, map progress, tool diagnostics, extraction results, and hash-chained events both internally and beside the image.

Completed images can be inspected through temporary read-only loop devices. TestDisk produces a retained, hashed analysis log. Recognized filesystems can be mounted with read-only safety options and copied without following symbolic links; PhotoRec is available as an explicit carving fallback. LUKS and BitLocker mappings are opened read-only through `cryptsetup`, with secrets supplied on stdin and removed from memory after the mapping attempt. Every extraction uses a unique output directory and a SHA-256 manifest of recovered regular files.

The dashboard gained a Recovery view for persistent progress, pause/resume/cancel controls, volume selection, TestDisk analysis, encrypted-volume secrets, and extraction choices. The live image packages `ddrescue`, TestDisk/PhotoRec, and `cryptsetup`. QEMU now tests healthy ext4 recovery, PhotoRec, LUKS recovery, full-destination blocking, disconnected-source blocking, blank-media assessment, and the existing verified wipe flow on disposable virtual disks.

## Owner-key Secure Boot

### `0399fc6` — `feat(live): add owner-key Secure Boot`

The live-image builder gained an explicit signed variant while preserving the ordinary hybrid image:

- `make secure-boot-key` creates a private RSA owner key plus PEM and DER certificates under the ignored build directory and refuses to overwrite them.
- `make secure-iso` validates key permissions and key/certificate agreement, then stages them only inside the temporary ArchISO profile.
- A package hook uses `systemd-ukify` to combine and sign the kernel, initramfs, command line, and OS metadata as one DZap UKI.
- The same key signs the x86-64 systemd-boot EFI executables. The secure UEFI loader entry references only the signed UKI.
- The build hook verifies its outputs, deletes the staged private material, and leaves public fingerprint metadata in the image; the builder publishes the enrollment certificate beside the ISO.
- The post-build verifier extracts the actual EFI system partition and independently checks every signature, required UKI section, published certificate, and absence of an unsigned loader fallback.

This is an owner-enrollment model: firmware does not trust the image until the DZap certificate is added to its `db`. The bootloader and UKI are authenticated, while the external ArchISO SquashFS remains outside the signature. Physical firmware qualification and authenticated-root design remain hardening tasks rather than claims of this milestone.

## Architecture diagrams

The current release documentation includes standalone Archify diagrams for system architecture, the wipe workflow, and the recovery workflow. Their editable JSON sources live beside the HTML artifacts under `docs/diagrams/`. Each artifact supports light and dark themes, multiple views, and direct image/vector export for team review.

## Current direction

All deployment work serves the live-USB appliance. The obsolete Electron sources and root Node packaging manifests were removed after the Rust and live-image paths became authoritative. Older commits retain the native desktop experiments for archaeology. The demo now includes recovery, owner-key Secure Boot artifacts, and shareable workflow documentation. The remaining release work is physical hardware qualification, image reduction/reproducibility, full root integrity, and key/evidence trust operations.
