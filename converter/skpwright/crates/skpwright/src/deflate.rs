//! A DEFLATE encoder that reproduces zlib bit for bit at the two settings
//! SketchUp uses for its 2026 archive (`docs/WRITING.md`): level 1
//! (`deflate_fast`) for `.dat` and `.xml` entries, level 9
//! (`deflate_slow`) for hatch pattern definitions and level 0 (stored
//! blocks) for images and `.skc` entries, with the caller's 64 KiB output
//! buffer, whose size shapes the stored blocks. Every stream in the corpus
//! re-encodes to its original bytes (the unit tests below), which is what
//! makes a re-packed archive byte-identical to the file SketchUp saved.
//!
//! The algorithm is zlib's (public, RFC 1951 plus zlib's own heuristics:
//! hash chains, block flushing by symbol count, the tree-length overflow
//! rule, the stored/static/dynamic choice), written out here rather than
//! taken from a compression crate because no crate promises zlib's exact
//! output. Only these three configurations are implemented; the encoder is
//! not general-purpose.

const W_BITS: usize = 15;
const W_SIZE: usize = 1 << W_BITS;
const W_MASK: usize = W_SIZE - 1;
const WINDOW_SIZE: usize = 2 * W_SIZE;
const MEM_LEVEL: usize = 8;
const HASH_BITS: usize = MEM_LEVEL + 7;
const HASH_SIZE: usize = 1 << HASH_BITS;
const HASH_MASK: usize = HASH_SIZE - 1;
const HASH_SHIFT: usize = HASH_BITS.div_ceil(MIN_MATCH);
const LIT_BUFSIZE: usize = 1 << (MEM_LEVEL + 6);
const PENDING_BUF_SIZE: usize = LIT_BUFSIZE * 4;
const SYM_END: usize = (LIT_BUFSIZE - 1) * 3;

const MIN_MATCH: usize = 3;
const MAX_MATCH: usize = 258;
const MIN_LOOKAHEAD: usize = MAX_MATCH + MIN_MATCH + 1;
const MAX_DIST: usize = W_SIZE - MIN_LOOKAHEAD;
const WIN_INIT: usize = MAX_MATCH;
const MAX_STORED: usize = 65535;
const NIL: u16 = 0;

/// zlib's configuration table for the levels reproduced: good, lazy,
/// nice, chain. Level 1 runs `deflate_fast`, level 9 `deflate_slow`.
fn config(level: u8) -> (usize, usize, usize, usize) {
    match level {
        9 => (32, 258, 258, 4096),
        _ => (4, 4, 8, 4),
    }
}

/// `deflate_slow` drops a 3-byte match this far back.
const TOO_FAR: usize = 4096;

const LENGTH_CODES: usize = 29;
const LITERALS: usize = 256;
const L_CODES: usize = LITERALS + 1 + LENGTH_CODES;
const D_CODES: usize = 30;
const BL_CODES: usize = 19;
const HEAP_SIZE: usize = 2 * L_CODES + 1;
const MAX_BITS: usize = 15;
const MAX_BL_BITS: usize = 7;
const END_BLOCK: usize = 256;
const REP_3_6: usize = 16;
const REPZ_3_10: usize = 17;
const REPZ_11_138: usize = 18;
const STORED_BLOCK: u32 = 0;
const STATIC_TREES: u32 = 1;
const DYN_TREES: u32 = 2;
const BUF_SIZE: usize = 16;

const EXTRA_LBITS: [u8; LENGTH_CODES] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const EXTRA_DBITS: [u8; D_CODES] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];
const EXTRA_BLBITS: [u8; BL_CODES] = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 3, 7];
const BL_ORDER: [usize; BL_CODES] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

/// Flush modes (zlib's values).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Flush {
    None,
    Finish,
}

#[derive(Clone, Copy, Default)]
struct Ct {
    freq_or_code: u16,
    len_or_dad: u16,
}

/// The static tables zlib builds in `tr_static_init`.
struct Static {
    ltree: [Ct; L_CODES + 2],
    dtree: [Ct; D_CODES],
    dist_code: [u8; 512],
    length_code: [u8; 256],
    base_length: [u16; LENGTH_CODES],
    base_dist: [u16; D_CODES],
}

fn statics() -> &'static Static {
    static S: std::sync::OnceLock<Static> = std::sync::OnceLock::new();
    S.get_or_init(|| {
        let mut length_code = [0u8; 256];
        let mut base_length = [0u16; LENGTH_CODES];
        let mut length = 0usize;
        for code in 0..LENGTH_CODES - 1 {
            base_length[code] = length as u16;
            for _ in 0..(1 << EXTRA_LBITS[code]) {
                length_code[length] = code as u8;
                length += 1;
            }
        }
        // Length 255 (match 258) is coded as 285, not 284 + 5 bits.
        length_code[length - 1] = (LENGTH_CODES - 1) as u8;
        let mut dist_code = [0u8; 512];
        let mut base_dist = [0u16; D_CODES];
        let mut dist = 0usize;
        for code in 0..16 {
            base_dist[code] = dist as u16;
            for _ in 0..(1 << EXTRA_DBITS[code]) {
                dist_code[dist] = code as u8;
                dist += 1;
            }
        }
        dist >>= 7;
        for code in 16..D_CODES {
            base_dist[code] = (dist << 7) as u16;
            for _ in 0..(1 << (EXTRA_DBITS[code] - 7)) {
                dist_code[256 + dist] = code as u8;
                dist += 1;
            }
        }
        let mut bl_count = [0u16; MAX_BITS + 1];
        let mut ltree = [Ct::default(); L_CODES + 2];
        let mut n = 0;
        while n <= 143 {
            ltree[n].len_or_dad = 8;
            n += 1;
            bl_count[8] += 1;
        }
        while n <= 255 {
            ltree[n].len_or_dad = 9;
            n += 1;
            bl_count[9] += 1;
        }
        while n <= 279 {
            ltree[n].len_or_dad = 7;
            n += 1;
            bl_count[7] += 1;
        }
        while n <= 287 {
            ltree[n].len_or_dad = 8;
            n += 1;
            bl_count[8] += 1;
        }
        gen_codes(&mut ltree, L_CODES + 1, &bl_count);
        let mut dtree = [Ct::default(); D_CODES];
        for (n, d) in dtree.iter_mut().enumerate() {
            d.len_or_dad = 5;
            d.freq_or_code = bi_reverse(n as u32, 5) as u16;
        }
        Static {
            ltree,
            dtree,
            dist_code,
            length_code,
            base_length,
            base_dist,
        }
    })
}

