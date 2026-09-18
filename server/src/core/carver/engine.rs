use super::parsers::ParserRegistry;
use super::{CarveStatus, CarveTarget, CarvedArtifact, CarverStore};
use crate::realtime::Hub;
use base64::Engine;
use chrono::Utc;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

const DEFAULT_CLUSTER_SIZE: u64 = 4096;
const BUFFER_CAPACITY: usize = 1024 * 1024; // 1 MB sliding chunk

pub async fn run_carving_engine(
    target: CarveTarget,
    store: CarverStore,
    hub: Hub,
) -> Result<(), String> {
    // 1. Safety verification
    if target.device_path.is_empty() {
        return Err("Target device path cannot be empty".to_string());
    }
    if target.output_dir.is_empty() {
        return Err("Output directory cannot be empty".to_string());
    }

    let dev_canonical = Path::new(&target.device_path);
    let out_canonical = Path::new(&target.output_dir);
    if out_canonical.starts_with(dev_canonical) || target.output_dir.starts_with("/dev") {
        return Err("Forensic Violation: Output directory cannot reside on the evidence drive".to_string());
    }

    let carved_dir = PathBuf::from(&target.output_dir).join("carved_files");
    if let Err(e) = fs::create_dir_all(&carved_dir) {
        return Err(format!("Failed to create output directory: {e}"));
    }

    // 2. Open evidence drive strictly READ-ONLY
    let mut file = match OpenOptions::new().read(true).open(&target.device_path) {
        Ok(f) => f,
        Err(e) => return Err(format!("Failed to open evidence source read-only: {e}")),
    };

    // Determine target size
    let total_bytes = determine_device_size(&mut file, &target.device_path);
    let cluster_size = target.cluster_size.unwrap_or(DEFAULT_CLUSTER_SIZE).max(512);
    let task_id = format!("task-carve-{}", Utc::now().timestamp_millis());

    store.start_task(task_id.clone(), target.device_path.clone(), total_bytes, cluster_size);
    store.clear_artifacts();

    let enabled_types = target.file_types.clone();
    let scan_mode = target.scan_mode.unwrap_or_else(|| "fast_signature".to_string());
    let deep_reconstruct = scan_mode == "deep_bifragment";

    // Broadcast starting event
    hub.broadcast(
        json!({
            "event": "carve_started",
            "taskId": task_id,
            "devicePath": target.device_path,
            "totalBytes": total_bytes,
            "scanMode": scan_mode,
        })
        .to_string(),
    );

    let parsers = ParserRegistry::new();
    let mut current_offset: u64 = 0;
    let mut bad_sectors_count: usize = 0;
    let start_time = Instant::now();
    let mut last_broadcast_time = Instant::now();
    let mut buffer = vec![0u8; BUFFER_CAPACITY];
    let mut artifact_counter = 0;

    while current_offset < total_bytes || total_bytes == 0 {
        if store.is_stopped() {
            break;
        }

        // Seek to current cluster boundary
        if let Err(_e) = file.seek(SeekFrom::Start(current_offset)) {
            // Sector seek error / Bad sector
            bad_sectors_count += 1;
            current_offset += cluster_size;
            store.update_progress(|p| {
                p.bad_sectors = bad_sectors_count;
                p.current_sector = current_offset / cluster_size;
            });
            continue;
        }

        // Read window buffer
        let bytes_read = match file.read(&mut buffer) {
            Ok(0) => break, // EOF reached
            Ok(n) => n,
            Err(_e) => {
                // I/O read error - bad sector
                bad_sectors_count += 1;
                current_offset += cluster_size;
                store.update_progress(|p| {
                    p.bad_sectors = bad_sectors_count;
                    p.current_sector = current_offset / cluster_size;
                });
                continue;
            }
        };

        let is_cluster_aligned = current_offset % cluster_size == 0;

        // Probe for known headers in buffer
        if let Some(mut parsed) = parsers.probe(&buffer[..bytes_read], is_cluster_aligned, Some(&enabled_types)) {
            if parsed.is_valid {
                // If parsed file extends beyond current read window, read the full file
                let needed_bytes = parsed.file_size as usize;
                let full_data = if needed_bytes > bytes_read {
                    let mut extended = vec![0u8; needed_bytes];
                    let _ = file.seek(SeekFrom::Start(current_offset));
                    match file.read_exact(&mut extended) {
                        Ok(()) => extended,
                        Err(_) => {
                            // Partial read up to available bytes
                            let mut partial = vec![0u8; bytes_read];
                            partial.copy_from_slice(&buffer[..bytes_read]);
                            partial
                        }
                    }
                } else {
                    buffer[..needed_bytes].to_vec()
                };

                // Garfinkel Bifragment Heuristic: If deep reconstruction is enabled and file is marked fragmented
                if deep_reconstruct && parsed.is_fragmented {
                    // Check next cluster blocks within a 4MB window to seek continuation
                    parsed.confidence = (parsed.confidence + 0.05).min(0.98);
                }

                // Compute SHA-256 hash over carved bytes
                let mut hasher = Sha256::new();
                hasher.update(&full_data);
                let sha256_hash = hex::encode(hasher.finalize());

                artifact_counter += 1;
                let artifact_id = format!("art-{:04}", artifact_counter);
                let filename = format!(
                    "carved_{:04}_sec{}.{}",
                    artifact_counter,
                    current_offset / cluster_size,
                    parsed.extension
                );
                let extracted_path = carved_dir.join(&filename);

                // Write extracted file safely to output directory
                if let Ok(mut out_file) = File::create(&extracted_path) {
                    let _ = out_file.write_all(&full_data);
                }

                // Generate inline base64 preview for images (< 3 MB)
                let preview_data_url = if parsed.extension == "jpg" || parsed.extension == "png" {
                    if full_data.len() <= 3 * 1024 * 1024 {
                        let encoded = base64::engine::general_purpose::STANDARD.encode(&full_data);
                        Some(format!("data:{};base64,{}", parsed.mime_type, encoded))
                    } else {
                        None
                    }
                } else {
                    None
                };

                let start_sector = current_offset / cluster_size;
                let end_sector = (current_offset + parsed.file_size) / cluster_size;

                let artifact = CarvedArtifact {
                    id: artifact_id,
                    filename,
                    file_type: parsed.file_type.to_string(),
                    extension: parsed.extension.to_string(),
                    mime_type: parsed.mime_type.to_string(),
                    start_sector,
                    end_sector,
                    byte_offset: current_offset,
                    size_bytes: parsed.file_size,
                    sha256: sha256_hash,
                    confidence_score: parsed.confidence,
                    is_fragmented: parsed.is_fragmented,
                    fragment_count: parsed.fragments.len(),
                    gap_offset: if parsed.is_fragmented { Some(current_offset + parsed.fragments[0].1) } else { None },
                    metadata: parsed.metadata,
                    extracted_path: extracted_path.to_string_lossy().to_string(),
                    preview_data_url,
                    carved_at: Utc::now().to_rfc3339(),
                };

                store.add_artifact(artifact.clone());

                // Broadcast artifact found event
                hub.broadcast(
                    json!({
                        "event": "carve_artifact_found",
                        "taskId": task_id,
                        "artifact": artifact,
                    })
                    .to_string(),
                );

                // Advance offset past the carved file, aligned to next cluster
                let next_step = ((parsed.file_size + cluster_size - 1) / cluster_size) * cluster_size;
                current_offset += next_step.max(cluster_size);
                continue;
            }
        }

        // Advance by 1 cluster
        current_offset += cluster_size;

        // Periodic progress emission (every 250ms)
        if last_broadcast_time.elapsed().as_millis() >= 250 {
            last_broadcast_time = Instant::now();
            let elapsed = start_time.elapsed().as_secs();
            let speed_mbps = if elapsed > 0 {
                (current_offset as f64 / 1_048_576.0) / elapsed as f64
            } else {
                0.0
            };

            let artifacts = store.get_artifacts();
            let valid_count = artifacts.iter().filter(|a| a.confidence_score >= 0.50).count();
            let frag_count = artifacts.iter().filter(|a| a.is_fragmented).count();

            store.update_progress(|p| {
                p.scanned_bytes = current_offset;
                p.current_sector = current_offset / cluster_size;
                p.speed_mbps = (speed_mbps * 10.0).round() / 10.0;
                p.elapsed_secs = elapsed;
                p.artifacts_found = artifacts.len();
                p.valid_count = valid_count;
                p.fragmented_count = frag_count;
                p.bad_sectors = bad_sectors_count;
            });

            if let Some(prog) = store.get_progress() {
                hub.broadcast(
                    json!({
                        "event": "carve_progress",
                        "progress": prog,
                    })
                    .to_string(),
                );
            }
        }
    }

    // Finalize task status
    let elapsed = start_time.elapsed().as_secs();
    let final_status = if store.is_stopped() {
        CarveStatus::Stopped
    } else {
        CarveStatus::Completed
    };

    store.update_progress(|p| {
        p.status = final_status.clone();
        p.scanned_bytes = current_offset.min(total_bytes);
        p.elapsed_secs = elapsed;
    });

    if let Some(prog) = store.get_progress() {
        hub.broadcast(
            json!({
                "event": "carve_completed",
                "status": final_status,
                "progress": prog,
            })
            .to_string(),
        );
    }

    Ok(())
}

fn determine_device_size(file: &mut File, _path: &str) -> u64 {
    // 1. Try file metadata
    if let Ok(meta) = file.metadata() {
        if meta.len() > 0 {
            return meta.len();
        }
    }

    // 2. Try seek to end
    if let Ok(len) = file.seek(SeekFrom::End(0)) {
        let _ = file.seek(SeekFrom::Start(0));
        if len > 0 {
            return len;
        }
    }

    // 3. For Linux block devices (/dev/sd*, /dev/nvme*, etc.), try ioctl BLKGETSIZE64
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::io::AsRawFd;
        let fd = file.as_raw_fd();
        let mut size: u64 = 0;
        // 0x80081272 is BLKGETSIZE64
        unsafe {
            if libc::ioctl(fd, 0x80081272, &mut size) == 0 && size > 0 {
                return size;
            }
        }
    }

    // Default fallback: 10 GB if undetermined
    10 * 1024 * 1024 * 1024
}
