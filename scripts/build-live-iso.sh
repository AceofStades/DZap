#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
BUILD_ROOT=${DZAP_ISO_BUILD_DIR:-"$REPO_ROOT/build/archiso"}
OUTPUT_DIR=${DZAP_ISO_OUT_DIR:-"$REPO_ROOT/out"}
BASE_PROFILE=${ARCHISO_PROFILE_DIR:-/usr/share/archiso/configs/releng}
PROFILE_DIR="$BUILD_ROOT/profile"
WORK_DIR="$BUILD_ROOT/work"
TARGET=x86_64-unknown-linux-musl
SOURCE_DATE_EPOCH=${SOURCE_DATE_EPOCH:-$(date +%s)}
export SOURCE_DATE_EPOCH
SECURE_BOOT=${DZAP_SECURE_BOOT:-0}
SECURE_BOOT_DIR=${DZAP_SECURE_BOOT_DIR:-"$REPO_ROOT/build/secure-boot"}
SECURE_BOOT_KEY=${DZAP_SECURE_BOOT_KEY:-"$SECURE_BOOT_DIR/db.key"}
SECURE_BOOT_CERT=${DZAP_SECURE_BOOT_CERT:-"$SECURE_BOOT_DIR/db.pem"}

cleanup_secure_boot_staging() {
    rm -rf "$PROFILE_DIR/airootfs/root/dzap-secure-boot"
}
trap cleanup_secure_boot_staging EXIT

require_command() {
    if ! command -v "$1" >/dev/null 2>&1; then
        echo "Missing required command: $1" >&2
        exit 1
    fi
}

remove_build_tree() {
    local path=$1

    if [[ -e $path ]]; then
        if (( EUID == 0 )); then
            rm -rf -- "$path"
        else
            unshare --map-auto --map-root-user -- rm -rf -- "$path"
        fi
    fi
}

for command in cargo mkarchiso npm rustup unshare; do
    require_command "$command"
done

case "$SECURE_BOOT" in
    0|1) ;;
    *)
        echo "DZAP_SECURE_BOOT must be 0 or 1." >&2
        exit 1
        ;;
esac

