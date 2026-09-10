// Port of server-go/core/certificate.go
use chrono::{DateTime, Utc};
use rsa::pkcs1::{DecodeRsaPrivateKey, EncodeRsaPrivateKey};
use rsa::pkcs8::{EncodePublicKey, LineEnding};
use rsa::{Pkcs1v15Sign, RsaPrivateKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use super::drives::log_line;
use super::jobs::{WipeJob, WipeJobStatus};
use super::verification::VerificationResult;

static APP_PRIVATE_KEY: OnceLock<RsaPrivateKey> = OnceLock::new();

/// Loads (or generates) the application private key. Fatal on error,
/// mirroring the Go `init()` behaviour.
pub fn init() {
    match load_or_generate_private_key() {
        Ok(key) => {
            let _ = APP_PRIVATE_KEY.set(key);
        }
        Err(e) => {
            eprintln!("FATAL: Could not load or generate the application private key: {e}");
            std::process::exit(1);
        }
    }
}

/// Test-only init: use an ephemeral key so tests never touch the real
/// key at ~/.config/DZap/private.pem.
#[cfg(test)]
pub fn init_for_tests() {
    let _ = APP_PRIVATE_KEY.get_or_init(|| {
        let mut rng = rand::thread_rng();
        RsaPrivateKey::new(&mut rng, 2048).expect("failed to generate test key")
    });
}

fn private_key() -> &'static RsaPrivateKey {
    APP_PRIVATE_KEY
        .get()
        .expect("certificate module not initialized")
}

fn key_path() -> Result<PathBuf, String> {
    let config_dir = dirs::config_dir().ok_or("could not get user config directory".to_string())?;
    Ok(config_dir.join("DZap").join("private.pem"))
}

fn load_or_generate_private_key() -> Result<RsaPrivateKey, String> {
    let key_path = key_path()?;
    load_or_generate_private_key_at(&key_path)
}

