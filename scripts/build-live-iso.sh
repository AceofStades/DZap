#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
BUILD_ROOT=${DZAP_ISO_BUILD_DIR:-"$REPO_ROOT/build/archiso"}
OUTPUT_DIR=${DZAP_ISO_OUT_DIR:-"$REPO_ROOT/out"}
BASE_PROFILE=${ARCHISO_PROFILE_DIR:-/usr/share/archiso/configs/releng}
PROFILE_DIR="$BUILD_ROOT/profile"
WORK_DIR="$BUILD_ROOT/work"
TARGET=x86_64-unknown-linux-musl

require_command() {
    if ! command -v "$1" >/dev/null 2>&1; then
        echo "Missing required command: $1" >&2
        exit 1
    fi
}

for command in cargo mkarchiso npm rustup; do
    require_command "$command"
done

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
    --target "$TARGET" \
    --manifest-path "$REPO_ROOT/server/Cargo.toml"

echo "==> Preparing ArchISO profile"
rm -rf "$PROFILE_DIR" "$WORK_DIR"
mkdir -p "$PROFILE_DIR" "$OUTPUT_DIR"
cp -a "$BASE_PROFILE/." "$PROFILE_DIR/"
cp -a "$REPO_ROOT/iso/airootfs/." "$PROFILE_DIR/airootfs/"

cat "$REPO_ROOT/iso/packages.x86_64" >> "$PROFILE_DIR/packages.x86_64"
LC_ALL=C sort -u -o "$PROFILE_DIR/packages.x86_64" "$PROFILE_DIR/packages.x86_64"

install -D -m 0755 \
    "$REPO_ROOT/server/target/$TARGET/release/server" \
    "$PROFILE_DIR/airootfs/usr/local/bin/dzap-server"
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
file_permissions["/usr/local/bin/dzap-kiosk"]="0:0:755"
EOF

echo "==> Building hybrid BIOS/UEFI image"
mkarchiso -v -r -w "$WORK_DIR" -o "$OUTPUT_DIR" "$PROFILE_DIR"

echo "==> ISO ready in $OUTPUT_DIR"
