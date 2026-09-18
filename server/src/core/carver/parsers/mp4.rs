use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Mp4ParseResult {
    pub is_valid: bool,
    pub file_size: u64,
    pub is_fragmented: bool,
    pub fragments: Vec<(u64, u64)>,
    pub confidence: f64,
    pub metadata: HashMap<String, String>,
    pub detected_footer: bool,
}

pub struct Mp4Parser;

impl Mp4Parser {
    pub fn new() -> Self {
        Self
    }

    /// Tests if the first 12 bytes contain a valid ftyp box
    pub fn is_mp4_header(buffer: &[u8]) -> bool {
        if buffer.len() < 12 {
            return false;
        }
        &buffer[4..8] == b"ftyp"
    }

    pub fn parse(&self, buffer: &[u8], is_cluster_aligned: bool) -> Option<Mp4ParseResult> {
        if !Self::is_mp4_header(buffer) {
            return None;
        }

        let max_len = buffer.len();
        let mut offset: usize = 0;
        let mut has_ftyp = false;
        let mut has_moov = false;
        let mut has_mdat = false;
        let mut major_brand = String::new();
        let mut metadata = HashMap::new();
        let mut boxes_found: Vec<String> = Vec::new();

        while offset + 8 <= max_len {
            let raw_size = u32::from_be_bytes([
                buffer[offset],
                buffer[offset + 1],
                buffer[offset + 2],
                buffer[offset + 3],
            ]) as u64;

            let box_type = &buffer[offset + 4..offset + 8];
            let box_type_str = String::from_utf8_lossy(box_type).to_string();

            // Validate that box type consists of printable ASCII characters
            if !box_type.iter().all(|&b| (0x20..=0x7e).contains(&b)) {
                break;
            }

            let (box_size, header_len) = if raw_size == 1 {
                if offset + 16 > max_len {
                    break;
                }
                let extended = u64::from_be_bytes([
                    buffer[offset + 8],
                    buffer[offset + 9],
                    buffer[offset + 10],
                    buffer[offset + 11],
                    buffer[offset + 12],
                    buffer[offset + 13],
                    buffer[offset + 14],
                    buffer[offset + 15],
                ]);
                (extended, 16usize)
            } else if raw_size == 0 {
                // Box extends to end of available buffer
                (max_len as u64 - offset as u64, 8usize)
            } else {
                (raw_size, 8usize)
            };

            if box_size < header_len as u64 {
                break;
            }

            boxes_found.push(box_type_str.clone());

            match box_type {
                b"ftyp" => {
                    has_ftyp = true;
                    if offset + 12 <= max_len {
                        major_brand = String::from_utf8_lossy(&buffer[offset + 8..offset + 12]).trim().to_string();
                    }
                }
                b"moov" => {
                    has_moov = true;
                    // Probe for mvhd duration inside moov
                    let moov_data_start = offset + header_len;
                    let moov_data_end = (offset as u64 + box_size).min(max_len as u64) as usize;
                    if moov_data_end > moov_data_start + 24 {
                        let mut sub = moov_data_start;
                        while sub + 8 <= moov_data_end {
                            let sub_size = u32::from_be_bytes([buffer[sub], buffer[sub+1], buffer[sub+2], buffer[sub+3]]) as usize;
                            if &buffer[sub+4..sub+8] == b"mvhd" && sub + 24 <= moov_data_end {
                                let version = buffer[sub + 8];
                                if version == 0 && sub + 28 <= moov_data_end {
                                    let timescale = u32::from_be_bytes([buffer[sub+20], buffer[sub+21], buffer[sub+22], buffer[sub+23]]);
                                    let duration = u32::from_be_bytes([buffer[sub+24], buffer[sub+25], buffer[sub+26], buffer[sub+27]]);
                                    if timescale > 0 {
                                        let secs = duration as f64 / timescale as f64;
                                        metadata.insert("duration".to_string(), format!("{:.1}s", secs));
                                    }
                                }
                                break;
                            }
                            if sub_size < 8 || sub + sub_size > moov_data_end {
                                break;
                            }
                            sub += sub_size;
                        }
                    }
                }
                b"mdat" => {
                    has_mdat = true;
                }
                _ => {}
            }

            let next_offset = offset as u64 + box_size;
            if next_offset > max_len as u64 {
                // Partial box at end of buffer
                offset = max_len;
                break;
            }

            offset = next_offset as usize;

            // Common termination condition for MP4: once both moov and mdat are accounted for
            if has_ftyp && has_moov && has_mdat {
                // If the next bytes are unallocated/zeros, stop
                if offset < max_len && buffer[offset] == 0x00 {
                    break;
                }
            }
        }

        let file_size = offset as u64;
        if file_size < 32 {
            return None;
        }

        let mut confidence: f64 = 0.0;
        if is_cluster_aligned {
            confidence += 0.40;
        } else {
            confidence += 0.30;
        }

        if has_ftyp {
            confidence += 0.10;
        }
        if has_moov {
            confidence += 0.25;
        }
        if has_mdat {
            confidence += 0.25;
        }

        confidence = (confidence * 100.0).round() / 100.0;
        let is_valid = has_ftyp && (has_moov || has_mdat) && confidence >= 0.50;

        metadata.insert("format".to_string(), "MPEG-4 Part 14 (MP4)".to_string());
        if !major_brand.is_empty() {
            metadata.insert("brand".to_string(), major_brand);
        }
        metadata.insert("atoms".to_string(), boxes_found.join(" > "));

        Some(Mp4ParseResult {
            is_valid,
            file_size,
            is_fragmented: false,
            fragments: vec![(0, file_size)],
            confidence,
            metadata,
            detected_footer: has_moov && has_mdat,
        })
    }
}