fn bi_reverse(mut code: u32, mut len: usize) -> u32 {
    let mut res = 0u32;
    loop {
        res |= code & 1;
        code >>= 1;
        res <<= 1;
        len -= 1;
        if len == 0 {
            break;
        }
    }
    res >> 1
}

fn gen_codes(tree: &mut [Ct], max_code: usize, bl_count: &[u16; MAX_BITS + 1]) {
    let mut next_code = [0u16; MAX_BITS + 1];
    let mut code = 0u32;
    for bits in 1..=MAX_BITS {
        code = (code + bl_count[bits - 1] as u32) << 1;
        next_code[bits] = code as u16;
    }
    for ct in tree.iter_mut().take(max_code + 1) {
        let len = ct.len_or_dad as usize;
        if len == 0 {
            continue;
        }
        ct.freq_or_code = bi_reverse(next_code[len] as u32, len) as u16;
        next_code[len] += 1;
    }
}

/// Which tree a `TreeDesc` describes.
#[derive(Clone, Copy)]
enum Which {
    Lit,
    Dist,
    Bl,
}

/// zlib's `deflate_state` — the parts that shape the output.
struct State {
    level: u8,
    good_match: usize,
    max_lazy: usize,
    nice_match: usize,
    max_chain: usize,
    match_available: bool,
    prev_match: usize,
    // pending output
    pending_buf: Vec<u8>,
    pending: usize,
    pending_out: usize,
    // window
    window: Vec<u8>,
    prev: Vec<u16>,
    head: Vec<u16>,
    ins_h: usize,
    block_start: i64,
    strstart: usize,
    lookahead: usize,
    insert: usize,
    match_start: usize,
    match_length: usize,
    prev_length: usize,
    high_water: usize,
    matches: u32,
    // trees
    dyn_ltree: [Ct; HEAP_SIZE],
    dyn_dtree: [Ct; 2 * D_CODES + 1],
    bl_tree: [Ct; 2 * BL_CODES + 1],
    l_max_code: usize,
    d_max_code: usize,
    bl_max_code: usize,
    bl_count: [u16; MAX_BITS + 1],
    heap: [usize; 2 * L_CODES + 1],
    heap_len: usize,
    heap_max: usize,
    depth: [u8; 2 * L_CODES + 1],
    sym_buf: Vec<u8>,
    sym_next: usize,
    opt_len: u64,
    static_len: u64,
    bi_buf: u32,
    bi_valid: usize,
    // stream
    finished: bool,
}

#[derive(PartialEq, Eq)]
enum BlockState {
    NeedMore,
    BlockDone,
    FinishStarted,
    FinishDone,
}

/// The caller-visible stream: input still to consume and the output
/// window of the current call (`avail_out`), as zlib's `z_stream`.
struct Stream<'a> {
    /// The whole input; `pos` is zlib's `next_in` as an offset into it.
    whole: &'a [u8],
    pos: usize,
    out: &'a mut Vec<u8>,
    avail_out: usize,
}

impl Stream<'_> {
    fn avail_in(&self) -> usize {
        self.whole.len() - self.pos
    }
    /// Consume `n` input bytes into `dst`.
    fn read_buf(&mut self, dst: &mut [u8]) -> usize {
        let n = dst.len().min(self.avail_in());
        dst[..n].copy_from_slice(&self.whole[self.pos..self.pos + n]);
        self.pos += n;
        n
    }
    /// Consume `n` input bytes straight to the output.
    fn copy_through(&mut self, n: usize) {
        self.out
            .extend_from_slice(&self.whole[self.pos..self.pos + n]);
        self.pos += n;
        self.avail_out -= n;
    }
    /// The last `n` bytes consumed (zlib reads them from `next_in - n`).
    fn consumed_tail(&self, n: usize) -> &[u8] {
        &self.whole[self.pos - n..self.pos]
    }
}

impl State {
    fn new(level: u8) -> State {
        let (good_match, max_lazy, nice_match, max_chain) = config(level);
        let mut s = State {
            level,
            good_match,
            max_lazy,
            nice_match,
            max_chain,
            match_available: false,
            prev_match: 0,
            pending_buf: vec![0; PENDING_BUF_SIZE],
            pending: 0,
            pending_out: 0,
            window: vec![0; WINDOW_SIZE + MAX_MATCH],
            prev: vec![0; W_SIZE],
            head: vec![0; HASH_SIZE],
            ins_h: 0,
            block_start: 0,
            strstart: 0,
            lookahead: 0,
            insert: 0,
            match_start: 0,
            match_length: MIN_MATCH - 1,
            prev_length: MIN_MATCH - 1,
            high_water: 0,
            matches: 0,
            dyn_ltree: [Ct::default(); HEAP_SIZE],
            dyn_dtree: [Ct::default(); 2 * D_CODES + 1],
            bl_tree: [Ct::default(); 2 * BL_CODES + 1],
            l_max_code: 0,
            d_max_code: 0,
            bl_max_code: 0,
            bl_count: [0; MAX_BITS + 1],
            heap: [0; 2 * L_CODES + 1],
            heap_len: 0,
            heap_max: 0,
            depth: [0; 2 * L_CODES + 1],
            sym_buf: vec![0; LIT_BUFSIZE * 3],
            sym_next: 0,
            opt_len: 0,
            static_len: 0,
            bi_buf: 0,
            bi_valid: 0,
            finished: false,
        };
        s.init_block();
        s
    }

    // ------------------------------------------------------------ output

    fn put_byte(&mut self, b: u8) {
        self.pending_buf[self.pending] = b;
        self.pending += 1;
    }

    fn put_short(&mut self, w: u32) {
        self.put_byte((w & 0xff) as u8);
        self.put_byte(((w >> 8) & 0xff) as u8);
    }

    fn send_bits(&mut self, value: u32, length: usize) {
        if self.bi_valid > BUF_SIZE - length {
            self.bi_buf |= value << self.bi_valid;
            let w = self.bi_buf & 0xffff;
            self.put_short(w);
            self.bi_buf = value >> (BUF_SIZE - self.bi_valid);
            self.bi_valid += length;
            self.bi_valid -= BUF_SIZE;
        } else {
            self.bi_buf |= value << self.bi_valid;
            self.bi_valid += length;
        }
        self.bi_buf &= 0xffff;
    }