pub(crate) fn load_or_generate_private_key_at(key_path: &Path) -> Result<RsaPrivateKey, String> {
    if key_path.exists() {
        let key_data = std::fs::read_to_string(key_path)
            .map_err(|e| format!("could not read private key file: {e}"))?;
        return RsaPrivateKey::from_pkcs1_pem(&key_data)
            .map_err(|e| format!("failed to decode PEM block containing private key: {e}"));
    }

    let mut rng = rand::thread_rng();
    let private_key = RsaPrivateKey::new(&mut rng, 2048)
        .map_err(|e| format!("failed to generate new private key: {e}"))?;

    let pem = private_key
        .to_pkcs1_pem(LineEnding::LF)
        .map_err(|e| format!("failed to encode private key: {e}"))?;

    if let Some(dir) = key_path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("failed to create config directory: {e}"))?;
        let _ = std::fs::set_permissions(dir, {
            use std::os::unix::fs::PermissionsExt;
            std::fs::Permissions::from_mode(0o700)
        });
    }
    std::fs::write(key_path, pem.as_bytes())
        .map_err(|e| format!("failed to save new private key: {e}"))?;
    let _ = std::fs::set_permissions(key_path, {
        use std::os::unix::fs::PermissionsExt;
        std::fs::Permissions::from_mode(0o600)
    });

    log_line(&format!("New private key saved to {}", key_path.display()));
    Ok(private_key)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CertificateData {
    pub job_id: String,
    pub device_path: String,
    pub device_model: String,
    pub device_serial: String,
    pub device_wwn: String,
    pub device_size_bytes: String,
    pub device_transport: String,
    pub device_major_minor: String,
    pub device_type: String,
    pub wipe_method: String,
    pub started_at: DateTime<Utc>,
    pub completed_at: DateTime<Utc>,
    pub timestamp: DateTime<Utc>,
    pub verification: VerificationResult,
    pub evidence_hash: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SignedCertificate {
    pub data: CertificateData,
    pub signature: String,
    #[serde(rename = "publicKey")]
    pub public_key: String,
    /// QR code module matrix for PDF rendering. Excluded from JSON,
    /// mirroring Go's `json:"-"`.
    #[serde(skip)]
    pub qr_code: Option<qrcode::QrCode>,
}

pub fn generate_certificate_for_job(job: &WipeJob) -> Result<SignedCertificate, String> {
    if job.status != WipeJobStatus::Verified {
        return Err("certificate requires a verified wipe job".to_string());
    }
    if !job.verify_evidence() {
        return Err("wipe job evidence verification failed".to_string());
    }
    let completed_at = job
        .completed_at
        .ok_or_else(|| "verified wipe job has no completion timestamp".to_string())?;
    let verification = job
        .verification
        .clone()
        .ok_or_else(|| "verified wipe job has no verification result".to_string())?;
    let cert_data = CertificateData {
        job_id: job.id.clone(),
        device_path: job.device_path.clone(),
        device_model: job.device_model.clone(),
        device_serial: job.identity.serial.clone(),
        device_wwn: job.identity.wwn.clone(),
        device_size_bytes: job.identity.size_bytes.clone(),
        device_transport: job.identity.transport.clone(),
        device_major_minor: job.identity.major_minor.clone(),
        device_type: job.device_type.clone(),
        wipe_method: job.method.clone(),
        started_at: job.started_at,
        completed_at,
        timestamp: Utc::now(),
        verification,
        evidence_hash: job.evidence_hash.clone(),
    };

    let hash = hash_certificate_data(&cert_data);

    let mut rng = rand::thread_rng();
    let signature_bytes = private_key()
        .sign_with_rng(&mut rng, Pkcs1v15Sign::new::<Sha256>(), &hash)
        .map_err(|e| format!("failed to sign certificate: {e}"))?;
    let signature = hex::encode(signature_bytes);

    let public_key = private_key()
        .to_public_key()
        .to_public_key_pem(LineEnding::LF)
        .map_err(|e| format!("failed to encode public key: {e}"))?;

    let mut signed_cert = SignedCertificate {
        data: cert_data,
        signature,
        public_key,
        qr_code: None,
    };

    // Generate QR Code from the JSON representation of the certificate
    let cert_json = serde_json::to_string(&signed_cert)
        .map_err(|e| format!("failed to marshal certificate to JSON for QR code: {e}"))?;

    let qr = qrcode::QrCode::new(cert_json.as_bytes())
        .map_err(|e| format!("failed to generate QR code: {e}"))?;
    signed_cert.qr_code = Some(qr);

    Ok(signed_cert)
}

pub(crate) fn hash_certificate_data(data: &CertificateData) -> Vec<u8> {
    let payload = serde_json::to_vec(data).expect("certificate data is serializable");
    Sha256::digest(payload).to_vec()
}

impl SignedCertificate {
    pub fn verify_signature(&self) -> bool {
        use rsa::pkcs8::DecodePublicKey;

        let Ok(public_key) = rsa::RsaPublicKey::from_public_key_pem(&self.public_key) else {
            return false;
        };
        let Ok(signature) = hex::decode(&self.signature) else {
            return false;
        };
        public_key
            .verify(
                Pkcs1v15Sign::new::<Sha256>(),
                &hash_certificate_data(&self.data),
                &signature,
            )
            .is_ok()
    }

    pub fn matches_job(&self, job: &WipeJob) -> bool {
        job.status == WipeJobStatus::Verified
            && job.completed_at == Some(self.data.completed_at)
            && self.data.job_id == job.id
            && self.data.device_path == job.device_path
            && self.data.device_model == job.device_model
            && self.data.device_serial == job.identity.serial
            && self.data.device_wwn == job.identity.wwn
            && self.data.device_size_bytes == job.identity.size_bytes
            && self.data.device_transport == job.identity.transport
            && self.data.device_major_minor == job.identity.major_minor
            && self.data.device_type == job.device_type
            && self.data.wipe_method == job.method
            && self.data.started_at == job.started_at
            && job.verification.as_ref() == Some(&self.data.verification)
            && self.data.evidence_hash == job.evidence_hash
    }
}

#[derive(Clone)]
pub struct CertificateStore {
    certificates: Arc<Mutex<HashMap<String, SignedCertificate>>>,
    directory: Option<Arc<PathBuf>>,
}

impl CertificateStore {
    pub fn in_memory() -> Self {
        Self {
            certificates: Arc::new(Mutex::new(HashMap::new())),
            directory: None,
        }
    }

    pub fn persistent(directory: PathBuf) -> Result<Self, String> {
        std::fs::create_dir_all(&directory)
            .map_err(|error| format!("failed to create certificate directory: {error}"))?;
        set_directory_permissions(&directory);

        let expected_public_key = private_key()
            .to_public_key()
            .to_public_key_pem(LineEnding::LF)
            .map_err(|error| format!("failed to encode application public key: {error}"))?;
        let mut certificates = HashMap::new();
        for entry in std::fs::read_dir(&directory)
            .map_err(|error| format!("failed to read certificate directory: {error}"))?
        {
            let entry =
                entry.map_err(|error| format!("failed to read certificate entry: {error}"))?;
            let path = entry.path();
            if path.is_dir() || path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let encoded = std::fs::read_to_string(&path)
                .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
            let certificate: SignedCertificate = serde_json::from_str(&encoded)
                .map_err(|error| format!("failed to parse {}: {error}", path.display()))?;
            if certificate.public_key != expected_public_key || !certificate.verify_signature() {
                return Err(format!(
                    "certificate verification failed for {}",
                    path.display()
                ));
            }
            let expected_file_name = format!("{}.json", certificate.data.job_id);
            if path.file_name().and_then(|value| value.to_str()) != Some(&expected_file_name) {
                return Err(format!(
                    "certificate job identifier does not match {}",
                    path.display()
                ));
            }
            if certificates
                .insert(certificate.data.job_id.clone(), certificate)
                .is_some()
            {
                return Err("duplicate persisted certificate job identifier".to_string());
            }
        }

        Ok(Self {
            certificates: Arc::new(Mutex::new(certificates)),
            directory: Some(Arc::new(directory)),
        })
    }

    pub fn get(&self, job_id: &str) -> Result<Option<SignedCertificate>, String> {
        Ok(self
            .certificates
            .lock()
            .map_err(|_| "certificate store lock was poisoned".to_string())?
            .get(job_id)
            .cloned())
    }

    pub fn list(&self) -> Result<Vec<SignedCertificate>, String> {
        let mut certificates: Vec<_> = self
            .certificates
            .lock()
            .map_err(|_| "certificate store lock was poisoned".to_string())?
            .values()
            .cloned()
            .collect();
        certificates.sort_by_key(|certificate| std::cmp::Reverse(certificate.data.timestamp));
        Ok(certificates)
    }

    pub fn save_if_absent(
        &self,
        certificate: SignedCertificate,
    ) -> Result<SignedCertificate, String> {
        let mut certificates = self
            .certificates
            .lock()
            .map_err(|_| "certificate store lock was poisoned".to_string())?;
        if let Some(existing) = certificates.get(&certificate.data.job_id) {
            return Ok(existing.clone());
        }

        if let Some(directory) = &self.directory {
            let encoded = serde_json::to_vec_pretty(&certificate)
                .map_err(|error| format!("failed to encode certificate: {error}"))?;
            atomic_write(
                directory,
                &format!("{}.json", certificate.data.job_id),
                &encoded,
            )?;
        }
        certificates.insert(certificate.data.job_id.clone(), certificate.clone());
        Ok(certificate)
    }
}

impl Default for CertificateStore {
    fn default() -> Self {
        Self::in_memory()
    }
}

fn atomic_write(directory: &Path, file_name: &str, contents: &[u8]) -> Result<(), String> {
    let destination = directory.join(file_name);
    let temporary = directory.join(format!(".{file_name}.tmp"));
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&temporary)
        .map_err(|error| format!("failed to open {}: {error}", temporary.display()))?;
    file.write_all(contents)
        .map_err(|error| format!("failed to write {}: {error}", temporary.display()))?;
    set_file_permissions(&temporary);
    file.sync_all()
        .map_err(|error| format!("failed to sync {}: {error}", temporary.display()))?;
    std::fs::rename(&temporary, &destination).map_err(|error| {
        format!(
            "failed to replace persisted certificate {}: {error}",
            destination.display()
        )
    })?;
    std::fs::File::open(directory)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| format!("failed to sync {}: {error}", directory.display()))?;
    Ok(())
}

