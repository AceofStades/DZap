# Evidence and certificates

DZap treats evidence as part of the wipe operation rather than a report assembled afterward. The backend owns the device identity, method, timestamps, state transitions, verification result, and certificate contents.

## Why server-owned evidence matters

An earlier style of certificate API could accept fields such as model, serial, method, or completion time from a browser. That lets a caller create a convincing certificate without proving those facts came from a completed operation.

The current certificate request contains only a server-generated job ID:

```json
{
  "jobId": "job-0123456789abcdef0123456789abcdef"
}
```

The backend loads the corresponding job, verifies its hash chain and terminal state, checks any existing certificate against that job, and then returns the signed record. Client-supplied certificate facts have no place in the request.

## Wipe job record

A job stores:

| Field | Meaning |
| --- | --- |
| `id` | `job-` followed by 32 random hexadecimal characters. |
| `devicePath` | Kernel device path authorized for the operation. |
| `deviceModel` and `deviceType` | Display and method-policy information detected by the backend. |
| `identity` | Model, serial, WWN, size, transport, and major/minor identity approved by the operator. |
| `method` | Stable method ID actually authorized. |
| `status` | `running`, `verifying`, `verified`, or `failed`. |
| `startedAt` | Time authorization evidence was created. |
| `sanitizationCompletedAt` | Time the destructive command ended successfully, before verification. |
| `completedAt` | Terminal verification or failure time. |
| `failure` | Terminal error text for failed jobs. |
| `verification` | Method-specific evidence for verified jobs. |
| `evidenceHash` | Hash of the final event in the chain. |
| `events` | Ordered evidence events. |

The job is created and persisted before the destructive worker starts. If recording authorization fails, the wipe request fails without starting sanitization.

## Event chain

Each event contains:

- Zero-based sequence number.
- UTC timestamp with nanosecond RFC 3339 formatting in the hash payload.
- Event type.
- Human-readable message.
- Previous event hash, or `null` for the first event.
- Current event hash.

The SHA-256 payload covers both immutable job context and event content:

```text
jobId
devicePath
deviceModel
deviceType
complete DeviceIdentity
method
sequence
timestamp
eventType
message
previousHash
```

The payload is serialized as camel-case JSON and hashed with SHA-256. Each event's `previousHash` must equal the prior event's `eventHash`; the job's `evidenceHash` must equal the last event hash.

This design means changing a serial, method, message, timestamp, sequence, verification result, or earlier hash invalidates the chain from that point onward.

## Valid event sequences

The store accepts only these shapes:

### Running

```text
0 wipe_authorized
```

There must be no sanitization completion time, terminal time, failure, or verification.

### Verifying

```text
0 wipe_authorized
1 sanitization_completed
```

The second event timestamp must equal `sanitizationCompletedAt`. There is no terminal time, failure, or verification yet.

### Verified

```text
0 wipe_authorized
1 sanitization_completed
2 verification_completed
```

The final message contains the exact serialized verification result. Its timestamp must equal `completedAt`; `failure` must be empty.

### Failed before command completion

```text
0 wipe_authorized
1 wipe_failed
```

### Failed during verification

```text
0 wipe_authorized
1 sanitization_completed
2 wipe_failed
```

In both failed forms, the terminal message must match the stored failure and the timestamp must match `completedAt`.

## Persistence behavior

Production uses one JSON file per job under the DZap configuration directory. Each update follows this sequence:

1. Serialize the complete job to pretty JSON.
2. Create or truncate `.<job-id>.json.tmp` in the destination directory.
3. Write all bytes.
4. Set file mode `0600` on Unix.
5. `fsync` the file.
6. Atomically rename it to `<job-id>.json`.
7. `fsync` the directory.

The directory is mode `0700`. Atomic replacement prevents readers from observing a partially written JSON record. File and directory syncing reduces the risk of an acknowledged transition being lost after sudden power failure, subject to filesystem and hardware behavior.

