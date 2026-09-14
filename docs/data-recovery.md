# Data recovery

DZap uses an image-first recovery workflow. It assesses the source, binds a separate destination, creates a resumable sector image, and performs every later recovery attempt against that image. The original source is never mounted, repaired, decrypted, or used as a PhotoRec target.

## Safety boundary

The backend is the authority for source and destination identity. Browser state never authorizes storage access by itself.

- The running system and DZap boot medium are rejected before source probes run.
- Assessment and imaging open the source read-only.
- Imaging requires the source and all descendants to be unmounted and inactive.
- Source and destination whole-drive identities must still match immediately before imaging.
- The source and destination must be different physical drives.
- Both drives remain reserved until ddrescue stops and its final state is recorded.
- Image inspection and file recovery revalidate and reserve the destination that contains the image.
- Recovery tools write only beneath that job's directory on the destination.

DZap does not run filesystem repair or write a reconstructed partition table. Repair commands change metadata and belong on a disposable copy made from the preserved image.

## Stage 1: assessment

`POST /api/recovery/assess` freshly discovers one whole drive and records its identity. It performs these read-only checks:

| Check | Evidence | Meaning |
| --- | --- | --- |
| Device and system protection | Linux block topology and protected mount ancestry | Rejects missing paths and the running system/live medium. |
| Mount state | Source and descendant mountpoints | Requires quiescence before imaging. |
| Active topology | RAID, LVM, crypt, and device-mapper descendants | Requires writable logical mappings to be deactivated. |
| Storage signatures | Partition-table and filesystem signatures reported by `lsblk` | Finds recognizable structured storage and encryption. |
| Encryption | LUKS, BitLocker, and active crypt mappings | Signals that an operator secret will be required after imaging. |
| Media condition | SMART status, ATA error counters, NVMe warnings, and sample read failures | Recommends image-first handling for degraded or uncertain media. |
| Content state | Five evenly spaced 64 KiB samples | Separates structured, non-blank, likely blank, and unknown evidence. |

Sparse samples are not a complete surface scan. `likely_blank` means every completed sample contained only `0x00` or `0xff` and no structure was recognized. It does not prove that a secure wipe completed. `non_blank` does not prove that useful files remain.

## Stage 2: destination plan

DZap discovers ext4, exFAT, and FAT32 volumes on removable USB drives. It excludes the source drive and every drive containing the running system or live medium. A destination record binds both its whole-drive identity and selected volume path.

`POST /api/recovery/plan` reruns discovery and returns individual checks. A ready plan requires:

- The assessed source identity still matches.
- The source is unmounted and has no active logical descendants.
- The destination drive and volume identities still match.
- The destination is mounted read-write.
- Free capacity covers the source-sized image plus a 64 MiB reserve.
- The filesystem supports one file as large as the source. FAT32 is blocked above its single-file limit.
- Neither drive is reserved by another DZap operation.

Planning writes nothing. `POST /api/recovery/jobs` repeats the plan, reserves both drives, and repeats the identity and capacity checks while those reservations are held.

## Stage 3: resumable imaging

The imaging worker runs GNU ddrescue with a regular-file destination:

```text
ddrescue --no-scrape --retry-passes=3 --sparse SOURCE source.img source.map
```

`--sparse` avoids allocating blocks for zero-filled input ranges on filesystems that support sparse files. The map file classifies rescued, unreadable, and pending ranges. DZap parses it while imaging and persists those byte counts and the handled percentage.

Pause and stop requests send `SIGINT`, giving ddrescue time to flush its map. If it does not exit within ten seconds, DZap stops it and retains the latest map. A nonzero ddrescue exit becomes a paused job rather than discarding the attempt. Resume revalidates both drives and continues with the same image and map. Space already allocated to the partial image is credited during the resume capacity check.

An image is marked complete only when ddrescue exits successfully and the map contains no pending ranges. Unreadable ranges may remain; their byte count stays visible and becomes part of the job record.

## Stage 4: inspect the image

Once imaging completes, DZap can run two read-only inspections:

- `POST /api/recovery/jobs/{id}/analyze` runs `testdisk /list` against `source.img`. It saves full stdout/stderr to a log on the destination and binds the log SHA-256 into job evidence. This is analysis only; it never asks TestDisk to write a partition table.
- `GET /api/recovery/jobs/{id}/volumes` attaches `source.img` through a fresh read-only loop device with partition scanning. It returns stable choices such as `whole-disk` and `partition-1`; transient `/dev/loopN` names never become API selectors.

