// Lossless metadata removal for JPEG and PNG: drops the segments/chunks that carry
// EXIF (camera, GPS), XMP, IPTC and text comments, and copies the pixel data untouched.

/// Removes APP1 (EXIF/XMP), APP13 (IPTC), COM and other application segments.
/// Keeps APP0 (JFIF), APP2 ICC profiles and APP14 (Adobe, affects color decoding).
pub fn strip_jpeg(data: &[u8]) -> Option<Vec<u8>> {
    if data.len() < 4 || data[0] != 0xFF || data[1] != 0xD8 {
        return None;
    }
    let mut out = Vec::with_capacity(data.len());
    out.extend_from_slice(&data[..2]);
    let mut i = 2;

    loop {
        // Skip fill bytes before a marker
        while i < data.len() && data[i] == 0xFF && data.get(i + 1) == Some(&0xFF) {
            i += 1;
        }
        if i + 4 > data.len() || data[i] != 0xFF {
            return None;
        }
        let marker = data[i + 1];
        if marker == 0xDA {
            // Start of scan: the rest is entropy-coded image data
            out.extend_from_slice(&data[i..]);
            return Some(out);
        }
        let len = u16::from_be_bytes([data[i + 2], data[i + 3]]) as usize;
        let end = i + 2 + len;
        if len < 2 || end > data.len() {
            return None;
        }
        let payload = &data[i + 4..end];
        let keep = match marker {
            0xE0 | 0xEE => true,
            0xE2 => payload.starts_with(b"ICC_PROFILE\0"),
            0xE1 | 0xE3..=0xED | 0xEF | 0xFE => false,
            _ => true,
        };
        if keep {
            out.extend_from_slice(&data[i..end]);
        }
        i = end;
    }
}

/// Removes eXIf, tEXt, zTXt, iTXt and tIME chunks.
pub fn strip_png(data: &[u8]) -> Option<Vec<u8>> {
    const SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
    if !data.starts_with(SIGNATURE) {
        return None;
    }
    let mut out = Vec::with_capacity(data.len());
    out.extend_from_slice(SIGNATURE);
    let mut i = SIGNATURE.len();

    while i + 12 <= data.len() {
        let len = u32::from_be_bytes(data[i..i + 4].try_into().ok()?) as usize;
        let end = i.checked_add(12)?.checked_add(len)?;
        if end > data.len() {
            return None;
        }
        let kind = &data[i + 4..i + 8];
        if !matches!(kind, b"eXIf" | b"tEXt" | b"zTXt" | b"iTXt" | b"tIME") {
            out.extend_from_slice(&data[i..end]);
        }
        i = end;
        if kind == b"IEND" {
            return Some(out);
        }
    }
    None
}
