//! CI-safe tests: XBAR packs built from scratch with the `lzxc` encoder, using our
//! understanding of the container (docs/formats/bpk.md). No game data involved.

use pf_fmt_bpk::Bpk;

const WINDOW: usize = 0x20000;
const FRAME: usize = 0x8000;

enum Store {
    Raw,
    Lzx,
    /// Compressed, except a final partial window that is stored raw.
    LzxRawTail,
}

/// Compress one window-sized segment as an independent LZX stream.
fn stream(segment: &[u8]) -> Vec<u8> {
    let mut enc = lzxc::Encoder::new(lzxc::WindowSize::KB128);
    let mut out: Vec<u8> = Vec::new();
    for frame in segment.chunks(FRAME) {
        let chunk = enc.encode_chunk(frame);
        if frame.len() == FRAME {
            out.extend((chunk.len() as u16).to_be_bytes());
        } else {
            out.push(0xFF);
            out.extend((frame.len() as u16).to_be_bytes());
            out.extend((chunk.len() as u16).to_be_bytes());
        }
        out.extend(chunk);
    }
    out
}

fn store(data: &[u8], how: &Store) -> Vec<u8> {
    match how {
        Store::Raw => data.to_vec(),
        Store::Lzx | Store::LzxRawTail => {
            let mut out: Vec<u8> = Vec::new();
            let segments: Vec<_> = data.chunks(WINDOW).collect();
            for (i, seg) in segments.iter().enumerate() {
                if i > 0 {
                    out.extend([0u8; 5]);
                }
                let last = i == segments.len() - 1;
                if last && i > 0 && matches!(how, Store::LzxRawTail) {
                    out.extend_from_slice(seg);
                } else {
                    out.extend(stream(seg));
                }
            }
            out
        }
    }
}

fn build(files: &[(&[u8], Store)]) -> Vec<u8> {
    let data_offset = 0x800usize;
    let mut data: Vec<u8> = Vec::new();
    let mut table: Vec<u8> = Vec::new();
    for (i, (bytes, how)) in files.iter().enumerate() {
        let stored = store(bytes, how);
        let start = (data_offset + data.len()) as u32;
        data.extend(&stored);
        let end = (data_offset + data.len()) as u32;
        let id = (i * 3) as u32;
        let hash = (i as u64 + 1) * 0x0101_0101_0101_0101;
        for word in [id, bytes.len() as u32, start, end, (hash >> 32) as u32, hash as u32, 0] {
            table.extend(word.to_be_bytes());
        }
    }
    let mut out: Vec<u8> = Vec::new();
    out.extend(b"XBAR");
    let header = [6, files.len() as u32, 0, WINDOW as u32, files.len() as u32 * 3, data_offset as u32, data.len() as u32, 0, 0, 0, 0];
    for word in header {
        out.extend(word.to_be_bytes());
    }
    out.extend(table);
    out.resize(data_offset, 0);
    out.extend(data);
    out
}

/// Compressible but not trivial test content.
fn sample(len: usize, seed: u8) -> Vec<u8> {
    (0..len).map(|i| ((i / 7) as u8).wrapping_mul(seed) ^ (i % 13) as u8).collect()
}

#[test]
fn reads_raw_and_compressed_entries() {
    let raw = b"exec something.cfg\r\n".to_vec();
    let small = sample(0x1234, 3);
    let multi_window = sample(WINDOW * 2 + 0x4321, 5);
    let raw_tail = sample(WINDOW + 0x3c, 7);

    let pack = build(&[
        (&raw, Store::Raw),
        (&small, Store::Lzx),
        (&multi_window, Store::Lzx),
        (&raw_tail, Store::LzxRawTail),
    ]);
    let bpk = Bpk::parse(&pack).unwrap();
    assert_eq!(bpk.entries.len(), 4);
    assert!(!bpk.entries[0].is_compressed());
    assert!(bpk.entries[1..].iter().all(|e| e.is_compressed()));

    let expected = [&raw, &small, &multi_window, &raw_tail];
    for (entry, want) in bpk.entries.iter().zip(expected) {
        assert_eq!(&bpk.read(&pack, entry).unwrap(), want, "entry {}", entry.id);
    }
}

#[test]
fn garbage_is_an_error_not_a_panic() {
    assert!(Bpk::parse(&vec![0u8; 64]).is_err());

    let mut pack = build(&[(&sample(0x9000, 9), Store::Lzx)]);
    // Claim far more entries than the file can hold.
    pack[8..12].copy_from_slice(&0x0FFF_FFFFu32.to_be_bytes());
    assert!(Bpk::parse(&pack).is_err());

    // Corrupt the compressed data: reading must fail cleanly.
    let mut pack = build(&[(&sample(0x9000, 9), Store::Lzx)]);
    let len = pack.len();
    pack.truncate(len - 100);
    let end = (len - 100) as u32;
    pack[0x30 + 12..0x30 + 16].copy_from_slice(&end.to_be_bytes());
    let bpk = Bpk::parse(&pack).unwrap();
    assert!(bpk.read(&pack, &bpk.entries[0]).is_err());
}
