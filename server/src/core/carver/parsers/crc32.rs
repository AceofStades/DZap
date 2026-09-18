// Standard IEEE 802.3 CRC32 implementation for PNG chunk validation.
// Fast table-driven computation with precalculated polynomial 0xEDB88320.

const CRC_TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            if c & 1 != 0 {
                c = 0xedb88320 ^ (c >> 1);
            } else {
                c >>= 1;
            }
            k += 1;
        }
        table[i] = c;
        i += 1;
    }
    table
};

pub fn crc32(data: &[u8]) -> u32 {
    let mut c = 0xffff_ffff;
    for &byte in data {
        c = CRC_TABLE[((c ^ byte as u32) & 0xff) as usize] ^ (c >> 8);
    }
    c ^ 0xffff_ffff
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crc32_standard() {
        assert_eq!(crc32(b""), 0);
        // "123456789" is the standard CRC32 check vector -> 0xCBF43926
        assert_eq!(crc32(b"123456789"), 0xcbf43926);
    }
}
