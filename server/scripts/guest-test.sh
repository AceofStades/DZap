#!/bin/sh
# Guest-side e2e test. Runs INSIDE the QEMU VM (Alpine live ISO) as root.
# Wipes /dev/vda — a scratch qcow2 virtual disk that exists only in the VM.
# All output goes to the serial console; the host driver greps for E2E lines.

log() { echo "E2E: $*"; }
fail() {
    echo "E2E: FAIL: $*"
    echo "E2E: server log: $(cat /tmp/dzap-server.log 2>/dev/null)"
    sync
    sleep 3
    poweroff -f
    sleep 5
    exit 1
}

SCRATCH=/dev/vda
RECOVERY_TARGET=/dev/sda
BASE=http://127.0.0.1:8080
# busybox wget stands in for curl on the live ISO.
http_get() { wget -qO- "$1"; }
http_post() { wget -qO- --header='Content-Type: application/json' --post-data="$2" "$1"; }

export HOME=/root

# Live ISO doesn't bring up loopback; the backend binds localhost:8080.
ifconfig lo up 2>/dev/null || true
grep -q localhost /etc/hosts 2>/dev/null || echo "127.0.0.1 localhost" >> /etc/hosts

# --- Start backend -----------------------------------------------------------
[ -x /root/server ] || fail "/root/server missing or not executable"
# Foreground sanity run: timeout kills it (rc=124 GNU / rc=143 busybox) if
# it stays alive; anything else means it crashed.
timeout 2 /root/server
RC=$?
[ "$RC" = 124 ] || [ "$RC" = 143 ] || fail "server foreground sanity run exited rc=$RC"
/root/server >/tmp/dzap-server.log 2>&1 &
SERVER_PID=$!
sleep 3
kill -0 $SERVER_PID 2>/dev/null || fail "server exited"

# Wait until the HTTP port actually answers (bind + DNS can lag on live ISO).
UP=0
for i in $(seq 1 15); do
    if http_get $BASE/api/drives >/dev/null 2>&1; then
        UP=1
        break
    fi
    sleep 1
done
if [ "$UP" != 1 ]; then
    kill -0 $SERVER_PID 2>/dev/null && echo "E2E: server alive but not answering" || echo "E2E: server died after startup"
    fail "backend never came up"
fi

# --- Test 1: /api/drives lists the scratch disk ------------------------------
DRIVES=$(http_get $BASE/api/drives) || fail "GET /api/drives failed"
echo "$DRIVES" | grep -q '"name":"/dev/vda"' || fail "/dev/vda missing from: $DRIVES"
log "PASS drives endpoint lists virtual scratch disk"

# --- Test 2: wipe-methods for the scratch disk -------------------------------
METHODS=$(http_get $BASE/api/drive/vda/wipe-methods) || fail "GET wipe-methods failed"
echo "$METHODS" | grep -q 'overwrite_1_pass' || fail "unexpected methods: $METHODS"
log "PASS wipe-methods endpoint"

# --- Test 3: assess and recover the disposable virtual source ----------------
mkfs.ext4 -F $SCRATCH >/tmp/mkfs-source.log 2>&1 || fail "could not format virtual recovery source"
mkdir -p /mnt/recovery-source /mnt/recovery-target
mount -t ext4 $SCRATCH /mnt/recovery-source || fail "could not mount virtual recovery source"
printf 'DZap filesystem recovery payload\n' >/mnt/recovery-source/document.txt
printf 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=' \
    | base64 -d >/mnt/recovery-source/photo.png
sync
umount /mnt/recovery-source || fail "could not unmount virtual recovery source"

mkfs.ext4 -F $RECOVERY_TARGET >/tmp/mkfs-target.log 2>&1 || fail "could not format virtual recovery target"
mount -t ext4 $RECOVERY_TARGET /mnt/recovery-target || fail "could not mount virtual recovery target"

SOURCE_HASH_BEFORE=$(sha256sum $SCRATCH | cut -d' ' -f1)
RECOVERY=$(http_post $BASE/api/recovery/assess '{"devicePath":"/dev/vda"}') || fail "POST recovery assessment failed"
echo "$RECOVERY" | grep -q '"decision":"caution"' || fail "unexpected recovery decision: $RECOVERY"
echo "$RECOVERY" | grep -q '"contentState":"structured_data"' || fail "filesystem source was not classified as structured: $RECOVERY"
echo "$RECOVERY" | grep -q '"encryption":"not_detected"' || fail "unexpected encryption classification: $RECOVERY"
SOURCE_HASH_AFTER=$(sha256sum $SCRATCH | cut -d' ' -f1)
[ "$SOURCE_HASH_BEFORE" = "$SOURCE_HASH_AFTER" ] || fail "recovery assessment changed the source"
log "PASS recovery assessment classified filesystem media without changing it"