    fn bi_windup(&mut self) {
        if self.bi_valid > 8 {
            let w = self.bi_buf;
            self.put_short(w);
        } else if self.bi_valid > 0 {
            let b = self.bi_buf as u8;
            self.put_byte(b);
        }
        self.bi_buf = 0;
        self.bi_valid = 0;
    }

    fn flush_pending(&mut self, strm: &mut Stream<'_>) {
        let len = self.pending.min(strm.avail_out);
        if len == 0 {
            return;
        }
        strm.out
            .extend_from_slice(&self.pending_buf[self.pending_out..self.pending_out + len]);
        strm.avail_out -= len;
        self.pending_out += len;
        self.pending -= len;
        if self.pending == 0 {
            self.pending_out = 0;
        }
    }

    // ------------------------------------------------------------ trees

    fn init_block(&mut self) {
        for n in 0..L_CODES {
            self.dyn_ltree[n].freq_or_code = 0;
        }
        for n in 0..D_CODES {
            self.dyn_dtree[n].freq_or_code = 0;
        }
        for n in 0..BL_CODES {
            self.bl_tree[n].freq_or_code = 0;
        }
        self.dyn_ltree[END_BLOCK].freq_or_code = 1;
        self.opt_len = 0;
        self.static_len = 0;
        self.sym_next = 0;
        self.matches = 0;
    }

    fn tally_lit(&mut self, c: u8) -> bool {
        self.sym_buf[self.sym_next] = 0;
        self.sym_buf[self.sym_next + 1] = 0;
        self.sym_buf[self.sym_next + 2] = c;
        self.sym_next += 3;
        self.dyn_ltree[c as usize].freq_or_code += 1;
        self.sym_next == SYM_END
    }

    fn tally_dist(&mut self, dist: usize, len: usize) -> bool {
        let st = statics();
        self.sym_buf[self.sym_next] = (dist & 0xff) as u8;
        self.sym_buf[self.sym_next + 1] = ((dist >> 8) & 0xff) as u8;
        self.sym_buf[self.sym_next + 2] = len as u8;
        self.sym_next += 3;
        let dist = dist - 1;
        self.dyn_ltree[st.length_code[len] as usize + LITERALS + 1].freq_or_code += 1;
        self.dyn_dtree[d_code(st, dist)].freq_or_code += 1;
        self.sym_next == SYM_END
    }

    fn tree(&self, w: Which) -> &[Ct] {
        match w {
            Which::Lit => &self.dyn_ltree,
            Which::Dist => &self.dyn_dtree,
            Which::Bl => &self.bl_tree,
        }
    }

    fn tree_mut(&mut self, w: Which) -> &mut [Ct] {
        match w {
            Which::Lit => &mut self.dyn_ltree,
            Which::Dist => &mut self.dyn_dtree,
            Which::Bl => &mut self.bl_tree,
        }
    }

    fn smaller(&self, w: Which, n: usize, m: usize) -> bool {
        let t = self.tree(w);
        t[n].freq_or_code < t[m].freq_or_code
            || (t[n].freq_or_code == t[m].freq_or_code && self.depth[n] <= self.depth[m])
    }

    fn pqdownheap(&mut self, w: Which, mut k: usize) {
        let v = self.heap[k];
        let mut j = k << 1;
        while j <= self.heap_len {
            if j < self.heap_len && self.smaller(w, self.heap[j + 1], self.heap[j]) {
                j += 1;
            }
            if self.smaller(w, v, self.heap[j]) {
                break;
            }
            self.heap[k] = self.heap[j];
            k = j;
            j <<= 1;
        }
        self.heap[k] = v;
    }

    /// `build_tree`: Huffman code lengths and codes for one tree; returns
    /// its `max_code`.
    fn build_tree(&mut self, w: Which) -> usize {
        let st = statics();
        let (elems, stree_len): (usize, Option<&[Ct]>) = match w {
            Which::Lit => (L_CODES, Some(&st.ltree[..])),
            Which::Dist => (D_CODES, Some(&st.dtree[..])),
            Which::Bl => (BL_CODES, None),
        };
        let mut max_code: i64 = -1;
        self.heap_len = 0;
        self.heap_max = HEAP_SIZE;
        for n in 0..elems {
            if self.tree(w)[n].freq_or_code != 0 {
                self.heap_len += 1;
                self.heap[self.heap_len] = n;
                max_code = n as i64;
                self.depth[n] = 0;
            } else {
                self.tree_mut(w)[n].len_or_dad = 0;
            }
        }
        while self.heap_len < 2 {
            let node = if max_code < 2 {
                max_code += 1;
                max_code as usize
            } else {
                0
            };
            self.heap_len += 1;
            self.heap[self.heap_len] = node;
            self.tree_mut(w)[node].freq_or_code = 1;
            self.depth[node] = 0;
            self.opt_len = self.opt_len.wrapping_sub(1);
            if let Some(stree) = stree_len {
                self.static_len = self.static_len.wrapping_sub(stree[node].len_or_dad as u64);
            }
        }
        let max_code = max_code as usize;
        let mut n = self.heap_len / 2;
        while n >= 1 {
            self.pqdownheap(w, n);
            n -= 1;
        }
        let mut node = elems;
        loop {
            // pqremove
            let n = self.heap[1];
            self.heap[1] = self.heap[self.heap_len];
            self.heap_len -= 1;
            self.pqdownheap(w, 1);
            let m = self.heap[1];
            self.heap_max -= 1;
            self.heap[self.heap_max] = n;
            self.heap_max -= 1;
            self.heap[self.heap_max] = m;
            let f = self.tree(w)[n].freq_or_code + self.tree(w)[m].freq_or_code;
            self.tree_mut(w)[node].freq_or_code = f;
            self.depth[node] = self.depth[n].max(self.depth[m]) + 1;
            self.tree_mut(w)[n].len_or_dad = node as u16;
            self.tree_mut(w)[m].len_or_dad = node as u16;
            self.heap[1] = node;
            node += 1;
            self.pqdownheap(w, 1);
            if self.heap_len < 2 {
                break;
            }
        }
        self.heap_max -= 1;
        self.heap[self.heap_max] = self.heap[1];
        self.gen_bitlen(w, max_code, stree_len);
        let bl_count = self.bl_count;
        gen_codes(self.tree_mut(w), max_code, &bl_count);
        max_code
    }