#[cfg(unix)]
fn set_directory_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700));
}

#[cfg(not(unix))]
fn set_directory_permissions(_path: &Path) {}

#[cfg(unix)]
fn set_file_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn set_file_permissions(_path: &Path) {}

// ---------------------------------------------------------------------------
// Minimal PDF generation (replaces gofpdf).
// ---------------------------------------------------------------------------

const MM: f64 = 72.0 / 25.4; // mm -> pt
const PAGE_W: f64 = 595.28; // A4
const PAGE_H: f64 = 841.89;

fn escape_pdf_text(text: &str) -> String {
    let mut out = String::new();
    for b in text.bytes() {
        match b {
            b'(' | b')' | b'\\' => {
                out.push('\\');
                out.push(b as char);
            }
            0x20..=0x7e => out.push(b as char),
            _ => out.push_str(&format!("\\{b:03o}")),
        }
    }
    out
}

struct Content {
    buf: String,
}

impl Content {
    fn new() -> Self {
        Content { buf: String::new() }
    }

    /// Draw text; `y_mm` is the baseline measured from the TOP of the page.
    fn text(&mut self, font: &str, size: f64, x_mm: f64, y_mm: f64, text: &str) {
        let x = x_mm * MM;
        let y = PAGE_H - y_mm * MM;
        self.buf.push_str(&format!(
            "BT /{font} {size} Tf 1 0 0 1 {x:.2} {y:.2} Tm ({}) Tj ET\n",
            escape_pdf_text(text)
        ));
    }

