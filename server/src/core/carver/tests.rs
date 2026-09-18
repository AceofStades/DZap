use super::parsers::crc32::crc32;
use super::parsers::jpeg::JpegParser;
use super::parsers::mp4::Mp4Parser;
use super::parsers::png::PngParser;
use super::report::generate_chain_of_custody_report;
use super::{CarvedArtifact, CarverStore};
use std::collections::HashMap;

#[test]
fn test_crc32_png_standard() {
    assert_eq!(crc32(b"123456789"), 0xcbf43926);
}

#[test]
fn test_jpeg_parser_minimal_valid() {
    let mut data = Vec::new();
    // SOI
    data.extend_from_slice(&[0xff, 0xd8, 0xff]);
    // APP0 marker with JFIF
    data.push(0xe0);
    let app0_len: u16 = 16;
    data.extend_from_slice(&app0_len.to_be_bytes());
    data.extend_from_slice(b"JFIF\x00\x01\x01\x00\x00\x01\x00\x01\x00\x00");

    // DQT marker
    data.extend_from_slice(&[0xff, 0xdb]);
    let dqt_len: u16 = 67;
    data.extend_from_slice(&dqt_len.to_be_bytes());
    data.extend_from_slice(&vec![0x01; 65]);

    // DHT marker
    data.extend_from_slice(&[0xff, 0xc4]);
    let dht_len: u16 = 30;
    data.extend_from_slice(&dht_len.to_be_bytes());
    data.extend_from_slice(&vec![0x00; 28]);

    // SOF0 (Baseline)
    data.extend_from_slice(&[0xff, 0xc0]);
    let sof_len: u16 = 11;
    data.extend_from_slice(&sof_len.to_be_bytes());
    data.push(8); // 8-bit precision
    data.extend_from_slice(&1080u16.to_be_bytes()); // Height: 1080
    data.extend_from_slice(&1920u16.to_be_bytes()); // Width: 1920
    data.extend_from_slice(&[3, 1, 0x11, 0]); // 3 components

    // SOS
    data.extend_from_slice(&[0xff, 0xda]);
    let sos_len: u16 = 8;
    data.extend_from_slice(&sos_len.to_be_bytes());
    data.extend_from_slice(&[1, 1, 0, 0, 63, 0]);

    // Compressed entropy data with byte-stuffed FF 00
    data.extend_from_slice(&[0x12, 0x34, 0xff, 0x00, 0x56, 0x78]);

    // Terminal EOI
    data.extend_from_slice(&[0xff, 0xd9]);

    let parser = JpegParser::new();
    let result = parser.parse(&data, true).expect("JPEG should parse successfully");

    assert!(result.is_valid);
    assert_eq!(result.file_size, data.len() as u64);
    assert!(result.detected_footer);
    assert!(result.confidence >= 0.90);
    assert_eq!(result.metadata.get("dimensions").map(|s| s.as_str()), Some("1920x1080"));
}

