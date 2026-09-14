# Data recovery

DZap's data-recovery workflow begins with a read-only assessment and an identity-bound image plan. The current implementation does not create an image, copy, repair, decrypt, reconstruct, or carve files yet. It gathers enough evidence to choose a safer recovery path without changing the source drive.

## Core rule

Recovery work must preserve the source. DZap opens content samples read-only, does not mount or repair filesystems during assessment, and tells the operator to place images and recovered files on a different drive. The source path is reserved during assessment so a wipe cannot start against it concurrently.

The running system and DZap boot medium are blocked before SMART, signature, or content probes run. Another operation holding the same device reservation also blocks assessment.

## Destination and image planning

After assessment, DZap discovers ext4, exFAT, and FAT32 volumes on removable USB drives. It excludes the complete source drive and every drive containing the running system or live medium. Each destination records the whole-drive identity plus the selected partition path and major/minor number.

Unmounted destinations can be mounted through DZap's controlled removable-media path. The backend re-detects both drives, compares both identities, proves they are physically separate, and reserves them for the mount operation. Mounting may update destination filesystem metadata; it never mounts the source.

`POST /api/recovery/plan` performs another fresh discovery and returns passed or blocked checks. A ready image plan requires an unmounted source with no active logical descendants, a mounted read-write destination, enough available space for every source byte plus a 64 MiB reserve, and a filesystem capable of holding the image. FAT32 is rejected when the source exceeds its single-file limit.

The plan calculates a `DZap-Recovery` output directory but creates nothing. It releases its operation checks when the response is returned. A future execution request must revalidate and reserve both identities atomically before starting `ddrescue`.

## Assessment checks

`POST /api/recovery/assess` freshly discovers the requested whole drive and returns its detected identity with these checks:

| Check | Evidence | Meaning |
| --- | --- | --- |
| Device and system protection | Linux block topology and protected mount ancestry | Rejects missing paths and avoids probing the running system or live medium. |
| Mount state | Device and descendant mountpoints | Warns when the source may change during recovery. |
| Active topology | RAID, LVM, encrypted, and device-mapper descendants | Warns until writable logical mappings are deactivated. |
| Storage signatures | `lsblk` filesystem and partition-table signatures on the source tree | Finds recognizable structured storage and feeds encryption detection. |
| Encryption | LUKS/BitLocker signatures and active `crypt` mappings | Requires valid unlock material and a read-only decrypted mapping. Encryption is not bypassed. |
| Media condition | SMART overall status, selected ATA counters, NVMe media errors/critical warnings, and sparse read failures | Recommends imaging first when damage indicators exist. |
| Content state | Five evenly spaced 64 KiB read-only samples plus detected signatures | Distinguishes structured data, unclassified non-blank bytes, a likely blank pattern, and insufficient evidence. |

ATA damage indicators currently include reallocated sectors, reported uncorrectable errors, pending sectors, and offline-uncorrectable sectors. NVMe checks include media errors and the controller critical-warning value.

## Interpretation limits

The assessment uses sparse samples, not a full surface read. Successful samples cannot prove that every sector is readable. A source containing only zeroes or `0xff` at all sampled locations, with no recognized storage signature, is classified as `likely_blank`. That result is consistent with blank or wiped media but does not prove that a secure wipe completed.

Likewise, non-blank bytes do not prove that files are recoverable. Random overwrite output, unknown encryption, damaged metadata, and useful file content can all appear non-blank. DZap therefore reports `non_blank` separately from `structured_data`.

SMART counters are warnings for recovery planning rather than a complete diagnosis. When SMART is unavailable, the media condition stays `unknown` even if sparse reads succeed.

## Decision states

- `ready`: recognized data, no detected encryption or damage, unmounted source, and all checks known.
- `caution`: one or more warnings or unknown results require operator review.
- `blocked`: the source is missing, protected system/live media, or reserved by another operation.

The assessment decision authorizes no write or recovery command. The subsequent image plan binds both device identities and applies destination checks, but also starts no command.

## Planned execution stages

1. Revalidate and reserve the planned source and destination together.
2. Prevent source automount for the duration of recovery.
3. Create a resumable `ddrescue` image and map file, especially for degraded or unknown media.
4. For encrypted storage, obtain operator-supplied unlock material and expose a read-only decrypted mapping without storing the secret.
5. Attempt filesystem-aware listing and copy from the image or read-only mapping.
6. Offer TestDisk reconstruction or PhotoRec-style carving only when metadata recovery is insufficient.
7. Persist recovery progress, source/destination identities, tool output, copied-file hashes, and failures without claiming that missing files never existed.

Automatic filesystem repair is excluded from the recovery source path. Repair tools can alter metadata and should run only against a disposable copy of an image through a separate explicit workflow.
