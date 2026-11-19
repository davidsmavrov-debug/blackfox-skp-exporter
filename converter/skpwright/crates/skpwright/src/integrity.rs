//! The word `meta/meta.dat` contributes to the header's integrity word
//! (§16.15). It depends on the number of archive entries, as
//! `k = ceil(entries / 2)`, and on the last four bytes of `meta.dat`
//! (the ones the integrity word's CRC leaves out): `V(1)` is
//! `0xFFFFFFFF`, `V(2)` is 0 and every further `V(k + 1)` is `V(k)`
//! complemented and run back through four zero bytes of the reflected
//! CRC-32 register (multiplied by x⁻³² modulo the CRC polynomial); the
//! word is `V(k)` xor the register after those four bytes, from 0 and
//! without the CRC's inversions. The bytes are 0 in the files of SketchUp
//! 2026 and the web app (the high half of their last history entry's
//! u64), which leaves `V(k)`; SketchUp 2027's `8d 5c 6a b4` give
//! `0x298B4D8A`. The rule reproduces every value read off SketchUp's
//! files (`docs/SKP_FORMAT.md` §16.15).

/// The reflected CRC-32 table (polynomial `0xEDB88320`).
fn table() -> [u32; 256] {
    let mut t = [0u32; 256];
    for (i, slot) in t.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 {
                (c >> 1) ^ 0xEDB8_8320
            } else {
                c >> 1
            };
        }
        *slot = c;
    }
    t
}

/// The register of the reflected CRC-32 after `bytes`, from 0 and
/// without the pre- and post-inversion of the checksum.
fn register(bytes: &[u8]) -> u32 {
    let t = table();
    bytes
        .iter()
        .fold(0u32, |r, &b| (r >> 8) ^ t[((r ^ b as u32) & 0xFF) as usize])
}

/// The word for `k = ceil(entries / 2)` and `tail`, the last four bytes
/// of `meta.dat`.
pub(crate) fn meta_word(k: usize, tail: [u8; 4]) -> Option<u32> {
    if k == 0 {
        return None;
    }
    let t = table();
    // The inverse of a register step `r = (r >> 8) ^ T[r & 0xFF]`: the
    // table's top bytes are all different, so the top byte of `r` names
    // the index.
    let mut by_top = [0u8; 256];
    for (i, e) in t.iter().enumerate() {
        by_top[(e >> 24) as usize] = i as u8;
    }
    let unshift = |mut r: u32| {
        for _ in 0..4 {
            let i = by_top[(r >> 24) as usize];
            r = ((r ^ t[i as usize]) << 8) | i as u32;
        }
        r
    };
    let mut v = if k == 1 { 0xFFFF_FFFF } else { 0 };
    for _ in 2..k {
        v = unshift(v ^ 0xFFFF_FFFF);
    }
    Some(v ^ register(&tail))
}

#[cfg(test)]
mod tests {
    use super::{meta_word, register};

    /// The last four bytes of a `meta.dat` of each writer line.
    const WEB: [u8; 4] = [0; 4];
    const DESKTOP: [u8; 4] = [0x8d, 0x5c, 0x6a, 0xb4];

    #[test]
    fn the_desktop_tail_gives_the_observed_difference() {
        assert_eq!(register(&DESKTOP), 0x298B_4D8A);
        assert_eq!(register(&WEB), 0);
    }

    /// Values read off files SketchUp wrote.
    #[test]
    fn the_recurrence_reproduces_the_observed_constants() {
        for (k, v) in [
            (5, 0xdb9e_1da6),
            (6, 0xe339_138e),
            (7, 0xc509_b474),
            (8, 0x771f_0805),
            (16, 0x4247_5460),
            (18, 0x9e2e_941c),
            (21, 0x94a5_71dd),
        ] {
            assert_eq!(meta_word(k, WEB), Some(v), "{{26. k={k}");
        }
        for (k, v) in [
            (4, 0x5449_6847),
            (5, 0xf215_502c),
            (6, 0xcab2_5e04),
            (7, 0xec82_f9fe),
            (8, 0x5e94_458f),
            (9, 0x586c_40d3),
            (10, 0x960a_3063),
            (11, 0x4f83_9ad0),
            (12, 0xd239_fe97),
            (13, 0xfe25_47f6),
            (14, 0x3fd0_ce74),
            (15, 0xa10d_3a64),
            (16, 0x6bcc_19ea),
            (17, 0x2a11_fd11),
            (18, 0xb7a5_d996),
            (19, 0xb29f_2b49),
            (20, 0x736b_cea7),
            (21, 0xbd2e_3c57),
            (22, 0xcc92_4678),
            (23, 0x3971_e2ea),
            (24, 0x14aa_ad3e),
            (25, 0x163c_2e92),
            (26, 0x4463_3dc8),
            (27, 0x70ff_bd55),
        ] {
            assert_eq!(meta_word(k, DESKTOP), Some(v), "{{27. k={k}");
        }
    }
}
