use chrono::{DateTime, Utc};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::certificate::{self, SignedCertificate};
use super::jobs::{WipeJob, WipeJobStatus};
use super::wiper;

const EVIDENCE_FORMAT_VERSION: u32 = 1;
const EVIDENCE_DIRECTORY: &str = "DZap-Evidence";
const JOB_FILE: &str = "job.json";
const CERTIFICATE_FILE: &str = "certificate.json";
const CERTIFICATE_PDF_FILE: &str = "certificate.pdf";
const PUBLIC_KEY_FILE: &str = "public-key.pem";
const MANIFEST_FILE: &str = "manifest.json";
const CONTROLLED_MOUNT_ROOT: &str = "/run/dzap-evidence";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportDestination {
    pub drive_path: String,
    pub drive_major_minor: String,
    pub device_path: String,
    pub device_major_minor: String,
    pub mount_path: Option<String>,
    pub model: String,
    pub serial: String,
    pub transport: String,
    pub filesystem: String,
    pub size_bytes: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestFile {
    pub name: String,
    pub size_bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceManifestData {
    pub evidence_format_version: u32,
    pub application_version: String,
    pub job_id: String,
    pub exported_at: DateTime<Utc>,
    pub key_fingerprint_sha256: String,
    pub qr_payload_file: String,
    pub files: Vec<ManifestFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceManifest {
    pub data: EvidenceManifestData,
    pub signature: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportResult {
    pub job_id: String,
    pub bundle_path: String,
    pub exported_at: DateTime<Utc>,
    pub key_fingerprint_sha256: String,
    pub already_existed: bool,
    pub destination: ExportDestination,
}

#[derive(Debug, Deserialize)]
struct LsblkOutput {
    blockdevices: Vec<LsblkDevice>,
}

#[derive(Debug, Deserialize)]
struct LsblkDevice {
    name: String,
    #[serde(rename = "type", default)]
    device_type: String,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    serial: Option<String>,
    #[serde(default)]
    size: Option<i64>,
    #[serde(default)]
    tran: Option<String>,
    #[serde(default)]
    rm: bool,
    #[serde(default)]
    ro: bool,
    #[serde(default)]
    mountpoints: Vec<Option<String>>,
    #[serde(default)]
    fstype: Option<String>,
    #[serde(rename = "maj:min", default)]
    major_minor: Option<String>,
    #[serde(default)]
    children: Vec<LsblkDevice>,
}

pub fn detect_export_destinations() -> Result<Vec<ExportDestination>, String> {
    let output = Command::new("lsblk")
        .args([
            "-J",
            "-b",
            "-o",
            "NAME,MODEL,SERIAL,SIZE,TYPE,TRAN,RM,RO,MOUNTPOINTS,FSTYPE,MAJ:MIN",
        ])
        .output()
        .map_err(|error| format!("lsblk command failed: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "lsblk command failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    export_destinations_from_lsblk(&output.stdout)
}

pub(crate) fn export_destinations_from_lsblk(
    stdout: &[u8],
) -> Result<Vec<ExportDestination>, String> {
    let output: LsblkOutput = serde_json::from_slice(stdout)
        .map_err(|error| format!("failed to parse lsblk JSON: {error}"))?;
    let mut destinations = Vec::new();

    for drive in &output.blockdevices {
        if drive.device_type != "disk" {
            continue;
        }
        let transport = normalized(&drive.tran);
        if !drive.rm && transport != "usb" {
            continue;
        }
        if drive.ro || tree_has_protected_mount(drive) {
            continue;
        }

        collect_export_destinations(drive, drive, &transport, &mut destinations);
    }

    destinations.sort_by(|left, right| {
        left.mount_path
            .cmp(&right.mount_path)
            .then_with(|| left.device_path.cmp(&right.device_path))
    });
    destinations.dedup();
    Ok(destinations)
}

fn collect_export_destinations(
    drive: &LsblkDevice,
    device: &LsblkDevice,
    transport: &str,
    destinations: &mut Vec<ExportDestination>,
) {
    let filesystem = normalized(&device.fstype);
    if !device.ro && supported_export_filesystem(&filesystem) {
        let mount_paths: Vec<_> = device
            .mountpoints
            .iter()
            .flatten()
            .map(|mount_path| mount_path.trim())
            .filter(|mount_path| !mount_path.is_empty())
            .collect();
        if mount_paths.is_empty() {
            destinations.push(destination_for(drive, device, transport, &filesystem, None));
        } else {
            for mount_path in mount_paths {
                if safe_export_mount(mount_path) {
                    destinations.push(destination_for(
                        drive,
                        device,
                        transport,
                        &filesystem,
                        Some(mount_path.to_string()),
                    ));
                }
            }
        }
    }

    for child in &device.children {
        collect_export_destinations(drive, child, transport, destinations);
    }
}

fn destination_for(
    drive: &LsblkDevice,
    device: &LsblkDevice,
    transport: &str,
    filesystem: &str,
    mount_path: Option<String>,
) -> ExportDestination {
    ExportDestination {
        drive_path: device_path(&drive.name),
        drive_major_minor: normalized(&drive.major_minor),
        device_path: device_path(&device.name),
        device_major_minor: normalized(&device.major_minor),
        mount_path,
        model: normalized(&drive.model),
        serial: normalized(&drive.serial),
        transport: transport.to_string(),
        filesystem: filesystem.to_string(),
        size_bytes: device.size.unwrap_or(0).to_string(),
    }
}

fn tree_has_protected_mount(device: &LsblkDevice) -> bool {
    device
        .mountpoints
        .iter()
        .flatten()
        .any(|mount_path| protected_mount(mount_path.trim()))
        || device.children.iter().any(tree_has_protected_mount)
}

fn safe_export_mount(mount_path: &str) -> bool {
    !mount_path.is_empty() && Path::new(mount_path).is_absolute() && !protected_mount(mount_path)
}

fn supported_export_filesystem(filesystem: &str) -> bool {
    matches!(filesystem, "vfat" | "exfat" | "ext4")
}

fn protected_mount(mount_path: &str) -> bool {
    matches!(mount_path, "/" | "/boot" | "/boot/efi" | "/usr" | "/var")
        || mount_path == "/run/archiso"
        || mount_path.starts_with("/run/archiso/")
}

fn normalized(value: &Option<String>) -> String {
    value.as_deref().unwrap_or_default().trim().to_string()
}

fn device_path(name: &str) -> String {
    if name.starts_with("/dev/") {
        name.to_string()
    } else {
        format!("/dev/{name}")
    }
}

pub fn export_evidence(
    job: &WipeJob,
    certificate: &SignedCertificate,
    expected_destination: &ExportDestination,
) -> Result<ExportResult, String> {
    let destination = detect_export_destinations()?
        .into_iter()
        .find(|candidate| candidate == expected_destination)
        .ok_or_else(|| {
            "export destination changed, disappeared, became read-only, or is no longer eligible"
                .to_string()
        })?;

    if wiper::device_is_reserved(&destination.drive_path)? {
        return Err(format!(
            "export destination {} is reserved by an active wipe or verification",
            destination.drive_path
        ));
    }

    let mount_path = destination
        .mount_path
        .as_deref()
        .ok_or_else(|| "export destination must be mounted before writing evidence".to_string())?;
    let mount_path = PathBuf::from(mount_path);
    export_bundle_to_mount(job, certificate, &destination, &mount_path)
}

pub fn mount_export_destination(
    expected_destination: &ExportDestination,
) -> Result<ExportDestination, String> {
    let destination = detect_export_destinations()?
        .into_iter()
        .find(|candidate| candidate == expected_destination)
        .ok_or_else(|| {
            "export destination changed, disappeared, became read-only, or is no longer eligible"
                .to_string()
        })?;
    if destination.mount_path.is_some() {
        return Ok(destination);
    }
    if wiper::device_is_reserved(&destination.drive_path)? {
        return Err(format!(
            "export destination {} is reserved by an active wipe or verification",
            destination.drive_path
        ));
    }

    let mount_root = Path::new(CONTROLLED_MOUNT_ROOT);
    std::fs::create_dir_all(mount_root).map_err(|error| {
        format!(
            "failed to create controlled mount root {}: {error}",
            mount_root.display()
        )
    })?;
    set_private_directory_permissions(mount_root);
    let mount_directory = mount_root.join(mount_directory_name(&destination.device_major_minor)?);
    prepare_mount_directory(&mount_directory)?;
    set_private_directory_permissions(&mount_directory);

    let options = if matches!(destination.filesystem.as_str(), "vfat" | "exfat") {
        "nodev,nosuid,noexec,uid=0,gid=0,umask=022"
    } else {
        "nodev,nosuid,noexec"
    };
    let output = Command::new("mount")
        .args(["-o", options])
        .arg(&destination.device_path)
        .arg(&mount_directory)
        .output()
        .map_err(|error| {
            let _ = std::fs::remove_dir(&mount_directory);
            format!("mount command failed: {error}")
        })?;
    if !output.status.success() {
        let _ = std::fs::remove_dir(&mount_directory);
        return Err(format!(
            "failed to mount {}: {}",
            destination.device_path,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    let mounted = detect_export_destinations()?.into_iter().find(|candidate| {
        candidate.drive_path == destination.drive_path
            && candidate.drive_major_minor == destination.drive_major_minor
            && candidate.device_path == destination.device_path
            && candidate.device_major_minor == destination.device_major_minor
            && candidate.mount_path.as_deref() == mount_directory.to_str()
    });
    match mounted {
        Some(mounted) => Ok(mounted),
        None => {
            let _ = Command::new("umount").arg(&mount_directory).output();
            let _ = std::fs::remove_dir(&mount_directory);
            Err("mounted export destination could not be revalidated".to_string())
        }
    }
}

fn mount_directory_name(major_minor: &str) -> Result<String, String> {
    if major_minor.is_empty()
        || !major_minor
            .chars()
            .all(|character| character.is_ascii_digit() || character == ':')
    {
        return Err("export destination has an invalid major/minor identity".to_string());
    }
    Ok(major_minor.replace(':', "-"))
}

fn prepare_mount_directory(path: &Path) -> Result<(), String> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => {
            if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
                return Err(format!(
                    "controlled export mount point {} is not a directory",
                    path.display()
                ));
            }
            let mut entries = std::fs::read_dir(path)
                .map_err(|error| format!("failed to inspect {}: {error}", path.display()))?;
            if entries.next().is_some() {
                return Err(format!(
                    "controlled export mount point {} is not empty",
                    path.display()
                ));
            }
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => std::fs::create_dir(path)
            .map_err(|error| {
                format!(
                    "failed to create export mount point {}: {error}",
                    path.display()
                )
            }),
        Err(error) => Err(format!("failed to inspect {}: {error}", path.display())),
    }
}

pub(crate) fn export_bundle_to_mount(
    job: &WipeJob,
    certificate: &SignedCertificate,
    destination: &ExportDestination,
    mount_path: &Path,
) -> Result<ExportResult, String> {
    validate_export_inputs(job, certificate, destination, mount_path)?;

    let files = bundle_files(job, certificate)?;
    let evidence_root = mount_path.join(EVIDENCE_DIRECTORY);
    std::fs::create_dir_all(&evidence_root).map_err(|error| {
        format!(
            "failed to create evidence directory {}: {error}",
            evidence_root.display()
        )
    })?;
    set_directory_permissions(&evidence_root);
    sync_directory(mount_path)?;

    let final_directory = evidence_root.join(&job.id);
    if final_directory.exists() {
        let manifest = validate_existing_bundle(&final_directory, job, certificate, &files)?;
        return Ok(export_result(
            job,
            destination,
            &final_directory,
            &manifest,
            true,
        ));
    }

    let temporary_directory = evidence_root.join(temporary_name(&job.id));
    std::fs::create_dir(&temporary_directory).map_err(|error| {
        format!(
            "failed to create temporary evidence bundle {}: {error}",
            temporary_directory.display()
        )
    })?;
    set_directory_permissions(&temporary_directory);

    for (name, contents) in &files {
        write_synced_file(&temporary_directory.join(name), contents)?;
    }

    let manifest = manifest_for(job, certificate, &files)?;
    let manifest_bytes = pretty_json(&manifest, "evidence manifest")?;
    write_synced_file(&temporary_directory.join(MANIFEST_FILE), &manifest_bytes)?;
    sync_directory(&temporary_directory)?;

    std::fs::rename(&temporary_directory, &final_directory).map_err(|error| {
        format!(
            "failed to publish evidence bundle {}: {error}; temporary data remains at {}",
            final_directory.display(),
            temporary_directory.display()
        )
    })?;
    sync_directory(&evidence_root)?;

    let validated = validate_bundle(&final_directory)?;
    if validated != manifest {
        return Err(format!(
            "evidence bundle readback disagreed with the written manifest at {}",
            final_directory.display()
        ));
    }

    Ok(export_result(
        job,
        destination,
        &final_directory,
        &validated,
        false,
    ))
}

fn validate_export_inputs(
    job: &WipeJob,
    certificate: &SignedCertificate,
    destination: &ExportDestination,
    mount_path: &Path,
) -> Result<(), String> {
    if job.status != WipeJobStatus::Verified || !job.verify_evidence() {
        return Err("evidence export requires a verified wipe job".to_string());
    }
    if !certificate.verify_signature() || !certificate.matches_job(job) {
        return Err(
            "evidence export requires a valid certificate bound to the wipe job".to_string(),
        );
    }
    if !super::jobs::valid_job_id(&job.id) {
        return Err("wipe job identifier is not safe for evidence export".to_string());
    }
    if !mount_path.is_absolute() || !mount_path.is_dir() {
        return Err(format!(
            "export mount {} is not an available absolute directory",
            mount_path.display()
        ));
    }
    let approved_mount = destination
        .mount_path
        .as_deref()
        .ok_or_else(|| "export destination is not mounted".to_string())?;
    if mount_path != Path::new(approved_mount) {
        return Err("export mount does not match the approved destination".to_string());
    }
    if protected_mount(approved_mount) {
        return Err("the running system or DZap boot media cannot receive exports".to_string());
    }
    if destination.drive_path == job.device_path
        || (!job.identity.major_minor.is_empty()
            && destination.drive_major_minor == job.identity.major_minor)
    {
        return Err("wipe target cannot also be the evidence export destination".to_string());
    }
    Ok(())
}

fn bundle_files(
    job: &WipeJob,
    certificate: &SignedCertificate,
) -> Result<BTreeMap<String, Vec<u8>>, String> {
    let mut files = BTreeMap::new();
    files.insert(JOB_FILE.to_string(), pretty_json(job, "wipe job")?);
    files.insert(
        CERTIFICATE_FILE.to_string(),
        pretty_json(certificate, "certificate")?,
    );
    files.insert(
        CERTIFICATE_PDF_FILE.to_string(),
        certificate.generate_pdf()?,
    );
    files.insert(
        PUBLIC_KEY_FILE.to_string(),
        certificate.public_key.as_bytes().to_vec(),
    );
    Ok(files)
}

fn pretty_json(value: &impl Serialize, description: &str) -> Result<Vec<u8>, String> {
    let mut encoded = serde_json::to_vec_pretty(value)
        .map_err(|error| format!("failed to encode {description}: {error}"))?;
    encoded.push(b'\n');
    Ok(encoded)
}

fn manifest_for(
    job: &WipeJob,
    certificate: &SignedCertificate,
    files: &BTreeMap<String, Vec<u8>>,
) -> Result<EvidenceManifest, String> {
    let data = EvidenceManifestData {
        evidence_format_version: EVIDENCE_FORMAT_VERSION,
        application_version: env!("CARGO_PKG_VERSION").to_string(),
        job_id: job.id.clone(),
        exported_at: Utc::now(),
        key_fingerprint_sha256: sha256(certificate.public_key.as_bytes()),
        qr_payload_file: CERTIFICATE_FILE.to_string(),
        files: files
            .iter()
            .map(|(name, contents)| ManifestFile {
                name: name.clone(),
                size_bytes: contents.len() as u64,
                sha256: sha256(contents),
            })
            .collect(),
    };
    let payload = serde_json::to_vec(&data)
        .map_err(|error| format!("failed to encode evidence manifest payload: {error}"))?;
    Ok(EvidenceManifest {
        data,
        signature: certificate::sign_payload(&payload)?,
    })
}

pub fn validate_bundle(directory: &Path) -> Result<EvidenceManifest, String> {
    let manifest_path = directory.join(MANIFEST_FILE);
    let manifest_bytes = std::fs::read(&manifest_path)
        .map_err(|error| format!("failed to read {}: {error}", manifest_path.display()))?;
    let manifest: EvidenceManifest = serde_json::from_slice(&manifest_bytes)
        .map_err(|error| format!("failed to parse {}: {error}", manifest_path.display()))?;

    if manifest.data.evidence_format_version != EVIDENCE_FORMAT_VERSION {
        return Err(format!(
            "unsupported evidence format version {}",
            manifest.data.evidence_format_version
        ));
    }
    if !super::jobs::valid_job_id(&manifest.data.job_id) {
        return Err("evidence manifest contains an invalid job identifier".to_string());
    }
    if manifest.data.qr_payload_file != CERTIFICATE_FILE {
        return Err("evidence manifest identifies an unexpected QR payload".to_string());
    }

    let required: BTreeSet<&str> = [
        JOB_FILE,
        CERTIFICATE_FILE,
        CERTIFICATE_PDF_FILE,
        PUBLIC_KEY_FILE,
    ]
    .into_iter()
    .collect();
    let listed: BTreeSet<&str> = manifest
        .data
        .files
        .iter()
        .map(|file| file.name.as_str())
        .collect();
    if listed != required || manifest.data.files.len() != required.len() {
        return Err("evidence manifest does not contain the exact required file set".to_string());
    }

    let mut directory_files = BTreeSet::new();
    for entry in std::fs::read_dir(directory)
        .map_err(|error| format!("failed to read {}: {error}", directory.display()))?
    {
        let entry = entry.map_err(|error| format!("failed to read evidence entry: {error}"))?;
        if !entry
            .file_type()
            .map_err(|error| format!("failed to inspect {}: {error}", entry.path().display()))?
            .is_file()
        {
            return Err(format!(
                "unexpected non-file entry in evidence bundle: {}",
                entry.path().display()
            ));
        }
        directory_files.insert(entry.file_name().to_string_lossy().into_owned());
    }
    let mut expected_directory_files: BTreeSet<String> =
        required.iter().map(|name| (*name).to_string()).collect();
    expected_directory_files.insert(MANIFEST_FILE.to_string());
    if directory_files != expected_directory_files {
        return Err("evidence bundle contains an unexpected or missing file".to_string());
    }

    for file in &manifest.data.files {
        let path = directory.join(&file.name);
        let contents = std::fs::read(&path)
            .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
        if contents.len() as u64 != file.size_bytes || sha256(&contents) != file.sha256 {
            return Err(format!(
                "evidence hash validation failed for {}",
                path.display()
            ));
        }
    }

    let job_path = directory.join(JOB_FILE);
    let job: WipeJob = serde_json::from_slice(
        &std::fs::read(&job_path)
            .map_err(|error| format!("failed to read {}: {error}", job_path.display()))?,
    )
    .map_err(|error| format!("failed to parse {}: {error}", job_path.display()))?;
    if job.id != manifest.data.job_id
        || job.status != WipeJobStatus::Verified
        || !job.verify_evidence()
    {
        return Err("exported wipe job evidence is invalid".to_string());
    }

    let certificate_path = directory.join(CERTIFICATE_FILE);
    let certificate: SignedCertificate = serde_json::from_slice(
        &std::fs::read(&certificate_path)
            .map_err(|error| format!("failed to read {}: {error}", certificate_path.display()))?,
    )
    .map_err(|error| format!("failed to parse {}: {error}", certificate_path.display()))?;
    if !certificate.verify_signature() || !certificate.matches_job(&job) {
        return Err("exported certificate is invalid or disagrees with the wipe job".to_string());
    }
    let manifest_payload = serde_json::to_vec(&manifest.data)
        .map_err(|error| format!("failed to encode evidence manifest payload: {error}"))?;
    if !certificate::verify_payload_signature(
        &certificate.public_key,
        &manifest_payload,
        &manifest.signature,
    ) {
        return Err("evidence manifest signature validation failed".to_string());
    }

    let public_key_path = directory.join(PUBLIC_KEY_FILE);
    let public_key = std::fs::read(&public_key_path)
        .map_err(|error| format!("failed to read {}: {error}", public_key_path.display()))?;
    if public_key != certificate.public_key.as_bytes()
        || manifest.data.key_fingerprint_sha256 != sha256(&public_key)
    {
        return Err("exported public key does not match the signed certificate".to_string());
    }

    Ok(manifest)
}

fn validate_existing_bundle(
    directory: &Path,
    job: &WipeJob,
    certificate: &SignedCertificate,
    expected_files: &BTreeMap<String, Vec<u8>>,
) -> Result<EvidenceManifest, String> {
    let manifest = validate_bundle(directory)?;
    if manifest.data.job_id != job.id {
        return Err("existing evidence bundle belongs to another wipe job".to_string());
    }
    for (name, expected) in expected_files {
        let existing = std::fs::read(directory.join(name)).map_err(|error| {
            format!(
                "failed to read existing evidence file {}: {error}",
                directory.join(name).display()
            )
        })?;
        if &existing != expected {
            return Err(format!(
                "existing evidence bundle for {} disagrees with server-owned evidence",
                job.id
            ));
        }
    }
    if !certificate.matches_job(job) {
        return Err("certificate no longer matches the wipe job".to_string());
    }
    Ok(manifest)
}

fn write_synced_file(path: &Path, contents: &[u8]) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(|error| format!("failed to create {}: {error}", path.display()))?;
    file.write_all(contents)
        .map_err(|error| format!("failed to write {}: {error}", path.display()))?;
    set_file_permissions(path);
    file.sync_all()
        .map_err(|error| format!("failed to sync {}: {error}", path.display()))
}

fn sync_directory(path: &Path) -> Result<(), String> {
    match File::open(path).and_then(|directory| directory.sync_all()) {
        Ok(()) => Ok(()),
        Err(error)
            if error.raw_os_error().is_some_and(|code| {
                code == libc::EINVAL || code == libc::ENOTSUP || code == libc::EOPNOTSUPP
            }) =>
        {
            Ok(())
        }
        Err(error) => Err(format!("failed to sync {}: {error}", path.display())),
    }
}

fn temporary_name(job_id: &str) -> String {
    let mut random = [0_u8; 8];
    rand::thread_rng().fill_bytes(&mut random);
    format!(".{job_id}.tmp-{}", hex::encode(random))
}

fn sha256(contents: &[u8]) -> String {
    hex::encode(Sha256::digest(contents))
}

fn export_result(
    job: &WipeJob,
    destination: &ExportDestination,
    directory: &Path,
    manifest: &EvidenceManifest,
    already_existed: bool,
) -> ExportResult {
    ExportResult {
        job_id: job.id.clone(),
        bundle_path: directory.display().to_string(),
        exported_at: manifest.data.exported_at,
        key_fingerprint_sha256: manifest.data.key_fingerprint_sha256.clone(),
        already_existed,
        destination: destination.clone(),
    }
}

#[cfg(unix)]
fn set_directory_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755));
}

#[cfg(not(unix))]
fn set_directory_permissions(_path: &Path) {}

#[cfg(unix)]
fn set_file_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o644));
}

#[cfg(unix)]
fn set_private_directory_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700));
}

#[cfg(not(unix))]
fn set_private_directory_permissions(_path: &Path) {}

#[cfg(not(unix))]
fn set_file_permissions(_path: &Path) {}
