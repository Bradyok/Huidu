//! zbin-split — explode a Huidu `.zbin` firmware bundle into a tree of small, human-readable
//! files that fit in a plain GitHub repo, and rebuild the `.zbin` from them.
//!
//!   zbin-split unpack <IN.zbin> <DIR>              # decompose (writes DIR/manifest.json, tree/, meta/)
//!   zbin-split pack   <DIR> <OUT.zbin> [--strict]  # rebuild (edits under tree/ allowed unless --strict)
//!   zbin-split verify <DIR>                        # strict rebuild in memory; must match the original
//!   zbin-split status <DIR>                        # list files under tree/ that differ from the original
//!
//! Unedited, every layer is re-emitted bit-for-bit (the output is the original .zbin). When files
//! under tree/ are edited, each enclosing layer regenerates its derived fields: DEFLATE is
//! recompressed, tar sizes/checksums, gzip CRC32/ISIZE, ZIP CRCs/sizes/offsets/central directory,
//! the HDPLAYER/MAGICPLAYER MD5 and XML length, and the sizes in the .zbin's fileInfo.xml.
//!
//! Layers understood (anything that does not parse is kept as an opaque leaf):
//!   ZIP (the .zbin itself, MagicPlayer payload, APKs/JARs)
//!   HDPLAYER / MAGICPLAYER .bin      magic, MD5, u32 XML length, XML (as tree/…/_header.xml), payload
//!   gzip, tar (GNU/ustar)
//!   raw DEFLATE                      preflate-rs (zlib/gzip-made) or a token script (rawdeflate.rs,
//!                                    for Go's compress/flate, which wrote the outer .zbin)
//! Leaves larger than PART_SIZE are split into `name.partNNN` pieces.

