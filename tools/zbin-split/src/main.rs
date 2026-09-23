//! zbin-split — explode a Huidu `.zbin` firmware bundle into a tree of small, human-readable
//! files that fit in a plain GitHub repo, and rebuild the original `.zbin` byte-for-byte.
//!
//!   zbin-split unpack <IN.zbin> <OUT_DIR>     # decompose (writes OUT_DIR/manifest.json, tree/, meta/)
//!   zbin-split pack   <OUT_DIR> <OUT.zbin>    # rebuild; every node is SHA-256 checked
//!   zbin-split verify <OUT_DIR>               # rebuild in memory and check against the manifest
//!
//! Layers understood (each falls back to an opaque leaf if it does not round-trip):
//!   ZIP (the .zbin itself, MagicPlayer payload, APKs/JARs)  — local headers, gaps, central dir kept raw
//!   HDPLAYER / MAGICPLAYER .bin                              — magic+MD5+len+XML header kept as a leaf
//!   gzip                                                     — header/trailer raw, body via preflate
//!   tar (GNU/ustar)                                          — 512-byte headers raw, member data as files
//!   raw DEFLATE                                              — preflate-rs: plaintext + tiny correction
//!                                                              stream that re-creates the exact bits
//! Leaves larger than PART_SIZE are split into `name.partNNN` pieces.

