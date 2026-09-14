# HTTP and WebSocket API

The Rust backend serves the API, WebSocket, and static dashboard on:

```text
http://127.0.0.1:8080
```

Examples below use `localhost`, which reaches the same loopback service. All JSON field names in responses are camel-case unless shown otherwise.

## Endpoint summary

| Method | Path | Purpose |
| --- | --- | --- |
| GET | `/api/drives` | Discover storage and available Android devices. |
| GET | `/api/drive/{name}/health` | Read SMART data and optional health prediction. |
| GET | `/api/drive/{name}/wipe-methods` | Return currently available methods for one target. |
| POST | `/api/wipe/preflight` | Run read-only safety checks and return an identity for approval. |
| POST | `/api/wipe` | Re-run checks with approved identity and start a job. |
| GET | `/api/wipe/jobs` | List jobs, newest first. |
| GET | `/api/wipe/jobs/{id}` | Load one authoritative job. |
| POST | `/api/wipe/pause` | Toggle pause for an active host wipe. |
| POST | `/api/wipe/abort` | Request cancellation for an active wipe. |
| POST | `/api/recovery/assess` | Run a read-only recovery-source assessment. |
| GET | `/api/recovery/destinations` | List eligible removable image destinations for one source. |
| POST | `/api/recovery/destinations/mount` | Revalidate and mount one selected recovery destination. |
| POST | `/api/recovery/plan` | Bind both identities and validate an image plan without starting it. |
| POST | `/api/unmount` | Unmount a non-system device and its direct mounted children. |
| GET | `/api/certificates` | List signed certificates, newest first. |
| POST | `/api/certificate` | Issue/return JSON, or PDF with `?format=pdf`. |
| POST | `/api/certificate/generate` | Alias for JSON certificate generation. |
| GET | `/api/evidence/destinations` | List eligible removable evidence volumes and their current mount state. |
| POST | `/api/evidence/mount` | Mount one exactly revalidated destination with restricted options. |
| POST | `/api/evidence/export` | Write and read back a verified evidence bundle. |
| GET | `/ws` | Stream progress and terminal events. |

Unknown non-API paths fall back to files in `DZAP_FRONTEND_DIR`.

## Error form

Most handler errors use:

```json
{
  "error": "Human-readable explanation"
}
```

Blocked wipe authorization is different: HTTP `412` returns the complete structured preflight plan so callers can show every failed check.

## Discover devices

```http
GET /api/drives
```

Representative response:

```json
{
  "storage": [
    {
      "name": "/dev/sdb",
      "model": "Example Disk",
      "serial": "SERIAL-123",
      "wwn": "0x5000000000000001",
      "size": "1000204886016",
      "transport": "sata",
      "majorMinor": "8:16",
      "type": "HDD",
      "isMounted": false,
      "isFrozen": false,
      "isOSDrive": false,
      "activeDependencies": [],
      "partitions": [
        {
          "name": "/dev/sdb1",
          "size": "1000203091968",
          "type": "ext4"
        }
      ]
    }
  ],
  "mobile": []
}
```

If one discovery family fails, its value can be `null` while the other family is still returned.

## Available wipe methods

The path parameter omits `/dev/` for storage:

```http
GET /api/drive/sdb/wipe-methods
```

Response:

```json
[
  {
    "id": "overwrite_1_pass",
    "name": "1-Pass Overwrite",
    "description": "Writes 0x00 across the full host-visible device."
  }
]
```

The list is device- and capability-dependent. Callers must not cache it as authorization; `POST /api/wipe` rechecks everything.

## Health

```http
GET /api/drive/sdb/health
```

Response fields are:

```json
{
  "predictedStatus": "Healthy",
  "failureProbability": 0.0,
  "smartStatus": "Passed",
  "smartAttributes": {
    "Power_On_Hours": {
      "name": "Power_On_Hours",
      "value": 1234
    }
  }
}
```

Unsupported SMART access produces `N/A`/`Not available` rather than a destructive-flow failure. SATA model prediction is optional and currently lacks a packaged model/runtime in the live image; SMART status remains useful when the model is skipped.

## Assess a recovery source

```http
POST /api/recovery/assess
Content-Type: application/json
```

