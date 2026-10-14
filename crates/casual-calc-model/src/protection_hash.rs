//! The password hashes OOXML protection carries, for Protect / Unprotect Sheet
//! (`XL-UX-023`).
//!
//! Two forms reach a file. Excel 2013 and later write the agile one —
//! `algorithmName="SHA-512" hashValue saltValue spinCount` — and everything
//! before (and most other writers) the legacy 16-bit `password="CC1A"`. A sheet
//! protected here gets the legacy form: Excel reads it, and it is the one the
//! schema defines for `sheetProtection` itself. Both are *verified*, so a sheet
//! protected in Excel can be unprotected here with its password and not
//! without it.
//!
//! Neither is security in any cryptographic sense — the legacy hash has 32 768
//! values and Excel's protection is a lock on a door with no walls — but the
//! password is never kept in clear, and a wrong one is refused.

/// Excel's legacy sheet-protection hash (ECMA-376 Part 4, §14.7.1), as the four
/// upper-case hex digits written to `password`. `None` for the empty password,
/// which Excel writes as no attribute at all.
#[must_use]
pub fn legacy_password_hash(password: &str) -> Option<String> {
    if password.is_empty() {
        return None;
    }
    // The algorithm works on the low byte of each character; Excel truncates
    // the password to 15 characters in the dialog.
    let bytes: Vec<u16> = password.encode_utf16().take(15).map(|c| c & 0xff).collect();
    let mut hash: u16 = 0;
    for &b in bytes.iter().rev() {
        hash = ((hash >> 14) & 1) | ((hash << 1) & 0x7fff);
        hash ^= b;
    }
    hash = ((hash >> 14) & 1) | ((hash << 1) & 0x7fff);
    // At most 15, so the cast cannot truncate.
    hash ^= bytes.len() as u16;
    hash ^= 0xCE4B;
    Some(format!("{hash:04X}"))
}

/// The agile hash: `H0 = SHA-512(salt ‖ UTF-16LE(password))`, then
/// `Hn = SHA-512(Hn-1 ‖ LE32(n-1))` for `spin` rounds.
#[must_use]
pub fn agile_password_hash(password: &str, salt: &[u8], spin: u32) -> [u8; 64] {
    let mut data = salt.to_vec();
    for unit in password.encode_utf16() {
        data.extend_from_slice(&unit.to_le_bytes());
    }
    let mut h = sha512(&data);
    let mut buf = [0u8; 68];
    for i in 0..spin {
        buf[..64].copy_from_slice(&h);
        buf[64..].copy_from_slice(&i.to_le_bytes());
        h = sha512(&buf);
    }
    h
}

/// Standard base64 (with padding), as `saltValue` and `hashValue` are written.
#[must_use]
pub fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let value = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => u32::from(c - b'A'),
            b'a'..=b'z' => u32::from(c - b'a') + 26,
            b'0'..=b'9' => u32::from(c - b'0') + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        })
    };
    let clean: Vec<u8> = text.bytes().filter(|c| !c.is_ascii_whitespace()).collect();
    let body: &[u8] = {
        let mut end = clean.len();
        while end > 0 && clean[end - 1] == b'=' {
            end -= 1;
        }
        &clean[..end]
    };
    let mut out = Vec::with_capacity(body.len() * 3 / 4);
    let mut acc = 0u32;
    let mut bits = 0;
    for &c in body {
        acc = (acc << 6) | value(c)?;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            // Masked to a byte.
            out.push(((acc >> bits) & 0xff) as u8);
        }
    }
    Some(out)
}