    fn gen_bitlen(&mut self, w: Which, max_code: usize, stree: Option<&[Ct]>) {
        let (extra, base, max_length): (&[u8], usize, usize) = match w {
            Which::Lit => (&EXTRA_LBITS, LITERALS + 1, MAX_BITS),
            Which::Dist => (&EXTRA_DBITS, 0, MAX_BITS),
            Which::Bl => (&EXTRA_BLBITS, 0, MAX_BL_BITS),
        };
        for bits in 0..=MAX_BITS {
            self.bl_count[bits] = 0;
        }
        let root = self.heap[self.heap_max];
        self.tree_mut(w)[root].len_or_dad = 0;
        let mut overflow: i32 = 0;
        let mut h = self.heap_max + 1;
        while h < HEAP_SIZE {
            let n = self.heap[h];
            let dad = self.tree(w)[n].len_or_dad as usize;
            let mut bits = self.tree(w)[dad].len_or_dad as usize + 1;
            if bits > max_length {
                bits = max_length;
                overflow += 1;
            }
            self.tree_mut(w)[n].len_or_dad = bits as u16;
            h += 1;
            if n > max_code {
                continue;
            }
            self.bl_count[bits] += 1;
            let xbits = if n >= base { extra[n - base] as u64 } else { 0 };
            let f = self.tree(w)[n].freq_or_code as u64;
            self.opt_len = self.opt_len.wrapping_add(f * (bits as u64 + xbits));
            if let Some(st) = stree {
                self.static_len = self
                    .static_len
                    .wrapping_add(f * (st[n].len_or_dad as u64 + xbits));
            }
        }
        if overflow == 0 {
            return;
        }
        loop {
            let mut bits = max_length - 1;
            while self.bl_count[bits] == 0 {
                bits -= 1;
            }
            self.bl_count[bits] -= 1;
            self.bl_count[bits + 1] += 2;
            self.bl_count[max_length] -= 1;
            overflow -= 2;
            if overflow <= 0 {
                break;
            }
        }
        let mut h = HEAP_SIZE;
        let mut bits = max_length;
        while bits != 0 {
            let mut n = self.bl_count[bits];
            while n != 0 {
                h -= 1;
                let m = self.heap[h];
                if m > max_code {
                    continue;
                }
                if self.tree(w)[m].len_or_dad as usize != bits {
                    let f = self.tree(w)[m].freq_or_code as u64;
                    let old = self.tree(w)[m].len_or_dad as u64;
                    self.opt_len = self
                        .opt_len
                        .wrapping_add((bits as u64).wrapping_sub(old).wrapping_mul(f));
                    self.tree_mut(w)[m].len_or_dad = bits as u16;
                }
                n -= 1;
            }
            bits -= 1;
        }
    }

    fn scan_tree(&mut self, w: Which, max_code: usize) {
        let mut prevlen: i32 = -1;
        let mut nextlen = self.tree(w)[0].len_or_dad as i32;
        let mut count = 0i32;
        let mut max_count = 7;
        let mut min_count = 4;
        if nextlen == 0 {
            max_count = 138;
            min_count = 3;
        }
        self.tree_mut(w)[max_code + 1].len_or_dad = 0xffff;
        for n in 0..=max_code {
            let curlen = nextlen;
            nextlen = self.tree(w)[n + 1].len_or_dad as i32;
            count += 1;
            if count < max_count && curlen == nextlen {
                continue;
            } else if count < min_count {
                self.bl_tree[curlen as usize].freq_or_code += count as u16;
            } else if curlen != 0 {
                if curlen != prevlen {
                    self.bl_tree[curlen as usize].freq_or_code += 1;
                }
                self.bl_tree[REP_3_6].freq_or_code += 1;
            } else if count <= 10 {
                self.bl_tree[REPZ_3_10].freq_or_code += 1;
            } else {
                self.bl_tree[REPZ_11_138].freq_or_code += 1;
            }
            count = 0;
            prevlen = curlen;
            if nextlen == 0 {
                max_count = 138;
                min_count = 3;
            } else if curlen == nextlen {
                max_count = 6;
                min_count = 3;
            } else {
                max_count = 7;
                min_count = 4;
            }
        }
    }

    fn send_code(&mut self, c: usize, tree: &[Ct]) {
        self.send_bits(tree[c].freq_or_code as u32, tree[c].len_or_dad as usize);
    }

    fn send_tree(&mut self, w: Which, max_code: usize) {
        let bl = self.bl_tree;
        let mut prevlen: i32 = -1;
        let mut nextlen = self.tree(w)[0].len_or_dad as i32;
        let mut count = 0i32;
        let mut max_count = 7;
        let mut min_count = 4;
        if nextlen == 0 {
            max_count = 138;
            min_count = 3;
        }
        for n in 0..=max_code {
            let curlen = nextlen;
            nextlen = self.tree(w)[n + 1].len_or_dad as i32;
            count += 1;
            if count < max_count && curlen == nextlen {
                continue;
            } else if count < min_count {
                loop {
                    self.send_code(curlen as usize, &bl);
                    count -= 1;
                    if count == 0 {
                        break;
                    }
                }
            } else if curlen != 0 {
                if curlen != prevlen {
                    self.send_code(curlen as usize, &bl);
                    count -= 1;
                }
                self.send_code(REP_3_6, &bl);
                self.send_bits((count - 3) as u32, 2);
            } else if count <= 10 {
                self.send_code(REPZ_3_10, &bl);
                self.send_bits((count - 3) as u32, 3);
            } else {
                self.send_code(REPZ_11_138, &bl);
                self.send_bits((count - 11) as u32, 7);
            }
            count = 0;
            prevlen = curlen;
            if nextlen == 0 {
                max_count = 138;
                min_count = 3;
            } else if curlen == nextlen {
                max_count = 6;
                min_count = 3;
            } else {
                max_count = 7;
                min_count = 4;
            }
        }
    }