use anyhow::{anyhow, bail, ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use md5::Md5;
use preflate_rs::{ExitCode, PreflateConfig, PreflateStreamProcessor, RecreateStreamProcessor};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

mod rawdeflate;

const FORMAT: &str = "huidu-zbin-split/2";
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
    Raw { data: Blob },
    Zeros { len: usize },
    /// A leaf file under tree/. `parts` > 0 means it is split into `path.partNNN`.
    File { path: String, size: usize, sha256: String, #[serde(default, skip_serializing_if = "is_zero")] parts: usize },
    /// DEFLATE re-created from the plaintext node plus preflate corrections.
    Deflate { size: usize, sha256: String, chunks: Vec<Chunk>, plain: Box<Node> },
    /// DEFLATE replayed from a recorded token script (see rawdeflate.rs).
    TokenDeflate { size: usize, sha256: String, script: Blob, plain: Box<Node> },
    /// gzip member: header, DEFLATE body, trailer (CRC32 + ISIZE + anything after).
    Gzip { size: usize, sha256: String, header: Blob, body: Box<Node>, trailer: Blob },
    /// tar archive: `parts` are TarMember nodes and raw glue (long-name records, dirs, links, end zeros).
    Tar { size: usize, sha256: String, parts: Vec<Node> },
    /// One tar member with data: 512-byte header, body, zero padding to 512.
    TarMember { header: Blob, body: Box<Node> },
    /// ZIP archive, entries in file order; central-directory records in their own order.
    Zip { size: usize, sha256: String, entries: Vec<ZipEntry>, cd: Vec<Blob>, eocd_at: usize, tail: Blob },
    /// Huidu package: `magic`, 16-byte MD5, u32 LE XML length, XML, payload.
    Vendor { size: usize, sha256: String, magic: String, md5: Blob, xml: Box<Node>, payload: Box<Node> },
}

#[derive(Serialize, Deserialize)]
struct ZipEntry {
    name: String,
    /// bytes between the previous entry (or file start) and this local header
    #[serde(default, skip_serializing_if = "Option::is_none")]
    before: Option<Box<Node>>,
    local: Blob,
    method: u16,
    /// method 8: a Deflate/TokenDeflate/File(.deflate) node; method 0: the stored data
    body: Box<Node>,
    /// data descriptor and any gap up to the next entry / central directory
    after: Blob,
    cd_index: usize,
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

fn crc32(d: &[u8]) -> u32 {
    let mut h = crc32fast::Hasher::new();
    h.update(d);
    h.finalize()
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

fn parse_vendor_bin(ctx: &mut Ctx, data: &[u8], logical: &str, depth: u32, magic: &str) -> Result<Option<Node>> {
    let m = magic.as_bytes();
    if !data.starts_with(m) || data.len() < m.len() + 20 {
        return Ok(None);
    }
    let lo = m.len() + 16;
    let xml_len = u32le(data, lo)?;
    let hdr_end = lo + 4 + xml_len;
    ensure!(hdr_end <= data.len(), "XML length past EOF");
    // Only claim the format if the MD5 is one we know how to regenerate.
    ensure!(vendor_md5(magic, &data[lo..hdr_end], &data[hdr_end..]) == data[m.len()..lo], "{magic} MD5 does not match known scheme");
    let dir = child_dir(logical);
    let md5 = ctx.blob(&data[m.len()..lo]);
    let xml = ctx.leaf(&format!("{dir}_header.xml"), &data[lo + 4..hdr_end]);
    let payload = decompose(ctx, &data[hdr_end..], &format!("{dir}payload"), depth + 1);
    Ok(Some(Node::Vendor {
        size: data.len(),
        sha256: sha_hex(data),
        magic: magic.into(),
        md5,
        xml: Box::new(xml),
        payload: Box::new(payload),
    }))
}

/// HDPLAYER: MD5 over everything after the MD5 (length + XML + payload). MAGICPLAYER: MD5 of the payload.
fn vendor_md5(magic: &str, len_and_xml: &[u8], payload: &[u8]) -> [u8; 16] {
    let mut h = Md5::new();
    if magic == "HDPLAYER" {
        h.update(len_and_xml);
    }
    h.update(payload);
    h.finalize().into()
}

fn parse_hdplayer(ctx: &mut Ctx, d: &[u8], l: &str, depth: u32) -> Result<Option<Node>> {
    parse_vendor_bin(ctx, d, l, depth, "HDPLAYER")
}

fn parse_magicplayer(ctx: &mut Ctx, d: &[u8], l: &str, depth: u32) -> Result<Option<Node>> {
    parse_vendor_bin(ctx, d, l, depth, "MAGICPLAYER")
}

fn u16le(d: &[u8], o: usize) -> Result<usize> {
    Ok(u16::from_le_bytes(d.get(o..o + 2).ok_or_else(|| anyhow!("short read @{o}"))?.try_into()?) as usize)
}
fn u32le(d: &[u8], o: usize) -> Result<usize> {
    Ok(u32::from_le_bytes(d.get(o..o + 4).ok_or_else(|| anyhow!("short read @{o}"))?.try_into()?) as usize)
}
fn put32(d: &mut [u8], o: usize, v: usize) -> Result<()> {
    ensure!(v <= u32::MAX as usize, "value {v} needs zip64");
    d[o..o + 4].copy_from_slice(&(v as u32).to_le_bytes());
    Ok(())
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

    struct Ent { name: String, method: usize, csize: usize, off: usize, cd_index: usize }
    let mut ents = Vec::with_capacity(count);
    let mut cd = Vec::with_capacity(count);
    let mut p = cd_off;
    for cd_index in 0..count {
        ensure!(data.get(p..p + 4) == Some(b"PK\x01\x02"), "bad central header @{p}");
        let (method, csize) = (u16le(data, p + 10)?, u32le(data, p + 20)?);
        let (nlen, xlen, clen) = (u16le(data, p + 28)?, u16le(data, p + 30)?, u16le(data, p + 32)?);
        let off = u32le(data, p + 42)?;
        let name = String::from_utf8_lossy(&data[p + 46..p + 46 + nlen]).into_owned();
        let end = p + 46 + nlen + xlen + clen;
        cd.push(ctx.blob(&data[p..end]));
        ents.push(Ent { name, method, csize, off, cd_index });
        p = end;
    }
    let cd_end = p;
    ents.sort_by_key(|e| e.off);

    let dir = child_dir(logical);
    let mut entries = Vec::new();
    for (i, e) in ents.iter().enumerate() {
        ensure!(data.get(e.off..e.off + 4) == Some(b"PK\x03\x04"), "bad local header for {}", e.name);
        let hlen = 30 + u16le(data, e.off + 26)? + u16le(data, e.off + 28)?;
        let dstart = e.off + hlen;
        let dend = dstart + e.csize;
        let next = ents.get(i + 1).map_or(cd_off, |n| n.off);
        ensure!(dend <= next, "entry {} overlaps the next entry", e.name);
        let body = &data[dstart..dend];
        let member = format!("{dir}{}", e.name);
        let body_node = match e.method {
            0 if e.name.ends_with('/') && body.is_empty() => Node::Raw { data: Blob::Inline { b64: String::new() } },
            0 => decompose(ctx, body, &member, depth + 1),
            8 => {
                let (node, used) = deflate_node(ctx, body, &member, depth)?;
                ensure!(used == body.len(), "entry {} has {} bytes after its DEFLATE stream", e.name, body.len() - used);
                node
            }
            m => ctx.leaf(&format!("{member}.method{m}"), body),
        };
        entries.push(ZipEntry {
            name: e.name.clone(),
            before: None,
            local: ctx.blob(&data[e.off..dstart]),
            method: e.method as u16,
            body: Box::new(body_node),
            after: ctx.blob(&data[dend..next]),
            cd_index: e.cd_index,
        });
    }
    if let Some(first) = ents.first() {
        if first.off > 0 {
            entries[0].before = Some(Box::new(ctx.raw(&data[..first.off])));
        }
    }
    ensure!(!entries.is_empty(), "empty zip");
    Ok(Some(Node::Zip {
        size: data.len(),
        sha256: sha_hex(data),
        entries,
        cd,
        eocd_at: eocd - cd_end,
        tail: ctx.blob(&data[cd_end..]),
    }))
}

/// A DEFLATE stream at the start of `comp`; returns (node, compressed bytes consumed).
/// If neither preflate nor a token script reproduces it, the whole of `comp` becomes an opaque leaf.
fn deflate_node(ctx: &mut Ctx, comp: &[u8], logical: &str, depth: u32) -> Result<(Node, usize)> {
    let t = Instant::now();
    let (node_fn, plain, used): (Box<dyn FnOnce(Node) -> Node>, Vec<u8>, usize) = match preflate(comp) {
        Ok((chunks, plain, used)) => {
            let chunks: Vec<Chunk> = chunks
                .into_iter()
                .map(|(corr, plain_len)| {
                    ctx.stats.correction_bytes += corr.len();
                    Chunk { plain_len, corrections: ctx.blob(&corr) }
                })
                .collect();
            let sha256 = sha_hex(&comp[..used]);
            (Box::new(move |p| Node::Deflate { size: used, sha256, chunks, plain: Box::new(p) }), plain, used)
        }
        Err(pe) => match rawdeflate::analyze(comp) {
            Ok((script, plain, used)) => {
                ctx.stats.token_streams += 1;
                ctx.stats.correction_bytes += script.len();
                let sha256 = sha_hex(&comp[..used]);
                let script = ctx.blob(&script);
                (Box::new(move |p| Node::TokenDeflate { size: used, sha256, script, plain: Box::new(p) }), plain, used)
            }
            Err(te) => {
                let msg = format!("{logical}: deflate not reproducible (preflate: {pe}; token script: {te}), stored compressed");
                eprintln!("  ! {msg}");
                ctx.stats.fallbacks.push(msg);
                return Ok((ctx.leaf(&format!("{logical}.deflate"), comp), comp.len()));
            }
        },
    };
    if plain.len() > 4 << 20 {
        eprintln!("  deflate {logical}: {used} -> {} bytes in {:.1}s", plain.len(), t.elapsed().as_secs_f64());
    }
    ctx.stats.deflate_streams += 1;
    ctx.stats.deflate_plain += plain.len();
    let plain_node = decompose(ctx, &plain, &strip_gz(logical), depth + 1);
    Ok((node_fn(plain_node), used))
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
    let header = ctx.blob(&data[..p]);
    let (body, used) = deflate_node(ctx, &data[p..], logical, depth)?;
    Ok(Some(Node::Gzip { size: data.len(), sha256: sha_hex(data), header, body: Box::new(body), trailer: ctx.blob(&data[p + used..]) }))
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

/// Rewrite a tar header's size field and checksum (GNU layout) for an edited member.
fn tar_set_size(h: &mut [u8], size: usize) {
    if size < 1 << 33 {
        h[124..136].copy_from_slice(format!("{size:011o}\0").as_bytes());
    } else {
        h[124] = 0x80;
        h[125..136].fill(0);
        h[128..136].copy_from_slice(&(size as u64).to_be_bytes());
    }
    h[148..156].fill(b' ');
    let sum: usize = h.iter().map(|&b| b as usize).sum();
    h[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
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
        let body = &data[dstart..dend];
        match typ {
            0 | b'0' | b'7' => {
                ensure!(data[dend..pad_end].iter().all(|&b| b == 0), "non-zero tar padding @{dend}");
                let mut name = tar_cstr(&h[0..100]);
                if &h[257..263] == b"ustar\0" {
                    let prefix = tar_cstr(&h[345..500]);
                    if !prefix.is_empty() {
                        name = format!("{prefix}/{name}");
                    }
                }
                let name = long_name.take().unwrap_or(name);
                let node = decompose(ctx, body, &format!("{dir}{name}"), depth + 1);
                parts.push(Node::TarMember { header: ctx.blob(h), body: Box::new(node) });
            }
            b'L' | b'K' | b'x' | b'g' => {
                if typ == b'L' {
                    long_name = Some(tar_cstr(body));
                } else if typ == b'x' {
                    if let Some(path) = String::from_utf8_lossy(body).lines().find_map(|l| l.split_once(" path=").map(|x| x.1.to_string())) {
                        long_name = Some(path);
                    }
                }
                parts.push(ctx.raw(&data[p..pad_end]));
            }
            b'1'..=b'6' => parts.push(ctx.raw(h)),
            t => bail!("unsupported tar member type {:?} @{p}", t as char),
        }
        p = pad_end;
    }
    if p < data.len() {
        parts.push(ctx.raw(&data[p..]));
    }
    Ok(Some(Node::Tar { size: data.len(), sha256: sha_hex(data), parts }))
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
    let (rebuilt, _) = rebuild(out, true)?;
    ensure!(rebuilt == data, "REBUILD MISMATCH");
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
    /// refuse any edit (verify / pack --strict)
    strict: bool,
    edited: Vec<String>,
    warnings: Vec<String>,
}

/// A rebuilt compressed stream: (compressed bytes, plaintext, changed-from-original).
type Stream = (Vec<u8>, Vec<u8>, bool);

fn deflate_fresh(plain: &[u8]) -> Result<Vec<u8>> {
    let mut e = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::best());
    e.write_all(plain)?;
    Ok(e.finish()?)
}

fn inflate(comp: &[u8]) -> Result<Vec<u8>> {
    let mut v = Vec::new();
    flate2::read::DeflateDecoder::new(comp).read_to_end(&mut v)?;
    Ok(v)
}

impl Loader<'_> {
    fn blob(&self, b: &Blob) -> Result<Vec<u8>> {
        match b {
            Blob::Inline { b64 } => Ok(B64.decode(b64)?),
            Blob::Stored { blob, size } => {
                let full = self.dir.join(blob);
                let parts = if *size > PART_SIZE { size.div_ceil(PART_SIZE) } else { 0 };
                let d = self.read_parts(&full, parts)?;
                ensure!(sha_hex(&d) == Path::new(blob).file_stem().unwrap().to_string_lossy(), "blob {blob} corrupt");
                Ok(d)
            }
        }
    }

    fn read_parts(&self, full: &Path, parts: usize) -> Result<Vec<u8>> {
        if parts == 0 {
            return std::fs::read(full).with_context(|| format!("read {}", full.display()));
        }
        let mut v = Vec::new();
        for i in 0..parts {
            let p = part_path(full, i);
            v.extend(std::fs::read(&p).with_context(|| format!("read {}", p.display()))?);
        }
        Ok(v)
    }

    /// Check a container's output against the original unless something inside it was edited.
    fn check(&self, what: &str, got: &[u8], dirty: bool, size: usize, sha256: &str) -> Result<()> {
        ensure!(dirty || (got.len() == size && sha_hex(got) == sha256), "{what} rebuilt wrong (corrupt meta/ or manifest?)");
        Ok(())
    }

    /// Append node `n` to `out`; returns true if anything beneath it differs from the original.
    fn eval(&mut self, n: &Node, out: &mut Vec<u8>) -> Result<bool> {
        Ok(match n {
            Node::Raw { data } => {
                out.extend(self.blob(data)?);
                false
            }
            Node::Zeros { len } => {
                out.resize(out.len() + len, 0);
                false
            }
            Node::File { path, size, sha256, parts } => {
                let d = self.read_parts(&self.dir.join("tree").join(path), *parts)?;
                let dirty = d.len() != *size || sha_hex(&d) != *sha256;
                if dirty {
                    ensure!(!self.strict, "tree/{path} differs from the original (strict mode)");
                    self.edited.push(path.clone());
                }
                out.extend(d);
                dirty
            }
            Node::Deflate { .. } | Node::TokenDeflate { .. } => {
                let (comp, _, dirty) = self.stream(n)?;
                out.extend(comp);
                dirty
            }
            Node::Gzip { size, sha256, header, body, trailer } => {
                let start = out.len();
                out.extend(self.blob(header)?);
                let (comp, plain, dirty) = self.stream(body)?;
                out.extend(comp);
                let mut tr = self.blob(trailer)?;
                if dirty {
                    ensure!(tr.len() >= 8, "gzip trailer missing; cannot update CRC");
                    tr[..4].copy_from_slice(&crc32(&plain).to_le_bytes());
                    tr[4..8].copy_from_slice(&(plain.len() as u32).to_le_bytes());
                }
                out.extend(tr);
                self.check("gzip", &out[start..], dirty, *size, sha256)?;
                dirty
            }
            Node::Tar { size, sha256, parts } => {
                let start = out.len();
                let mut dirty = false;
                for p in parts {
                    dirty |= self.eval(p, out)?;
                }
                self.check("tar", &out[start..], dirty, *size, sha256)?;
                dirty
            }
            Node::TarMember { header, body } => {
                let mut h = self.blob(header)?;
                let mut data = Vec::new();
                let dirty = self.eval(body, &mut data)?;
                if dirty {
                    tar_set_size(&mut h, data.len());
                }
                out.extend(h);
                let pad = data.len().div_ceil(512) * 512 - data.len();
                out.extend(data);
                out.resize(out.len() + pad, 0);
                dirty
            }
            Node::Zip { size, sha256, entries, cd, eocd_at, tail } => self.eval_zip(out, entries, cd, *eocd_at, tail, *size, sha256)?,
            Node::Vendor { size, sha256, magic, md5, xml, payload } => {
                let start = out.len();
                let mut x = Vec::new();
                let mut body = Vec::new();
                let dirty = self.eval(xml, &mut x)? | self.eval(payload, &mut body)?;
                let mut len_xml = (x.len() as u32).to_le_bytes().to_vec();
                len_xml.extend(&x);
                let digest = if dirty { vendor_md5(magic, &len_xml, &body).to_vec() } else { self.blob(md5)? };
                out.extend(magic.as_bytes());
                out.extend(digest);
                out.extend(len_xml);
                out.extend(body);
                self.check(magic, &out[start..], dirty, *size, sha256)?;
                dirty
            }
        })
    }

    /// Rebuild a compressed stream node (Deflate/TokenDeflate, or an opaque `.deflate` leaf).
    fn stream(&mut self, n: &Node) -> Result<Stream> {
        match n {
            Node::Deflate { size, sha256, chunks, plain } => {
                let mut text = Vec::new();
                if self.eval(plain, &mut text)? {
                    return Ok((deflate_fresh(&text)?, text, true));
                }
                let mut r = RecreateStreamProcessor::new();
                let mut comp = Vec::with_capacity(*size);
                let mut off = 0usize;
                for c in chunks {
                    let corr = self.blob(&c.corrections)?;
                    let piece = text.get(off..off + c.plain_len).ok_or_else(|| anyhow!("plaintext shorter than chunk table"))?;
                    let (bytes, _) = r.recompress(&mut Cursor::new(piece), &corr).map_err(|e| anyhow!("recompress: {e}"))?;
                    comp.extend(bytes);
                    off += c.plain_len;
                }
                ensure!(off == text.len(), "plaintext longer than chunk table");
                self.check("deflate stream", &comp, false, *size, sha256)?;
                Ok((comp, text, false))
            }
            Node::TokenDeflate { size, sha256, script, plain } => {
                let mut text = Vec::new();
                if self.eval(plain, &mut text)? {
                    return Ok((deflate_fresh(&text)?, text, true));
                }
                let comp = rawdeflate::rebuild(&self.blob(script)?, &text)?;
                self.check("token-script deflate", &comp, false, *size, sha256)?;
                Ok((comp, text, false))
            }
            other => {
                let mut comp = Vec::new();
                let dirty = self.eval(other, &mut comp)?;
                let text = inflate(&comp)?;
                Ok((comp, text, dirty))
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn eval_zip(&mut self, out: &mut Vec<u8>, entries: &[ZipEntry], cd: &[Blob], eocd_at: usize, tail: &Blob, size: usize, sha256: &str) -> Result<bool> {
        let start = out.len();
        // Phase 1: rebuild every entry's data.
        let mut built: Vec<(Vec<u8>, Vec<u8>, bool)> = Vec::with_capacity(entries.len());
        for e in entries {
            built.push(match e.method {
                8 => self.stream(&e.body)?,
                0 => {
                    let mut d = Vec::new();
                    let dirty = self.eval(&e.body, &mut d)?;
                    (d.clone(), d, dirty)
                }
                m => {
                    let mut d = Vec::new();
                    let dirty = self.eval(&e.body, &mut d)?;
                    ensure!(!dirty, "{}: editing ZIP method-{m} entries is not supported", e.name);
                    (d, Vec::new(), false)
                }
            });
        }
        let mut dirty = built.iter().any(|b| b.2);

        // Huidu .zbin: fileInfo.xml lists every .bin with its size; keep it truthful.
        if dirty {
            if let Some(fi) = entries.iter().position(|e| e.name == "fileInfo.xml") {
                let mut xml = String::from_utf8(built[fi].1.clone()).context("fileInfo.xml is not UTF-8")?;
                for (e, b) in entries.iter().zip(&built) {
                    let key = format!("name=\"{}\" size=\"", e.name);
                    if let Some(p) = xml.find(&key) {
                        let vs = p + key.len();
                        let ve = vs + xml[vs..].find('"').ok_or_else(|| anyhow!("bad fileInfo.xml"))?;
                        xml.replace_range(vs..ve, &b.1.len().to_string());
                    }
                }
                if xml.as_bytes() != built[fi].1 {
                    let plain = xml.into_bytes();
                    let comp = if entries[fi].method == 8 { deflate_fresh(&plain)? } else { plain.clone() };
                    built[fi] = (comp, plain, true);
                    eprintln!("  updated fileInfo.xml sizes");
                }
            }
        }

        // Phase 2: lay out local headers + data, then the central directory with fixed-up fields.
        let mut offsets = vec![0usize; cd.len()];
        for (e, (comp, plain, edirty)) in entries.iter().zip(&built) {
            if let Some(b) = &e.before {
                dirty |= self.eval(b, out)?;
            }
            offsets[e.cd_index] = out.len() - start;
            let mut local = self.blob(&e.local)?;
            let mut after = self.blob(&e.after)?;
            if *edirty {
                let (crc, flags) = (crc32(plain) as usize, u16le(&local, 6)?);
                if flags & 8 == 0 {
                    put32(&mut local, 14, crc)?;
                    put32(&mut local, 18, comp.len())?;
                    put32(&mut local, 22, plain.len())?;
                } else {
                    let d = if after.starts_with(b"PK\x07\x08") { 4 } else { 0 };
                    ensure!(after.len() >= d + 12, "{}: data descriptor missing", e.name);
                    put32(&mut after, d, crc)?;
                    put32(&mut after, d + 4, comp.len())?;
                    put32(&mut after, d + 8, plain.len())?;
                }
                if after.windows(16).any(|w| w == b"APK Sig Block 42") {
                    self.warnings.push(format!("{}: APK signing block is now stale; re-sign this APK before installing", e.name));
                }
            }
            out.extend(local);
            out.extend(comp);
            out.extend(after);
        }
        let cd_start = out.len() - start;
        let mut cd_edits = vec![None; cd.len()];
        for (e, (comp, plain, edirty)) in entries.iter().zip(&built) {
            if *edirty {
                cd_edits[e.cd_index] = Some((crc32(plain) as usize, comp.len(), plain.len()));
            }
        }
        for (i, rec) in cd.iter().enumerate() {
            let mut r = self.blob(rec)?;
            if let Some((crc, cs, us)) = cd_edits[i] {
                put32(&mut r, 16, crc)?;
                put32(&mut r, 20, cs)?;
                put32(&mut r, 24, us)?;
            }
            if dirty {
                put32(&mut r, 42, offsets[i])?;
            }
            out.extend(r);
        }
        let cd_size = out.len() - start - cd_start;
        let mut t = self.blob(tail)?;
        if dirty {
            put32(&mut t, eocd_at + 12, cd_size)?;
            put32(&mut t, eocd_at + 16, cd_start)?;
        }
        out.extend(t);
        self.check("zip", &out[start..], dirty, size, sha256)?;
        Ok(dirty)
    }
}

fn load_manifest(dir: &Path) -> Result<Manifest> {
    let m: Manifest = serde_json::from_slice(&std::fs::read(dir.join("manifest.json")).context("read manifest.json")?)?;
    ensure!(m.format == FORMAT, "manifest format {} (this build reads {FORMAT}); re-run unpack", m.format);
    Ok(m)
}

/// Rebuild the image; returns (bytes, loader with the edit list and warnings).
fn rebuild(dir: &Path, strict: bool) -> Result<(Vec<u8>, Vec<String>)> {
    let m = load_manifest(dir)?;
    let mut l = Loader { dir, strict, edited: vec![], warnings: vec![] };
    let mut out = Vec::with_capacity(m.size);
    let dirty = l.eval(&m.root, &mut out)?;
    if !dirty {
        ensure!(out.len() == m.size && sha_hex(&out) == m.sha256, "final image sha256 mismatch");
    }
    for w in &l.warnings {
        eprintln!("  warning: {w}");
    }
    Ok((out, l.edited))
}

fn status(dir: &Path) -> Result<Vec<String>> {
    fn walk(n: &Node, f: &mut dyn FnMut(&str, usize, &str, usize)) {
        match n {
            Node::File { path, size, sha256, parts } => f(path, *size, sha256, *parts),
            Node::Deflate { plain, .. } | Node::TokenDeflate { plain, .. } => walk(plain, f),
            Node::Gzip { body, .. } | Node::TarMember { body, .. } => walk(body, f),
            Node::Tar { parts, .. } => parts.iter().for_each(|p| walk(p, f)),
            Node::Zip { entries, .. } => entries.iter().for_each(|e| {
                if let Some(b) = &e.before {
                    walk(b, f);
                }
                walk(&e.body, f)
            }),
            Node::Vendor { xml, payload, .. } => {
                walk(xml, f);
                walk(payload, f)
            }
            Node::Raw { .. } | Node::Zeros { .. } => {}
        }
    }
    let m = load_manifest(dir)?;
    let l = Loader { dir, strict: false, edited: vec![], warnings: vec![] };
    let mut changed = vec![];
    let mut err = None;
    walk(&m.root, &mut |path, size, sha, parts| match l.read_parts(&dir.join("tree").join(path), parts) {
        Ok(d) if d.len() == size && sha_hex(&d) == sha => {}
        Ok(_) => changed.push(path.to_string()),
        Err(e) => err = Some(e),
    });
    if let Some(e) = err {
        return Err(e);
    }
    Ok(changed)
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let usage = "usage:\n  zbin-split unpack <IN.zbin> <DIR>\n  zbin-split pack <DIR> <OUT.zbin> [--strict]\n  zbin-split verify <DIR>\n  zbin-split status <DIR>";
    match args.get(1).map(String::as_str) {
        Some("unpack") if args.len() == 4 => unpack(Path::new(&args[2]), Path::new(&args[3])),
        Some("pack") if args.len() == 4 || (args.len() == 5 && args[4] == "--strict") => {
            let t = Instant::now();
            let dir = Path::new(&args[2]);
            let (img, edited) = rebuild(dir, args.len() == 5)?;
            std::fs::write(&args[3], &img)?;
            let m = load_manifest(dir)?;
            let sha = sha_hex(&img);
            eprintln!("wrote {} ({} bytes, sha256 {sha}) in {:.1}s", args[3], img.len(), t.elapsed().as_secs_f64());
            if edited.is_empty() {
                eprintln!("byte-identical to the original {}", m.source_name);
            } else {
                eprintln!("MODIFIED image ({} edited file(s), derived CRCs/sizes/MD5s regenerated):", edited.len());
                for e in &edited {
                    eprintln!("  {e}");
                }
            }
            Ok(())
        }
        Some("verify") if args.len() == 3 => {
            let (img, _) = rebuild(Path::new(&args[2]), true)?;
            eprintln!("OK: byte-identical to the original, {} bytes, sha256 {}", img.len(), sha_hex(&img));
            Ok(())
        }
        Some("status") if args.len() == 3 => {
            let changed = status(Path::new(&args[2]))?;
            if changed.is_empty() {
                eprintln!("clean: tree/ matches the original");
            }
            for c in changed {
                println!("modified: tree/{c}");
            }
            Ok(())
        }
        _ => bail!("{usage}"),
    }
}