SOURCE_IDENTITY=$(echo "$RECOVERY" | jq -c '.identity') || fail "recovery assessment identity missing"
DESTINATIONS=$(http_get "$BASE/api/recovery/destinations?sourceDevicePath=%2Fdev%2Fvda") || fail "GET recovery destinations failed"
DESTINATION=$(echo "$DESTINATIONS" | jq -c 'map(select(.drivePath == "/dev/sda" and .mountPath == "/mnt/recovery-target"))[0]')
[ "$DESTINATION" != null ] || fail "USB recovery destination missing: $DESTINATIONS"
PLAN_REQUEST=$(jq -nc \
    --arg source "/dev/vda" \
    --argjson identity "$SOURCE_IDENTITY" \
    --argjson destination "$DESTINATION" \
    '{sourceDevicePath:$source,expectedSourceIdentity:$identity,destination:$destination}')
IMAGE_PLAN=$(http_post $BASE/api/recovery/plan "$PLAN_REQUEST") || fail "POST recovery plan failed"
echo "$IMAGE_PLAN" | grep -q '"decision":"ready"' || fail "recovery image plan blocked: $IMAGE_PLAN"

IMAGE_STARTED=$(http_post $BASE/api/recovery/jobs "$PLAN_REQUEST") || fail "POST recovery image failed"
RECOVERY_JOB_ID=$(echo "$IMAGE_STARTED" | jq -r '.jobId')
[ -n "$RECOVERY_JOB_ID" ] && [ "$RECOVERY_JOB_ID" != null ] || fail "recovery start response missing job ID: $IMAGE_STARTED"
IMAGE_COMPLETE=0
for i in $(seq 1 120); do
    RECOVERY_JOB=$(http_get "$BASE/api/recovery/jobs/$RECOVERY_JOB_ID") || fail "GET recovery job failed"
    if echo "$RECOVERY_JOB" | grep -q '"status":"image_complete"'; then
        IMAGE_COMPLETE=1
        break
    fi
    sleep 1
done
[ "$IMAGE_COMPLETE" = 1 ] || fail "ddrescue image did not complete: $RECOVERY_JOB"
echo "$RECOVERY_JOB" | grep -Eq '"evidenceHash":"[0-9a-f]{64}"' || fail "recovery image evidence hash missing"
[ "$(sha256sum $SCRATCH | cut -d' ' -f1)" = "$SOURCE_HASH_BEFORE" ] || fail "ddrescue imaging changed the source"
log "PASS ddrescue created a persistent source image without changing the source"

TESTDISK=$(http_post "$BASE/api/recovery/jobs/$RECOVERY_JOB_ID/analyze" '{}') || fail "POST TestDisk analysis failed"
echo "$TESTDISK" | grep -Eq '"logSha256":"[0-9a-f]{64}"' || fail "TestDisk analysis missing log hash: $TESTDISK"
VOLUMES=$(http_get "$BASE/api/recovery/jobs/$RECOVERY_JOB_ID/volumes") || fail "GET recovery volumes failed"
echo "$VOLUMES" | jq -e 'any(.id == "whole-disk" and .filesystem == "ext4" and .filesystemCopySupported == true)' >/dev/null \
    || fail "imaged ext4 volume was not detected: $VOLUMES"
log "PASS read-only TestDisk and volume inspection completed"

FILES_REQUEST='{"method":"filesystem_copy","volumeId":"whole-disk"}'
http_post "$BASE/api/recovery/jobs/$RECOVERY_JOB_ID/recover" "$FILES_REQUEST" >/tmp/recovery-start.json \
    || fail "POST filesystem recovery failed"
FILES_COMPLETE=0
for i in $(seq 1 90); do
    RECOVERY_JOB=$(http_get "$BASE/api/recovery/jobs/$RECOVERY_JOB_ID") || fail "GET filesystem recovery job failed"
    if echo "$RECOVERY_JOB" | jq -e '.status == "completed" and .recoveryResult.method == "filesystem_copy"' >/dev/null; then
        FILES_COMPLETE=1
        break
    fi
    sleep 1
