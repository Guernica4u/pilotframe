//! LZHAM decompression for Titanfall PC VPK chunks.
//!
//! Respawn compressed VPK chunks with LZHAM "alpha" using a 2^20-byte (1 MB)
//! dictionary. Each chunk is an independent stream, and the VPK directory gives
//! its exact uncompressed size, so we decode straight into a buffer of that size.
//!
//! This wraps the original C++ library (MIT) through `lzham-alpha-sys`. It's
//! native only; a pure-Rust decoder could replace it later for the web build.

use std::io;

/// Dictionary size used for Titanfall VPKs.
pub const DICT_SIZE_LOG2: u32 = 20;

/// Decompress one chunk. `size` is the expected uncompressed length.
pub fn decompress(src: &[u8], size: usize) -> io::Result<Vec<u8>> {
    use lzham_alpha_sys as sys;

    let params = sys::lzham_decompress_params {
        m_struct_size: std::mem::size_of::<sys::lzham_decompress_params>() as u32,
        m_dict_size_log2: DICT_SIZE_LOG2,
        m_decompress_flags: sys::lzham_decompress_flags_LZHAM_DECOMP_FLAG_OUTPUT_UNBUFFERED as u32,
        m_num_seed_bytes: 0,
        m_pSeed_bytes: std::ptr::null(),
    };
    let mut out = vec![0u8; size];
    let mut out_len = size;
    let mut adler = 0u32;

    // SAFETY: every pointer refers to a live buffer of the length passed with it,
    // `params` is fully initialised, and the library doesn't keep any pointer
    // after it returns.
    #[allow(unsafe_code)]
    let status = unsafe {
        sys::lzham_decompress_memory(
            &params,
            out.as_mut_ptr(),
            &mut out_len,
            src.as_ptr(),
            src.len(),
            &mut adler,
        )
    };

    if status != sys::lzham_decompress_status_t_LZHAM_DECOMP_STATUS_SUCCESS {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("LZHAM decompression failed (status {status})"),
        ));
    }
    if out_len != size {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("LZHAM produced {out_len} bytes, expected {size}"),
        ));
    }
    Ok(out)
}
