# Testing strategy

DZap uses several test layers because pure unit tests cannot prove that an Axum route works, and an HTTP test cannot prove that a booted guest actually overwrote a disk. Destructive tests are isolated to disposable files and virtual disks.

## Test layout

Rust module tests live beside implementation when they need crate-private helpers. Black-box integration tests live under `server/tests/` and use the public crate/API surface.

| Location | Tests | Main coverage |
| --- | ---: | --- |
| `server/src/core/wiper_test.rs` | 19 | Method policy, JSON compatibility, overwrite extent/patterns, cancel/pause controls, reservations, child output draining, ATA cleanup, progress shape. |
| `server/src/core/certificate_test.rs` | 8 | Complete certificate hashing, signature verification, job binding, JSON/PDF, store idempotence, tamper rejection, key permissions. |
| `server/src/core/predict_test.rs` | 6 | SATA/NVMe SMART parsing, health status, feature tensor mapping, probability threshold. |
| `server/src/core/drives_test.rs` | 5 | Drive mapping/classification, recursive topology, Android parsing, malformed discovery, ATA frozen parsing. |
| `server/src/core/evidence_export_test.rs` | 4 | Removable destination filtering, bundle write/readback, idempotence, target exclusion, and tamper rejection. |
| `server/src/api_test.rs` | 3 | Certificate handler uses verified server-owned jobs and rejects invalid states. |
| `server/src/realtime_test.rs` | 2 | Hub broadcast behavior. |
| `server/tests/api.rs` | 18 | Real HTTP/WS server, routes, invalid requests, origin rules, static frontend serving, job, certificate, and export behavior. |
| `server/tests/preflight.rs` | 10 | Mounted/system/dependency/identity decisions, method boundaries, HPA and DCO parsing. |
| `server/tests/nvme.rs` | 5 | Controller paths, capability parsing, command flags, sanitize-log success/failure. |
| `server/tests/jobs.rs` | 5 | Job lifecycle, persistence, restart failure, tamper detection, certificate/job startup matching. |
| `server/tests/verification.rs` | 4 | Full readback success, mismatch, size mismatch, method policy. |
| `server/tests/ata.rs` | 3 | Security capability parser and normal/enhanced command arguments. |
| `server/tests/drives.rs` | 3 | Public drive shapes, nested topology, ArchISO boot-media protection. |

Current total: **95 Rust tests**.

## Safety rule for automated tests

No ordinary Rust test points at a real block device.

- Overwrite tests use uniquely named regular files under the temporary directory.
- API tests submit nonexistent device paths when exercising destructive rejection.
- Discovery parsers consume captured/synthetic `lsblk`, `hdparm`, SMART, or NVMe output.
- Firmware command tests validate generated arguments and parsed status rather than sending commands.

The one test that really wipes a block device runs inside QEMU and targets a disposable qcow2 disk.

## Rust unit and integration suite

Run:

```bash
cd server
cargo test
```

This validates both unit and integration tests. The integration suite starts the real Axum router on ephemeral loopback ports.

Strict linting:

```bash
cargo clippy --all-targets -- -D warnings
```

Formatting:

```bash
cargo fmt --all -- --check
```

The release/live build also uses `cargo build --locked` so dependency-lock drift becomes a build failure.

## Frontend checks

```bash
cd frontend
npm ci
npm run build
npx tsc --noEmit
```

The build must produce a static `out/index.html` and `_next` assets that the Rust fallback service can serve. The Rust API integration suite separately proves index and nested static asset delivery from a temporary frontend directory.

The current Next configuration skips type and lint validation during `next build`, which is why `npx tsc --noEmit` is run separately. Removing those skips is planned cleanup.

## Destructive QEMU end-to-end test

Run from the repository root:

```bash
./server/scripts/e2e-qemu.sh
```

The harness:

1. Builds the musl backend.
2. Downloads/caches an Alpine virtual ISO under `/tmp/dzap-e2e`.
3. Creates a new 64 MiB scratch qcow2 disk.
4. Boots Alpine with a serial console.
5. Transfers the backend and guest test script through QEMU user networking.
6. Starts the backend as guest root.
7. Confirms `/api/drives` and method discovery.
8. Fills `/dev/vda` with random bytes.
9. Runs read-only preflight and extracts its identity.
10. Starts `overwrite_1_pass` with that approved identity.
11. Reads the full virtual disk back against 64 MiB of zeroes.
12. Waits for a `verified` server job with full-readback evidence.
13. Generates and validates JSON certificate fields.
14. Generates a PDF and checks its `%PDF-1.4` header.

The host disk is never passed through to the VM. The only destructive path is `/dev/vda` inside the guest, backed by `/tmp/dzap-e2e/scratch.qcow2`.

This test proves that the actual Rust overwrite, API orchestration, verification, evidence, and certificate pipeline work together on a Linux block device.

## Live-image smoke test

Build and boot-test the product image:

```bash
make iso
make smoke-iso
```

The smoke test uses the generated ISO's kernel, initramfs, compressed root filesystem, service unit, installed backend, and static frontend. It confirms the live image is protected as the OS drive when attached as a disk.

It creates only a temporary 256 MiB raw scratch disk and does not invoke a wipe. Its focus is product boot and safety initialization.

The direct-kernel smoke boot does not exercise the BIOS SYSLINUX or UEFI systemd-boot menu. Image metadata can be inspected with `xorriso`, while firmware boot still requires interactive QEMU or physical-machine testing.

## Shell and packaging checks

```bash
make check-live
git diff --check
```

`check-live` syntax-checks the builder, runner, kiosk shell files, and compiles the Python smoke harness. `git diff --check` catches whitespace errors in tracked changes.

## Recommended local gate

For ordinary backend/frontend changes:

```bash
cd server
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
cd ../frontend
npm run build
npx tsc --noEmit
cd ..
make check-live
```

Run the destructive QEMU test when changing preflight, wipe execution, verification, job evidence, or certificate generation.

Run `make iso && make smoke-iso` when changing dependencies, the static-serving boundary, ArchISO files, service startup, kiosk startup, or boot-media detection.

## What is proven today

- Parser and state-machine behavior across normal and malformed inputs.
- Method exposure is scoped by device class and advertised firmware capability.
- System, mounted, dependency-backed, changed-identity, HPA, and DCO cases block.
- Overwrites cover exact extents, flush writes, support controls, and finish with expected patterns.
- Firmware commands are constructed correctly and status output is strictly parsed.
- Verification evidence must match method policy and approved identity.
- Evidence persistence rejects tampering and interrupted state is failed on restart.
- Certificates are signed, job-bound, idempotent, and available as JSON/PDF.
- Evidence bundles are written atomically, read back, and rejected after tampering.
- HTTP, WebSocket, origin policy, and static serving work through a real listener.
- A real guest block device can pass the complete overwrite-to-certificate path.
- The packaged live root boots, starts DZap, serves the UI, and protects its own media.

## What is not proven yet

- Actual ATA Security Erase behavior on qualified SATA hardware.
- Actual enhanced erase on drives that advertise it.
- Actual NVMe crypto, block, and overwrite sanitize across controller vendors.
- Behavior through USB-to-SATA/NVMe bridges.
- BIOS and UEFI boot across a documented hardware matrix.
- Power-loss behavior during each sanitization method.
- Export, safe removal, and reboot retention on physical FAT32, exFAT, and ext4 USB media.
- Secure Boot.
- Recovery behavior during physical long-running wipes, browser restarts, and backend restarts.

These are release risks and belong in the roadmap rather than being implied by a green unit suite.