    fn build_bl_tree(&mut self) -> usize {
        let l = self.l_max_code;
        let d = self.d_max_code;
        self.scan_tree(Which::Lit, l);
        self.scan_tree(Which::Dist, d);
        self.bl_max_code = self.build_tree(Which::Bl);
        let mut max_blindex = BL_CODES - 1;
        while max_blindex >= 3 {
            if self.bl_tree[BL_ORDER[max_blindex]].len_or_dad != 0 {
                break;
            }
            max_blindex -= 1;
        }
        self.opt_len = self
            .opt_len
            .wrapping_add(3 * (max_blindex as u64 + 1) + 5 + 5 + 4);
        max_blindex
    }

    fn send_all_trees(&mut self, lcodes: usize, dcodes: usize, blcodes: usize) {
        self.send_bits((lcodes - 257) as u32, 5);
        self.send_bits((dcodes - 1) as u32, 5);
        self.send_bits((blcodes - 4) as u32, 4);
        for &code in &BL_ORDER[..blcodes] {
            let len = self.bl_tree[code].len_or_dad as u32;
            self.send_bits(len, 3);
        }
        self.send_tree(Which::Lit, lcodes - 1);
        self.send_tree(Which::Dist, dcodes - 1);
    }

    fn compress_block(&mut self, ltree: &[Ct], dtree: &[Ct]) {
        let st = statics();
        let mut sx = 0;
        while sx < self.sym_next {
            let mut dist = self.sym_buf[sx] as usize | ((self.sym_buf[sx + 1] as usize) << 8);
            let mut lc = self.sym_buf[sx + 2] as usize;
            sx += 3;
            if dist == 0 {
                self.send_code(lc, ltree);
            } else {
                let code = st.length_code[lc] as usize;
                self.send_code(code + LITERALS + 1, ltree);
                let extra = EXTRA_LBITS[code] as usize;
                if extra != 0 {
                    lc -= st.base_length[code] as usize;
                    self.send_bits(lc as u32, extra);
                }
                dist -= 1;
                let code = d_code(st, dist);
                self.send_code(code, dtree);
                let extra = EXTRA_DBITS[code] as usize;
                if extra != 0 {
                    dist -= st.base_dist[code] as usize;
                    self.send_bits(dist as u32, extra);
                }
            }
        }
        self.send_code(END_BLOCK, ltree);
    }

    /// `_tr_stored_block` with no payload: the header of a block whose
    /// bytes the caller copies straight from the input.
    fn stored_header(&mut self, last: bool) {
        self.send_bits((STORED_BLOCK << 1) + last as u32, 3);
        self.bi_windup();
        self.put_short(0);
        self.put_short(0xffff);
    }

    /// `_tr_stored_block` with a window-resident buffer.
    fn stored_from_window(&mut self, start: usize, stored_len: usize, last: bool) {
        self.send_bits((STORED_BLOCK << 1) + last as u32, 3);
        self.bi_windup();
        self.put_short(stored_len as u32);
        self.put_short((!stored_len as u32) & 0xffff);
        if stored_len != 0 {
            let p = self.pending;
            let (w, pb) = (&self.window, &mut self.pending_buf);
            pb[p..p + stored_len].copy_from_slice(&w[start..start + stored_len]);
            self.pending += stored_len;
        }
    }

    /// `_tr_flush_block`: choose stored/static/dynamic for the current
    /// block and emit it. `buf` is the window start of the block's bytes
    /// when `block_start >= 0`.
    fn flush_block(&mut self, buf: Option<usize>, stored_len: usize, last: bool) {
        let (mut opt_lenb, static_lenb, max_blindex);
        if self.level > 0 {
            self.l_max_code = self.build_tree(Which::Lit);
            self.d_max_code = self.build_tree(Which::Dist);
            max_blindex = self.build_bl_tree();
            opt_lenb = (self.opt_len + 3 + 7) >> 3;
            static_lenb = (self.static_len + 3 + 7) >> 3;
            if static_lenb <= opt_lenb {
                opt_lenb = static_lenb;
            }
        } else {
            opt_lenb = stored_len as u64 + 5;
            static_lenb = opt_lenb;
            max_blindex = 0;
        }
        if let Some(buf) = buf.filter(|_| stored_len as u64 + 4 <= opt_lenb) {
            self.stored_from_window(buf, stored_len, last);
        } else if static_lenb == opt_lenb {
            self.send_bits((STATIC_TREES << 1) + last as u32, 3);
            let st = statics();
            self.compress_block(&st.ltree, &st.dtree);
        } else {
            self.send_bits((DYN_TREES << 1) + last as u32, 3);
            let (l, d) = (self.l_max_code + 1, self.d_max_code + 1);
            self.send_all_trees(l, d, max_blindex + 1);
            let lt = self.dyn_ltree;
            let dt = self.dyn_dtree;
            self.compress_block(&lt, &dt);
        }
        self.init_block();
        if last {
            self.bi_windup();
        }
    }

    // ------------------------------------------------------------ window

    fn update_hash(&mut self, c: u8) {
        self.ins_h = ((self.ins_h << HASH_SHIFT) ^ c as usize) & HASH_MASK;
    }

    fn insert_string(&mut self, s: usize) -> u16 {
        self.update_hash(self.window[s + MIN_MATCH - 1]);
        let head = self.head[self.ins_h];
        self.prev[s & W_MASK] = head;
        self.head[self.ins_h] = s as u16;
        head
    }

    fn slide_hash(&mut self) {
        for h in self.head.iter_mut() {
            *h = if *h as usize >= W_SIZE {
                *h - W_SIZE as u16
            } else {
                NIL
            };
        }
        for p in self.prev.iter_mut() {
            *p = if *p as usize >= W_SIZE {
                *p - W_SIZE as u16
            } else {
                NIL
            };
        }
    }

