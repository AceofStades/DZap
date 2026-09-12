# Live-USB build and operation

The live image turns DZap into a self-contained appliance. It boots its own kernel and userspace, avoids dependence on an installed OS, starts the privileged service automatically, and presents the dashboard locally.

## Supported image target

The current builder targets:

- x86-64 CPUs.
- Legacy BIOS through SYSLINUX.
- UEFI through systemd-boot.
- Hybrid ISO layout suitable for optical boot or raw imaging to USB.
- Arch Linux hosts with the `archiso` package installed.

Secure Boot signing is not implemented. ARM and other architectures are not current targets.

## Host requirements

On an Arch Linux build host:

```bash
sudo pacman -S --needed archiso nodejs npm qemu-desktop rustup
rustup default stable
rustup target add x86_64-unknown-linux-musl
```

The build script explicitly checks for `cargo`, `mkarchiso`, `npm`, and `rustup`, the selected ArchISO profile, and the musl target.

The smoke test additionally uses:

- `bsdtar` to read the generated ISO.
- `qemu-system-x86_64` to boot it.
- Python 3 with `pexpect` on the host.
- `jq` inside the current `releng`-based live image.

## Build command

```bash
make iso
```

This calls `scripts/build-live-iso.sh`. Useful overrides are:

| Variable | Default | Purpose |
| --- | --- | --- |
| `DZAP_ISO_BUILD_DIR` | `build/archiso` | Temporary copied profile and work tree. |
| `DZAP_ISO_OUT_DIR` | `out` | Finished image directory. |
| `ARCHISO_PROFILE_DIR` | `/usr/share/archiso/configs/releng` | Installed base profile copied for this build. |

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

`mkarchiso -v -r` then creates the hybrid image in `out/`. The output name includes the build date.

## Source-controlled overlay

| Path | Purpose |
| --- | --- |
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

1. BIOS or UEFI loads the corresponding boot entry.
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

1. Reads the generated SYSLINUX configuration to obtain the ArchISO search UUID.
2. Extracts the ISO's own kernel and initramfs to a temporary directory.
3. Creates a 256 MiB throwaway raw disk.
4. Direct-boots the kernel in QEMU with a serial console.
5. Attaches the ISO read-only as a virtio disk, mimicking live media, and attaches the scratch disk separately.
6. Logs into the serial console as root.
7. Waits for the packaged backend.
8. Confirms the backend service is active.
9. Confirms the packaged frontend returns HTML.
10. Confirms `/run/dzap` belongs to `dzap`.
11. Confirms the packaged TUI is executable, tty2 is active, and the TUI process is running.
12. Queries `/api/drives` and requires `/dev/vda`, the live image, to be both mounted and marked as the OS drive.
13. Powers off the guest.

This test exercises the packaged root filesystem and startup service. Direct kernel boot bypasses the firmware bootloader menu, so BIOS/UEFI image metadata and physical boot must also be tested.

The first complete smoke-tested image was about 1.8 GiB. QEMU software emulation took roughly two to three minutes to reach the guest checks. Those measurements are observations, not fixed limits.

## Interactive QEMU run

```bash
make run-iso
```

The launcher selects the newest image, creates a 4 GiB qcow2 disk at `build/archiso/test-disk.qcow2`, uses KVM when accessible, and opens the graphical boot with standard VGA.

Use these QEMU keys after the image reaches the dashboard:

- `Ctrl+Alt+F2`: switch to the Rust TUI.
- `Ctrl+Alt+F1`: return to the Chromium dashboard.
- `Ctrl+Alt+G`: release QEMU's keyboard and mouse grab.

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
- Secure Boot is not supported.
- The live image has not yet passed a published physical-hardware matrix.
- Frontend dependency audit findings remain to be resolved.
- The optional ONNX health model/runtime is not packaged.
- The Rust TUI is read-only; wipe authorization, progress, certificates, and evidence export remain in the graphical dashboard.
