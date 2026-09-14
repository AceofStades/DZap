#!/usr/bin/env python3
"""Verify the signed UEFI artifacts embedded in a DZap ISO."""

from __future__ import annotations

import argparse
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_CERTIFICATE = REPO_ROOT / "build" / "secure-boot" / "db.pem"
SIGNED_MEMBERS = (
    "EFI/BOOT/BOOTX64.EFI",
    "EFI/BOOT/BOOTIA32.EFI",
    "arch/boot/x86_64/vmlinuz-dzap.efi",
)
ENTRY_MEMBER = "loader/entries/01-dzap.conf"
ENROLLMENT_CERTIFICATE = "loader/keys/dzap-secure-boot.cer"
REQUIRED_UKI_SECTIONS = (".linux", ".initrd", ".cmdline", ".osrel")


def latest_secure_iso() -> Path | None:
    images = sorted((REPO_ROOT / "out" / "secure").glob("dzap-*.iso"))
    return images[-1] if images else None


def require_command(command: str) -> None:
    if shutil.which(command) is None:
        raise RuntimeError(f"required command is missing: {command}")


def read_iso_file(image: Path, member: str) -> bytes:
    return subprocess.check_output(["bsdtar", "-xOf", image, member])


def verify_signature(path: Path, certificate: Path) -> None:
    subprocess.run(
        ["sbverify", "--cert", str(certificate), str(path)],
        check=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
    )


def certificate_der(certificate: Path) -> bytes:
    return subprocess.check_output(
        ["openssl", "x509", "-in", certificate, "-outform", "DER"]
    )


def verify_image(image: Path, certificate: Path) -> None:
    for command in ("bsdtar", "objdump", "openssl", "sbverify"):
        require_command(command)

    entry = read_iso_file(image, ENTRY_MEMBER).decode("utf-8")
    if not re.search(
        r"^efi\s+/arch/boot/x86_64/vmlinuz-dzap\.efi\s*$", entry, re.MULTILINE
    ):
        raise RuntimeError("UEFI entry does not launch the signed DZap UKI")
    if re.search(r"^(linux|initrd)\s+", entry, re.MULTILINE):
        raise RuntimeError("UEFI entry exposes an unsigned split kernel or initramfs")

    embedded_certificate = read_iso_file(image, ENROLLMENT_CERTIFICATE)
    if embedded_certificate != certificate_der(certificate):
        raise RuntimeError("ISO enrollment certificate does not match the signing key")

    with tempfile.TemporaryDirectory(prefix="dzap-secure-boot-") as temp:
        scratch = Path(temp)
        extracted: dict[str, Path] = {}
        for member in SIGNED_MEMBERS:
            destination = scratch / Path(member).name
            destination.write_bytes(read_iso_file(image, member))
            verify_signature(destination, certificate)
            extracted[member] = destination

        uki = extracted["arch/boot/x86_64/vmlinuz-dzap.efi"]
        sections = subprocess.check_output(
            ["objdump", "-h", str(uki)], text=True, errors="replace"
        )
        missing = [section for section in REQUIRED_UKI_SECTIONS if section not in sections]
        if missing:
            raise RuntimeError(
                "signed UKI is missing required sections: " + ", ".join(missing)
            )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("image", nargs="?", type=Path)
    parser.add_argument("--certificate", type=Path, default=DEFAULT_CERTIFICATE)
    args = parser.parse_args()

    image = args.image or latest_secure_iso()
    if image is None or not image.is_file():
        parser.error("signed DZap ISO not found; run `make secure-iso` first")
    if not args.certificate.is_file():
        parser.error(f"signing certificate not found: {args.certificate}")

    try:
        verify_image(image.resolve(), args.certificate.resolve())
    except (OSError, RuntimeError, subprocess.CalledProcessError) as error:
        print(f"Secure Boot verification failed: {error}", file=sys.stderr)
        return 1

    print(f"DZap Secure Boot artifacts verified: {image}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