```json
{
  "devicePath": "/dev/sdb"
}
```

The path must identify a freshly detected whole storage drive. The backend blocks its own system/live medium and sources reserved by another storage operation. During assessment it reserves the path, reads storage signatures and SMART/NVMe health indicators, and samples five evenly spaced regions without mounting or writing to the source.

The response contains `decision` (`ready`, `caution`, or `blocked`), the detected identity, individual checks, signatures, encryption and media states, sparse-sample counts, and ordered recommendations. Content is classified as `structured_data`, `non_blank`, `likely_blank`, or `unknown`.

A `likely_blank` result is not proof of a completed secure wipe. A `non_blank` result is not proof that useful files remain. See [Data recovery](data-recovery.md) for the exact interpretation and planned execution pipeline.

Missing, protected, and busy sources return HTTP `200` with a structured `blocked` assessment. Invalid JSON returns `400`; discovery failures return `500`.

## Select and validate an image destination

```http
GET /api/recovery/destinations?sourceDevicePath=/dev/sdb
```

This lists removable USB volumes supported by the existing controlled-mount path: ext4, exFAT, and FAT32. The source's physical drive and the running system/live medium are excluded. Mounted entries include current filesystem size, available bytes, and read-only state. An unmounted entry returns those fields as `null` until it is mounted.

The dashboard can mount a selected destination through:

```http
POST /api/recovery/destinations/mount
Content-Type: application/json
```

The request sends `sourceDevicePath`, `expectedSourceIdentity`, and the exact `destination` object returned by discovery. The backend freshly revalidates both whole-drive identities, proves they are different physical drives, reserves both paths during the mount, and uses restricted mount options. Mounting can update the destination filesystem's metadata; the recovery source is never mounted by this operation. A changed, busy, protected, or ineligible device returns `409`.

Build the non-executing plan with the same request shape:

```http
POST /api/recovery/plan
Content-Type: application/json
```

The response contains `decision` (`ready` or `blocked`), fresh source and destination identities, image size, a 64 MiB metadata/free-space reserve, the planned `DZap-Recovery` directory, and individual checks. A ready plan requires:

- The source identity still matches its assessment.
- The source is neither system media nor mounted and has no active RAID, LVM, crypt, or device-mapper descendant.
- The destination drive and partition identity still match the selection and differ from the source drive.
- The destination is mounted read-write and has enough available bytes for a full source-sized image plus the reserve.
- The filesystem can hold one source-sized file. FAT32 is blocked for sources larger than its single-file limit; exFAT or ext4 is required.
- Neither physical drive is held by another storage operation at planning time.

Safety failures return HTTP `200` with a structured blocked plan. The execution endpoint will revalidate and reserve both identities again because a plan does not hold devices after it is returned.

## Preflight and authorization handshake

### 1. Read-only preflight

```http
POST /api/wipe/preflight
Content-Type: application/json
```

```json
{
  "DevicePath": "/dev/sdb",
  "Method": "overwrite_1_pass",
  "DeviceSerial": "/dev/sdb",
  "DeviceType": "HDD",
  "DeviceModel": "Example Disk"
}
```

The backend accepts legacy Pascal-case, camel-case, and snake-case aliases for wipe configuration fields. New callers should use the form shown above to match the frontend types.

`DeviceSerial` is a legacy request field and the current storage frontend sends the device path there. It is not the safety identity. The backend derives the authoritative serial and other identity fields from fresh device discovery, returns them in `identity`, and requires that full object as `ExpectedIdentity` when the wipe is authorized.

Ready response:

```json
{
  "decision": "ready",
  "devicePath": "/dev/sdb",
  "deviceModel": "Example Disk",
  "deviceType": "HDD",
  "method": "overwrite_1_pass",
  "identity": {
    "model": "Example Disk",
    "serial": "SERIAL-123",
    "wwn": "0x5000000000000001",
    "sizeBytes": "1000204886016",
    "transport": "sata",
    "majorMinor": "8:16"
  },
  "checks": [
    {
      "code": "device_exists",
      "status": "passed",
      "message": "Device is present."
    }
  ]
}
```

