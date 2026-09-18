# Live-USB build and operation

The live image turns DZap into a self-contained appliance. It boots its own kernel and userspace, avoids dependence on an installed OS, starts the privileged service automatically, and presents the dashboard locally.

## Supported image target

The current builder targets:

- x86-64 CPUs.
- Legacy BIOS through SYSLINUX.
- UEFI through systemd-boot.
- Hybrid ISO layout suitable for optical boot or raw imaging to USB.
- Optional owner-key Secure Boot for the UEFI systemd-boot and unified kernel image path.
- Arch Linux hosts with the `archiso` package installed.

ARM and other architectures are not current targets. The owner-key image requires certificate enrollment and is not automatically trusted by factory firmware keys.

## Host requirements

On an Arch Linux build host:

```bash
sudo pacman -S --needed archiso nodejs npm qemu-desktop rustup sbsigntools
rustup default stable
rustup target add x86_64-unknown-linux-musl
```

The build script explicitly checks for `cargo`, `mkarchiso`, `npm`, and `rustup`, the selected ArchISO profile, and the musl target.

The smoke test additionally uses:

- `bsdtar` to read the generated ISO.
- `qemu-system-x86_64` to boot it.
- Python 3 with `pexpect` on the host.
- `jq` inside the current `releng`-based live image.

The signed build additionally uses OpenSSL to create/check the owner key and `sbverify` from `sbsigntools` to verify finished EFI artifacts. `systemd-ukify` and `sbsigntools` are installed inside the temporary ArchISO build root for UKI construction and signing.

## Build command

```bash
make build-iso
```

This calls `scripts/build-live-iso.sh`. Useful overrides are:

| Variable | Default | Purpose |
| --- | --- | --- |
| `DZAP_ISO_BUILD_DIR` | `build/archiso` | Temporary copied profile and work tree. |
| `DZAP_ISO_OUT_DIR` | `out` | Finished image directory. |
| `ARCHISO_PROFILE_DIR` | `/usr/share/archiso/configs/releng` | Installed base profile copied for this build. |

## Owner-key Secure Boot

Generate the owner key once:

```bash
make secure-boot-key
```

This creates three ignored files under `build/secure-boot/`:

| File | Handling |
| --- | --- |
| `db.key` | RSA private key, mode `0600`; retain only on the controlled build host. |
| `db.pem` | PEM certificate used by the build and host verifier. |
| `db.cer` | DER certificate to enroll in the firmware Secure Boot `db`. |

The generator refuses to replace existing key material. Back up the key securely if the same trust identity must sign later images; losing it prevents updates under that identity. Never publish, commit, enroll, or copy `db.key` to the live USB.

Build and verify the signed variant:

```bash
make secure-iso
make verify-secure-iso
```

The signed ISO and a public `dzap-secure-boot.cer` enrollment companion are written to `out/secure/`; the ordinary `make build-iso` output remains in `out/`. `make secure-iso` also runs the verifier automatically and fails the build if signatures, UKI sections, certificate publication, or the signed-only loader entry are wrong. To verify an explicitly selected image whose companion certificate is in the same directory:

```bash
make verify-secure-iso ISO=/path/to/dzap.iso
```

Before booting with Secure Boot enabled, use the machine firmware's custom-key or key-management screen to enroll `out/secure/dzap-secure-boot.cer` (the same certificate as `build/secure-boot/db.cer`) in the allowed signature database (`db`). Firmware interfaces differ, and some require setup/custom mode before they accept an owner key. Preserve any vendor keys needed by the machine and follow its firmware documentation. The unmodified machine will reject DZap until this certificate is enrolled because DZap does not carry a Microsoft-trusted signature.

The signed chain covers systemd-boot and a UKI containing the kernel, initramfs, command line, and OS metadata. The external ArchISO SquashFS is not part of that signature. Treat this as an owner-enrolled Secure Boot demo rather than verified-root or measured-boot support.

## Build stages

### 1. Export the dashboard

```text
npm --prefix frontend ci
npm --prefix frontend run build
```

Next.js is configured for static export, producing `frontend/out`. The backend can serve this directory without a Node.js process in the live system.

### 2. Build the Rust binaries

```text
cargo build --locked --release \
  --features tui \
  --target x86_64-unknown-linux-musl \
  --manifest-path server/Cargo.toml
```

