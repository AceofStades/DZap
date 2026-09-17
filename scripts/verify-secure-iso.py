#!/usr/bin/env python3
"""Verify the signed UEFI artifacts embedded in a DZap ISO."""

from __future__ import annotations

import argparse
import json
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_CERTIFICATE = REPO_ROOT / "build" / "secure-boot" / "db.pem"
SIGNED_MEMBERS = (
    "/EFI/BOOT/BOOTX64.EFI",
    "/EFI/BOOT/BOOTIA32.EFI",
    "/arch/boot/x86_64/vmlinuz-dzap.efi",
)
ENTRY_MEMBER = "/loader/entries/01-dzap.conf"
REQUIRED_UKI_SECTIONS = (".linux", ".initrd", ".cmdline", ".osrel")
EFI_START_PATTERN = re.compile(r"EFI image start and load size:\s+(\d+) \* 2048")
FORBIDDEN_ROOTFS_PATHS = (
    "root/dzap-secure-boot",
    "etc/pacman.d/hooks/99-dzap-secure-boot.hook",
    "usr/local/lib/dzap/make-uki",
)


def latest_secure_iso() -> Path | None:
    images = sorted((REPO_ROOT / "out" / "secure").glob("dzap-*.iso"))
    return images[-1] if images else None


def require_command(command: str) -> None:
    if shutil.which(command) is None:
        raise RuntimeError(f"required command is missing: {command}")


def esp_image_spec(image: Path) -> str:
    report = subprocess.check_output(
        ["xorriso", "-indev", str(image), "-report_el_torito", "plain"],
        stderr=subprocess.STDOUT,
        text=True,
    )
    matches = EFI_START_PATTERN.findall(report)
    if len(matches) != 1:
        raise RuntimeError(f"expected one UEFI El Torito image, found {len(matches)}")
    byte_offset = int(matches[0]) * 2048
    return f"{image}@@{byte_offset}"


def copy_esp_file(esp: str, member: str, destination: Path) -> None:
    subprocess.run(
        ["mcopy", "-i", esp, f"::{member}", str(destination)],
        check=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
    )


def certificate_fingerprint(certificate: Path) -> str:
    fingerprint = subprocess.check_output(
        [
            "openssl",
            "x509",
            "-in",
            str(certificate),
            "-noout",
            "-fingerprint",
            "-sha256",
        ],
        text=True,
    ).strip()
    return fingerprint.partition("=")[2]


def verify_live_root(image: Path, scratch: Path, certificate: Path) -> None:
    rootfs = scratch / "airootfs.sfs"
    with rootfs.open("wb") as output:
        subprocess.run(
            ["bsdtar", "-xOf", str(image), "arch/x86_64/airootfs.sfs"],
            check=True,
            stdout=output,
        )
    listing = subprocess.check_output(
        ["unsquashfs", "-ll", str(rootfs)], text=True, errors="replace"
    )
    leaked = [path for path in FORBIDDEN_ROOTFS_PATHS if path in listing]
    if leaked:
        raise RuntimeError(
            "temporary Secure Boot material leaked into the live root: "
            + ", ".join(leaked)
        )

    metadata_bytes = subprocess.check_output(
        [
            "unsquashfs",
            "-cat",
            str(rootfs),
            "usr/share/dzap/secure-boot.json",
        ]
    )
    metadata = json.loads(metadata_bytes)
    expected_metadata = {
        "schemaVersion": 1,
        "mode": "owner-key",
        "certificateSha256Fingerprint": certificate_fingerprint(certificate),
        "enrollmentArtifact": "dzap-secure-boot.cer",
        "enrollmentArtifactLocation": "beside-iso",
    }
    if metadata != expected_metadata:
        raise RuntimeError("live-root Secure Boot metadata is missing or inconsistent")


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