The same endpoint can return HTTP `200` with `decision: "blocked"`; a blocked plan is a successful evaluation, not a server error.

### 2. Start using the approved identity

Repeat the request with the exact returned identity:

```http
POST /api/wipe
Content-Type: application/json
```

```json
{
  "DevicePath": "/dev/sdb",
  "Method": "overwrite_1_pass",
  "DeviceSerial": "/dev/sdb",
  "DeviceType": "HDD",
  "DeviceModel": "Example Disk",
  "ExpectedIdentity": {
    "model": "Example Disk",
    "serial": "SERIAL-123",
    "wwn": "0x5000000000000001",
    "sizeBytes": "1000204886016",
    "transport": "sata",
    "majorMinor": "8:16"
  }
}
```

Accepted response is HTTP `202`:

```json
{
  "status": "Wipe process started",
  "jobId": "job-0123456789abcdef0123456789abcdef",
  "deviceId": "/dev/sdb"
}
```

Relevant status codes:

| Status | Meaning |
| --- | --- |
| `202 Accepted` | Authorization evidence was persisted and the worker started. |
| `400 Bad Request` | Invalid JSON or missing/invalid fields. |
| `409 Conflict` | Another storage operation already reserves the path. |
| `412 Precondition Failed` | Authorization preflight blocked; body is the plan. |
| `500 Internal Server Error` | Discovery, worker setup, or evidence persistence failed. |

## Jobs

```http
GET /api/wipe/jobs
GET /api/wipe/jobs/job-0123456789abcdef0123456789abcdef
```

The list is sorted newest first. A single missing job returns `404`. Job JSON contains the full identity, state timestamps, failure or verification result, evidence hash, and event chain described in [Evidence and certificates](evidence-and-certificates.md).

## Pause and abort

Both accept a job ID or raw device path:

```json
{
  "deviceId": "job-0123456789abcdef0123456789abcdef"
}
```

```http
POST /api/wipe/pause
POST /api/wipe/abort
```

Pause toggles the flag; the response does not distinguish pause from resume:

```json
{
  "message": "Wipe pause request received"
}
```

Abort acknowledges that the cancellation flag was set:

```json
{
  "message": "Wipe abort request received"
}
```