use anyhow::{anyhow, bail, ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use preflate_rs::{ExitCode, PreflateConfig, PreflateStreamProcessor, RecreateStreamProcessor};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::time::Instant;

mod rawdeflate;

const FORMAT: &str = "huidu-zbin-split/1";
/// GitHub warns at 50 MiB and rejects at 100 MiB; stay well under both.
const PART_SIZE: usize = 45 * 1024 * 1024;
/// Raw glue up to this size is stored inline (base64) in the manifest instead of meta/.
const INLINE_MAX: usize = 1024;
const MAX_DEPTH: u32 = 8;

// ───────────────────────────── manifest model ─────────────────────────────

#[derive(Serialize, Deserialize, Clone)]
#[serde(untagged)]
enum Blob {
    Inline { b64: String },
    Stored { blob: String, size: usize },
}

#[derive(Serialize, Deserialize)]
struct Chunk {
    plain_len: usize,
    corrections: Blob,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Node {
    /// Concatenation of parts; `label` names the container format (zip/tar/gzip/...).
    Seq { label: String, size: usize, sha256: String, parts: Vec<Node> },
    Raw { data: Blob },
    Zeros { len: usize },
    /// A leaf file under tree/. `parts` > 0 means it is split into `path.partNNN`.
    File { path: String, size: usize, sha256: String, #[serde(default, skip_serializing_if = "is_zero")] parts: usize },
    /// A DEFLATE stream re-created from the plaintext node plus preflate corrections.
    Deflate { size: usize, sha256: String, chunks: Vec<Chunk>, plain: Box<Node> },
    /// A DEFLATE stream replayed from a recorded token script (see rawdeflate.rs) — used when
    /// preflate cannot model the encoder (Go's compress/flate wrote the outer .zbin).
    TokenDeflate { size: usize, sha256: String, script: Blob, plain: Box<Node> },
}

fn is_zero(n: &usize) -> bool {
    *n == 0
}

#[derive(Serialize, Deserialize)]
struct Manifest {
    format: String,
    source_name: String,
    size: usize,
    sha256: String,
    root: Node,
}

fn sha_hex(d: &[u8]) -> String {
    let h = Sha256::digest(d);
    h.iter().map(|b| format!("{b:02x}")).collect()
}

// ───────────────────────────── unpack ─────────────────────────────

struct Ctx {
    leaves: Vec<(String, Vec<u8>)>,
    blobs: Vec<(String, Vec<u8>)>,
    blob_keys: HashSet<String>,
    used_files: HashSet<String>,
    used_dirs: HashSet<String>,
    reserved: Vec<String>,
    stats: Stats,
}

#[derive(Default)]
struct Stats {
    deflate_streams: usize,
    deflate_plain: usize,
    token_streams: usize,
    correction_bytes: usize,
    fallbacks: Vec<String>,
}

struct Checkpoint(usize, usize, usize);

impl Ctx {
    fn checkpoint(&self) -> Checkpoint {
        Checkpoint(self.leaves.len(), self.blobs.len(), self.reserved.len())
    }

    fn rollback(&mut self, cp: Checkpoint) {
        self.leaves.truncate(cp.0);
        for (k, _) in self.blobs.drain(cp.1..) {
            self.blob_keys.remove(&k);
        }
        for p in self.reserved.drain(cp.2..) {
            self.used_files.remove(&p);
        }
    }

    fn blob(&mut self, data: &[u8]) -> Blob {
        if data.len() <= INLINE_MAX {
            return Blob::Inline { b64: B64.encode(data) };
        }
        let h = sha_hex(data);
        let name = format!("meta/{}/{}.blob", &h[..2], h);
        if self.blob_keys.insert(name.clone()) {
            self.blobs.push((name.clone(), data.to_vec()));
        }
        Blob::Stored { blob: name, size: data.len() }
    }

    fn raw(&mut self, data: &[u8]) -> Node {
        if data.len() > 64 && data.iter().all(|&b| b == 0) {
            Node::Zeros { len: data.len() }
        } else {
            Node::Raw { data: self.blob(data) }
        }
    }

    /// Reserve a unique, Windows/git-safe path under tree/ for a logical member path.
    fn reserve(&mut self, logical: &str) -> String {
        let comps: Vec<String> = logical.split(['/', '\\']).filter(|c| !c.is_empty() && *c != ".").map(sanitize).collect();
        let comps = if comps.is_empty() { vec!["_unnamed".to_string()] } else { comps };
        let base = comps.join("/");
        for n in 0.. {
            let cand = if n == 0 { base.clone() } else { format!("{base}~{n}") };
            let lc = cand.to_lowercase();
            let parts: Vec<&str> = lc.split('/').collect();
            let ancestor_is_file = (1..parts.len()).any(|i| self.used_files.contains(&parts[..i].join("/")));
            if ancestor_is_file || self.used_files.contains(&lc) || self.used_dirs.contains(&lc) {
                if ancestor_is_file && n > 50 {
                    return self.reserve(&format!("_conflicts/{}", base.replace('/', "__")));
                }
                continue;
            }
            for i in 1..parts.len() {
                self.used_dirs.insert(parts[..i].join("/"));
            }
            self.used_files.insert(lc.clone());
            self.reserved.push(lc);
            return cand;
        }
        unreachable!()
    }

    fn leaf(&mut self, logical: &str, data: &[u8]) -> Node {
        let path = self.reserve(logical);
        let parts = if data.len() > PART_SIZE { data.len().div_ceil(PART_SIZE) } else { 0 };
        let node = Node::File { path: path.clone(), size: data.len(), sha256: sha_hex(data), parts };
        self.leaves.push((path, data.to_vec()));
        node
    }
}

fn sanitize(c: &str) -> String {
    let mut s: String = c
        .chars()
        .map(|ch| if ch < ' ' || "<>:\"|?*".contains(ch) { '_' } else { ch })
        .collect();
    if s == ".." {
        s = "_up".into();
    }
    // Windows strips trailing dots/spaces; git treats .git* names specially.
    while s.ends_with('.') || s.ends_with(' ') {
        s.pop();
        s.push('_');
    }
    let stem = s.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = ["CON", "PRN", "AUX", "NUL"].contains(&stem.as_str())
        || ((stem.starts_with("COM") || stem.starts_with("LPT")) && stem.len() == 4 && stem.as_bytes()[3].is_ascii_digit());
    if reserved || s.to_ascii_lowercase().starts_with(".git") {
        s.insert(0, '_');
    }
    if s.is_empty() {
        s = "_".into();
    }
    s
}

fn child_dir(logical: &str) -> String {
    if logical.is_empty() { String::new() } else { format!("{logical}.d/") }
}

/// Decompose `data` (logical name `logical`) into a node; never fails — worst case it is a leaf.
fn decompose(ctx: &mut Ctx, data: &[u8], logical: &str, depth: u32) -> Node {
    if depth < MAX_DEPTH {
        type Parser = fn(&mut Ctx, &[u8], &str, u32) -> Result<Option<Node>>;
        let parsers: [(&str, Parser); 5] = [
            ("hdplayer", parse_hdplayer),
            ("zip", parse_zip),
            ("gzip", parse_gzip),
            ("tar", parse_tar),
            ("magicplayer", parse_magicplayer),
        ];
        for (name, p) in parsers {
            let cp = ctx.checkpoint();
            match p(ctx, data, logical, depth) {
                Ok(Some(node)) => return node,
                Ok(None) => ctx.rollback(cp),
                Err(e) => {
                    ctx.rollback(cp);
                    let msg = format!("{logical}: {name} parse failed, kept as opaque leaf: {e:#}");
                    eprintln!("  ! {msg}");
                    ctx.stats.fallbacks.push(msg);
                    break;
                }
            }
        }
    }
    let name = if logical.is_empty() { "_root" } else { logical };
    ctx.leaf(name, data)
}

fn seq(label: &str, data: &[u8], parts: Vec<Node>) -> Node {
    Node::Seq { label: label.into(), size: data.len(), sha256: sha_hex(data), parts }
}

/// Huidu firmware package: MAGIC, 16-byte MD5 of the rest, u32 LE XML length, XML, payload.
fn parse_vendor_bin(ctx: &mut Ctx, data: &[u8], logical: &str, depth: u32, magic: &[u8], label: &str) -> Result<Option<Node>> {
    if !data.starts_with(magic) || data.len() < magic.len() + 20 {
        return Ok(None);
    }
    let lo = magic.len() + 16;
    let xml_len = u32::from_le_bytes(data[lo..lo + 4].try_into()?) as usize;
    let hdr_end = lo + 4 + xml_len;
    ensure!(hdr_end <= data.len(), "XML length past EOF");
    let dir = child_dir(logical);
    let header = ctx.leaf(&format!("{dir}_header.xml.hdr"), &data[..hdr_end]);
    let payload = decompose(ctx, &data[hdr_end..], &format!("{dir}payload"), depth + 1);
    Ok(Some(seq(label, data, vec![header, payload])))
}

fn parse_hdplayer(ctx: &mut Ctx, d: &[u8], l: &str, depth: u32) -> Result<Option<Node>> {
    parse_vendor_bin(ctx, d, l, depth, b"HDPLAYER", "hdplayer")
}

fn parse_magicplayer(ctx: &mut Ctx, d: &[u8], l: &str, depth: u32) -> Result<Option<Node>> {
    parse_vendor_bin(ctx, d, l, depth, b"MAGICPLAYER", "magicplayer")
}

fn u16le(d: &[u8], o: usize) -> Result<usize> {
    Ok(u16::from_le_bytes(d.get(o..o + 2).ok_or_else(|| anyhow!("short read @{o}"))?.try_into()?) as usize)
}
fn u32le(d: &[u8], o: usize) -> Result<usize> {
    Ok(u32::from_le_bytes(d.get(o..o + 4).ok_or_else(|| anyhow!("short read @{o}"))?.try_into()?) as usize)
}

fn parse_zip(ctx: &mut Ctx, data: &[u8], logical: &str, depth: u32) -> Result<Option<Node>> {
    if !data.starts_with(b"PK\x03\x04") {
        return Ok(None);
    }
    let lo = data.len().saturating_sub(22 + 65535);
    let eocd = (lo..=data.len().saturating_sub(22))
        .rev()
        .find(|&i| &data[i..i + 4] == b"PK\x05\x06")
        .ok_or_else(|| anyhow!("no end-of-central-directory"))?;
    let count = u16le(data, eocd + 10)?;
    let cd_off = u32le(data, eocd + 16)?;
    ensure!(cd_off != 0xFFFF_FFFF && count != 0xFFFF, "zip64 not supported");
    ensure!(cd_off <= eocd, "central directory offset past EOCD");

    struct Ent { name: String, method: usize, csize: usize, off: usize }
    let mut ents = Vec::with_capacity(count);
    let mut p = cd_off;
    for _ in 0..count {
        ensure!(data.get(p..p + 4) == Some(b"PK\x01\x02"), "bad central header @{p}");
        let (method, csize) = (u16le(data, p + 10)?, u32le(data, p + 20)?);
        let (nlen, xlen, clen) = (u16le(data, p + 28)?, u16le(data, p + 30)?, u16le(data, p + 32)?);
        let off = u32le(data, p + 42)?;
        let name = String::from_utf8_lossy(&data[p + 46..p + 46 + nlen]).into_owned();
        ents.push(Ent { name, method, csize, off });
        p += 46 + nlen + xlen + clen;
    }
    ents.sort_by_key(|e| e.off);

    let dir = child_dir(logical);
    let mut parts = Vec::new();
    let mut cur = 0usize;
    for e in &ents {
        ensure!(e.off >= cur, "overlapping zip entries");
        if e.off > cur {
            parts.push(ctx.raw(&data[cur..e.off]));
        }
        ensure!(data.get(e.off..e.off + 4) == Some(b"PK\x03\x04"), "bad local header for {}", e.name);
        let hlen = 30 + u16le(data, e.off + 26)? + u16le(data, e.off + 28)?;
        let dstart = e.off + hlen;
        let dend = dstart + e.csize;
        ensure!(dend <= cd_off, "entry {} runs into central directory", e.name);
        parts.push(ctx.raw(&data[e.off..dstart]));
        let body = &data[dstart..dend];
        let member = format!("{dir}{}", e.name);
        if e.name.ends_with('/') && body.is_empty() {
            // directory entry, nothing to store
        } else if e.method == 0 {
            parts.push(decompose(ctx, body, &member, depth + 1));
        } else if e.method == 8 {
            parts.extend(deflate_node(ctx, body, &member, depth)?);
        } else {
            parts.push(ctx.leaf(&format!("{member}.method{}", e.method), body));
        }
        cur = dend;
    }
    // data descriptors, signing blocks, central directory and EOCD
    if cur < data.len() {
        parts.push(ctx.raw(&data[cur..]));
    }
    Ok(Some(seq("zip", data, parts)))
}

/// A DEFLATE stream at the start of `comp`; returns [Deflate node, raw trailing bytes if any].
/// If preflate cannot reproduce it, the compressed bytes are kept as an opaque leaf.
fn deflate_node(ctx: &mut Ctx, comp: &[u8], logical: &str, depth: u32) -> Result<Vec<Node>> {
    let t = Instant::now();
    let (node_fn, plain, used): (Box<dyn FnOnce(&mut Ctx, Node) -> Node>, Vec<u8>, usize) = match preflate(comp) {
        Ok((chunks, plain, used)) => {
            let chunks: Vec<Chunk> = chunks
                .into_iter()
                .map(|(corr, plain_len)| {
                    ctx.stats.correction_bytes += corr.len();
                    Chunk { plain_len, corrections: ctx.blob(&corr) }
                })
                .collect();
            let sha256 = sha_hex(&comp[..used]);
            (Box::new(move |_, p| Node::Deflate { size: used, sha256, chunks, plain: Box::new(p) }), plain, used)
        }
        Err(pe) => match rawdeflate::analyze(comp) {
            Ok((script, plain, used)) => {
                ctx.stats.token_streams += 1;
                ctx.stats.correction_bytes += script.len();
                let sha256 = sha_hex(&comp[..used]);
                let script = ctx.blob(&script);
                (Box::new(move |_, p| Node::TokenDeflate { size: used, sha256, script, plain: Box::new(p) }), plain, used)
            }
            Err(te) => {
                let msg = format!("{logical}: deflate not reproducible (preflate: {pe}; token script: {te}), stored compressed");
                eprintln!("  ! {msg}");
                ctx.stats.fallbacks.push(msg);
                return Ok(vec![ctx.leaf(&format!("{logical}.deflate"), comp)]);
            }
        },
    };
    if plain.len() > 4 << 20 {
        eprintln!("  deflate {logical}: {used} -> {} bytes in {:.1}s", plain.len(), t.elapsed().as_secs_f64());
    }
    ctx.stats.deflate_streams += 1;
    ctx.stats.deflate_plain += plain.len();
    let plain_node = decompose(ctx, &plain, &strip_gz(logical), depth + 1);
    let mut out = vec![node_fn(ctx, plain_node)];
    if used < comp.len() {
        out.push(ctx.raw(&comp[used..]));
    }
    Ok(out)
}

fn strip_gz(l: &str) -> String {
    for (suf, rep) in [(".tar.gz", ".tar"), (".tgz", ".tar"), (".gz", "")] {
        if let Some(s) = l.strip_suffix(suf) {
            return format!("{s}{rep}");
        }
    }
    l.to_string()
}

type Chunks = Vec<(Vec<u8>, usize)>;

fn preflate(comp: &[u8]) -> Result<(Chunks, Vec<u8>, usize)> {
    const STEP: usize = 8 << 20;
    let cfg = PreflateConfig { max_chain_length: 4096, plain_text_limit: usize::MAX, verify_compression: true };
    let mut p = PreflateStreamProcessor::new(&cfg);
    let (mut start, mut end) = (0usize, comp.len().min(STEP));
    let mut chunks = Vec::new();
    let mut plain = Vec::new();
    while !p.is_done() {
        match p.decompress(&comp[start..end]) {
            Ok(r) => {
                if r.compressed_size == 0 && !p.is_done() {
                    ensure!(end < comp.len(), "deflate stream truncated");
                    end = comp.len().min(end + STEP);
                    continue;
                }
                start += r.compressed_size;
                let text = p.plain_text().text();
                plain.extend_from_slice(text);
                chunks.push((r.corrections, text.len()));
                p.shrink_to_dictionary();
                end = comp.len().min(start + STEP);
            }
            Err(e) if e.exit_code() == ExitCode::ShortRead && end < comp.len() => {
                end = comp.len().min(end + STEP);
            }
            Err(e) => bail!("{:?}: {}", e.exit_code(), e),
        }
    }
    Ok((chunks, plain, start))
}

fn parse_gzip(ctx: &mut Ctx, data: &[u8], logical: &str, depth: u32) -> Result<Option<Node>> {
    if data.len() < 18 || data[..3] != [0x1f, 0x8b, 8] {
        return Ok(None);
    }
    let flg = data[3];
    let mut p = 10;
    if flg & 4 != 0 {
        p += 2 + u16le(data, p)?;
    }
    for bit in [8u8, 16] {
        if flg & bit != 0 {
            p += data[p..].iter().position(|&b| b == 0).ok_or_else(|| anyhow!("unterminated gzip string"))? + 1;
        }
    }
    if flg & 2 != 0 {
        p += 2;
    }
    ensure!(p < data.len(), "gzip header past EOF");
    let mut parts = vec![ctx.raw(&data[..p])];
    // deflate_node appends the trailer (CRC32 + ISIZE, plus anything after) as raw
    parts.extend(deflate_node(ctx, &data[p..], logical, depth)?);
    Ok(Some(seq("gzip", data, parts)))
}

fn tar_num(f: &[u8]) -> Result<usize> {
    if f[0] & 0x80 != 0 {
        return Ok(f[1..].iter().fold(0usize, |a, &b| (a << 8) | b as usize));
    }
    let s = std::str::from_utf8(f)?.trim_matches(|c: char| c == '\0' || c == ' ');
    Ok(if s.is_empty() { 0 } else { usize::from_str_radix(s, 8)? })
}

fn tar_cstr(f: &[u8]) -> String {
    let n = f.iter().position(|&b| b == 0).unwrap_or(f.len());
    String::from_utf8_lossy(&f[..n]).into_owned()
}

fn tar_header_ok(h: &[u8]) -> bool {
    let Ok(want) = tar_num(&h[148..156]) else { return false };
    let sum: usize = h.iter().enumerate().map(|(i, &b)| if (148..156).contains(&i) { 32 } else { b as usize }).sum();
    sum == want
}

fn parse_tar(ctx: &mut Ctx, data: &[u8], logical: &str, depth: u32) -> Result<Option<Node>> {
    if data.len() < 1024 || !tar_header_ok(&data[..512]) {
        return Ok(None);
    }
    let dir = child_dir(logical);
    let mut parts = Vec::new();
    let mut p = 0usize;
    let mut long_name: Option<String> = None;
    while p + 512 <= data.len() {
        let h = &data[p..p + 512];
        if h.iter().all(|&b| b == 0) {
            break;
        }
        ensure!(tar_header_ok(h), "bad tar header checksum @{p}");
        let typ = h[156];
        let size = tar_num(&h[124..136])?;
        let dlen = if b"123456".contains(&typ) { 0 } else { size };
        let dstart = p + 512;
        let dend = dstart + dlen;
        let pad_end = dstart + dlen.div_ceil(512) * 512;
        ensure!(pad_end <= data.len(), "tar member past EOF @{p}");
        parts.push(ctx.raw(h));
        let body = &data[dstart..dend];
        match typ {
            b'L' => {
                long_name = Some(tar_cstr(body));
                parts.push(ctx.raw(body));
            }
            b'K' | b'x' | b'g' => {
                if typ == b'x' {
                    if let Some(path) = String::from_utf8_lossy(body).lines().find_map(|l| l.split_once(" path=").map(|x| x.1.to_string())) {
                        long_name = Some(path);
                    }
                }
                parts.push(ctx.raw(body));
            }
            _ => {
                let mut name = tar_cstr(&h[0..100]);
                if &h[257..263] == b"ustar\0" {
                    let prefix = tar_cstr(&h[345..500]);
                    if !prefix.is_empty() {
                        name = format!("{prefix}/{name}");
                    }
                }
                let name = long_name.take().unwrap_or(name);
                if dlen > 0 || typ == b'0' || typ == 0 {
                    parts.push(decompose(ctx, body, &format!("{dir}{name}"), depth + 1));
                }
            }
        }
        if pad_end > dend {
            parts.push(ctx.raw(&data[dend..pad_end]));
        }
        p = pad_end;
    }
    if p < data.len() {
        parts.push(ctx.raw(&data[p..]));
    }
    Ok(Some(seq("tar", data, parts)))
}

fn unpack(input: &Path, out: &Path) -> Result<()> {
    let data = std::fs::read(input).with_context(|| format!("read {}", input.display()))?;
    ensure!(!out.join("manifest.json").exists(), "{} already contains a manifest; remove it first", out.display());
    let source_name = input.file_name().unwrap().to_string_lossy().into_owned();
    eprintln!("unpacking {source_name} ({} bytes)", data.len());
    let t = Instant::now();
    let mut ctx = Ctx {
        leaves: vec![],
        blobs: vec![],
        blob_keys: HashSet::new(),
        used_files: HashSet::new(),
        used_dirs: HashSet::new(),
        reserved: vec![],
        stats: Stats::default(),
    };
    let root = decompose(&mut ctx, &data, "", 0);
    let manifest = Manifest { format: FORMAT.into(), source_name, size: data.len(), sha256: sha_hex(&data), root };

    let mut written = 0usize;
    let mut biggest = 0usize;
    for (path, bytes) in ctx.leaves.iter().chain(ctx.blobs.iter()) {
        let is_leaf = !path.starts_with("meta/");
        let full = if is_leaf { out.join("tree").join(path) } else { out.join(path) };
        std::fs::create_dir_all(full.parent().unwrap())?;
        if bytes.len() > PART_SIZE {
            for (i, piece) in bytes.chunks(PART_SIZE).enumerate() {
                std::fs::write(part_path(&full, i), piece)?;
                biggest = biggest.max(piece.len());
            }
        } else {
            std::fs::write(&full, bytes).with_context(|| format!("write {}", full.display()))?;
            biggest = biggest.max(bytes.len());
        }
        written += bytes.len();
    }
    std::fs::write(out.join("manifest.json"), serde_json::to_vec_pretty(&manifest)?)?;

    let s = &ctx.stats;
    eprintln!(
        "done in {:.1}s: {} leaves + {} meta blobs, {} bytes on disk (largest file {} bytes)\n  \
         {} deflate streams ({} via token script) re-created from {} plaintext bytes with {} bytes of corrections",
        t.elapsed().as_secs_f64(),
        ctx.leaves.len(),
        ctx.blobs.len(),
        written,
        biggest,
        s.deflate_streams,
        s.token_streams,
        s.deflate_plain,
        s.correction_bytes
    );
    if !s.fallbacks.is_empty() {
        eprintln!("  {} opaque fallbacks (still byte-exact, just not exploded):", s.fallbacks.len());
        for f in &s.fallbacks {
            eprintln!("    {f}");
        }
    }
    eprintln!("verifying rebuild...");
    let rebuilt = rebuild(out)?;
    ensure!(rebuilt.len() == data.len() && rebuilt == data, "REBUILD MISMATCH");
    eprintln!("OK: rebuild is byte-identical (sha256 {})", manifest.sha256);
    Ok(())
}

fn part_path(full: &Path, i: usize) -> PathBuf {
    let mut s = full.as_os_str().to_owned();
    s.push(format!(".part{i:03}"));
    PathBuf::from(s)
}

// ───────────────────────────── pack ─────────────────────────────

struct Loader<'a> {
    dir: &'a Path,
}

impl Loader<'_> {
    fn blob(&self, b: &Blob) -> Result<Vec<u8>> {
        match b {
            Blob::Inline { b64 } => Ok(B64.decode(b64)?),
            Blob::Stored { blob, size } => {
                let d = self.read_maybe_split(&self.dir.join(blob), *size)?;
                ensure!(sha_hex(&d) == Path::new(blob).file_stem().unwrap().to_string_lossy(), "blob {blob} corrupt");
                Ok(d)
            }
        }
    }

    fn read_maybe_split(&self, full: &Path, size: usize) -> Result<Vec<u8>> {
        if size > PART_SIZE {
            let mut v = Vec::with_capacity(size);
            for i in 0..size.div_ceil(PART_SIZE) {
                let p = part_path(full, i);
                v.extend(std::fs::read(&p).with_context(|| format!("read {}", p.display()))?);
            }
            Ok(v)
        } else {
            std::fs::read(full).with_context(|| format!("read {}", full.display()))
        }
    }

    fn eval(&self, n: &Node, out: &mut Vec<u8>) -> Result<()> {
        match n {
            Node::Raw { data } => out.extend(self.blob(data)?),
            Node::Zeros { len } => out.resize(out.len() + len, 0),
            Node::File { path, size, sha256, .. } => {
                let d = self.read_maybe_split(&self.dir.join("tree").join(path), *size)?;
                ensure!(d.len() == *size && sha_hex(&d) == *sha256, "tree/{path} was modified (size/sha256 mismatch)");
                out.extend(d);
            }
            Node::Seq { label, size, sha256, parts } => {
                let start = out.len();
                for p in parts {
                    self.eval(p, out)?;
                }
                let got = &out[start..];
                ensure!(got.len() == *size && sha_hex(got) == *sha256, "{label} container rebuilt wrong");
            }
            Node::Deflate { size, sha256, chunks, plain } => {
                let mut text = Vec::new();
                self.eval(plain, &mut text)?;
                let mut r = RecreateStreamProcessor::new();
                let (mut off, start) = (0usize, out.len());
                for c in chunks {
                    let corr = self.blob(&c.corrections)?;
                    let piece = text.get(off..off + c.plain_len).ok_or_else(|| anyhow!("plaintext shorter than chunk table"))?;
                    let (bytes, _) = r.recompress(&mut Cursor::new(piece), &corr).map_err(|e| anyhow!("recompress: {e}"))?;
                    out.extend(bytes);
                    off += c.plain_len;
                }
                ensure!(off == text.len(), "plaintext longer than chunk table");
                let got = &out[start..];
                ensure!(got.len() == *size && sha_hex(got) == *sha256, "deflate stream rebuilt wrong");
            }
            Node::TokenDeflate { size, sha256, script, plain } => {
                let mut text = Vec::new();
                self.eval(plain, &mut text)?;
                let bytes = rawdeflate::rebuild(&self.blob(script)?, &text)?;
                ensure!(bytes.len() == *size && sha_hex(&bytes) == *sha256, "token-script deflate rebuilt wrong");
                out.extend(bytes);
            }
        }
        Ok(())
    }
}

fn load_manifest(dir: &Path) -> Result<Manifest> {
    let m: Manifest = serde_json::from_slice(&std::fs::read(dir.join("manifest.json")).context("read manifest.json")?)?;
    ensure!(m.format == FORMAT, "unknown manifest format {}", m.format);
    Ok(m)
}

fn rebuild(dir: &Path) -> Result<Vec<u8>> {
    let m = load_manifest(dir)?;
    let mut out = Vec::with_capacity(m.size);
    Loader { dir }.eval(&m.root, &mut out)?;
    ensure!(out.len() == m.size && sha_hex(&out) == m.sha256, "final image sha256 mismatch");
    Ok(out)
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let usage = "usage:\n  zbin-split unpack <IN.zbin> <OUT_DIR>\n  zbin-split pack <DIR> <OUT.zbin>\n  zbin-split verify <DIR>";
    match args.get(1).map(String::as_str) {
        Some("unpack") if args.len() == 4 => unpack(Path::new(&args[2]), Path::new(&args[3])),
        Some("pack") if args.len() == 4 => {
            let t = Instant::now();
            let dir = Path::new(&args[2]);
            let img = rebuild(dir)?;
            std::fs::write(&args[3], &img)?;
            let m = load_manifest(dir)?;
            eprintln!("wrote {} ({} bytes, sha256 {}) in {:.1}s — byte-identical to {}", args[3], img.len(), m.sha256, t.elapsed().as_secs_f64(), m.source_name);
            Ok(())
        }
        Some("verify") if args.len() == 3 => {
            let img = rebuild(Path::new(&args[2]))?;
            eprintln!("OK: {} bytes, sha256 {}", img.len(), sha_hex(&img));
            Ok(())
        }
        _ => bail!("{usage}"),
    }
}
