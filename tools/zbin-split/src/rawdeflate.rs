//! Exact DEFLATE re-encoding from a recorded token script — the fallback for streams preflate
//! cannot model (e.g. Go's compress/flate, which emits incomplete Huffman trees).
//!
//! `analyze` inflates a stream and records everything the encoder decided that is not implied
//! by the plaintext: block boundaries/types, the raw dynamic-header bits, the pad bits before
//! stored blocks and at the end, and every match (length, distance). Literals come from the
//! plaintext. `rebuild` replays that script bit-for-bit. Cheap for stored-heavy streams; for
//! match-heavy streams the script grows with the match count, so preflate is tried first.
//!
//! Script format (LEB128 varints):
//!   per block: u8 (bfinal | btype << 1)
//!     stored:        pad_bits_value, len
//!     fixed/dynamic: [dynamic only: nbits, packed header bits]
//!                    repeat { literal_run, lenfield [, dist] } until lenfield == 0 (end of block)
//!                    lenfield = match_len - 2, or 257 for length 258 sent as code 284 + 31
//!   after the final block: trailing pad bits value

use anyhow::{bail, ensure, Result};

const LBASE: [u16; 29] = [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
const LEXT: [u8; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
const DBASE: [u32; 30] = [1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577];
const DEXT: [u8; 30] = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];
const CL_ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];

struct BitReader<'a> {
    d: &'a [u8],
    pos: usize, // in bits
}

impl BitReader<'_> {
    fn bits(&mut self, n: u32) -> Result<u32> {
        let mut v = 0u32;
        for i in 0..n {
            let byte = *self.d.get(self.pos >> 3).ok_or_else(|| anyhow::anyhow!("deflate: unexpected end of data"))?;
            v |= (((byte >> (self.pos & 7)) & 1) as u32) << i;
            self.pos += 1;
        }
        Ok(v)
    }
}

#[derive(Default)]
struct BitWriter {
    out: Vec<u8>,
    acc: u64,
    n: u32,
}

impl BitWriter {
    fn put(&mut self, v: u32, n: u32) {
        self.acc |= (v as u64) << self.n;
        self.n += n;
        while self.n >= 8 {
            self.out.push(self.acc as u8);
            self.acc >>= 8;
            self.n -= 8;
        }
    }
    fn pad_len(&self) -> u32 {
        (8 - self.n % 8) % 8
    }
}

/// Canonical Huffman code (decode via puff-style counts; encode via bit-reversed codes).
/// Tolerates incomplete codes, as zlib's inflate does.
struct Huff {
    count: [u16; 16],
    symbol: Vec<u16>,
    code: Vec<(u32, u32)>, // per symbol: (reversed code, length)
}

impl Huff {
    fn new(lens: &[u8]) -> Result<Self> {
        let mut count = [0u16; 16];
        for &l in lens {
            ensure!(l < 16, "code length {l}");
            count[l as usize] += 1;
        }
        count[0] = 0;
        let mut left: i32 = 1;
        for &c in &count[1..] {
            left = left * 2 - c as i32;
            ensure!(left >= 0, "over-subscribed Huffman code");
        }
        let mut offs = [0u16; 16];
        for l in 1..15 {
            offs[l + 1] = offs[l] + count[l];
        }
        let mut symbol = vec![0u16; lens.len()];
        for (s, &l) in lens.iter().enumerate() {
            if l != 0 {
                symbol[offs[l as usize] as usize] = s as u16;
                offs[l as usize] += 1;
            }
        }
        let mut next = [0u32; 16];
        let mut c = 0u32;
        for l in 1..16 {
            next[l] = c;
            c = (c + count[l] as u32) << 1;
        }
        let code = lens
            .iter()
            .map(|&l| {
                if l == 0 {
                    return (0, 0);
                }
                let v = next[l as usize];
                next[l as usize] += 1;
                let rev = (0..l).fold(0u32, |r, i| (r << 1) | ((v >> i) & 1));
                (rev, l as u32)
            })
            .collect();
        Ok(Huff { count, symbol, code })
    }

    fn decode(&self, br: &mut BitReader) -> Result<usize> {
        let (mut code, mut first, mut index) = (0i32, 0i32, 0i32);
        for len in 1..16 {
            code |= br.bits(1)? as i32;
            let count = self.count[len] as i32;
            if code - count < first {
                return Ok(self.symbol[(index + (code - first)) as usize] as usize);
            }
            index += count;
            first = (first + count) << 1;
            code <<= 1;
        }
        bail!("invalid Huffman code")
    }