`--locked` requires `Cargo.lock` to agree with the manifest. The musl target produces static PIE binaries for the backend and TUI, so the live image does not need matching Rust runtime libraries for either process.

### 3. Prepare a fresh profile

The script removes the previous profile and work tree, copies the host's installed ArchISO `releng` profile, and overlays `iso/airootfs`.

It appends `iso/packages.x86_64` to the base package list and sorts unique names. DZap currently adds:

```text
chromium
curl
hdparm
mesa
nvme-cli
openbox
smartmontools
ttf-liberation
xf86-video-vesa
xorg-server
xorg-xauth
xorg-xinit
xorg-xset
```

The `releng` base contributes Linux, initramfs/ArchISO hooks, firmware, bootloaders, `util-linux`, and many general rescue packages.

### 4. Install application artifacts

The backend is installed at:

```text
/usr/local/bin/dzap-server
```

The read-only terminal client is installed at:

```text
/usr/local/bin/dzap-tui
```

The static dashboard is copied to:

```text
/opt/dzap/frontend
```

The backend service is enabled in `multi-user.target`. The profile explicitly sets mode `0755` for `dzap-server`, `dzap-tui`, and `dzap-kiosk`; relying only on source file modes caused the first QEMU-built image to fail with systemd status `203/EXEC`, which the live smoke test caught.

### 5. Customize and build

The copied profile receives:

- ISO name `dzap`.
- Label `DZAP_LIVE_<YYYYMM>`.
- Publisher `DZap Project`.
- Application `DZap Secure Wipe Live USB`.
- A BIOS entry named `DZap Secure Wipe`, selected after SYSLINUX's minimum nonzero timeout without displaying a prompt or menu.
- A UEFI entry named `DZap Secure Wipe`, selected immediately by systemd-boot without showing its menu by default.

The builder removes the copied UEFI installer, speech, and memory-test entries before installing the DZap entry. The source-controlled BIOS configuration also omits ArchISO's installer menu and maintenance entries. On UEFI systems, holding a key during firmware handoff can still request systemd-boot's menu for diagnosis, but the normal path proceeds directly into DZap.

`mkarchiso -v -r` then creates the hybrid image in `out/`. The output name includes the build date.

### 6. Sign the UEFI path when requested

For `make secure-iso`, a build-only pacman hook runs after ArchISO creates the kernel and initramfs. `systemd-ukify` combines those artifacts with the ISO search command line, signs the resulting `vmlinuz-dzap.efi`, and writes it below `/boot`. The hook signs the systemd-boot EFI executables, verifies each signature immediately, and removes the private key, helper, and hook before the root filesystem is packed. The copied UEFI loader entry points only at the signed UKI.

## Source-controlled overlay

| Path | Purpose |
| --- | --- |
| `iso/boot/syslinux/archiso_sys.cfg` | Selects the DZap BIOS entry automatically without a boot prompt. |
| `iso/boot/syslinux/archiso_sys-linux.cfg` | Defines the DZap BIOS kernel and initramfs entry. |
| `iso/boot/efiboot/loader/loader.conf` | Selects the DZap UEFI entry immediately without disabling diagnostic boot-option editing. |
| `iso/boot/efiboot/loader/entries/01-dzap.conf` | Defines the DZap UEFI kernel and initramfs entry. |
| `iso/boot/efiboot/loader/entries/01-dzap-secure.conf` | Defines the signed UKI entry used only by the Secure Boot build. |
| `iso/secure-boot/99-dzap-secure-boot.hook` | Runs UKI creation and signing during the temporary package transaction. |
| `iso/secure-boot/make-uki.sh` | Builds/verifies the UKI and signs systemd-boot, then removes staged secrets. |
| `iso/packages.x86_64` | DZap-specific runtime packages. |
| `iso/airootfs/etc/systemd/system/dzap-backend.service` | Root backend startup and restart policy. |
| `iso/airootfs/etc/systemd/system/getty@tty1.service.d/autologin.conf` | tty1 autologin as `dzap` after backend/sysusers/tmpfiles. |
| `iso/airootfs/etc/systemd/system/getty@tty2.service.d/autologin.conf` | tty2 autologin as `dzap` after the backend starts. |
| `iso/airootfs/etc/systemd/system/getty.target.wants/getty@tty2.service` | Starts the second virtual console for the TUI. |
| `iso/airootfs/etc/profile.d/dzap-kiosk.sh` | Starts X on tty1 or the Rust TUI on tty2 for `dzap`. |
| `iso/airootfs/usr/lib/sysusers.d/dzap.conf` | Creates the unprivileged kiosk account. |
| `iso/airootfs/usr/lib/tmpfiles.d/dzap.conf` | Creates the volatile kiosk home and root-owned evidence mount root. |
| `iso/airootfs/usr/local/bin/dzap-kiosk` | Starts Openbox and Chromium after backend readiness. |

