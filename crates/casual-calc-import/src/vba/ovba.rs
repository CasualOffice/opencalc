//! `[MS-OVBA]` §2.4.1 decompression: the LZ77-style `CompressedContainer`
//! every VBA source text and the `dir` stream are stored in.
//!
//! A container is a signature byte `0x01` followed by chunks. Each chunk has a
//! two-byte header (size, signature `0b011`, compressed flag) and decompresses
//! to at most 4096 bytes. A compressed chunk is a run of token sequences: a
//! flag byte, then eight tokens, each a literal byte or a two-byte copy token
//! whose offset/length split depends on how far into the chunk the output is.
//!
//! # Hostile input
//!
//! The output is bounded by a caller-held **budget** shared across every
//! container in a project, so a file of many small, highly compressible
//! streams cannot together expand past it; a copy token reaching before the
//! start of its chunk is an error, not an out-of-bounds read; and a chunk that
//! decompresses past 4096 bytes is refused.

use super::VbaError;

/// Decompressed size of one chunk.
const CHUNK_LEN: usize = 4096;

/// Decompress the container in `data`, charging every byte produced to
/// `budget`. Exhausting the budget is [`VbaError::DecompressedTooLarge`].
pub(crate) fn decompress(data: &[u8], budget: &mut usize) -> Result<Vec<u8>, VbaError> {
    let bad = VbaError::BadCompressedContainer;
    match data.first() {
        Some(0x01) => {}
        _ => return Err(bad("no 0x01 signature")),
    }
    let mut out: Vec<u8> = Vec::new();
    let mut pos = 1usize;
    while pos < data.len() {
        let header = data
            .get(pos..pos + 2)
            .map(|h| u16::from_le_bytes([h[0], h[1]]))
            .ok_or(bad("truncated chunk header"))?;
        pos += 2;
        let chunk_size = usize::from(header & 0x0FFF) + 3;
        if (header >> 12) & 0b111 != 0b011 {
            return Err(bad("chunk signature is not 0b011"));
        }
        let compressed = header & 0x8000 != 0;
        // The chunk's data ends `chunk_size` bytes after its header began; the
        // last chunk of a stream may be cut short by the stream's end.
        let end = (pos - 2 + chunk_size).min(data.len());
        let chunk_start = out.len();

        if !compressed {
            // An uncompressed chunk is always 4096 raw bytes.
            let raw = data
                .get(pos..pos + CHUNK_LEN)
                .ok_or(bad("truncated uncompressed chunk"))?;
            charge(budget, raw.len())?;
            out.extend_from_slice(raw);
            pos += CHUNK_LEN;
            continue;
        }

        while pos < end {
            let flags = data[pos];
            pos += 1;
            for bit in 0..8 {
                if pos >= end {
                    break;
                }
                if flags & (1 << bit) == 0 {
                    charge(budget, 1)?;
                    out.push(data[pos]);
                    pos += 1;
                } else {
                    let token = data
                        .get(pos..pos + 2)
                        .filter(|_| pos + 2 <= end)
                        .map(|t| u16::from_le_bytes([t[0], t[1]]))
                        .ok_or(bad("truncated copy token"))?;
                    pos += 2;
                    let difference = out.len() - chunk_start;
                    if difference == 0 {
                        return Err(bad("copy token before any output"));
                    }
                    // `[MS-OVBA]` §2.4.1.3.19.1: the offset field gets
                    // ceil(log2(difference)) bits, at least 4.
                    let bit_count =
                        ((usize::BITS - (difference - 1).leading_zeros()) as usize).max(4);
                    let length_mask = 0xFFFFu16 >> bit_count;
                    let offset = usize::from(token >> (16 - bit_count)) + 1;
                    let length = usize::from(token & length_mask) + 3;
                    if offset > difference {
                        return Err(bad("copy token reaches before its chunk"));
                    }
                    charge(budget, length)?;
                    let from = out.len() - offset;
                    // Byte by byte: the source may overlap what is being written,
                    // which is how a run of one repeated byte is encoded.
                    for k in 0..length {
                        let b = out[from + k];
                        out.push(b);
                    }
                }
                if out.len() - chunk_start > CHUNK_LEN {
                    return Err(bad("a chunk decompresses past 4096 bytes"));
                }
            }
        }
        pos = end;
    }
    Ok(out)
}

fn charge(budget: &mut usize, n: usize) -> Result<(), VbaError> {
    *budget = budget
        .checked_sub(n)
        .ok_or(VbaError::DecompressedTooLarge)?;
    Ok(())
}