    fn fill_window(&mut self, strm: &mut Stream<'_>) {
        loop {
            let mut more = WINDOW_SIZE - self.lookahead - self.strstart;
            if self.strstart >= W_SIZE + MAX_DIST {
                self.window.copy_within(W_SIZE..W_SIZE + (W_SIZE - more), 0);
                self.match_start = self.match_start.wrapping_sub(W_SIZE);
                self.strstart -= W_SIZE;
                self.block_start -= W_SIZE as i64;
                if self.insert > self.strstart {
                    self.insert = self.strstart;
                }
                self.slide_hash();
                more += W_SIZE;
            }
            if strm.avail_in() == 0 {
                break;
            }
            let at = self.strstart + self.lookahead;
            let n = strm.read_buf(&mut self.window[at..at + more]);
            self.lookahead += n;
            if self.lookahead + self.insert >= MIN_MATCH {
                let mut str = self.strstart - self.insert;
                self.ins_h = self.window[str] as usize;
                self.update_hash(self.window[str + 1]);
                while self.insert != 0 {
                    self.update_hash(self.window[str + MIN_MATCH - 1]);
                    self.prev[str & W_MASK] = self.head[self.ins_h];
                    self.head[self.ins_h] = str as u16;
                    str += 1;
                    self.insert -= 1;
                    if self.lookahead + self.insert < MIN_MATCH {
                        break;
                    }
                }
            }
            if !(self.lookahead < MIN_LOOKAHEAD && strm.avail_in() != 0) {
                break;
            }
        }
        if self.high_water < WINDOW_SIZE {
            let curr = self.strstart + self.lookahead;
            if self.high_water < curr {
                let init = (WINDOW_SIZE - curr).min(WIN_INIT);
                self.window[curr..curr + init].fill(0);
                self.high_water = curr + init;
            } else if self.high_water < curr + WIN_INIT {
                let init = (curr + WIN_INIT - self.high_water).min(WINDOW_SIZE - self.high_water);
                self.window[self.high_water..self.high_water + init].fill(0);
                self.high_water += init;
            }
        }
    }

    fn longest_match(&mut self, mut cur_match: usize) -> usize {
        let mut chain_length = self.max_chain;
        let strstart = self.strstart;
        let mut best_len = self.prev_length;
        let mut nice_match = self.nice_match;
        let limit = if strstart > MAX_DIST {
            strstart - MAX_DIST
        } else {
            NIL as usize
        };
        let strend = strstart + MAX_MATCH;
        let w = &self.window;
        let mut scan_end1 = w[strstart + best_len - 1];
        let mut scan_end = w[strstart + best_len];
        if self.prev_length >= self.good_match {
            chain_length >>= 2;
        }
        if nice_match > self.lookahead {
            nice_match = self.lookahead;
        }
        loop {
            let m = cur_match;
            if w[m + best_len] != scan_end
                || w[m + best_len - 1] != scan_end1
                || w[m] != w[strstart]
                || w[m + 1] != w[strstart + 1]
            {
                // next candidate
            } else {
                let mut scan = strstart + 2;
                let mut mp = m + 2;
                loop {
                    scan += 1;
                    mp += 1;
                    if w[scan] != w[mp] || scan >= strend {
                        break;
                    }
                }
                let len = MAX_MATCH - (strend - scan);
                if len > best_len {
                    self.match_start = cur_match;
                    best_len = len;
                    if len >= nice_match {
                        break;
                    }
                    scan_end1 = w[strstart + best_len - 1];
                    scan_end = w[strstart + best_len];
                }
            }
            cur_match = self.prev[cur_match & W_MASK] as usize;
            if cur_match <= limit {
                break;
            }
            chain_length -= 1;
            if chain_length == 0 {
                break;
            }
        }
        if best_len <= self.lookahead {
            best_len
        } else {
            self.lookahead
        }
    }

    fn flush_block_only(&mut self, strm: &mut Stream<'_>, last: bool) {
        let buf = (self.block_start >= 0).then_some(self.block_start as usize);
        let len = (self.strstart as i64 - self.block_start) as usize;
        self.flush_block(buf, len, last);
        self.block_start = self.strstart as i64;
        self.flush_pending(strm);
    }

    // ------------------------------------------------------------ level 1

    fn deflate_fast(&mut self, strm: &mut Stream<'_>, flush: Flush) -> BlockState {
        loop {
            if self.lookahead < MIN_LOOKAHEAD {
                self.fill_window(strm);
                if self.lookahead < MIN_LOOKAHEAD && flush == Flush::None {
                    return BlockState::NeedMore;
                }
                if self.lookahead == 0 {
                    break;
                }
            }
            let mut hash_head = NIL;
            if self.lookahead >= MIN_MATCH {
                hash_head = self.insert_string(self.strstart);
            }
            if hash_head != NIL && self.strstart - hash_head as usize <= MAX_DIST {
                self.match_length = self.longest_match(hash_head as usize);
            }
            let bflush;
            if self.match_length >= MIN_MATCH {
                bflush = self.tally_dist(
                    self.strstart - self.match_start,
                    self.match_length - MIN_MATCH,
                );
                self.lookahead -= self.match_length;
                if self.match_length <= self.max_lazy && self.lookahead >= MIN_MATCH {
                    self.match_length -= 1;
                    loop {
                        self.strstart += 1;
                        self.insert_string(self.strstart);
                        self.match_length -= 1;
                        if self.match_length == 0 {
                            break;
                        }
                    }
                    self.strstart += 1;
                } else {
                    self.strstart += self.match_length;
                    self.match_length = 0;
                    self.ins_h = self.window[self.strstart] as usize;
                    self.update_hash(self.window[self.strstart + 1]);
                }
            } else {
                bflush = self.tally_lit(self.window[self.strstart]);
                self.lookahead -= 1;
                self.strstart += 1;
            }
            if bflush {
                self.flush_block_only(strm, false);
                if strm.avail_out == 0 {
                    return BlockState::NeedMore;
                }
            }
        }
        self.insert = if self.strstart < MIN_MATCH - 1 {
            self.strstart
        } else {
            MIN_MATCH - 1
        };
        if flush == Flush::Finish {
            self.flush_block_only(strm, true);
            if strm.avail_out == 0 {
                return BlockState::FinishStarted;
            }
            return BlockState::FinishDone;
        }
        if self.sym_next != 0 {
            self.flush_block_only(strm, false);
            if strm.avail_out == 0 {
                return BlockState::NeedMore;
            }
        }
        BlockState::BlockDone
    }

    // ------------------------------------------------------------ level 9