done
[ "$FILES_COMPLETE" = 1 ] || fail "filesystem recovery did not complete: $RECOVERY_JOB"
RECOVERED_OUTPUT=$(echo "$RECOVERY_JOB" | jq -r '.recoveryResult.outputDirectory')
RECOVERY_MANIFEST=$(echo "$RECOVERY_JOB" | jq -r '.recoveryResult.manifestPath')
RECOVERY_MANIFEST_HASH=$(echo "$RECOVERY_JOB" | jq -r '.recoveryResult.manifestSha256')
[ "$(cat "$RECOVERED_OUTPUT/document.txt")" = "DZap filesystem recovery payload" ] || fail "recovered file content differs"
[ "$(sha256sum "$RECOVERY_MANIFEST" | cut -d' ' -f1)" = "$RECOVERY_MANIFEST_HASH" ] || fail "recovery manifest hash differs"
echo "$RECOVERY_JOB" | jq -e '.recoveryResult.recoveredFileCount >= 2' >/dev/null || fail "recovery manifest missed files"
log "PASS filesystem-aware recovery preserved files and hash manifest"

PHOTOREC_REQUEST='{"method":"photorec","volumeId":"whole-disk"}'
http_post "$BASE/api/recovery/jobs/$RECOVERY_JOB_ID/recover" "$PHOTOREC_REQUEST" >/tmp/photorec-start.json \
    || fail "POST PhotoRec recovery failed"
PHOTOREC_COMPLETE=0
for i in $(seq 1 180); do
    RECOVERY_JOB=$(http_get "$BASE/api/recovery/jobs/$RECOVERY_JOB_ID") || fail "GET PhotoRec recovery job failed"
    if echo "$RECOVERY_JOB" | jq -e '.status == "completed" and .recoveryResult.method == "photorec"' >/dev/null; then
        PHOTOREC_COMPLETE=1
        break
    fi
    sleep 1
done
[ "$PHOTOREC_COMPLETE" = 1 ] || fail "PhotoRec recovery did not complete: $RECOVERY_JOB"
echo "$RECOVERY_JOB" | jq -e '.recoveryResult.recoveredFileCount > 0' >/dev/null || fail "PhotoRec did not carve a file"
log "PASS PhotoRec carved the source image and recorded file hashes"

# --- Test 4: image and unlock a disposable LUKS source -----------------------
RECOVERY_SECRET='DZap-e2e-LUKS-secret'
printf %s "$RECOVERY_SECRET" \
    | cryptsetup luksFormat --type luks2 --batch-mode --key-file=- $SCRATCH \
    || fail "could not create virtual LUKS source"
printf %s "$RECOVERY_SECRET" \
    | cryptsetup open --key-file=- $SCRATCH dzap-e2e-source \
    || fail "could not open virtual LUKS source"
mkfs.ext4 -F /dev/mapper/dzap-e2e-source >/tmp/mkfs-luks.log 2>&1 \
    || fail "could not format encrypted virtual filesystem"
mount -t ext4 /dev/mapper/dzap-e2e-source /mnt/recovery-source \
    || fail "could not mount encrypted virtual filesystem"
printf 'DZap encrypted recovery payload\n' >/mnt/recovery-source/encrypted.txt
sync
umount /mnt/recovery-source || fail "could not unmount encrypted virtual filesystem"
cryptsetup close dzap-e2e-source || fail "could not close virtual LUKS source"

LUKS_HASH_BEFORE=$(sha256sum $SCRATCH | cut -d' ' -f1)
LUKS_ASSESSMENT=$(http_post $BASE/api/recovery/assess '{"devicePath":"/dev/vda"}') \
    || fail "POST encrypted recovery assessment failed"
echo "$LUKS_ASSESSMENT" | grep -q '"encryption":"detected"' \
    || fail "LUKS source was not classified as encrypted: $LUKS_ASSESSMENT"
LUKS_IDENTITY=$(echo "$LUKS_ASSESSMENT" | jq -c '.identity')
LUKS_PLAN_REQUEST=$(jq -nc \
    --arg source "/dev/vda" \
    --argjson identity "$LUKS_IDENTITY" \
    --argjson destination "$DESTINATION" \
    '{sourceDevicePath:$source,expectedSourceIdentity:$identity,destination:$destination}')
LUKS_STARTED=$(http_post $BASE/api/recovery/jobs "$LUKS_PLAN_REQUEST") \
    || fail "POST encrypted recovery image failed"
LUKS_JOB_ID=$(echo "$LUKS_STARTED" | jq -r '.jobId')
LUKS_IMAGE_COMPLETE=0
for i in $(seq 1 120); do
    LUKS_JOB=$(http_get "$BASE/api/recovery/jobs/$LUKS_JOB_ID") || fail "GET encrypted recovery job failed"
    if echo "$LUKS_JOB" | grep -q '"status":"image_complete"'; then
        LUKS_IMAGE_COMPLETE=1
        break
    fi
    sleep 1
