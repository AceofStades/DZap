#!/usr/bin/env python3
"""Boot the DZap live filesystem in QEMU and verify its critical services."""

from __future__ import annotations

import argparse
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

import pexpect


REPO_ROOT = Path(__file__).resolve().parent.parent
KERNEL_PATH = "arch/boot/x86_64/vmlinuz-linux"
INITRAMFS_PATH = "arch/boot/x86_64/initramfs-linux.img"
SYSLINUX_CONFIG = "boot/syslinux/archiso_sys-linux.cfg"
SYSLINUX_MENU = "boot/syslinux/archiso_sys.cfg"
UEFI_LOADER = "loader/loader.conf"
UEFI_ENTRY = "loader/entries/01-dzap.conf"


def latest_iso() -> Path | None:
    images = sorted((REPO_ROOT / "out").glob("dzap-*.iso"))
    return images[-1] if images else None


def require_command(command: str) -> None:
    if shutil.which(command) is None:
        raise RuntimeError(f"required command is missing: {command}")


def read_iso_file(image: Path, member: str) -> bytes:
    return subprocess.check_output(["bsdtar", "-xOf", image, member])


def extract_iso_file(image: Path, member: str, destination: Path) -> None:
    destination.write_bytes(read_iso_file(image, member))


def boot_and_check(image: Path, timeout: int) -> None:
    for command in ("bsdtar", "qemu-system-x86_64"):
        require_command(command)

    syslinux = read_iso_file(image, SYSLINUX_CONFIG).decode()
    syslinux_menu = read_iso_file(image, SYSLINUX_MENU).decode()
    uefi_loader = read_iso_file(image, UEFI_LOADER).decode()
    uefi_entry = read_iso_file(image, UEFI_ENTRY).decode()
    boot_configuration = "\n".join(
        (syslinux, syslinux_menu, uefi_loader, uefi_entry)
    )
    if "Arch Linux install medium" in boot_configuration:
        raise RuntimeError("generated image still contains the Arch installer boot entry")
    if "LABEL dzap" not in syslinux or "DEFAULT dzap" not in syslinux_menu:
        raise RuntimeError("generated BIOS configuration does not default to DZap")
    if "TIMEOUT 1" not in syslinux_menu or "PROMPT 0" not in syslinux_menu:
        raise RuntimeError("generated BIOS configuration is not set to boot immediately")
    if "default 01-dzap.conf" not in uefi_loader or "timeout 0" not in uefi_loader:
        raise RuntimeError("generated UEFI configuration is not set to boot DZap immediately")
    if "title    DZap Secure Wipe" not in uefi_entry:
        raise RuntimeError("generated UEFI entry is not branded for DZap")

    uuid_match = re.search(r"archisosearchuuid=([^\s]+)", syslinux)
    if uuid_match is None:
        raise RuntimeError("could not find the ArchISO search UUID")

    with tempfile.TemporaryDirectory(prefix="dzap-live-smoke-") as temp:
        scratch_dir = Path(temp)
        kernel = scratch_dir / "vmlinuz-linux"
        initramfs = scratch_dir / "initramfs-linux.img"
        test_disk = scratch_dir / "test-disk.raw"
        boot_log = scratch_dir / "boot.log"
        extract_iso_file(image, KERNEL_PATH, kernel)
        extract_iso_file(image, INITRAMFS_PATH, initramfs)
        with test_disk.open("wb") as disk:
            disk.truncate(256 * 1024 * 1024)

        kernel_args = (
            f"archisobasedir=arch archisosearchuuid={uuid_match.group(1)} "
            "console=ttyS0,115200n8 systemd.show_status=1"
        )
        qemu_args = [
            "-m",
            "3072",
            "-smp",
            "2",
            "-kernel",
            str(kernel),
            "-initrd",
            str(initramfs),
            "-append",
            kernel_args,
            "-drive",
            f"if=virtio,format=raw,readonly=on,file={image}",
            "-drive",
            f"if=virtio,format=raw,file={test_disk}",
            "-vga",
            "std",
            "-display",
            "none",
            "-serial",
            "stdio",
            "-monitor",
            "none",
            "-no-reboot",
        ]
        if Path("/dev/kvm").exists():
            qemu_args.insert(0, "-enable-kvm")

        with boot_log.open("w", encoding="utf-8") as log:
            guest = pexpect.spawn(
                "qemu-system-x86_64",
                qemu_args,
                encoding="utf-8",
                codec_errors="replace",
                timeout=timeout,
            )
            guest.logfile_read = log
            try:
                guest.expect("login:")
                guest.sendline("root")
                guest.expect(r"[#] ")
                check = (
                    "ok=OK; failed=FAILED; "
                    "for i in $(seq 1 60); do "
                    "curl -fsS http://127.0.0.1:8080/ >/tmp/dzap-index 2>/dev/null && break; "
                    "systemctl is-failed --quiet dzap-backend && break; "
                    "sleep 1; done; "
                    "if systemctl is-active --quiet dzap-backend && "
                    "grep -q '<html' /tmp/dzap-index && "
                    "test \"$(stat -c %U /run/dzap)\" = dzap && "
                    "test -x /usr/local/bin/dzap-tui && "
                    "systemctl is-active --quiet getty@tty2 && "
                    "pgrep -x dzap-tui >/dev/null && "
                    "curl -fsS http://127.0.0.1:8080/api/drives | "
                    "jq -e '.storage | any(.name == \"/dev/vda\" and "
                    ".isOSDrive == true and .isMounted == true)' >/dev/null && "
                    "curl -fsS -H 'Content-Type: application/json' "
                    "-d '{\"devicePath\":\"/dev/vda\"}' "
                    "http://127.0.0.1:8080/api/recovery/assess | "
                    "jq -e '.decision == \"blocked\" and "
                    "(.checks | any(.code == \"protected_system\" and "
                    ".status == \"blocked\"))' >/dev/null && "
                    "true; then echo DZAP_LIVE_SMOKE_$ok; else "
                    "systemctl status --no-pager dzap-backend; "
                    "journalctl --no-pager -u dzap-backend -n 50; "
                    "echo DZAP_LIVE_SMOKE_$failed; fi"
                )
                guest.sendline(check)
                result = guest.expect(
                    ["DZAP_LIVE_SMOKE_OK", "DZAP_LIVE_SMOKE_FAILED"], timeout=90
                )
                if result != 0:
                    raise RuntimeError("guest health checks failed")
                guest.sendline("systemctl poweroff")
                guest.expect(pexpect.EOF, timeout=30)
            except Exception as error:
                guest.close(force=True)
                log.flush()
                lines = boot_log.read_text(errors="replace").splitlines()[-120:]
                raise RuntimeError(
                    f"live image smoke test failed: {error}\n" + "\n".join(lines)
                ) from error


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("image", nargs="?", type=Path)
    parser.add_argument("--timeout", type=int, default=300)
    args = parser.parse_args()

    image = (args.image or latest_iso())
    if image is None or not image.is_file():
        parser.error("DZap ISO not found; run `make iso` first")

    try:
        boot_and_check(image.resolve(), args.timeout)
    except (OSError, RuntimeError, subprocess.CalledProcessError) as error:
        print(error, file=sys.stderr)
        return 1
    print(f"DZap live image smoke test passed: {image}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