    fn deflate_slow(&mut self, strm: &mut Stream<'_>, flush: Flush) -> BlockState {
        loop {
            if self.lookahead < MIN_LOOKAHEAD {
                self.fill_window(strm);
                if self.lookahead < MIN_LOOKAHEAD && flush == Flush::None {
                    return BlockState::NeedMore;
                }
                if self.lookahead == 0 {
                    break;
                }
            }
            let mut hash_head = NIL;
            if self.lookahead >= MIN_MATCH {
                hash_head = self.insert_string(self.strstart);
            }
            self.prev_length = self.match_length;
            self.prev_match = self.match_start;
            self.match_length = MIN_MATCH - 1;
            if hash_head != NIL
                && self.prev_length < self.max_lazy
                && self.strstart - hash_head as usize <= MAX_DIST
            {
                self.match_length = self.longest_match(hash_head as usize);
                if self.match_length <= 5
                    && self.match_length == MIN_MATCH
                    && self.strstart - self.match_start > TOO_FAR
                {
                    self.match_length = MIN_MATCH - 1;
                }
            }
            if self.prev_length >= MIN_MATCH && self.match_length <= self.prev_length {
                let max_insert = self.strstart + self.lookahead - MIN_MATCH;
                let bflush = self.tally_dist(
                    self.strstart - 1 - self.prev_match,
                    self.prev_length - MIN_MATCH,
                );
                self.lookahead -= self.prev_length - 1;
                self.prev_length -= 2;
                loop {
                    self.strstart += 1;
                    if self.strstart <= max_insert {
                        self.insert_string(self.strstart);
                    }
                    self.prev_length -= 1;
                    if self.prev_length == 0 {
                        break;
                    }
                }
                self.match_available = false;
                self.match_length = MIN_MATCH - 1;
                self.strstart += 1;
                if bflush {
                    self.flush_block_only(strm, false);
                    if strm.avail_out == 0 {
                        return BlockState::NeedMore;
                    }
                }
            } else if self.match_available {
                let bflush = self.tally_lit(self.window[self.strstart - 1]);
                if bflush {
                    self.flush_block_only(strm, false);
                }
                self.strstart += 1;
                self.lookahead -= 1;
                if strm.avail_out == 0 {
                    return BlockState::NeedMore;
                }
            } else {
                self.match_available = true;
                self.strstart += 1;
                self.lookahead -= 1;
            }
        }
        if self.match_available {
            self.tally_lit(self.window[self.strstart - 1]);
            self.match_available = false;
        }
        self.insert = if self.strstart < MIN_MATCH - 1 {
            self.strstart
        } else {
            MIN_MATCH - 1
        };
        if flush == Flush::Finish {
            self.flush_block_only(strm, true);
            if strm.avail_out == 0 {
                return BlockState::FinishStarted;
            }
            return BlockState::FinishDone;
        }
        if self.sym_next != 0 {
            self.flush_block_only(strm, false);
            if strm.avail_out == 0 {
                return BlockState::NeedMore;
            }
        }
        BlockState::BlockDone
    }

    // ------------------------------------------------------------ level 0

    fn deflate_stored(&mut self, strm: &mut Stream<'_>, flush: Flush) -> BlockState {
        let mut min_block = (PENDING_BUF_SIZE - 5).min(W_SIZE);
        let mut last = false;
        let mut used = strm.avail_in();
        loop {
            let mut len = MAX_STORED;
            let mut have = (self.bi_valid + 42) >> 3;
            if strm.avail_out < have {
                break;
            }
            have = strm.avail_out - have;
            let mut left = (self.strstart as i64 - self.block_start) as usize;
            if len > left + strm.avail_in() {
                len = left + strm.avail_in();
            }
            if len > have {
                len = have;
            }
            if len < min_block
                && ((len == 0 && flush != Flush::Finish)
                    || flush == Flush::None
                    || len != left + strm.avail_in())
            {
                break;
            }
            last = flush == Flush::Finish && len == left + strm.avail_in();
            self.stored_header(last);
            let p = self.pending;
            self.pending_buf[p - 4] = (len & 0xff) as u8;
            self.pending_buf[p - 3] = ((len >> 8) & 0xff) as u8;
            self.pending_buf[p - 2] = (!len & 0xff) as u8;
            self.pending_buf[p - 1] = ((!len >> 8) & 0xff) as u8;
            self.flush_pending(strm);
            if left != 0 {
                if left > len {
                    left = len;
                }
                let bs = self.block_start as usize;
                strm.out.extend_from_slice(&self.window[bs..bs + left]);
                strm.avail_out -= left;
                self.block_start += left as i64;
                len -= left;
            }
            if len != 0 {
                strm.copy_through(len);
            }
            if last {
                break;
            }
        }
        used -= strm.avail_in();
        if used != 0 {
            if used >= W_SIZE {
                self.matches = 2;
                self.window[..W_SIZE].copy_from_slice(strm.consumed_tail(W_SIZE));
                self.strstart = W_SIZE;
                self.insert = self.strstart;
            } else {
                if WINDOW_SIZE - self.strstart <= used {
                    self.strstart -= W_SIZE;
                    self.window.copy_within(W_SIZE..W_SIZE + self.strstart, 0);
                    if self.matches < 2 {
                        self.matches += 1;
                    }
                    if self.insert > self.strstart {
                        self.insert = self.strstart;
                    }
                }
                let at = self.strstart;
                self.window[at..at + used].copy_from_slice(strm.consumed_tail(used));
                self.strstart += used;
                self.insert += used.min(W_SIZE - self.insert);
            }
            self.block_start = self.strstart as i64;
        }
        if self.high_water < self.strstart {
            self.high_water = self.strstart;
        }
        if last {
            return BlockState::FinishDone;
        }
        // (Z_SYNC_FLUSH / Z_FULL_FLUSH branch omitted: never used here.)
        let mut have = WINDOW_SIZE - self.strstart;
        if strm.avail_in() > have && self.block_start >= W_SIZE as i64 {
            self.block_start -= W_SIZE as i64;
            self.strstart -= W_SIZE;
            self.window.copy_within(W_SIZE..W_SIZE + self.strstart, 0);
            if self.matches < 2 {
                self.matches += 1;
            }
            have += W_SIZE;
            if self.insert > self.strstart {
                self.insert = self.strstart;
            }
        }
        if have > strm.avail_in() {
            have = strm.avail_in();
        }
        if have != 0 {
            let at = self.strstart;
            strm.read_buf(&mut self.window[at..at + have]);
            self.strstart += have;
            self.insert += have.min(W_SIZE - self.insert);
        }
        if self.high_water < self.strstart {
            self.high_water = self.strstart;
        }
        let have5 = (self.bi_valid + 42) >> 3;
        let have = (PENDING_BUF_SIZE - have5).min(MAX_STORED);
        min_block = have.min(W_SIZE);
        let left = (self.strstart as i64 - self.block_start) as usize;
        if left >= min_block
            || ((left != 0 || flush == Flush::Finish)
                && flush != Flush::None
                && strm.avail_in() == 0
                && left <= have)
        {
            let len = left.min(have);
            last = flush == Flush::Finish && strm.avail_in() == 0 && len == left;
            let bs = self.block_start as usize;
            self.stored_from_window(bs, len, last);
            self.block_start += len as i64;
            self.flush_pending(strm);
        }
        if last {
            BlockState::FinishStarted
        } else {
            BlockState::NeedMore
        }
    }