On startup, the store checks file names, job ID format, JSON parsing, hash chain, and state invariants. Duplicate IDs or invalid evidence stop startup.

Any valid persisted job left in `running` or `verifying` is changed to `failed` with the message:

```text
backend restarted before terminal evidence was recorded
```

DZap does not claim that a restarted operation completed, even if a controller may have continued work independently.

## Certificate contents

A certificate includes:

- Job ID.
- Device path, model, serial, WWN, approved byte size, transport, and major/minor number.
- Device type and wipe method.
- Job start and verified completion times.
- Certificate issue timestamp.
- Complete `VerificationResult`.
- Final job evidence hash.
- Hexadecimal signature.
- PEM public key.

The certificate's data is compared field-by-field with the verified job whenever it is loaded or returned. The certificate cannot be reused for another identity, method, result, or completion time.

## Signature construction

At startup, DZap loads or generates a 2048-bit RSA private key. The key is stored as PKCS#1 PEM in:

```text
~/.config/DZap/private.pem
```

Certificate data is serialized to JSON, hashed with SHA-256, and signed with RSA PKCS#1 v1.5 for SHA-256. The signature is encoded as lowercase hexadecimal. The matching public key is embedded in SubjectPublicKeyInfo PEM form.

The verifier can therefore check that:

- The certificate data has not changed since signing.
- The signature was produced by the private key corresponding to the embedded public key.

The embedded key alone does not establish who controls that key. External trust requires a known public-key fingerprint, an organizational signing hierarchy, or another distribution mechanism. That release/identity policy is not implemented yet.

## Certificate store

Certificates are persisted one per job ID using the same atomic write, sync, and Unix permission approach as jobs. Generation is idempotent: if a valid certificate already exists for a job, the backend returns it instead of issuing a new timestamp and signature.

At startup, every persisted certificate must:

- Parse correctly.
- Contain the public key belonging to the current private key.
- Have a valid signature.
- Have a file name matching its job ID.
- Match a valid persisted verified job.

Failure stops backend startup. This avoids presenting a partially inconsistent evidence directory.

## JSON, QR, and PDF forms

The default certificate endpoint returns JSON. The QR code encodes that signed JSON representation without the in-memory QR matrix field.

With `?format=pdf`, the backend generates a minimal A4 PDF containing:

- Device and job identifiers.
- Method and evidence hash.
- Verification strategy and bytes checked.
- Readback SHA-256.
- Identity-revalidation result.
- QR code.
- Signature and public key.

The PDF is generated directly in Rust and returned with `Content-Type: application/pdf` and an attachment filename. The JSON object remains the canonical machine-readable form.

## Live-USB retention limitation

The service sets `HOME=/root`, so the live system writes its key, jobs, and certificates under `/root/.config/DZap`. ArchISO provides a writable overlay, allowing state to survive backend restarts during the current boot.

That overlay is volatile. Removing power or rebooting loses the key and stored records. Browser downloads also land under the kiosk user's volatile home unless the operator explicitly chooses mounted persistent media through a supported export workflow.

The planned solution must export an evidence bundle to a separate writable volume and make the destination explicit. It should include at least:

- Job JSON.
- Certificate JSON.
- Certificate PDF.
- Firmware-status raw material where applicable.
- Public-key fingerprint and bundle manifest.
- Hashes for every exported file.

Until that exists, operators must treat a successful in-session certificate as temporary.

## What the evidence proves

Given an uncompromised running backend and signing key, the current evidence can prove that DZap:

- Approved a specific detected identity and method.
- Recorded an ordered, untampered state transition chain.
- Received successful command completion before verification.
- Collected a verification result matching the method's policy.
- Bound the certificate to that job and verification result.

It does not independently prove:

- That device firmware implemented its command honestly.
- That inaccessible remapped flash cells were physically cleared.
- That the live system clock was externally trusted.
- That the embedded public key belongs to a particular organization.
- That evidence survived after the live session ended.