#[test]
fn test_png_parser_chunk_and_crc() {
    let mut data = Vec::new();
    // Magic
    data.extend_from_slice(&[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);

    // IHDR
    let ihdr_len: u32 = 13;
    data.extend_from_slice(&ihdr_len.to_be_bytes());
    let mut ihdr_payload = Vec::new();
    ihdr_payload.extend_from_slice(b"IHDR");
    ihdr_payload.extend_from_slice(&800u32.to_be_bytes()); // Width 800
    ihdr_payload.extend_from_slice(&600u32.to_be_bytes()); // Height 600
    ihdr_payload.push(8); // Bit depth
    ihdr_payload.push(6); // Color type RGBA
    ihdr_payload.push(0); // Compression
    ihdr_payload.push(0); // Filter
    ihdr_payload.push(0); // Interlace
    let ihdr_crc = crc32(&ihdr_payload);
    data.extend_from_slice(&ihdr_payload[4..]); // Only data
    data.extend_from_slice(&ihdr_crc.to_be_bytes());

    // Fix: the payload in chunk traversal has chunk_type + data:
    // Let's format properly:
    let mut correct_png = Vec::new();
    correct_png.extend_from_slice(&[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
    correct_png.extend_from_slice(&13u32.to_be_bytes());
    correct_png.extend_from_slice(b"IHDR");
    correct_png.extend_from_slice(&800u32.to_be_bytes());
    correct_png.extend_from_slice(&600u32.to_be_bytes());
    correct_png.extend_from_slice(&[8, 6, 0, 0, 0]);
    let mut check_chunk = Vec::new();
    check_chunk.extend_from_slice(b"IHDR");
    check_chunk.extend_from_slice(&800u32.to_be_bytes());
    check_chunk.extend_from_slice(&600u32.to_be_bytes());
    check_chunk.extend_from_slice(&[8, 6, 0, 0, 0]);
    let ihdr_crc = crc32(&check_chunk);
    correct_png.extend_from_slice(&ihdr_crc.to_be_bytes());

    // IEND
    correct_png.extend_from_slice(&0u32.to_be_bytes());
    correct_png.extend_from_slice(b"IEND");
    let iend_crc = crc32(b"IEND");
    correct_png.extend_from_slice(&iend_crc.to_be_bytes());

    let parser = PngParser::new();
    let result = parser.parse(&correct_png, true).expect("PNG should parse successfully");

    assert!(result.is_valid);
    assert_eq!(result.file_size, correct_png.len() as u64);
    assert!(result.detected_footer);
    assert_eq!(result.metadata.get("dimensions").map(|s| s.as_str()), Some("800x600"));
}

#[test]
fn test_mp4_parser_atoms() {
    let mut data = Vec::new();
    // ftyp box
    let ftyp_size: u32 = 24;
    data.extend_from_slice(&ftyp_size.to_be_bytes());
    data.extend_from_slice(b"ftyp");
    data.extend_from_slice(b"mp42"); // major brand
    data.extend_from_slice(&0u32.to_be_bytes()); // minor version
    data.extend_from_slice(b"isom");
    data.extend_from_slice(b"mp42");

    // moov box
    let moov_size: u32 = 16;
    data.extend_from_slice(&moov_size.to_be_bytes());
    data.extend_from_slice(b"moov");
    data.extend_from_slice(&[0u8; 8]);

    // mdat box
    let mdat_size: u32 = 32;
    data.extend_from_slice(&mdat_size.to_be_bytes());
    data.extend_from_slice(b"mdat");
    data.extend_from_slice(&[0xaa; 24]);

    let parser = Mp4Parser::new();
    let result = parser.parse(&data, true).expect("MP4 should parse successfully");

    assert!(result.is_valid);
    assert_eq!(result.file_size, data.len() as u64);
    assert!(result.detected_footer);
    assert_eq!(result.metadata.get("brand").map(|s| s.as_str()), Some("mp42"));
}

#[test]
fn test_carver_store_and_chain_of_custody() {
    let store = CarverStore::new();
    store.start_task("test-task-1".to_string(), "/dev/mock0".to_string(), 1024 * 1024 * 10, 4096);

    let art = CarvedArtifact {
        id: "art-001".to_string(),
        filename: "test.jpg".to_string(),
        file_type: "JPEG Image".to_string(),
        extension: "jpg".to_string(),
        mime_type: "image/jpeg".to_string(),
        start_sector: 0,
        end_sector: 2,
        byte_offset: 0,
        size_bytes: 8192,
        sha256: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".to_string(),
        confidence_score: 0.95,
        is_fragmented: false,
        fragment_count: 1,
        gap_offset: None,
        metadata: HashMap::new(),
        extracted_path: "/tmp/test.jpg".to_string(),
        preview_data_url: None,
        carved_at: "2026-09-18T12:00:00Z".to_string(),
    };

    store.add_artifact(art.clone());
    let arts = store.get_artifacts();
    assert_eq!(arts.len(), 1);

    let report = generate_chain_of_custody_report("/dev/mock0", store.get_progress().as_ref(), &arts);
    assert_eq!(report.total_artifacts_carved, 1);
    assert_eq!(report.high_confidence_count, 1);
    assert!(!report.integrity_digest_sha256.is_empty());
    assert!(!report.digital_signature_seal.is_empty());
}