    /// zlib's `deflate()` for a raw stream (no header/trailer), restricted
    /// to `Z_NO_FLUSH` and `Z_FINISH`. Returns `true` on `Z_STREAM_END`.
    fn deflate(&mut self, strm: &mut Stream<'_>, flush: Flush) -> bool {
        if self.pending != 0 {
            self.flush_pending(strm);
            if strm.avail_out == 0 {
                return false;
            }
        }
        if strm.avail_in() != 0 || self.lookahead != 0 || (flush != Flush::None && !self.finished) {
            let bstate = if self.level == 0 {
                self.deflate_stored(strm, flush)
            } else if self.level == 9 {
                self.deflate_slow(strm, flush)
            } else {
                self.deflate_fast(strm, flush)
            };
            if bstate == BlockState::FinishStarted || bstate == BlockState::FinishDone {
                self.finished = true;
            }
            if bstate == BlockState::NeedMore || bstate == BlockState::FinishStarted {
                return false;
            }
            // BlockDone: with Z_NO_FLUSH nothing more to do.
        }
        if flush != Flush::Finish {
            return false;
        }
        true
    }
}

fn d_code(st: &Static, dist: usize) -> usize {
    if dist < 256 {
        st.dist_code[dist] as usize
    } else {
        st.dist_code[256 + (dist >> 7)] as usize
    }
}

/// The caller's output buffer, as observed: every stored block in the
/// corpus is 65,531 bytes, i.e. 65,536 minus the 5-byte block header.
const OUT_BUF: usize = 65536;

/// Compress `data` as SketchUp's zlib does: `level` 0 (stored), 1
/// (fast) or 9 (slow, maximum), raw DEFLATE, the whole input in one `deflate(Z_NO_FLUSH)` loop
/// over 64 KiB output buffers and then `deflate(Z_FINISH)` to the end.
pub(crate) fn compress(data: &[u8], level: u8) -> Vec<u8> {
    assert!(
        matches!(level, 0 | 1 | 9),
        "only zlib levels 0, 1 and 9 are reproduced"
    );
    let mut out = Vec::with_capacity(data.len() + data.len() / 1000 + 64);
    let mut s = State::new(level);
    {
        let mut strm = Stream {
            whole: data,
            pos: 0,
            out: &mut out,
            avail_out: OUT_BUF,
        };
        // zipWriteInFileInZip: deflate(Z_NO_FLUSH) while input remains,
        // refilling the output buffer whenever it fills.
        while strm.avail_in() > 0 {
            if strm.avail_out == 0 {
                strm.avail_out = OUT_BUF;
            }
            s.deflate(&mut strm, Flush::None);
        }
        // zipCloseFileInZip: deflate(Z_FINISH) until Z_STREAM_END.
        loop {
            if strm.avail_out == 0 {
                strm.avail_out = OUT_BUF;
            }
            if s.deflate(&mut strm, Flush::Finish) {
                break;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inflate(raw: &[u8], size: usize) -> Vec<u8> {
        let mut out = vec![0u8; size];
        let n = miniz_oxide::inflate::decompress_slice_iter_to_slice(
            &mut out,
            std::iter::once(raw),
            false,
            true,
        )
        .unwrap();
        out.truncate(n);
        out
    }

    #[test]
    fn small_inputs_inflate_back() {
        for level in [0u8, 1, 9] {
            for data in [
                &b""[..],
                b"a",
                b"abc",
                b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                b"the quick brown fox jumps over the lazy dog, the quick brown fox",
            ] {
                let c = compress(data, level);
                assert_eq!(inflate(&c, data.len() + 16), data, "level {level}");
            }
        }
    }

    #[test]
    fn large_inputs_inflate_back() {
        let mut data = Vec::new();
        let mut x: u32 = 12345;
        for i in 0..300_000usize {
            x = x.wrapping_mul(1103515245).wrapping_add(12345);
            data.push(if i % 7 == 0 {
                (x >> 16) as u8
            } else {
                b'a' + (i % 5) as u8
            });
        }
        for level in [0u8, 1, 9] {
            let c = compress(&data, level);
            assert_eq!(inflate(&c, data.len() + 16), data, "level {level}");
        }
    }

    /// Every stream of every 2026 corpus file re-encodes to its original
    /// bytes: level 1 for `.dat`/`.xml`, level 9 for hatch definitions, level 0 for
    /// everything else.
    #[test]
    fn reproduces_every_corpus_stream() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus");
        let mut files: Vec<_> = std::fs::read_dir(root.join("2026"))
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("skp"))
            .collect();
        files.push(root.join("third-party/theater-2026.skp"));
        files.extend(
            std::fs::read_dir(root.join("2027"))
                .unwrap()
                .map(|e| e.unwrap().path())
                .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("skp")),
        );
        files.sort();
        let mut streams = 0;
        for p in files {
            let d = std::fs::read(&p).unwrap();
            let start = d.windows(4).position(|w| w == b"PK\x03\x04").unwrap();
            let a = crate::zip::Archive::open(&d, start).unwrap();
            for e in &a.entries {
                let raw = a.raw(e);
                let level = match e.flags() & 0x6 {
                    0x6 => 1,
                    0x2 => 9,
                    _ => 0,
                };
                let content = a.read_entry(e).unwrap();
                let ours = compress(&content, level);
                assert!(
                    ours == raw,
                    "{}: {} (level {level}, {} bytes): re-encoded {} bytes, original {}",
                    p.display(),
                    e.name,
                    content.len(),
                    ours.len(),
                    raw.len()
                );
                streams += 1;
            }
        }
        assert!(streams > 1000, "{streams} streams");
    }
}
