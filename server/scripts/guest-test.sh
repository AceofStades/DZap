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

# --- Test 3: actually wipe the virtual disk ----------------------------------
dd if=/dev/urandom of=$SCRATCH bs=1M 2>/dev/null

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

# --- Test 4: certificate endpoint --------------------------------------------
CERT_REQUEST=$(printf '{"jobId":"%s"}' "$JOB_ID")
CERT=$(http_post $BASE/api/certificate "$CERT_REQUEST")
echo "$CERT" | grep -q '"signature":"' || fail "certificate missing signature: $CERT"
echo "$CERT" | grep -q '"evidenceHash":"' || fail "certificate missing evidence hash: $CERT"
log "PASS certificate generation"

# --- Test 5: PDF format -------------------------------------------------------
wget -q -O /tmp/cert.pdf --header='Content-Type: application/json' \
    --post-data="$CERT_REQUEST" \
    "$BASE/api/certificate?format=pdf" || fail "PDF endpoint failed"
head -c 8 /tmp/cert.pdf | grep -q '%PDF-1.4' || fail "not a PDF"
log "PASS PDF generation"

kill $SERVER_PID 2>/dev/null
log "ALL TESTS PASSED"
poweroff -f
