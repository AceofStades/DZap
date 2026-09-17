#!/usr/bin/env bash
set -euo pipefail

KEY_DIR=/root/dzap-secure-boot
KEY_FILE="$KEY_DIR/db.key"
CERT_FILE="$KEY_DIR/db.pem"
CMDLINE_FILE="$KEY_DIR/cmdline"
UKI_FILE=/boot/vmlinuz-dzap.efi
HOOK_FILE=/etc/pacman.d/hooks/99-dzap-secure-boot.hook
SELF_FILE=/usr/local/lib/dzap/make-uki

cleanup() {
    rm -rf "$KEY_DIR"
    rm -f "$HOOK_FILE" "$SELF_FILE"
}
trap cleanup EXIT

for command in sbsign sbverify ukify; do
    command -v "$command" >/dev/null 2>&1 || {
        echo "Missing Secure Boot build command: $command" >&2
        exit 1
    }
done

for input in "$KEY_FILE" "$CERT_FILE" "$CMDLINE_FILE" /boot/vmlinuz-linux /boot/initramfs-linux.img; do
    [[ -f $input ]] || {
        echo "Missing Secure Boot build input: $input" >&2
        exit 1
    }
done

umask 077
UKI_TEMP=$(mktemp /boot/.vmlinuz-dzap.efi.XXXXXX)
rm -f "$UKI_TEMP"
ukify build \
    --linux=/boot/vmlinuz-linux \
    --initrd=/boot/initramfs-linux.img \
    --cmdline="@$CMDLINE_FILE" \
    --os-release=@/etc/os-release \
    --secureboot-private-key="$KEY_FILE" \
    --secureboot-certificate="$CERT_FILE" \
    --sign-kernel \
    --output="$UKI_TEMP"
sbverify --cert "$CERT_FILE" "$UKI_TEMP"
install -m 0644 "$UKI_TEMP" "$UKI_FILE"
rm -f "$UKI_TEMP"

while IFS= read -r -d '' loader; do
    signed_loader=$(mktemp "${loader}.signed.XXXXXX")
    rm -f "$signed_loader"
    sbsign \
        --key "$KEY_FILE" \
        --cert "$CERT_FILE" \
        --output "$signed_loader" \
        "$loader"
    sbverify --cert "$CERT_FILE" "$signed_loader"
    install -m 0644 "$signed_loader" "$loader"
    rm -f "$signed_loader"
done < <(find /usr/lib/systemd/boot/efi -maxdepth 1 -type f -name 'systemd-boot*.efi' -print0)
