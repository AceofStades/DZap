use crate::core::certificate::*;
use crate::core::drives::DeviceIdentity;
use crate::core::jobs::JobStore;
use crate::core::preflight::{PreflightDecision, WipePlan};
use crate::core::verification::{VerificationResult, VerificationStrategy};
use chrono::{TimeZone, Utc};

fn completed_job(model: &str, serial: &str, method: &str) -> crate::core::jobs::WipeJob {
    let store = JobStore::in_memory();
    let created = store
        .create(&WipePlan {
            decision: PreflightDecision::Ready,
            device_path: "/dev/test".to_string(),
            device_model: model.to_string(),
            device_type: "HDD".to_string(),
            method: method.to_string(),
            identity: Some(DeviceIdentity {
                model: model.to_string(),
                serial: serial.to_string(),
                wwn: "0x1234".to_string(),
                size_bytes: "4096".to_string(),
                transport: "sata".to_string(),
                major_minor: "8:16".to_string(),
            }),
            checks: Vec::new(),
        })
        .unwrap();
    store.begin_verification(&created.id).unwrap();
    store
        .complete_verification(&created.id, verification(method))
        .unwrap()
}

fn verification(method: &str) -> VerificationResult {
    let mut result = VerificationResult {
        strategy: match method {
            "sata_secure_erase" => VerificationStrategy::AtaSecurityStatusAndSamples,
            "nvme_format" => VerificationStrategy::NvmeFormatStatusAndSamples,
            _ => VerificationStrategy::FullPatternReadback,
        },
        bytes_checked: 4096,
        readback_sha256: "a".repeat(64),
        expected_pattern: match method {
            "overwrite_1_pass" => Some("0x00".to_string()),
            "overwrite_2_pass" => Some("0xAA".to_string()),
            "overwrite_3_pass" => Some("0x55".to_string()),
            _ => None,
        },
        firmware_status_sha256: None,
        identity_revalidated: true,
    };
    if matches!(method, "sata_secure_erase" | "nvme_format") {
        result.firmware_status_sha256 = Some("b".repeat(64));
    }
    result
}

#[test]
fn hash_covers_the_complete_certificate_record() {
    let data = CertificateData {
        job_id: "job-test".to_string(),
        device_path: "/dev/test".to_string(),
        device_model: "Samsung SSD 980".to_string(),
        device_serial: "S123456".to_string(),
        device_wwn: "0x1234".to_string(),
        device_size_bytes: "4096".to_string(),
        device_transport: "nvme".to_string(),
        device_major_minor: "259:0".to_string(),
        device_type: "NVMe SSD".to_string(),
        wipe_method: "overwrite_1_pass".to_string(),
        started_at: Utc.with_ymd_and_hms(2025, 1, 15, 10, 0, 0).unwrap(),
        completed_at: Utc.with_ymd_and_hms(2025, 1, 15, 10, 29, 0).unwrap(),
        timestamp: Utc.with_ymd_and_hms(2025, 1, 15, 10, 30, 0).unwrap(),
        verification: verification("overwrite_1_pass"),
        evidence_hash: "abc123".to_string(),
    };
    let hash = hash_certificate_data(&data);
    assert_eq!(hash.len(), 32);

    use sha2::{Digest, Sha256};
    let expected = Sha256::digest(
        br#"{"jobId":"job-test","devicePath":"/dev/test","deviceModel":"Samsung SSD 980","deviceSerial":"S123456","deviceWwn":"0x1234","deviceSizeBytes":"4096","deviceTransport":"nvme","deviceMajorMinor":"259:0","deviceType":"NVMe SSD","wipeMethod":"overwrite_1_pass","startedAt":"2025-01-15T10:00:00Z","completedAt":"2025-01-15T10:29:00Z","timestamp":"2025-01-15T10:30:00Z","verification":{"strategy":"full_pattern_readback","bytesChecked":4096,"readbackSha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","expectedPattern":"0x00","firmwareStatusSha256":null,"identityRevalidated":true},"evidenceHash":"abc123"}"#,
    );
    assert_eq!(hash, expected.to_vec());
}