    fn put(&self, bw: &mut BitWriter, sym: usize) -> Result<()> {
        let (c, l) = self.code[sym];
        ensure!(l != 0, "symbol {sym} has no code");
        bw.put(c, l);
        Ok(())
    }
}

fn fixed_lens() -> (Vec<u8>, Vec<u8>) {
    let mut ll = vec![8u8; 288];
    ll[144..256].fill(9);
    ll[256..280].fill(7);
    (ll, vec![5u8; 30])
}

/// Reads HLIT/HDIST/HCLEN and the code-length sequence; returns (litlen lens, dist lens).
fn read_dynamic_header(br: &mut BitReader) -> Result<(Vec<u8>, Vec<u8>)> {
    let hlit = br.bits(5)? as usize + 257;
    let hdist = br.bits(5)? as usize + 1;
    let hclen = br.bits(4)? as usize + 4;
    let mut cl = [0u8; 19];
    for &i in &CL_ORDER[..hclen] {
        cl[i] = br.bits(3)? as u8;
    }
    let clh = Huff::new(&cl)?;
    let mut lens: Vec<u8> = Vec::with_capacity(hlit + hdist);
    while lens.len() < hlit + hdist {
        let s = clh.decode(br)?;
        let (v, r) = match s {
            0..=15 => (s as u8, 1),
            16 => (*lens.last().ok_or_else(|| anyhow::anyhow!("repeat with no previous length"))?, 3 + br.bits(2)? as usize),
            17 => (0, 3 + br.bits(3)? as usize),
            _ => (0, 11 + br.bits(7)? as usize),
        };
        lens.extend(std::iter::repeat_n(v, r));
    }
    ensure!(lens.len() == hlit + hdist, "code lengths overrun");
    let dist = lens.split_off(hlit);
    Ok((lens, dist))
}

fn put_varint(out: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        out.push(v as u8 | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

struct Script<'a> {
    d: &'a [u8],
    p: usize,
}

impl Script<'_> {
    fn varint(&mut self) -> Result<u64> {
        let (mut v, mut shift) = (0u64, 0);
        loop {
            let b = *self.d.get(self.p).ok_or_else(|| anyhow::anyhow!("script truncated"))?;
            self.p += 1;
            v |= ((b & 0x7f) as u64) << shift;
            if b & 0x80 == 0 {
                return Ok(v);
            }
            shift += 7;
        }
    }
    fn byte(&mut self) -> Result<u8> {
        let b = *self.d.get(self.p).ok_or_else(|| anyhow::anyhow!("script truncated"))?;
        self.p += 1;
        Ok(b)
    }
}

/// Inflate `comp` (raw DEFLATE, may be followed by other data) into (script, plaintext, bytes used).
pub fn analyze(comp: &[u8]) -> Result<(Vec<u8>, Vec<u8>, usize)> {
    let mut br = BitReader { d: comp, pos: 0 };
    let mut script = Vec::new();
    let mut plain: Vec<u8> = Vec::new();
    loop {
        let bfinal = br.bits(1)?;
        let btype = br.bits(2)?;
        script.push((bfinal | btype << 1) as u8);
        match btype {
            0 => {
                let pad = (8 - br.pos % 8) % 8;
                put_varint(&mut script, br.bits(pad as u32)? as u64);
                let len = br.bits(16)?;
                let nlen = br.bits(16)?;
                ensure!(len == !nlen & 0xffff, "stored block LEN/NLEN mismatch");
                put_varint(&mut script, len as u64);
                let start = br.pos / 8;
                let body = comp.get(start..start + len as usize).ok_or_else(|| anyhow::anyhow!("stored block past end"))?;
                plain.extend_from_slice(body);
                br.pos += 8 * len as usize;
            }
            1 | 2 => {
                let (ll, dl) = if btype == 1 {
                    fixed_lens()
                } else {
                    let hstart = br.pos;
                    let lens = read_dynamic_header(&mut br)?;
                    let nbits = br.pos - hstart;
                    put_varint(&mut script, nbits as u64);
                    let mut hb = BitReader { d: comp, pos: hstart };
                    let mut bw = BitWriter::default();
                    for _ in 0..nbits {
                        bw.put(hb.bits(1)?, 1);
                    }
                    let pad = bw.pad_len();
                    bw.put(0, pad);
                    script.extend(bw.out);
                    lens
                };
                let (lh, dh) = (Huff::new(&ll)?, Huff::new(&dl)?);
                let mut run = 0u64;
                loop {
                    let s = lh.decode(&mut br)?;
                    if s < 256 {
                        plain.push(s as u8);
                        run += 1;
                        continue;
                    }
                    put_varint(&mut script, run);
                    run = 0;
                    if s == 256 {
                        put_varint(&mut script, 0);
                        break;
                    }
                    let li = s - 257;
                    ensure!(li < 29, "bad length symbol {s}");
                    let len = LBASE[li] as usize + br.bits(LEXT[li] as u32)? as usize;
                    let ds = dh.decode(&mut br)?;
                    ensure!(ds < 30, "bad distance symbol {ds}");
                    let dist = DBASE[ds] as usize + br.bits(DEXT[ds] as u32)? as usize;
                    ensure!(dist <= plain.len(), "distance before start of stream");
                    let lenfield = if len == 258 && li == 27 { 257 } else { len - 2 };
                    put_varint(&mut script, lenfield as u64);
                    put_varint(&mut script, dist as u64);
                    let from = plain.len() - dist;
                    for k in 0..len {
                        let b = plain[from + k];
                        plain.push(b);
                    }
                }
            }
            _ => bail!("reserved block type 3"),
        }
        if bfinal == 1 {
            break;
        }
    }
    let pad = (8 - br.pos % 8) % 8;
    put_varint(&mut script, br.bits(pad as u32)? as u64);
    let used = br.pos / 8;
    // Self-check: the script must reproduce the stream exactly.
    ensure!(rebuild(&script, &plain)? == comp[..used], "token script does not round-trip");
    Ok((script, plain, used))
}