The project intentionally does not vendor all ArchISO boot files. Copying the installed profile keeps the project close to ArchISO's current boot structure, though release reproducibility still requires recording or pinning that input.

## Boot sequence

1. BIOS or UEFI selects the DZap entry immediately without showing the Arch installer menu. With Secure Boot enabled, firmware first authenticates systemd-boot and systemd-boot authenticates the DZap UKI against the enrolled owner key.
2. The ArchISO initramfs locates the image and mounts it below `/run/archiso`.
3. The compressed root filesystem receives a writable temporary overlay.
4. systemd creates the `dzap` account and `/run/dzap`.
5. `dzap-backend.service` starts as root.
6. The backend loads or creates its signing key, validates jobs and certificates, then binds loopback port 8080.
7. tty1 logs in as `dzap`.
8. The profile script runs rootless Xorg on tty1.
9. Openbox starts, display power saving is disabled, and the kiosk polls `/` for up to 60 seconds.
10. Chromium opens `http://127.0.0.1:8080/` in kiosk mode.
11. tty2 logs in as `dzap` and starts the read-only Rust TUI against the same backend.

If the backend never becomes ready, the kiosk script exits with an error instead of opening a disconnected UI.

## Automated live smoke test

After building:

```bash
make smoke-iso
```

`scripts/smoke-live-iso.py` selects the newest `out/dzap-*.iso` unless given an explicit path. It:

1. Confirms the generated BIOS and UEFI configurations select DZap immediately and contain no Arch installer entry.
2. Reads the generated SYSLINUX configuration to obtain the ArchISO search UUID.
3. Extracts the ISO's own kernel and initramfs to a temporary directory.
4. Creates a 256 MiB throwaway raw disk.
5. Direct-boots the kernel in QEMU with a serial console.
6. Attaches the ISO read-only as a virtio disk, mimicking live media, and attaches the scratch disk separately.
7. Logs into the serial console as root.
8. Waits for the packaged backend.
9. Confirms the backend service is active.
10. Confirms the packaged frontend returns HTML.
11. Confirms `/run/dzap` belongs to `dzap`.
12. Confirms the packaged TUI is executable, tty2 is active, and the TUI process is running.
13. Queries `/api/drives` and requires `/dev/vda`, the live image, to be both mounted and marked as the OS drive.
14. Requires recovery assessment of that live image to be blocked before any source probe.
15. Powers off the guest.

This test exercises the packaged root filesystem and startup service. Direct kernel boot bypasses the firmware bootloader menu, so BIOS/UEFI image metadata and physical boot must also be tested.

## Automated Secure Boot artifact verification

`scripts/verify-secure-iso.py` independently extracts the EFI system partition from the completed ISO and checks:

1. Every x86-64 systemd-boot EFI executable validates against the requested certificate.
2. `vmlinuz-dzap.efi` validates against the same certificate.
3. The UKI contains `.linux`, `.initrd`, `.cmdline`, and `.osrel` sections, and its signed command line selects the ISO's actual ArchISO search UUID.
4. The enrollment certificate published beside the ISO is byte-for-byte the DER form of the host certificate.
5. The active UEFI loader entry selects the UKI and contains no `linux` or `initrd` fallback directives.
6. The packed live root contains none of the staged signing directory, build hook, or signing helper.
7. The public Secure Boot metadata inside the live root matches the signing certificate and names the enrollment companion accurately.

These checks prove artifact construction and signature consistency. The existing QEMU smoke test direct-boots the kernel, so it does not prove firmware enforcement or successful key enrollment.

The first complete smoke-tested image was about 1.8 GiB. QEMU software emulation took roughly two to three minutes to reach the guest checks. Those measurements are observations, not fixed limits.

## Interactive QEMU run

```bash
make run-iso
```