#[test]
fn certificate_json_contains_job_owned_evidence() {
    init_for_tests();
    let job = completed_job("Model X", "SN42", "nvme_format");
    let cert = generate_certificate_for_job(&job).unwrap();

    let v = serde_json::to_value(&cert).unwrap();
    // Exact top-level keys, and QR data must NOT leak into JSON (Go: json:"-").
    let obj = v.as_object().unwrap();
    let keys: std::collections::HashSet<&str> = obj.keys().map(String::as_str).collect();
    assert_eq!(
        keys,
        ["data", "signature", "publicKey"].into_iter().collect()
    );
    let data = &v["data"];
    for key in [
        "jobId",
        "devicePath",
        "deviceModel",
        "deviceSerial",
        "deviceWwn",
        "deviceSizeBytes",
        "deviceTransport",
        "deviceMajorMinor",
        "deviceType",
        "wipeMethod",
        "startedAt",
        "completedAt",
        "timestamp",
        "verification",
        "evidenceHash",
    ] {
        assert!(data.get(key).is_some(), "missing key {key} in {data}");
    }
    assert_eq!(data["jobId"], serde_json::json!(job.id));
    assert_eq!(data["deviceModel"], serde_json::json!("Model X"));
    assert_eq!(data["deviceSerial"], serde_json::json!("SN42"));
    assert_eq!(data["wipeMethod"], serde_json::json!("nvme_format"));
    assert_eq!(data["evidenceHash"], serde_json::json!(job.evidence_hash));

    // RSA-2048 signature => 256 bytes => 512 hex chars.
    let sig = v["signature"].as_str().unwrap();
    assert_eq!(sig.len(), 512);
    assert!(sig.chars().all(|c| c.is_ascii_hexdigit()));

    // Public key is a PEM-encoded PKIX key.
    let pem = v["publicKey"].as_str().unwrap();
    assert!(pem.starts_with("-----BEGIN PUBLIC KEY-----"));
    assert!(pem.trim_end().ends_with("-----END PUBLIC KEY-----"));

    // A QR code was attached for the PDF (but stays out of JSON).
    assert!(cert.qr_code.is_some());
}

#[test]
fn signature_verifies_against_public_key() {
    init_for_tests();
    let cert = generate_certificate_for_job(&completed_job("M", "S", "overwrite_2_pass")).unwrap();

    let hash = hash_certificate_data(&cert.data);
    let sig_bytes = hex::decode(&cert.signature).unwrap();

    // Parse the PEM public key from the certificate itself and verify the
    // way Go's rsa.VerifyPKCS1v15 would: digest + DigestInfo prefix.
    use rsa::Pkcs1v15Sign;
    use rsa::pkcs8::DecodePublicKey;
    let pub_key = rsa::RsaPublicKey::from_public_key_pem(&cert.public_key).unwrap();
    pub_key
        .verify(Pkcs1v15Sign::new::<sha2::Sha256>(), &hash, &sig_bytes)
        .expect("signature must verify against the embedded public key");

    // And it must NOT verify for tampered data.
    let mut tampered = cert.data.clone();
    tampered.device_serial = "TAMPERED".to_string();
    let bad_hash = hash_certificate_data(&tampered);
    assert!(
        pub_key
            .verify(Pkcs1v15Sign::new::<sha2::Sha256>(), &bad_hash, &sig_bytes)
            .is_err()
    );
}

#[test]
fn certificate_requires_terminal_untampered_job_evidence() {
    init_for_tests();
    let job = completed_job("M", "S", "overwrite_1_pass");
    let cert = generate_certificate_for_job(&job).unwrap();
    assert!(cert.matches_job(&job));

    let mut running = job.clone();
    running.status = crate::core::jobs::WipeJobStatus::Running;
    let error = match generate_certificate_for_job(&running) {
        Ok(_) => panic!("running job received a certificate"),
        Err(error) => error,
    };
    assert!(error.contains("verified"));

    let mut changed = job;
    changed.method = "overwrite_3_pass".to_string();
    assert!(!changed.verify_evidence());
    assert!(!cert.matches_job(&changed));
}