    /// Filled rectangle; `y_mm` is the TOP edge of the rect.
    fn rect_fill(&mut self, x_mm: f64, y_mm: f64, w_mm: f64, h_mm: f64) {
        let x = x_mm * MM;
        let y = PAGE_H - (y_mm + h_mm) * MM;
        self.buf.push_str(&format!(
            "{x:.2} {y:.2} {:.2} {:.2} re f\n",
            w_mm * MM,
            h_mm * MM
        ));
    }

    /// Stroked rectangle; `y_mm` is the TOP edge of the rect.
    fn rect_stroke(&mut self, x_mm: f64, y_mm: f64, w_mm: f64, h_mm: f64) {
        let x = x_mm * MM;
        let y = PAGE_H - (y_mm + h_mm) * MM;
        self.buf.push_str(&format!(
            "{x:.2} {y:.2} {:.2} {:.2} re S\n",
            w_mm * MM,
            h_mm * MM
        ));
    }
}

/// Wrap text to fit a width, given a monospace font size in pt.
fn wrap_mono(text: &str, font_size: f64, max_width_mm: f64) -> Vec<String> {
    let char_w_mm = font_size * 0.6 / MM;
    let max_chars = (max_width_mm / char_w_mm).floor().max(1.0) as usize;
    let mut lines = Vec::new();
    for raw_line in text.lines() {
        let mut line = raw_line;
        while line.len() > max_chars {
            lines.push(line[..max_chars].to_string());
            line = &line[max_chars..];
        }
        lines.push(line.to_string());
    }
    lines
}

