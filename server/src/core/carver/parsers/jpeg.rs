use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct JpegParseResult {
    pub is_valid: bool,
    pub file_size: u64,
    pub is_fragmented: bool,
    pub fragments: Vec<(u64, u64)>,
    pub confidence: f64,
    pub metadata: HashMap<String, String>,
    pub detected_footer: bool,
}

pub struct JpegParser;

impl JpegParser {
    pub fn new() -> Self {
        Self
    }

    pub fn header_magic(&self) -> &'static [u8] {
        &[0xff, 0xd8, 0xff]
    }

    /// Parses a buffer starting at a candidate JPEG SOI.
    /// `buffer`: slice of bytes starting at candidate offset.
    /// `is_cluster_aligned`: whether the start offset is a multiple of the sector/cluster size (e.g. 4096).
    pub fn parse(&self, buffer: &[u8], is_cluster_aligned: bool) -> Option<JpegParseResult> {
        if buffer.len() < 4 || &buffer[0..3] != &[0xff, 0xd8, 0xff] {
            return None;
        }

        let mut offset: usize = 2; // right after 0xFF 0xD8
        let mut has_dqt = false;
        let mut has_dht = false;
        let mut has_sof = false;
        let mut has_sos = false;
        let mut has_exif_or_jfif = false;
        let mut width: u32 = 0;
        let mut height: u32 = 0;
        let mut camera_model: Option<String> = None;
        let mut metadata = HashMap::new();

        let max_len = buffer.len();

        // 1. Traverse structural markers until SOS (0xFF 0xDA)
        while offset + 3 < max_len {
            if buffer[offset] != 0xff {
                // Garbage or alignment error in marker stream
                break;
            }

            let marker = buffer[offset + 1];

            // Stray 0xFF padding before marker
            if marker == 0xff {
                offset += 1;
                continue;
            }

            // End of Image encountered before SOS
            if marker == 0xd9 {
                offset += 2;
                break;
            }

            // Standalone markers with no length payload (RST0..RST7, SOI, TEM)
            if (0xd0..=0xd7).contains(&marker) || marker == 0xd8 || marker == 0x01 {
                offset += 2;
                continue;
            }

            // Start of Scan: marks beginning of compressed bitstream
            if marker == 0xda {
                has_sos = true;
                if offset + 4 <= max_len {
                    let sos_len = u16::from_be_bytes([buffer[offset + 2], buffer[offset + 3]]) as usize;
                    offset += 2 + sos_len;
                } else {
                    offset += 2;
                }
                break;
            }

            // Standard segment with 16-bit big-endian length
            if offset + 3 >= max_len {
                break;
            }
            let seg_len = u16::from_be_bytes([buffer[offset + 2], buffer[offset + 3]]) as usize;
            if seg_len < 2 || offset + 2 + seg_len > max_len {
                // Truncated segment
                break;
            }

            let seg_data_start = offset + 4;
            let seg_data_end = offset + 2 + seg_len;

            match marker {
                // APP0 (JFIF)
                0xe0 => {
                    if seg_data_end - seg_data_start >= 4 && &buffer[seg_data_start..seg_data_start + 4] == b"JFIF" {
                        has_exif_or_jfif = true;
                        metadata.insert("format".to_string(), "JPEG/JFIF".to_string());
                    }
                }
                // APP1 (EXIF)
                0xe1 => {
                    if seg_data_end - seg_data_start >= 6 && &buffer[seg_data_start..seg_data_start + 4] == b"Exif" {
                        has_exif_or_jfif = true;
                        metadata.insert("format".to_string(), "JPEG/EXIF".to_string());
                        // Simple check for ASCII camera model string in EXIF block
                        if let Ok(exif_str) = std::str::from_utf8(&buffer[seg_data_start..seg_data_end]) {
                            for brand in ["Canon", "Nikon", "Sony", "Apple", "Samsung", "Google", "Fujifilm"] {
                                if exif_str.contains(brand) {
                                    camera_model = Some(brand.to_string());
                                    break;
                                }
                            }
                        }
                    }
                }
                // DQT (Quantization Table)
                0xdb => {
                    has_dqt = true;
                }
                // DHT (Huffman Table)
                0xc4 => {
                    has_dht = true;
                }
                // SOF0 (Baseline) or SOF2 (Progressive)
                0xc0 | 0xc1 | 0xc2 => {
                    has_sof = true;
                    if seg_data_end - seg_data_start >= 5 {
                        // Data layout: precision (1 byte), height (2 bytes), width (2 bytes)
                        let h = u16::from_be_bytes([buffer[seg_data_start + 1], buffer[seg_data_start + 2]]) as u32;
                        let w = u16::from_be_bytes([buffer[seg_data_start + 3], buffer[seg_data_start + 4]]) as u32;
                        if w > 0 && h > 0 {
                            width = w;
                            height = h;
                        }
                    }
                }
                _ => {}
            }

            offset = seg_data_end;
        }

        // 2. Scan entropy bitstream after SOS until terminal EOI (0xFF 0xD9)
        let mut eoi_found = false;
        let mut eoi_offset = offset;
        let mut is_fragmented = false;
        let mut gap_candidate = 0;

        if has_sos && offset < max_len {
            let scan_start = offset;
            let mut i = scan_start;
            let mut consecutive_zeros = 0;

            while i + 1 < max_len {
                let b = buffer[i];

                // Check for potential gap: long runs of 0x00 or unallocated sector wipe patterns
                if b == 0x00 {
                    consecutive_zeros += 1;
                    if consecutive_zeros > 4096 && gap_candidate == 0 {
                        is_fragmented = true;
                        gap_candidate = i - consecutive_zeros;
                    }
                } else {
                    consecutive_zeros = 0;
                }

                if b == 0xff {
                    let next = buffer[i + 1];
                    if next == 0xd9 {
                        // Terminal EOI found!
                        eoi_found = true;
                        eoi_offset = i + 2;
                        break;
                    } else if next == 0x00 || (0xd0..=0xd7).contains(&next) {
                        // Byte stuffing (0xFF 0x00) or restart marker (RST)
                        i += 2;
                        continue;
                    } else if next >= 0xc0 && next <= 0xfe {
                        // Unexpected structural marker inside entropy stream -> possible fragmentation or corrupt sector
                        if !is_fragmented {
                            is_fragmented = true;
                            gap_candidate = i;
                        }
                    }
                }
                i += 1;
            }

            if !eoi_found && max_len > scan_start {
                eoi_offset = max_len;
            }
        }

        let file_size = eoi_offset as u64;

        // 3. Compute forensic confidence score
        let mut confidence: f64 = 0.0;

        // Base header and alignment
        if is_cluster_aligned {
            confidence += 0.40;
        } else {
            confidence += 0.30;
        }

        // Structural markers presence
        let mut markers_score = 0.0;
        if has_dqt { markers_score += 0.05; }
        if has_dht { markers_score += 0.05; }
        if has_sof { markers_score += 0.05; }
        if has_sos { markers_score += 0.05; }
        confidence += markers_score;

        // Metadata presence
        if has_exif_or_jfif {
            confidence += 0.10;
        }

        // Terminal EOI validation
        if eoi_found {
            confidence += 0.30;
        } else if file_size > 1024 {
            // Partial carving without clean footer
            confidence += 0.10;
        }

        if is_fragmented {
            confidence = (confidence * 0.85).min(0.95);
        }

        confidence = (confidence * 100.0).round() / 100.0;
        let is_valid = confidence >= 0.50 && file_size >= 64;

        if width > 0 && height > 0 {
            metadata.insert("dimensions".to_string(), format!("{}x{}", width, height));
        }
        if let Some(cam) = camera_model {
            metadata.insert("device".to_string(), cam);
        }
        metadata.insert("markers".to_string(), format!("DQT:{}, DHT:{}, SOF:{}, SOS:{}", has_dqt, has_dht, has_sof, has_sos));

        let fragments = if is_fragmented && gap_candidate > 0 {
            vec![(0, gap_candidate as u64), (gap_candidate as u64, (file_size - gap_candidate as u64))]
        } else {
            vec![(0, file_size)]
        };

        Some(JpegParseResult {
            is_valid,
            file_size,
            is_fragmented,
            fragments,
            confidence,
            metadata,
            detected_footer: eoi_found,
        })
    }
}