/// SHA-512 (FIPS 180-4). Here rather than as a dependency: the model crate has
/// none beyond serde, and this is the only digest the project needs.
#[must_use]
pub fn sha512(data: &[u8]) -> [u8; 64] {
    const K: [u64; 80] = [
        0x428a2f98d728ae22,
        0x7137449123ef65cd,
        0xb5c0fbcfec4d3b2f,
        0xe9b5dba58189dbbc,
        0x3956c25bf348b538,
        0x59f111f1b605d019,
        0x923f82a4af194f9b,
        0xab1c5ed5da6d8118,
        0xd807aa98a3030242,
        0x12835b0145706fbe,
        0x243185be4ee4b28c,
        0x550c7dc3d5ffb4e2,
        0x72be5d74f27b896f,
        0x80deb1fe3b1696b1,
        0x9bdc06a725c71235,
        0xc19bf174cf692694,
        0xe49b69c19ef14ad2,
        0xefbe4786384f25e3,
        0x0fc19dc68b8cd5b5,
        0x240ca1cc77ac9c65,
        0x2de92c6f592b0275,
        0x4a7484aa6ea6e483,
        0x5cb0a9dcbd41fbd4,
        0x76f988da831153b5,
        0x983e5152ee66dfab,
        0xa831c66d2db43210,
        0xb00327c898fb213f,
        0xbf597fc7beef0ee4,
        0xc6e00bf33da88fc2,
        0xd5a79147930aa725,
        0x06ca6351e003826f,
        0x142929670a0e6e70,
        0x27b70a8546d22ffc,
        0x2e1b21385c26c926,
        0x4d2c6dfc5ac42aed,
        0x53380d139d95b3df,
        0x650a73548baf63de,
        0x766a0abb3c77b2a8,
        0x81c2c92e47edaee6,
        0x92722c851482353b,
        0xa2bfe8a14cf10364,
        0xa81a664bbc423001,
        0xc24b8b70d0f89791,
        0xc76c51a30654be30,
        0xd192e819d6ef5218,
        0xd69906245565a910,
        0xf40e35855771202a,
        0x106aa07032bbd1b8,
        0x19a4c116b8d2d0c8,
        0x1e376c085141ab53,
        0x2748774cdf8eeb99,
        0x34b0bcb5e19b48a8,
        0x391c0cb3c5c95a63,
        0x4ed8aa4ae3418acb,
        0x5b9cca4f7763e373,
        0x682e6ff3d6b2b8a3,
        0x748f82ee5defb2fc,
        0x78a5636f43172f60,
        0x84c87814a1f0ab72,
        0x8cc702081a6439ec,
        0x90befffa23631e28,
        0xa4506cebde82bde9,
        0xbef9a3f7b2c67915,
        0xc67178f2e372532b,
        0xca273eceea26619c,
        0xd186b8c721c0c207,
        0xeada7dd6cde0eb1e,
        0xf57d4f7fee6ed178,
        0x06f067aa72176fba,
        0x0a637dc5a2c898a6,
        0x113f9804bef90dae,
        0x1b710b35131c471b,
        0x28db77f523047d84,
        0x32caab7b40c72493,
        0x3c9ebe0a15c9bebc,
        0x431d67c49c100d4c,
        0x4cc5d4becb3e42b6,
        0x597f299cfc657e2a,
        0x5fcb6fab3ad6faec,
        0x6c44198c4a475817,
    ];
    let mut h: [u64; 8] = [
        0x6a09e667f3bcc908,
        0xbb67ae8584caa73b,
        0x3c6ef372fe94f82b,
        0xa54ff53a5f1d36f1,
        0x510e527fade682d1,
        0x9b05688c2b3e6c1f,
        0x1f83d9abfb41bd6b,
        0x5be0cd19137e2179,
    ];
    let mut msg = data.to_vec();
    let bit_len = (data.len() as u128) * 8;
    msg.push(0x80);
    while msg.len() % 128 != 112 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());
    for block in msg.chunks_exact(128) {
        let mut w = [0u64; 80];
        for (i, word) in block.chunks_exact(8).enumerate() {
            let mut b = [0u8; 8];
            b.copy_from_slice(word);
            w[i] = u64::from_be_bytes(b);
        }
        for i in 16..80 {
            let s0 = w[i - 15].rotate_right(1) ^ w[i - 15].rotate_right(8) ^ (w[i - 15] >> 7);
            let s1 = w[i - 2].rotate_right(19) ^ w[i - 2].rotate_right(61) ^ (w[i - 2] >> 6);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..80 {
            let s1 = e.rotate_right(14) ^ e.rotate_right(18) ^ e.rotate_right(41);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(28) ^ a.rotate_right(34) ^ a.rotate_right(39);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (slot, v) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *slot = slot.wrapping_add(v);
        }
    }
    let mut out = [0u8; 64];
    for (i, v) in h.iter().enumerate() {
        out[i * 8..i * 8 + 8].copy_from_slice(&v.to_be_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// The values Excel itself writes: `password="83AF"` for "password",
    /// `CBEB` for "test" — the published reference pair for this algorithm.
    #[test]
    fn the_legacy_hash_matches_excel() {
        assert_eq!(legacy_password_hash("password").as_deref(), Some("83AF"));
        assert_eq!(legacy_password_hash("test").as_deref(), Some("CBEB"));
        assert_eq!(legacy_password_hash("abc").as_deref(), Some("CC1A"));
        assert_eq!(legacy_password_hash(""), None);
    }

    #[test]
    fn sha512_matches_fips_180_vectors() {
        assert_eq!(
            hex(&sha512(b"abc")),
            "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a\
             2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f"
        );
        assert_eq!(
            hex(&sha512(b"")),
            "cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce\
             47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e"
        );
        // Two blocks: the 112-byte padding boundary.
        let long = vec![b'a'; 200];
        assert_eq!(
            hex(&sha512(&long)),
            "4b11459c33f52a22ee8236782714c150a3b2c60994e9acee17fe68947a3e6789\
             f31e7668394592da7bef827cddca88c4e6f86e4df7ed1ae6cba71f3e98faee9f"
        );
    }

    /// An agile hash computed independently (Python `hashlib`, the same
    /// construction) for password "secret", salt 16 bytes 0..15, 1000 spins.
    #[test]
    fn the_agile_hash_matches_an_independent_computation() {
        let salt: Vec<u8> = (0..16).collect();
        let got = agile_password_hash("secret", &salt, 1000);
        assert_eq!(hex(&got), AGILE_SECRET);
        assert_eq!(base64_decode("AAECAwQFBgcICQoLDA0ODw==").unwrap(), salt);
    }

    const AGILE_SECRET: &str = "6de46c3845b98609df1a90b4afe81dbc8a64587d19d4c310773bdb4c5b1c2b052cfcff1c8229152c88d1265895c4f345218f082963f0f5f4de9eb34541c3d461";
}

#[cfg(test)]
mod sheet_protection_tests {
    use crate::SheetProtection;

    /// What Protect Sheet writes is what Excel writes for the same dialog.
    #[test]
    fn protect_writes_excels_attributes() {
        let p = SheetProtection::protect("password", &["selectLockedCells", "selectUnlockedCells"]);
        let attrs: Vec<(&str, &str)> = p
            .attrs
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        // Excel's own output for the default dialog with password "password":
        // `<sheetProtection password="83AF" sheet="1" objects="1" scenarios="1"/>`.
        assert_eq!(
            attrs,
            [
                ("objects", "1"),
                ("password", "83AF"),
                ("scenarios", "1"),
                ("sheet", "1")
            ]
        );
        assert!(p.allows("selectLockedCells"));
        assert!(!p.allows("formatCells"));
        assert!(!p.allows("objects"));
        let q = SheetProtection::protect("", &["formatColumns", "sort"]);
        assert_eq!(q.attrs.get("formatColumns").map(String::as_str), Some("0"));
        assert_eq!(
            q.attrs.get("selectLockedCells").map(String::as_str),
            Some("1")
        );
        assert!(
            q.permits("formatColumns"),
            "the resize guard reads it the same way"
        );
        assert!(!q.has_password() && q.verify_password("anything"));
    }

    #[test]
    fn a_password_is_verified_never_stored() {
        let p = SheetProtection::protect("s3cret", &[]);
        assert!(!p.attrs.values().any(|v| v.contains("s3cret")));
        assert!(p.has_password());
        assert!(p.verify_password("s3cret"));
        assert!(!p.verify_password("S3cret"));
        assert!(!p.verify_password(""));
    }

    /// A sheet Excel 2013+ protected: the agile SHA-512 form is checked too.
    #[test]
    fn an_excel_agile_hash_is_verified() {
        let mut p = SheetProtection::enabled();
        for (k, v) in [
            ("algorithmName", "SHA-512"),
            ("saltValue", "AAECAwQFBgcICQoLDA0ODw=="),
            ("spinCount", "1000"),
            (
                "hashValue",
                "beRsOEW5hgnfGpC0r+gdvIpkWH0Z1MMQdzvbTFscKwUs/P8cgikVLIjRJliVxPNFIY8IKWPw9fTenrNFQcPUYQ==",
            ),
        ] {
            p.attrs.insert(k.to_owned(), v.to_owned());
        }
        assert!(p.has_password());
        assert!(p.verify_password("secret"));
        assert!(!p.verify_password("Secret"));
    }
}