def iso_search_uuid(image: Path) -> str:
    listing = subprocess.check_output(
        ["bsdtar", "-tf", str(image)], text=True, errors="replace"
    )
    matches = re.findall(r"^boot/(\d{4}(?:-\d{2}){5}-00)\.uuid$", listing, re.MULTILINE)
    if len(matches) != 1:
        raise RuntimeError(f"expected one ArchISO search UUID, found {len(matches)}")
    return matches[0]


def verify_image(
    image: Path, certificate: Path, enrollment_certificate: Path
) -> None:
    for command in (
        "bsdtar",
        "mcopy",
        "objcopy",
        "objdump",
        "openssl",
        "sbverify",
        "unsquashfs",
        "xorriso",
    ):
        require_command(command)

    if enrollment_certificate.read_bytes() != certificate_der(certificate):
        raise RuntimeError("published enrollment certificate does not match the signing key")

    with tempfile.TemporaryDirectory(prefix="dzap-secure-boot-") as temp:
        scratch = Path(temp)
        esp = esp_image_spec(image)

        entry_path = scratch / "01-dzap.conf"
        copy_esp_file(esp, ENTRY_MEMBER, entry_path)
        entry = entry_path.read_text(encoding="utf-8")
        if not re.search(
            r"^efi\s+/arch/boot/x86_64/vmlinuz-dzap\.efi\s*$",
            entry,
            re.MULTILINE,
        ):
            raise RuntimeError("UEFI entry does not launch the signed DZap UKI")
        if re.search(r"^(linux|initrd)\s+", entry, re.MULTILINE):
            raise RuntimeError("UEFI entry exposes an unsigned split kernel or initramfs")

        extracted: dict[str, Path] = {}
        for member in SIGNED_MEMBERS:
            destination = scratch / Path(member).name
            copy_esp_file(esp, member, destination)
            verify_signature(destination, certificate)
            extracted[member] = destination

        uki = extracted["/arch/boot/x86_64/vmlinuz-dzap.efi"]
        sections = subprocess.check_output(
            ["objdump", "-h", str(uki)], text=True, errors="replace"
        )
        missing = [section for section in REQUIRED_UKI_SECTIONS if section not in sections]
        if missing:
            raise RuntimeError(
                "signed UKI is missing required sections: " + ", ".join(missing)
            )

        cmdline_path = scratch / "cmdline"
        subprocess.run(
            ["objcopy", f"--dump-section=.cmdline={cmdline_path}", str(uki)],
            check=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
        )
        cmdline = cmdline_path.read_bytes().rstrip(b"\0\n").decode("utf-8")
        expected_cmdline = (
            "archisobasedir=arch archisosearchuuid=" + iso_search_uuid(image)
        )
        if cmdline != expected_cmdline:
            raise RuntimeError(
                "signed UKI command line does not match the ISO search UUID"
            )

        verify_live_root(image, scratch, certificate)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("image", nargs="?", type=Path)
    parser.add_argument("--certificate", type=Path, default=DEFAULT_CERTIFICATE)
    parser.add_argument("--enrollment-certificate", type=Path)
    args = parser.parse_args()

    image = args.image or latest_secure_iso()
    if image is None or not image.is_file():
        parser.error("signed DZap ISO not found; run `make secure-iso` first")
    if not args.certificate.is_file():
        parser.error(f"signing certificate not found: {args.certificate}")
    enrollment_certificate = (
        args.enrollment_certificate or image.parent / "dzap-secure-boot.cer"
    )
    if not enrollment_certificate.is_file():
        parser.error(
            "published enrollment certificate not found: "
            f"{enrollment_certificate}"
        )

    try:
        verify_image(
            image.resolve(),
            args.certificate.resolve(),
            enrollment_certificate.resolve(),
        )
    except (
        json.JSONDecodeError,
        OSError,
        RuntimeError,
        subprocess.CalledProcessError,
    ) as error:
        print(f"Secure Boot verification failed: {error}", file=sys.stderr)
        return 1

    print(f"DZap Secure Boot artifacts verified: {image}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