impl SignedCertificate {
    pub fn generate_pdf(&self) -> Result<Vec<u8>, String> {
        let mut c = Content::new();

        // --- Title ---
        c.text("F1", 20.0, 10.0, 25.0, "Data Destruction Certificate");

        // --- Certificate Details ---
        c.text("F1", 12.0, 10.0, 42.0, "Device Model:");
        c.text("F2", 12.0, 50.0, 42.0, &self.data.device_model);
        c.text("F1", 10.0, 10.0, 50.0, "Device Serial:");
        c.text("F2", 10.0, 50.0, 50.0, &self.data.device_serial);
        c.text("F1", 10.0, 10.0, 58.0, "Wipe Method:");
        c.text("F2", 10.0, 50.0, 58.0, &self.data.wipe_method);
        c.text("F1", 10.0, 10.0, 66.0, "Job ID:");
        c.text("F3", 8.0, 50.0, 66.0, &self.data.job_id);
        c.text("F1", 10.0, 10.0, 74.0, "Evidence Hash:");
        c.text("F3", 7.0, 50.0, 74.0, &self.data.evidence_hash);
        c.text("F1", 9.0, 10.0, 82.0, "Verification:");
        c.text(
            "F2",
            9.0,
            50.0,
            82.0,
            &format!(
                "{:?}, {} bytes checked",
                self.data.verification.strategy, self.data.verification.bytes_checked
            ),
        );
        c.text("F1", 9.0, 10.0, 90.0, "Readback SHA-256:");
        c.text(
            "F3",
            7.0,
            50.0,
            90.0,
            &self.data.verification.readback_sha256,
        );
        c.text("F1", 9.0, 10.0, 98.0, "Identity Revalidated:");
        c.text(
            "F2",
            9.0,
            50.0,
            98.0,
            if self.data.verification.identity_revalidated {
                "Yes"
            } else {
                "No"
            },
        );

        // --- QR Code for Verification ---
        let generated_qr;
        let qr = if let Some(qr) = &self.qr_code {
            qr
        } else {
            let encoded = serde_json::to_vec(self)
                .map_err(|error| format!("failed to encode certificate for QR code: {error}"))?;
            generated_qr = qrcode::QrCode::new(encoded)
                .map_err(|error| format!("failed to generate QR code: {error}"))?;
            &generated_qr
        };
        {
            let quiet = 4isize;
            let width = qr.width() as isize;
            let total = (width + quiet * 2) as f64;
            let cell = 40.0 / total; // mm per module
            let colors = qr.to_colors();
            for y in 0..width {
                for x in 0..width {
                    if colors[(y * width + x) as usize] == qrcode::Color::Dark {
                        c.rect_fill(
                            150.0 + (x + quiet) as f64 * cell,
                            25.0 + (y + quiet) as f64 * cell,
                            cell,
                            cell,
                        );
                    }
                }
            }
        }
        c.text("F4", 9.0, 150.0, 72.0, "Scan to Verify");

        // --- Digital Signature ---
        let mut y = 113.0;
        c.text("F1", 10.0, 10.0, y, "Digital Signature (SHA256withRSA):");
        y += 5.0;
        let sig_lines = wrap_mono(&self.signature, 8.0, 186.0);
        let block_h = sig_lines.len() as f64 * 4.0 + 2.0;
        c.rect_stroke(10.0, y - 3.0, 190.0, block_h);
        for line in &sig_lines {
            y += 4.0;
            c.text("F3", 8.0, 11.0, y, line);
        }
        y += 5.0 + 5.0;

        c.text("F1", 10.0, 10.0, y, "Public Key:");
        y += 5.0;
        let key_lines = wrap_mono(&self.public_key, 7.0, 186.0);
        let block_h = key_lines.len() as f64 * 4.0 + 2.0;
        c.rect_stroke(10.0, y - 3.0, 190.0, block_h);
        for line in &key_lines {
            y += 4.0;
            c.text("F3", 7.0, 11.0, y, line);
        }

        build_pdf(&c.buf)
    }
}

fn build_pdf(content: &str) -> Result<Vec<u8>, String> {
    let mut buf: Vec<u8> = Vec::new();
    let mut offsets: Vec<usize> = Vec::new();

    buf.extend_from_slice(b"%PDF-1.4\n");

    let obj = |buf: &mut Vec<u8>, offsets: &mut Vec<usize>, body: &[u8]| {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n", offsets.len()).as_bytes());
        buf.extend_from_slice(body);
        buf.extend_from_slice(b"\nendobj\n");
    };

    obj(&mut buf, &mut offsets, b"<< /Type /Catalog /Pages 2 0 R >>");
    obj(
        &mut buf,
        &mut offsets,
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
    );
    obj(
        &mut buf,
        &mut offsets,
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {PAGE_W} {PAGE_H}] /Resources << /Font << /F1 4 0 R /F2 5 0 R /F3 6 0 R /F4 7 0 R >> >> /Contents 8 0 R >>"
        )
        .as_bytes(),
    );
    for base_font in [
        "Helvetica-Bold",
        "Helvetica",
        "Courier",
        "Helvetica-Oblique",
    ] {
        obj(
            &mut buf,
            &mut offsets,
            format!("<< /Type /Font /Subtype /Type1 /BaseFont /{base_font} >>").as_bytes(),
        );
    }
    obj(
        &mut buf,
        &mut offsets,
        format!(
            "<< /Length {} >>\nstream\n{content}endstream",
            content.len()
        )
        .as_bytes(),
    );

    let xref_start = buf.len();
    let count = offsets.len() + 1;
    buf.extend_from_slice(format!("xref\n0 {count}\n").as_bytes());
    buf.extend_from_slice(b"0000000000 65535 f \n");
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size {count} /Root 1 0 R >>\nstartxref\n{xref_start}\n%%EOF\n")
            .as_bytes(),
    );

    Ok(buf)
}