/// Replay a token script over its plaintext, producing the original DEFLATE bytes.
pub fn rebuild(script: &[u8], plain: &[u8]) -> Result<Vec<u8>> {
    let mut sc = Script { d: script, p: 0 };
    let mut bw = BitWriter::default();
    let mut pos = 0usize; // in plain
    loop {
        let h = sc.byte()?;
        let (bfinal, btype) = ((h & 1) as u32, (h >> 1) as u32);
        bw.put(bfinal, 1);
        bw.put(btype, 2);
        match btype {
            0 => {
                let pad = bw.pad_len();
                bw.put(sc.varint()? as u32, pad);
                let len = sc.varint()? as usize;
                bw.put(len as u32, 16);
                bw.put(!len as u32 & 0xffff, 16);
                let body = plain.get(pos..pos + len).ok_or_else(|| anyhow::anyhow!("plaintext too short"))?;
                bw.out.extend_from_slice(body);
                pos += len;
            }
            1 | 2 => {
                let (ll, dl) = if btype == 1 {
                    fixed_lens()
                } else {
                    let nbits = sc.varint()? as usize;
                    let bytes = sc.d.get(sc.p..sc.p + nbits.div_ceil(8)).ok_or_else(|| anyhow::anyhow!("script truncated"))?;
                    sc.p += nbits.div_ceil(8);
                    let mut hb = BitReader { d: bytes, pos: 0 };
                    for _ in 0..nbits {
                        bw.put(hb.bits(1)?, 1);
                    }
                    read_dynamic_header(&mut BitReader { d: bytes, pos: 0 })?
                };
                let (lh, dh) = (Huff::new(&ll)?, Huff::new(&dl)?);
                loop {
                    let run = sc.varint()? as usize;
                    let lits = plain.get(pos..pos + run).ok_or_else(|| anyhow::anyhow!("plaintext too short"))?;
                    for &b in lits {
                        lh.put(&mut bw, b as usize)?;
                    }
                    pos += run;
                    let lenfield = sc.varint()? as usize;
                    if lenfield == 0 {
                        lh.put(&mut bw, 256)?;
                        break;
                    }
                    let (len, li) = if lenfield == 257 {
                        (258, 27)
                    } else {
                        let len = lenfield + 2;
                        (len, LBASE.iter().rposition(|&b| b as usize <= len).unwrap())
                    };
                    lh.put(&mut bw, 257 + li)?;
                    bw.put((len - LBASE[li] as usize) as u32, LEXT[li] as u32);
                    let dist = sc.varint()? as usize;
                    let di = DBASE.iter().rposition(|&b| b as usize <= dist).unwrap();
                    dh.put(&mut bw, di)?;
                    bw.put((dist - DBASE[di] as usize) as u32, DEXT[di] as u32);
                    pos += len;
                }
            }
            _ => bail!("bad block type in script"),
        }
        if bfinal == 1 {
            break;
        }
    }
    let pad = bw.pad_len();
    bw.put(sc.varint()? as u32, pad);
    ensure!(pos == plain.len(), "plaintext longer than script");
    ensure!(sc.p == script.len(), "trailing script bytes");
    Ok(bw.out)
}
