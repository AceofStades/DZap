# Bootable USB roadmap

This roadmap covers the remaining work for DZap as an **x86-64 bootable USB appliance**. It deliberately excludes native Windows, macOS, Electron, and installed-Linux application packaging. Priorities are based on whether a gap can lose evidence, permit an unsafe operation, or prevent the live image from working on real hardware.

The current implementation already discovers disks, blocks known-dangerous targets, performs capability-aware wipe operations, verifies the result, records a hash-chained job history, and generates signed certificates. It also builds and boots as an ArchISO image. The work below turns those pieces into a release that can be trusted outside the development environment.

## Priority definitions

| Priority | Meaning |
|---|---|
| **P0** | Required before treating the image as a usable erasure appliance. |
| **P1** | Required for a dependable public release, but does not invalidate the core safety boundary by itself. |
| **P2** | Hardening and polish after the first dependable release. |

## P0: Persistent evidence export

### Why this is first

The live image currently stores the signing key, wipe jobs, and certificates below `/root/.config/DZap`. ArchISO gives that path a writable overlay, but the overlay is normally volatile. Records survive a backend restart during one boot and disappear when the machine reboots unless the operator exports them.

That means a verified wipe can succeed while its only durable evidence is lost. Evidence persistence is therefore the highest-priority product gap.

### Required behavior

1. Provide an explicit **Export evidence** operation after verification and from the certificate history view.
2. Discover removable export destinations separately from wipe candidates.
3. Require the operator to select the destination by stable identity and current mount point.
4. Never automatically choose or mount a destination that is reserved for wiping, is the running live medium, or backs a protected mount.
5. Export one self-contained bundle containing:
   - the canonical job record;
   - the certificate JSON;
   - the certificate PDF;
   - the certificate QR payload or a reproducible representation of it;
   - the public verification key;
   - a manifest with SHA-256 hashes for every file;
   - the application version and evidence format version.
6. Write the bundle atomically where the destination filesystem permits it: create a temporary path, flush each file, flush the directory, rename into place, then flush the parent directory.
7. Report partial writes as failures and leave a clearly marked temporary bundle that can be retried or removed.
8. Read the completed bundle back and validate all manifest hashes before telling the operator that export succeeded.
9. Make repeated exports of the same job idempotent or assign deterministic, collision-safe names.

### Signing-key decision

The current image creates or reuses an RSA private key in volatile state. Before release, choose and document one trust model:

| Model | Benefit | Cost |
|---|---|---|
| Per-boot key | Simple and limits key lifetime. | Certificates from different boots have unrelated identities; the public key must always travel with each export. |
| Persistent appliance key | Gives one appliance a stable identity. | Requires a writable encrypted or physically controlled persistence volume and a recovery/rotation process. |
| Operator-provided key | Lets an organisation own its trust root. | Needs secure import, validation, permission handling, and a way to avoid exposing the private key in the UI or logs. |

The format should include a key fingerprint regardless of the model. The private key must never be included in an evidence bundle.

### Acceptance criteria

- An exported bundle remains verifiable on another machine after the live system has shut down.
- Modifying a job event, certificate field, PDF, or manifest entry causes offline verification to fail.
- Removing the export drive mid-write produces an error and never reports success.
- A drive selected or reserved as a wipe target cannot simultaneously become an export destination.
- The live medium and every protected storage ancestor or descendant remain ineligible.
- Integration tests cover FAT32 and at least one Linux filesystem because removable media commonly uses both.

## P0: Real-hardware qualification

QEMU proves that the ISO boots and that DZap can overwrite a disposable virtual disk. It cannot establish firmware command behavior, storage-controller quirks, USB bridge support, or the breadth of machines that can boot the image.

### Minimum qualification matrix

| Area | Minimum coverage | Evidence to retain |
|---|---|---|
| Firmware boot | One legacy BIOS machine and two UEFI implementations | Boot log, model/firmware version, screenshot or console capture, pass/fail notes. |
| SATA HDD | Direct AHCI connection, one-pass and three-pass overwrite | Preflight report, wipe job, full verification record, post-wipe SMART data. |
| SATA SSD | Security erase; enhanced erase where advertised | `hdparm -I` capability snapshot, command result, verification evidence. |
| NVMe SSD | Every sanitize action advertised by the test controller; format fallback | `nvme id-ctrl`, sanitize log, job and verification records. |
| USB storage | Flash drive plus a SATA-to-USB bridge | Enumeration, supported methods, write/readback verification. |
| Complex topology | LVM, device mapper encryption, software RAID, mounted partitions, and swap | Proof that preflight blocks the parent and related devices. |
| Failure handling | Unplugged USB target, command failure, reboot during a job | Recovered job status and absence of a false certificate. |
| Display/input | Common Intel/AMD graphics, at least one HiDPI display, keyboard-only flow | Kiosk startup and usability notes. |

Use sacrificial media. Record exact drive model, firmware, transport, controller, kernel version, and DZap image digest for every result. A passing test for one SSD model must not be generalized to every device implementing the same nominal command.

### Release blockers found during qualification

Treat any of the following as P0:

- the live medium can be selected or wiped;
- the operator can bypass a failed mandatory preflight check;
- a device identity change is not detected before destructive work or verification;
- a failed or interrupted operation produces a verified job or certificate;
- firmware reports completion but DZap cannot retain the raw status needed to justify the result;
- the UI reports success before evidence has been durably exported.

