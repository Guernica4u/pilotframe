//! `.vfont`: a TrueType/OpenType font, lightly scrambled. Layout (from reading
//! the game's font loader): the file ends with the tag `VFONT1`. The byte before
//! the tag, `n`, is the length of a trailer (including that byte) that follows the
//! font data; the trailer's other `n - 1` bytes are a salt. The key starts at
//! 0xA7 and is xor-ed with each salt byte + 0xA7. Each data byte is then xor-ed
//! with the key, and the key becomes that (scrambled) byte + 0xA7.

const TAG: &[u8] = b"VFONT1";

/// The plain font file, or `None` if this isn't a `.vfont`.
pub fn decode(file: &[u8]) -> Option<Vec<u8>> {
    let body = file.strip_suffix(TAG)?;
    let n = usize::from(*body.last()?);
    let data_len = body.len().checked_sub(n)?;
    let (data, trailer) = body.split_at(data_len);
    let salt = &trailer[..n.saturating_sub(1)];
    let mut key = salt.iter().fold(0xA7u8, |k, &s| k ^ s.wrapping_add(0xA7));
    Some(
        data.iter()
            .map(|&b| {
                let plain = b ^ key;
                key = b.wrapping_add(0xA7);
                plain
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Scramble the way the format does, so the test needs no game files.
    fn encode(font: &[u8], salt: &[u8]) -> Vec<u8> {
        let mut key = salt.iter().fold(0xA7u8, |k, &s| k ^ s.wrapping_add(0xA7));
        let mut out: Vec<u8> = font
            .iter()
            .map(|&p| {
                let b = p ^ key;
                key = b.wrapping_add(0xA7);
                b
            })
            .collect();
        out.extend_from_slice(salt);
        out.push(salt.len() as u8 + 1);
        out.extend_from_slice(TAG);
        out
    }

    #[test]
    fn round_trips() {
        let font = b"\x00\x01\x00\x00 a pretend TrueType font".to_vec();
        let file = encode(&font, b"salty bytes");
        assert_eq!(decode(&file).unwrap(), font);
    }

    #[test]
    fn rejects_other_files() {
        assert!(decode(b"\x00\x01\x00\x00plain ttf").is_none());
    }
}