#[test]
fn pdf_is_well_formed_and_contains_certificate_content() {
    init_for_tests();
    let cert =
        generate_certificate_for_job(&completed_job("PdfModel", "PdfSerial", "sata_secure_erase"))
            .unwrap();
    let pdf = cert.generate_pdf().unwrap();

    // Structural checks.
    assert!(pdf.starts_with(b"%PDF-1.4\n"));
    assert!(pdf.windows(5).any(|w| w == b"%%EOF"));
    let text = String::from_utf8_lossy(&pdf);
    for needle in [
        "/Type /Catalog",
        "/Type /Pages",
        "/BaseFont /Helvetica-Bold",
        "/BaseFont /Courier",
        "xref",
        "trailer",
        "startxref",
        "Data Destruction Certificate",
        "Device Model:",
        "PdfModel",
        "Device Serial:",
        "PdfSerial",
        "Evidence Hash:",
        "Verification:",
        "Readback SHA-256:",
        "Identity Revalidated:",
        "Digital Signature",
        "Public Key:",
        "Scan to Verify",
        &cert.signature[..32], // signature text appears in the content stream
    ] {
        assert!(text.contains(needle), "PDF missing {needle:?}");
    }

    // The QR code should produce a non-trivial number of dark module rects.
    let dark_rects = text.matches(" re f").count();
    assert!(dark_rects > 100, "expected QR modules, found {dark_rects}");

    // Every xref offset must point at its object header.
    let xref_pos = text.find("xref\n").unwrap();
    let offsets: Vec<usize> = text[xref_pos..]
        .lines()
        .skip(2) // "xref" and "0 N"
        .filter(|l| l.ends_with(" n "))
        .map(|l| l[..10].parse().unwrap())
        .collect();
    assert!(!offsets.is_empty());
    for (idx, off) in offsets.iter().enumerate() {
        let at = &pdf[*off..];
        let header = format!("{} 0 obj", idx + 1);
        assert!(
            at.starts_with(header.as_bytes()),
            "xref entry {} points to wrong offset",
            idx + 1
        );
    }
}

#[test]
fn generated_certificate_round_trips_through_list_format() {
    // The certificates endpoint deserializes stored JSON back into
    // SignedCertificate — make sure our Serialize/Deserialize are symmetric.
    init_for_tests();
    let cert =
        generate_certificate_for_job(&completed_job("RT", "SN", "overwrite_3_pass")).unwrap();
    let json = serde_json::to_string(&cert).unwrap();
    let back: SignedCertificate = serde_json::from_str(&json).unwrap();
    assert_eq!(back.data.device_model, "RT");
    assert_eq!(back.signature, cert.signature);
    assert_eq!(back.public_key, cert.public_key);
}

#[test]
fn certificate_store_is_idempotent_and_rejects_tampering() {
    init_for_tests();
    let job = completed_job("Stored", "STORE-SN", "overwrite_1_pass");
    let first = generate_certificate_for_job(&job).unwrap();
    let replacement = generate_certificate_for_job(&job).unwrap();
    let directory = std::env::temp_dir().join(format!(
        "dzap-test-cert-store-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let store = CertificateStore::persistent(directory.clone()).unwrap();
    let stored = store.save_if_absent(first.clone()).unwrap();
    let unchanged = store.save_if_absent(replacement).unwrap();
    assert_eq!(unchanged.signature, stored.signature);

    let reloaded = CertificateStore::persistent(directory.clone()).unwrap();
    let loaded = reloaded.get(&job.id).unwrap().unwrap();
    assert_eq!(loaded.signature, first.signature);
    assert!(loaded.verify_signature());
    assert!(loaded.matches_job(&job));

    let path = directory.join(format!("{}.json", job.id));
    let json = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, json.replace("STORE-SN", "TAMPERED")).unwrap();
    let error = match CertificateStore::persistent(directory.clone()) {
        Ok(_) => panic!("tampered certificate was accepted"),
        Err(error) => error,
    };
    assert!(
        error.contains("certificate verification failed"),
        "unexpected: {error}"
    );
    std::fs::remove_dir_all(directory).ok();
}

#[test]
fn private_key_is_persisted_with_restricted_permissions_and_reloaded() {
    use rsa::traits::PublicKeyParts;
    use std::os::unix::fs::PermissionsExt;

    let dir = std::env::temp_dir().join(format!("dzap-test-key-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("private.pem");

    let generated = load_or_generate_private_key_at(&path).unwrap();
    assert!(path.exists());
    assert_eq!(
        std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );

    let reloaded = load_or_generate_private_key_at(&path).unwrap();
    assert_eq!(generated.n(), reloaded.n());
    assert_eq!(generated.e(), reloaded.e());

    std::fs::write(&path, "not a private key").unwrap();
    let err = load_or_generate_private_key_at(&path).unwrap_err();
    assert!(
        err.starts_with("failed to decode PEM block containing private key:"),
        "got: {err}"
    );

    std::fs::remove_dir_all(&dir).ok();
}