## P1: Smaller and reproducible live image

The build currently starts from ArchISO's full `releng` profile and adds DZap. This is convenient but produces an image around 1.8 GiB and inherits packages that the appliance does not use.

### Planned work

- Replace the copied package set with an audited minimal list for boot, networking needed locally, storage tooling, Xorg/Openbox/Chromium, fonts, and DZap.
- Remove interactive rescue tools only after confirming they are not needed for driver coverage or field diagnostics.
- Pin or snapshot package sources so the same source revision cannot silently produce materially different images on different days.
- Embed the Git revision, build timestamp policy, package manifest, Rust lockfile digest, and frontend lockfile digest in the image.
- Generate and publish SHA-256 hashes for release images.
- Add a clean-environment build in CI and compare package manifests between builds.

Bit-for-bit reproducibility may require normalizing filesystem timestamps, ISO metadata, compression options, and package snapshot dates. Until that is achieved, call builds traceable rather than reproducible.

## P1: Frontend reliability and honest product text

The dashboard works for the tested flow, but several pieces still reflect earlier product assumptions or development shortcuts.

### Planned work

- Read `tab` and `jobId` from the URL when the dashboard starts so a newly created wipe opens the intended progress view.
- Reconnect the WebSocket with bounded backoff and reload the authoritative job record after reconnecting.
- Derive the WebSocket URL from the current page origin instead of assuming `ws://localhost:8080/ws`.
- Make pause and abort controls describe the actual method semantics. Pause applies to host overwrite chunks; firmware commands may not be pausable after submission.
- Remove stale claims such as DoD/Gutmann branding unless a named method is implemented and verified exactly as claimed.
- Replace leftover product names and desktop-application metadata with the DZap live-appliance identity.
- Re-enable TypeScript and lint failures in the production build, then fix every resulting error instead of suppressing the gate.
- Audit and update frontend dependencies until known production-impacting advisories are resolved or documented with a bounded exception.

The backend remains the authority for device identity, supported methods, preflight approval, job state, and certificate contents. UI fixes must preserve that ownership.

## P1: Recovery and operator-visible state

Job persistence already validates records at startup and converts interrupted nonterminal jobs to failures. The live workflow still needs clearer recovery behavior.

### Planned work

- Show recovered failed jobs with the exact interruption reason after a backend restart.
- Let the UI resume observing a running or verifying job after a page reload.
- Display the evidence-chain validation result and certificate/export availability in job history.
- Expose enough firmware status to distinguish command rejection, device-reported failure, timeout, and an operator abort request.
- Add an explicit retry path that creates a new job and never mutates the evidence of the failed attempt.
- Bound history growth and define archival behavior once durable export exists.

## P1: Health model packaging decision

SMART collection is useful independently of the optional ONNX prediction model. The current backend looks for a relative model path, while the live-image build does not intentionally package the model and runtime as a supported feature.

Choose one release behavior:

1. Package a versioned model and runtime, verify their digests at startup, document the input features and output limits, and test inference inside the ISO; or
2. Remove prediction from the release path and present only measured SMART data until a validated model is ready.

Silent fallback should become an explicit capability state so the dashboard does not imply that a prediction ran when it did not.

## P2: Release hardening

- Sign release checksums and document how operators verify them before writing the USB.
- Evaluate Secure Boot support and define who owns the signing keys and revocation process.
- Add an offline evidence verifier that accepts an exported bundle and produces a clear valid/invalid report without trusting DZap's backend.
- Add accessibility checks for keyboard navigation, focus visibility, contrast, progress announcements, and destructive-action confirmation.
- Improve display fallback behavior for unsupported graphics hardware and provide a readable console error when kiosk startup fails.
- Document release versioning, evidence-format compatibility, key rotation, vulnerability response, and supported hardware policy.
- Add localization only after safety-critical wording has stable meanings and translations can be reviewed by storage-domain experts.

## Explicitly out of scope

The following work is not part of the current product plan:

- Electron packaging;
- native Windows or macOS applications;
- an installed Linux desktop package;
- wiping the computer that is actively running DZap from its installed operating system;
- cloud accounts or mandatory network services;
- Android sanitization without a device-specific, verifiable erasure design.

Old source code or metadata related to those directions can be removed once it is confirmed that the live-image build and documentation no longer reference it.

## Release definition of done

A first dependable bootable-USB release is ready when all of the following are true:

- the ISO builds from a clean documented environment and its inputs are traceable;
- BIOS and UEFI boot paths pass the hardware qualification matrix;
- supported SATA, NVMe, HDD, and USB methods pass on declared test hardware;
- protected mounts, the live medium, active topology, identity changes, frozen ATA devices, hidden capacity, and unsupported firmware capabilities fail closed;
- every successful job has method-appropriate verification and a valid evidence chain;
- verified evidence can be atomically exported and independently checked after reboot;
- interrupted and failed jobs cannot produce certificates;
- the dashboard reconnects to authoritative job state and describes pause/abort limits accurately;
- production frontend checks pass and release-impacting dependency advisories are resolved or explicitly bounded;
- the release image, checksum, package manifest, source revision, and operator guide are published together.

Work should stay in this order: preserve evidence first, qualify destructive behavior on real hardware second, then improve image production and operator experience. Any newly discovered safety or false-success issue moves ahead of the existing P1 and P2 items.