The launcher selects the newest image, creates a 4 GiB qcow2 disk at `build/archiso/test-disk.qcow2`, uses KVM when accessible, and opens the graphical boot with standard VGA.

If your host does not reserve guest tty shortcuts, `Ctrl+Alt+F2` switches to the Rust TUI and `Ctrl+Alt+F1` returns to the Chromium dashboard. Arch and other Linux hosts commonly intercept those combinations, so the reliable QEMU sequence is:

1. Press `Ctrl+Alt+2` to open QEMU's monitor console.
2. Enter `sendkey ctrl-alt-f2` for the TUI or `sendkey ctrl-alt-f1` for the kiosk.
3. Press `Ctrl+Alt+1` to return to the guest display.

Use `Ctrl+Alt+G` to release QEMU's keyboard and mouse grab.

The TUI uses arrow keys to select a device and method, `Enter` to run read-only preflight, `R` to refresh, and `Q` to quit. Quitting returns to the tty2 login, which automatically starts it again.

You may deliberately wipe the displayed QEMU scratch disk. Verify the model, size, and path in the UI first. The qcow2 file can be recreated when a clean test target is needed.

Use a custom disk or ISO with:

```bash
DZAP_VM_DISK=/tmp/my-dzap-disk.qcow2 ./scripts/run-live-iso.sh out/dzap-YYYY.MM.DD-x86_64.iso
```

## Inspecting the image

Basic bootability and checksum checks:

```bash
file out/dzap-*.iso
sha256sum out/dzap-*.iso
xorriso -indev out/dzap-*.iso \
  -report_el_torito plain \
  -report_system_area plain
```

Expected image properties include a DOS/MBR boot sector, El Torito BIOS entry, UEFI entry, and isohybrid GPT/system area.

## Writing physical media

Raw imaging destroys the chosen destination. Identify it with at least two independent facts such as model, size, and serial:

```bash
lsblk -o NAME,PATH,MODEL,SERIAL,SIZE,TRAN,MOUNTPOINTS
```

Unmount destination partitions, then write the whole ISO to the whole USB device, never to one of its partitions. A typical Linux command is:

```bash
sudo dd if=out/dzap-YYYY.MM.DD-x86_64.iso of=/dev/sdX bs=16M status=progress conv=fsync
```

Replace `/dev/sdX` only after verifying the device. A graphical imaging utility that shows model and size clearly is also suitable.

## Physical validation checklist

For each test machine, record:

- Manufacturer/model and firmware version.
- BIOS or UEFI mode and Secure Boot state.
- USB controller/port type.
- Whether the boot menu appears.
- Time until the dashboard is usable.
- Display resolution, keyboard, mouse, and scaling behavior.
- Whether the live USB is visibly marked as protected.
- All discovered internal/external drives and their identity fields.
- SATA/NVMe capability results.
- A non-destructive preflight against each target class.
- Destructive results only for explicitly disposable media.
- Exported evidence and its hashes once persistence is implemented.

At minimum, test one legacy BIOS machine, one UEFI machine, SATA through AHCI, native NVMe, a USB mass-storage bridge, and a system containing LVM or encrypted descendants.

For persistent evidence testing, attach a second USB drive containing FAT32, exFAT, or ext4. In the Certificates view, select that destination, export a verified job, record the displayed bundle path and key fingerprint, and use **Safely remove USB**. Reattach the drive after reboot and confirm that all five bundle files remain readable. Independent post-reboot signature verification remains part of the offline-verifier qualification work.

## Current image limitations

- The full `releng` package list makes the image larger and slower than necessary.
- Build inputs come from the current host Arch repositories and installed profile rather than a pinned snapshot.
- The writable overlay and kiosk home are volatile.
- Secure Boot uses an owner key that must be enrolled per machine; factory-key trust, key rotation/revocation, and signed release distribution are not implemented.
- The signed UKI does not authenticate the external ArchISO SquashFS.
- The live image has not yet passed a published physical-hardware matrix.
- Frontend dependency audit findings remain to be resolved.
- The optional ONNX health model/runtime is not packaged.
- Data recovery executes resumable ddrescue imaging, read-only TestDisk analysis, LUKS/BitLocker mapping, filesystem-aware copy, and PhotoRec carving. Recovery records and per-file hash manifests are stored on the selected destination.
- The Rust TUI is read-only; wipe authorization, progress, certificates, and evidence export remain in the graphical dashboard.