The loop device is detached after inspection. A failed kernel partition scan does not destroy the image: the TestDisk log and whole-image PhotoRec path remain available.

## Stage 5: encrypted images

LUKS and BitLocker volumes are opened through `cryptsetup` with `--readonly`. The selected loop volume is the encrypted input, and the decrypted mapper is used only for that recovery attempt.

The unlock secret:

- Is accepted only in the `POST .../recover` request.
- Is passed to cryptsetup through standard input, never a command-line argument.
- Is never placed in a job event, diagnostic, log, output path, or JSON record.
- Is zeroed from the backend-owned string after it is handed to cryptsetup.
- Must be provided again for another attempt or after a restart.

The mapper is closed before the loop image is detached. DZap cannot bypass encryption; an incorrect password or recovery key produces a failed attempt while preserving the image.

## Stage 6: recovery methods

### Filesystem copy

Filesystem copy is the preferred first attempt because it preserves readable names and directories. DZap supports read-only mounts for ext2/3/4, XFS, Btrfs, FAT, exFAT, and NTFS/NTFS3. It adds `nodev`, `nosuid`, and `noexec`; ext3/4 also use `noload`, and XFS uses `norecovery`.

Before copying, DZap walks the mounted image to total regular-file bytes and verifies destination capacity plus a 64 MiB reserve. It then:

1. Creates a new private attempt directory below the job directory.
2. Traverses entries in deterministic byte-name order.
3. Never follows symbolic links.
4. Skips symbolic links, sockets, devices, FIFOs, and other special entries.
5. Creates recovered directories as mode `0700` and files as mode `0600`.
6. Streams and hashes each copied regular file with SHA-256.
7. Syncs each output file before recording its manifest entry.

The JSON-lines manifest stores the display path, lossless hexadecimal path bytes, size, and SHA-256 for every copied file.

### PhotoRec carving

PhotoRec is the explicit fallback when filesystem metadata cannot be mounted or the filesystem copy did not find the needed files. DZap runs PhotoRec in scripted mode against the selected image volume. On ext-family filesystems it enables PhotoRec's ext2 allocation mode; other selections use a whole-volume signature scan.

Carving can recover content after names and directory metadata are gone, but filenames and folder structure are usually lost. DZap requires enough free capacity for the selected volume plus its reserve, keeps PhotoRec logs/session data inside the job directory, and hashes every regular carved file into a separate manifest after PhotoRec exits successfully.

A completed filesystem-copy job can start a later PhotoRec attempt. Each attempt gets a new output directory and manifest, so fallback work does not replace earlier recovered files.

## Persistent records and restart behavior

Each job writes records to both locations:

```text
/root/.config/DZap/recovery-jobs/recovery-<id>.json
DESTINATION/DZap-Recovery/recovery-<id>/job.json
```

The destination job directory also contains:

```text
source.img
source.map
ddrescue.log
testdisk-<attempt>.log
filesystem_copy-files-<attempt>/
filesystem_copy-<attempt>.manifest.jsonl
photorec-files-<attempt>.1/
photorec-<attempt>.manifest.jsonl
photorec-<attempt>.log
photorec-work-<attempt>/
```

Authorization, pause/resume, image completion, TestDisk analysis, extraction start, failure, cancellation, and recovery completion are hash-chained events. Immutable source/destination identities and artifact paths are included in every event hash. Completion evidence binds the method, output directory, file and byte counts, skipped-entry count, manifest path, and manifest SHA-256.

After a backend restart:

- An active ddrescue job becomes `paused` and can resume from `source.map`.
- An active filesystem or PhotoRec attempt becomes `failed`; its image and partial output remain, and another attempt may be started.
- Completed and cancelled records remain terminal evidence for that attempt. A completed image can still be reused for a different recovery method.

WebSocket events provide live display updates. The persisted job returned by `GET /api/recovery/jobs/{id}` remains authoritative.

## Capacity and interpretation limits

Image planning reserves space for the image, not a second full copy of every file. Filesystem copy measures readable regular files before writing. PhotoRec cannot know its final yield, so DZap requires free space equal to the selected volume plus 64 MiB before starting.

File hashes prove the bytes DZap wrote to the destination. They do not prove that a damaged source file is semantically intact, that unreadable sectors contained no other file, or that PhotoRec found every possible signature.

The automated QEMU suite exercises a real ext4 source, ddrescue imaging, TestDisk analysis, filesystem recovery, PhotoRec carving, LUKS unlock, secret non-persistence, destination-full blocking, disconnected-source blocking, and post-wipe blank classification on disposable virtual disks. Physical USB qualification remains necessary for controller-specific behavior and performance.