done
[ "$LUKS_IMAGE_COMPLETE" = 1 ] || fail "encrypted ddrescue image did not complete: $LUKS_JOB"
[ "$(sha256sum $SCRATCH | cut -d' ' -f1)" = "$LUKS_HASH_BEFORE" ] || fail "encrypted imaging changed the source"
LUKS_VOLUMES=$(http_get "$BASE/api/recovery/jobs/$LUKS_JOB_ID/volumes") || fail "GET LUKS volumes failed"
echo "$LUKS_VOLUMES" | jq -e 'any(.id == "whole-disk" and .encryption == "luks")' >/dev/null \
    || fail "LUKS image volume was not detected: $LUKS_VOLUMES"

LUKS_RECOVER_REQUEST=$(jq -nc \
    --arg secret "$RECOVERY_SECRET" \
    '{method:"filesystem_copy",volumeId:"whole-disk",passphrase:$secret}')
http_post "$BASE/api/recovery/jobs/$LUKS_JOB_ID/recover" "$LUKS_RECOVER_REQUEST" >/tmp/luks-recovery-start.json \
    || fail "POST LUKS filesystem recovery failed"
LUKS_RECOVERY_COMPLETE=0
for i in $(seq 1 90); do
    LUKS_JOB=$(http_get "$BASE/api/recovery/jobs/$LUKS_JOB_ID") || fail "GET LUKS file recovery failed"
    if echo "$LUKS_JOB" | jq -e '.status == "completed" and .recoveryResult.method == "filesystem_copy"' >/dev/null; then
        LUKS_RECOVERY_COMPLETE=1
        break
    fi
    sleep 1
done
[ "$LUKS_RECOVERY_COMPLETE" = 1 ] || fail "LUKS file recovery did not complete: $LUKS_JOB"
LUKS_OUTPUT=$(echo "$LUKS_JOB" | jq -r '.recoveryResult.outputDirectory')
[ "$(cat "$LUKS_OUTPUT/encrypted.txt")" = "DZap encrypted recovery payload" ] \
    || fail "encrypted recovered file content differs"
if grep -R "$RECOVERY_SECRET" /root/.config/DZap/recovery-jobs >/dev/null 2>&1; then
    fail "LUKS secret was persisted in internal recovery records"
fi
if find /mnt/recovery-target/DZap-Recovery -name job.json -exec grep -l "$RECOVERY_SECRET" {} + | grep -q .; then
    fail "LUKS secret was persisted in destination recovery records"
fi
log "PASS LUKS image was unlocked read-only without persisting its secret"

# Leave only 32 MiB free and prove a fresh image is blocked before writing.
AVAILABLE_KIB=$(df -k /mnt/recovery-target | awk 'NR == 2 {print $4}')
FILL_KIB=$((AVAILABLE_KIB - 32768))
[ "$FILL_KIB" -gt 0 ] || fail "recovery target lacks room for destination-full test"
fallocate -l "${FILL_KIB}K" /mnt/recovery-target/destination-full.test \
    || fail "could not fill virtual recovery destination"
FULL_PLAN=$(http_post $BASE/api/recovery/plan "$LUKS_PLAN_REQUEST") || fail "POST destination-full plan failed"
echo "$FULL_PLAN" | jq -e '.decision == "blocked" and any(.checks[]; .code == "destination_capacity" and .status == "blocked")' >/dev/null \
    || fail "destination-full image plan was not blocked: $FULL_PLAN"
rm -f /mnt/recovery-target/destination-full.test
DISCONNECTED_PLAN=$(echo "$LUKS_PLAN_REQUEST" | jq '.sourceDevicePath = "/dev/vdz"')
DISCONNECTED=$(http_post $BASE/api/recovery/plan "$DISCONNECTED_PLAN") || fail "POST disconnected plan failed"
echo "$DISCONNECTED" | jq -e '.decision == "blocked" and any(.checks[]; .code == "source_present" and .status == "blocked")' >/dev/null \
    || fail "disconnected source was not blocked: $DISCONNECTED"
log "PASS destination-full and disconnected recovery plans were blocked"

