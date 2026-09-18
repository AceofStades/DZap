pub mod crc32;
pub mod jpeg;
pub mod png;
pub mod mp4;

use std::collections::HashMap;
pub use jpeg::{JpegParser, JpegParseResult};
pub use png::{PngParser, PngParseResult};
pub use mp4::{Mp4Parser, Mp4ParseResult};

#[derive(Debug, Clone)]
pub struct UnifiedParseResult {
    pub file_type: &'static str,
    pub extension: &'static str,
    pub mime_type: &'static str,
    pub is_valid: bool,
    pub file_size: u64,
    pub is_fragmented: bool,
    pub fragments: Vec<(u64, u64)>,
    pub confidence: f64,
    pub metadata: HashMap<String, String>,
    pub detected_footer: bool,
}

pub struct ParserRegistry {
    pub jpeg: JpegParser,
    pub png: PngParser,
    pub mp4: Mp4Parser,
}

impl ParserRegistry {
    pub fn new() -> Self {
        Self {
            jpeg: JpegParser::new(),
            png: PngParser::new(),
            mp4: Mp4Parser::new(),
        }
    }

    /// Try parsing buffer against all enabled formats or specific formats.
    pub fn probe(&self, buffer: &[u8], is_cluster_aligned: bool, enabled_types: Option<&[String]>) -> Option<UnifiedParseResult> {
        let is_enabled = |ext: &str| -> bool {
            match enabled_types {
                None => true,
                Some(types) => types.iter().any(|t| t.eq_ignore_ascii_case(ext)),
            }
        };

        // Check JPEG
        if is_enabled("jpeg") || is_enabled("jpg") {
            if buffer.len() >= 3 && &buffer[0..3] == &[0xff, 0xd8, 0xff] {
                if let Some(res) = self.jpeg.parse(buffer, is_cluster_aligned) {
                    return Some(UnifiedParseResult {
                        file_type: "JPEG Image",
                        extension: "jpg",
                        mime_type: "image/jpeg",
                        is_valid: res.is_valid,
                        file_size: res.file_size,
                        is_fragmented: res.is_fragmented,
                        fragments: res.fragments,
                        confidence: res.confidence,
                        metadata: res.metadata,
                        detected_footer: res.detected_footer,
                    });
                }
            }
        }

        // Check PNG
        if is_enabled("png") {
            if buffer.len() >= 8 && &buffer[0..8] == &[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a] {
                if let Some(res) = self.png.parse(buffer, is_cluster_aligned) {
                    return Some(UnifiedParseResult {
                        file_type: "PNG Image",
                        extension: "png",
                        mime_type: "image/png",
                        is_valid: res.is_valid,
                        file_size: res.file_size,
                        is_fragmented: res.is_fragmented,
                        fragments: res.fragments,
                        confidence: res.confidence,
                        metadata: res.metadata,
                        detected_footer: res.detected_footer,
                    });
                }
            }
        }

        // Check MP4
        if is_enabled("mp4") || is_enabled("m4v") || is_enabled("mov") {
            if Mp4Parser::is_mp4_header(buffer) {
                if let Some(res) = self.mp4.parse(buffer, is_cluster_aligned) {
                    return Some(UnifiedParseResult {
                        file_type: "MP4 Video",
                        extension: "mp4",
                        mime_type: "video/mp4",
                        is_valid: res.is_valid,
                        file_size: res.file_size,
                        is_fragmented: res.is_fragmented,
                        fragments: res.fragments,
                        confidence: res.confidence,
                        metadata: res.metadata,
                        detected_footer: res.detected_footer,
                    });
                }
            }
        }

        None
    }
}