These operations return `500` if no active control entry exists. See [Pause and abort semantics](safety-model.md#pause-and-abort-semantics) before using them with firmware operations.

## Unmount

```http
POST /api/unmount
Content-Type: application/json
```

```json
{
  "device": "/dev/sdb"
}
```

Success:

```json
{
  "status": "Device unmounted successfully"
}
```

The backend re-reads topology and refuses system/live media. It reports `500` when discovery or any requested unmount fails.

## Certificates

List:

```http
GET /api/certificates
```

Issue or return an idempotent JSON certificate:

```http
POST /api/certificate
Content-Type: application/json
```

```json
{
  "jobId": "job-0123456789abcdef0123456789abcdef"
}
```

PDF form:

```http
POST /api/certificate?format=pdf
```

The request body is identical. PDF responses use `application/pdf` with an attachment disposition.

Certificate status behavior:

| Status | Meaning |
| --- | --- |
| `200 OK` | Valid certificate returned. |
| `404 Not Found` | Job does not exist. |
| `409 Conflict` | Job is not verified, evidence is invalid, or stored certificate disagrees with the job. |
| `500 Internal Server Error` | Key, signing, persistence, or PDF generation failed. |

## Persistent evidence export

List removable partitions that use FAT32, exFAT, or ext4:

```http
GET /api/evidence/destinations
```

Representative response:

```json
[
  {
    "drivePath": "/dev/sdc",
    "driveMajorMinor": "8:32",
    "devicePath": "/dev/sdc1",
    "deviceMajorMinor": "8:33",
    "mountPath": null,
    "model": "Evidence USB",
    "serial": "EXPORT-SERIAL",
    "transport": "usb",
    "filesystem": "vfat",
    "sizeBytes": "64021856256"
  }
]
```

The list excludes read-only devices, unsupported filesystems, internal non-removable drives, and any drive containing a protected system or `/run/archiso` mount. USB transport is accepted even when a bridge reports `RM=0`.

An unmounted destination must be mounted explicitly. Send back the complete object returned by discovery:

```http
POST /api/evidence/mount
Content-Type: application/json
```

```json
{
  "destination": {
    "drivePath": "/dev/sdc",
    "driveMajorMinor": "8:32",
    "devicePath": "/dev/sdc1",
    "deviceMajorMinor": "8:33",
    "mountPath": null,
    "model": "Evidence USB",
    "serial": "EXPORT-SERIAL",
    "transport": "usb",
    "filesystem": "vfat",
    "sizeBytes": "64021856256"
  }
}
```

The backend discovers the device again and requires an exact match. It refuses a device reserved by another storage operation, then mounts it below `/run/dzap-evidence` with `nodev,nosuid,noexec`. FAT and exFAT also receive explicit root ownership and `umask=022`. The response is the refreshed destination with a non-null `mountPath`.

Export a verified job by sending that refreshed destination:

```http
POST /api/evidence/export
Content-Type: application/json
```

```json
{
  "jobId": "job-0123456789abcdef0123456789abcdef",
  "destination": {
    "drivePath": "/dev/sdc",
    "driveMajorMinor": "8:32",
    "devicePath": "/dev/sdc1",
    "deviceMajorMinor": "8:33",
    "mountPath": "/run/dzap-evidence/8-33",
    "model": "Evidence USB",
    "serial": "EXPORT-SERIAL",
    "transport": "usb",
    "filesystem": "vfat",
    "sizeBytes": "64021856256"
  }
}
```

Success returns the bundle path, export timestamp, public-key fingerprint, destination identity, and `alreadyExisted`. Repeating the same export validates and returns the existing bundle. A changed or corrupted bundle is rejected rather than overwritten.

The bundle is created at `DZap-Evidence/<job-id>/` and contains `job.json`, `certificate.json`, `certificate.pdf`, `public-key.pem`, and `manifest.json`. The manifest includes an RSA signature over its metadata and file hashes. `409 Conflict` covers stale destination identity, active reservations, unverified jobs, invalid existing bundles, and unavailable media. Unknown jobs return `404`; malformed requests return `400`.

## WebSocket

The packaged dashboard derives the socket endpoint from its page origin. A dashboard opened at `http://127.0.0.1:8080/` therefore connects to:

```text
ws://127.0.0.1:8080/ws
```

The socket is broadcast-only. Clients do not send commands through it.

Overwrite progress resembles:

```json
{
  "jobId": "job-...",
  "deviceId": "/dev/sdb",
  "deviceModel": "Example Disk",
  "method": "overwrite_1_pass",
  "methodName": "1-Pass Overwrite",
  "status": "Pass 1/1",
  "progress": 42.5,
  "currentPass": 1,
  "totalPasses": 1,
  "speed": "120.32 MB/s",
  "eta": "480s",
  "sectorNumber": 425000000000
}
```

Terminal verified event:

```json
{
  "status": "verified",
  "jobId": "job-...",
  "deviceId": "/dev/sdb",
  "evidenceHash": "<64 hex characters>",
  "verification": {
    "strategy": "full_pattern_readback",
    "bytesChecked": 1000204886016,
    "readbackSha256": "<64 hex characters>",
    "expectedPattern": "0x00",
    "firmwareStatusSha256": null,
    "identityRevalidated": true
  }
}
```

Failure events contain `status: "failed"` and `error`. Clients should use `jobId` to refresh the complete authoritative record over HTTP after a terminal event.

Broadcast receivers that lag skip missed messages and continue with newer messages. The frontend reconnects after closure with exponential delays capped at ten seconds. Every successful connection reloads authoritative job records over HTTP, and terminal events reload their specific job. Session logs contain only WebSocket messages observed by that browser; they are not evidence records.

## Browser-origin policy

Allowed origins are loopback hostnames on development port 3000 and live port 8080:

```text
http://localhost:3000
http://127.0.0.1:3000
http://[::1]:3000
http://localhost:8080
http://127.0.0.1:8080
http://[::1]:8080
```

WebSocket requests with any other present `Origin` receive `403 Forbidden`. CORS allows GET, POST, and `Content-Type` for the listed origins.
