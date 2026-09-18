use std::collections::HashMap;
use super::crc32::crc32;

#[derive(Debug, Clone)]
pub struct PngParseResult {
    pub is_valid: bool,
    pub file_size: u64,
    pub is_fragmented: bool,
    pub fragments: Vec<(u64, u64)>,
    pub confidence: f64,
    pub metadata: HashMap<String, String>,
    pub detected_footer: bool,
}

pub struct PngParser;

impl PngParser {
    pub fn new() -> Self {
        Self
    }

    pub fn header_magic(&self) -> &'static [u8] {
        &[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]
    }

    pub fn parse(&self, buffer: &[u8], is_cluster_aligned: bool) -> Option<PngParseResult> {
        let magic = self.header_magic();
        if buffer.len() < 8 || &buffer[0..8] != magic {
            return None;
        }

        let mut offset = 8;
        let mut width: u32 = 0;
        let mut height: u32 = 0;
        let mut bit_depth: u8 = 0;
        let mut color_type: u8 = 0;
        let mut idat_count = 0;
        let mut idat_crc_valid_count = 0;
        let mut has_ihdr = false;
        let mut has_iend = false;
        let mut metadata = HashMap::new();
        let max_len = buffer.len();

        while offset + 12 <= max_len {
            let length = u32::from_be_bytes([
                buffer[offset],
                buffer[offset + 1],
                buffer[offset + 2],
                buffer[offset + 3],
            ]) as usize;

            let chunk_type = &buffer[offset + 4..offset + 8];
            let type_str = String::from_utf8_lossy(chunk_type).to_string();

            // Sanity check on chunk length to avoid runaway or corrupt allocations
            if length > 32 * 1024 * 1024 {
                break;
            }

            let total_chunk_len = 12 + length;
            if offset + total_chunk_len > max_len {
                // Truncated chunk at end of buffer
                break;
            }

            let chunk_payload = &buffer[offset + 4..offset + 8 + length];
            let expected_crc = u32::from_be_bytes([
                buffer[offset + 8 + length],
                buffer[offset + 9 + length],
                buffer[offset + 10 + length],
                buffer[offset + 11 + length],
            ]);

            let actual_crc = crc32(chunk_payload);
            let crc_matches = actual_crc == expected_crc;

            if chunk_type == b"IHDR" {
                if length == 13 && crc_matches {
                    has_ihdr = true;
                    width = u32::from_be_bytes([
                        buffer[offset + 8],
                        buffer[offset + 9],
                        buffer[offset + 10],
                        buffer[offset + 11],
                    ]);
                    height = u32::from_be_bytes([
                        buffer[offset + 12],
                        buffer[offset + 13],
                        buffer[offset + 14],
                        buffer[offset + 15],
                    ]);
                    bit_depth = buffer[offset + 16];
                    color_type = buffer[offset + 17];
                }
            } else if chunk_type == b"IDAT" {
                idat_count += 1;
                if crc_matches {
                    idat_crc_valid_count += 1;
                }
            } else if chunk_type == b"IEND" {
                if crc_matches {
                    has_iend = true;
                    offset += total_chunk_len;
                    break;
                }
            }

            offset += total_chunk_len;

            // Stop if unknown or invalid non-ASCII chunk type occurs
            if !type_str.chars().all(|c| c.is_ascii_alphabetic()) {
                break;
            }
        }

        let file_size = offset as u64;

        // Confidence scoring
        let mut confidence: f64 = 0.0;
        if is_cluster_aligned {
            confidence += 0.40;
        } else {
            confidence += 0.30;
        }

        if has_ihdr && width > 0 && height > 0 {
            confidence += 0.20;
        }

        if idat_count > 0 && idat_crc_valid_count == idat_count {
            confidence += 0.20;
        } else if idat_count > 0 {
            confidence += 0.10;
        }

        if has_iend {
            confidence += 0.20;
        }

        confidence = (confidence * 100.0).round() / 100.0;
        let is_valid = confidence >= 0.50 && has_ihdr;

        metadata.insert("format".to_string(), "Portable Network Graphics (PNG)".to_string());
        if width > 0 && height > 0 {
            metadata.insert("dimensions".to_string(), format!("{}x{}", width, height));
            metadata.insert("bit_depth".to_string(), format!("{}-bit", bit_depth));
            metadata.insert("color_type".to_string(), format!("type {}", color_type));
        }
        metadata.insert("idat_chunks".to_string(), format!("{}/{} valid CRC", idat_crc_valid_count, idat_count));

        Some(PngParseResult {
            is_valid,
            file_size,
            is_fragmented: false,
            fragments: vec![(0, file_size)],
            confidence,
            metadata,
            detected_footer: has_iend,
        })
    }
}
