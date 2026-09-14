#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
KEY_DIR=${1:-"$REPO_ROOT/build/secure-boot"}
KEY_FILE="$KEY_DIR/db.key"
CERT_FILE="$KEY_DIR/db.pem"
DER_FILE="$KEY_DIR/db.cer"

if ! command -v openssl >/dev/null 2>&1; then
    echo "Missing required command: openssl" >&2
    exit 1
fi

for output in "$KEY_FILE" "$CERT_FILE" "$DER_FILE"; do
    if [[ -e $output ]]; then
        echo "Refusing to replace existing Secure Boot material: $output" >&2
        exit 1
    fi
done

umask 077
mkdir -p "$KEY_DIR"
TEMP_DIR=$(mktemp -d "$KEY_DIR/.generating.XXXXXX")
trap 'rm -rf "$TEMP_DIR"' EXIT

openssl req \
    -new \
    -x509 \
    -newkey rsa:2048 \
    -nodes \
    -sha256 \
    -days 3650 \
    -subj '/CN=DZap Demo Secure Boot/' \
    -addext 'keyUsage=critical,digitalSignature' \
    -keyout "$TEMP_DIR/db.key" \
    -out "$TEMP_DIR/db.pem"
openssl x509 \
    -in "$TEMP_DIR/db.pem" \
    -outform DER \
    -out "$TEMP_DIR/db.cer"

install -m 0600 "$TEMP_DIR/db.key" "$KEY_FILE"
install -m 0644 "$TEMP_DIR/db.pem" "$CERT_FILE"
install -m 0644 "$TEMP_DIR/db.cer" "$DER_FILE"

echo "DZap demo Secure Boot key created in $KEY_DIR"
openssl x509 -in "$CERT_FILE" -noout -subject -fingerprint -sha256
echo "Keep db.key private. Enroll db.cer on each demo machine."