# --- Test 5: actually wipe the recovered virtual source ----------------------
PREFLIGHT_REQUEST='{"DevicePath":"/dev/vda","Method":"overwrite_1_pass","DeviceSerial":"","DeviceType":"HDD","DeviceModel":"QEMU HARDDISK"}'
PLAN=$(http_post $BASE/api/wipe/preflight "$PREFLIGHT_REQUEST") || fail "POST /api/wipe/preflight failed"
echo "$PLAN" | grep -q '"decision":"ready"' || fail "wipe preflight blocked: $PLAN"
IDENTITY=$(echo "$PLAN" | sed -n 's/.*"identity":\({[^}]*}\),"checks".*/\1/p')
[ -n "$IDENTITY" ] || fail "preflight response missing device identity: $PLAN"

WIPE_REQUEST=$(printf '{"DevicePath":"/dev/vda","Method":"overwrite_1_pass","DeviceSerial":"","DeviceType":"HDD","DeviceModel":"QEMU HARDDISK","ExpectedIdentity":%s}' "$IDENTITY")
RESULT=$(http_post $BASE/api/wipe "$WIPE_REQUEST")
echo "$RESULT" | grep -q 'Wipe process started' || fail "POST /api/wipe rejected: $RESULT"
JOB_ID=$(echo "$RESULT" | sed -n 's/.*"jobId":"\([^"]*\)".*/\1/p')
[ -n "$JOB_ID" ] || fail "wipe response missing server job ID: $RESULT"

# Poll until the whole disk reads back as zeros (pass-1 pattern is 0x00).
dd if=/dev/zero of=/tmp/zero-probe bs=1M count=1 2>/dev/null
WIPED=0
for i in $(seq 1 120); do
    sleep 1
    if dd if=$SCRATCH bs=1M count=1 2>/dev/null | cmp -s - /tmp/zero-probe; then
        WIPED=1
        break
    fi
done
[ "$WIPED" = 1 ] || fail "wipe did not complete within 120s"

# Full-disk verification against a same-sized zero file (in RAM).
dd if=/dev/zero of=/tmp/zeros bs=1M count=64 2>/dev/null
cmp -s $SCRATCH /tmp/zeros || fail "scratch disk still has non-zero bytes after wipe"
log "PASS overwrite_1_pass zeroed the entire virtual disk"

# The device contents can become observable just before the worker records its
# terminal evidence event. Wait for that server-owned completion record.
JOB_VERIFIED=0
for i in $(seq 1 15); do
    JOB=$(http_get "$BASE/api/wipe/jobs/$JOB_ID") || fail "GET wipe job failed"
    if echo "$JOB" | grep -q '"status":"verified"'; then
        JOB_VERIFIED=1
        break
    fi
    sleep 1
done
[ "$JOB_VERIFIED" = 1 ] || fail "wipe job did not record verification: $JOB"
echo "$JOB" | grep -Eq '"evidenceHash":"[0-9a-f]{64}"' || fail "wipe job missing evidence hash: $JOB"
echo "$JOB" | grep -q '"strategy":"full_pattern_readback"' || fail "wipe job missing full readback evidence: $JOB"
echo "$JOB" | grep -q '"bytesChecked":67108864' || fail "wipe job did not verify the full virtual disk: $JOB"
log "PASS server recorded full-readback hash-chained wipe evidence"

BLANK_ASSESSMENT=$(http_post $BASE/api/recovery/assess '{"devicePath":"/dev/vda"}') \
    || fail "POST blank recovery assessment failed"
echo "$BLANK_ASSESSMENT" | grep -q '"contentState":"likely_blank"' \
    || fail "wiped virtual source was not classified as likely blank: $BLANK_ASSESSMENT"
log "PASS wiped virtual media was conservatively classified as likely blank"

# --- Test 6: certificate endpoint --------------------------------------------
CERT_REQUEST=$(printf '{"jobId":"%s"}' "$JOB_ID")
CERT=$(http_post $BASE/api/certificate "$CERT_REQUEST")
echo "$CERT" | grep -q '"signature":"' || fail "certificate missing signature: $CERT"
echo "$CERT" | grep -q '"evidenceHash":"' || fail "certificate missing evidence hash: $CERT"
log "PASS certificate generation"

# --- Test 7: PDF format -------------------------------------------------------
wget -q -O /tmp/cert.pdf --header='Content-Type: application/json' \
    --post-data="$CERT_REQUEST" \
    "$BASE/api/certificate?format=pdf" || fail "PDF endpoint failed"
head -c 8 /tmp/cert.pdf | grep -q '%PDF-1.4' || fail "not a PDF"
log "PASS PDF generation"

kill $SERVER_PID 2>/dev/null
log "ALL TESTS PASSED"
poweroff -f