if [[ $SECURE_BOOT == 1 ]]; then
    for command in date openssl sbverify; do
        require_command "$command"
    done
    for input in "$SECURE_BOOT_KEY" "$SECURE_BOOT_CERT"; do
        if [[ ! -f $input || -L $input ]]; then
            echo "Secure Boot input must be a regular, non-symlink file: $input" >&2
            exit 1
        fi
    done
    key_mode=$(stat -c '%a' "$SECURE_BOOT_KEY")
    if (( (8#$key_mode & 077) != 0 )); then
        echo "Secure Boot private key must not be readable by group or others: $SECURE_BOOT_KEY" >&2
        exit 1
    fi

    key_public=$(openssl pkey -in "$SECURE_BOOT_KEY" -pubout -outform DER 2>/dev/null | sha256sum | cut -d ' ' -f 1)
    cert_public=$(openssl x509 -in "$SECURE_BOOT_CERT" -pubkey -noout 2>/dev/null \
        | openssl pkey -pubin -outform DER 2>/dev/null \
        | sha256sum \
        | cut -d ' ' -f 1)
    if [[ -z $key_public || $key_public != "$cert_public" ]]; then
        echo "Secure Boot certificate does not match the private key." >&2
        exit 1
    fi
fi

if [[ ! -f "$BASE_PROFILE/profiledef.sh" ]]; then
    echo "ArchISO releng profile not found at $BASE_PROFILE" >&2
    echo "Install archiso or set ARCHISO_PROFILE_DIR." >&2
    exit 1
fi

if ! rustup target list --installed | grep -qx "$TARGET"; then
    echo "Rust target $TARGET is not installed." >&2
    echo "Run: rustup target add $TARGET" >&2
    exit 1
fi

echo "==> Exporting dashboard"
npm --prefix "$REPO_ROOT/frontend" ci
npm --prefix "$REPO_ROOT/frontend" run build

echo "==> Building static Rust backend"
cargo build \
    --locked \
    --release \
    --features tui \
    --target "$TARGET" \
    --manifest-path "$REPO_ROOT/server/Cargo.toml"

echo "==> Preparing ArchISO profile"
remove_build_tree "$PROFILE_DIR"
remove_build_tree "$WORK_DIR"
mkdir -p "$PROFILE_DIR" "$OUTPUT_DIR"
cp -a "$BASE_PROFILE/." "$PROFILE_DIR/"
cp -a "$REPO_ROOT/iso/airootfs/." "$PROFILE_DIR/airootfs/"

install -m 0644 \
    "$REPO_ROOT/iso/boot/syslinux/archiso_sys.cfg" \
    "$PROFILE_DIR/syslinux/archiso_sys.cfg"
install -m 0644 \
    "$REPO_ROOT/iso/boot/syslinux/archiso_sys-linux.cfg" \
    "$PROFILE_DIR/syslinux/archiso_sys-linux.cfg"
rm -f "$PROFILE_DIR/efiboot/loader/entries/"*.conf
install -m 0644 \
    "$REPO_ROOT/iso/boot/efiboot/loader/loader.conf" \
    "$PROFILE_DIR/efiboot/loader/loader.conf"
if [[ $SECURE_BOOT == 1 ]]; then
    install -m 0644 \
        "$REPO_ROOT/iso/boot/efiboot/loader/entries/01-dzap-secure.conf" \
        "$PROFILE_DIR/efiboot/loader/entries/01-dzap.conf"
else
    install -m 0644 \
        "$REPO_ROOT/iso/boot/efiboot/loader/entries/01-dzap.conf" \
        "$PROFILE_DIR/efiboot/loader/entries/01-dzap.conf"
fi

cat "$REPO_ROOT/iso/packages.x86_64" >> "$PROFILE_DIR/packages.x86_64"
LC_ALL=C sort -u -o "$PROFILE_DIR/packages.x86_64" "$PROFILE_DIR/packages.x86_64"

if [[ $SECURE_BOOT == 1 ]]; then
    echo "==> Staging Secure Boot signing inputs"
    printf '%s\n' sbsigntools systemd-ukify >> "$PROFILE_DIR/packages.x86_64"
    LC_ALL=C sort -u -o "$PROFILE_DIR/packages.x86_64" "$PROFILE_DIR/packages.x86_64"

    install -D -m 0644 \
        "$REPO_ROOT/iso/secure-boot/99-dzap-secure-boot.hook" \
        "$PROFILE_DIR/airootfs/etc/pacman.d/hooks/99-dzap-secure-boot.hook"
    install -D -m 0700 \
        "$REPO_ROOT/iso/secure-boot/make-uki.sh" \
        "$PROFILE_DIR/airootfs/usr/local/lib/dzap/make-uki"
    install -d -m 0700 "$PROFILE_DIR/airootfs/root/dzap-secure-boot"
    install -m 0600 "$SECURE_BOOT_KEY" \
        "$PROFILE_DIR/airootfs/root/dzap-secure-boot/db.key"
    install -m 0644 "$SECURE_BOOT_CERT" \
        "$PROFILE_DIR/airootfs/root/dzap-secure-boot/db.pem"

    secure_boot_uuid=$(TZ=UTC date --date="@$SOURCE_DATE_EPOCH" +%Y-%m-%d-%H-%M-%S-00)
    printf 'archisobasedir=arch archisosearchuuid=%s\n' "$secure_boot_uuid" \
        > "$PROFILE_DIR/airootfs/root/dzap-secure-boot/cmdline"
    chmod 0600 "$PROFILE_DIR/airootfs/root/dzap-secure-boot/cmdline"

    install -d -m 0755 "$PROFILE_DIR/airootfs/usr/share/dzap"
    cert_fingerprint=$(openssl x509 -in "$SECURE_BOOT_CERT" -noout -fingerprint -sha256 | cut -d= -f2)
    cat > "$PROFILE_DIR/airootfs/usr/share/dzap/secure-boot.json" <<EOF
{
  "schemaVersion": 1,
  "mode": "owner-key",
  "certificateSha256Fingerprint": "$cert_fingerprint",
  "enrollmentArtifact": "dzap-secure-boot.cer",
  "enrollmentArtifactLocation": "beside-iso"
}
EOF
fi

install -D -m 0755 \
    "$REPO_ROOT/server/target/$TARGET/release/server" \
    "$PROFILE_DIR/airootfs/usr/local/bin/dzap-server"
install -D -m 0755 \
    "$REPO_ROOT/server/target/$TARGET/release/dzap-tui" \
    "$PROFILE_DIR/airootfs/usr/local/bin/dzap-tui"
install -d -m 0755 "$PROFILE_DIR/airootfs/opt/dzap/frontend"
cp -a "$REPO_ROOT/frontend/out/." "$PROFILE_DIR/airootfs/opt/dzap/frontend/"

mkdir -p "$PROFILE_DIR/airootfs/etc/systemd/system/multi-user.target.wants"
ln -sfn ../dzap-backend.service \
    "$PROFILE_DIR/airootfs/etc/systemd/system/multi-user.target.wants/dzap-backend.service"

sed -i \
    -e 's/^iso_name=.*/iso_name="dzap"/' \
    -e 's/^iso_label=.*/iso_label="DZAP_LIVE_$(date +%Y%m)"/' \
    -e 's/^iso_publisher=.*/iso_publisher="DZap Project"/' \
    -e 's/^iso_application=.*/iso_application="DZap Secure Wipe Live USB"/' \
    "$PROFILE_DIR/profiledef.sh"

cat >> "$PROFILE_DIR/profiledef.sh" <<'EOF'
file_permissions["/usr/local/bin/dzap-server"]="0:0:755"
file_permissions["/usr/local/bin/dzap-tui"]="0:0:755"
file_permissions["/usr/local/bin/dzap-kiosk"]="0:0:755"
EOF

if [[ $SECURE_BOOT == 1 ]]; then
    cat >> "$PROFILE_DIR/profiledef.sh" <<'EOF'
file_permissions["/root/dzap-secure-boot/"]="0:0:600"
file_permissions["/root/dzap-secure-boot"]="0:0:700"
file_permissions["/usr/local/lib/dzap/make-uki"]="0:0:700"
EOF
fi

echo "==> Building hybrid BIOS/UEFI image"
mkarchiso -v -r -w "$WORK_DIR" -o "$OUTPUT_DIR" "$PROFILE_DIR"

if [[ $SECURE_BOOT == 1 ]]; then
    signed_iso=$(find "$OUTPUT_DIR" -maxdepth 1 -type f -name 'dzap-*.iso' -print \
        | LC_ALL=C sort \
        | tail -n 1)
    if [[ -z $signed_iso ]]; then
        echo "Signed ISO was not created in $OUTPUT_DIR" >&2
        exit 1
    fi
    enrollment_certificate="$OUTPUT_DIR/dzap-secure-boot.cer"
    enrollment_certificate_tmp=$(mktemp "$OUTPUT_DIR/.dzap-secure-boot.cer.XXXXXX")
    openssl x509 \
        -in "$SECURE_BOOT_CERT" \
        -outform DER \
        -out "$enrollment_certificate_tmp"
    chmod 0644 "$enrollment_certificate_tmp"
    mv -f "$enrollment_certificate_tmp" "$enrollment_certificate"
    "$REPO_ROOT/scripts/verify-secure-iso.py" \
        "$signed_iso" \
        --certificate "$SECURE_BOOT_CERT" \
        --enrollment-certificate "$enrollment_certificate"
fi

echo "==> ISO ready in $OUTPUT_DIR"
