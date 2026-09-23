//! zbin-split — explode a Huidu `.zbin` firmware bundle into a tree of small, human-readable
//! files that fit in a plain GitHub repo, and rebuild the `.zbin` from them.
//!
//!   zbin-split unpack <IN.zbin> <DIR>              # decompose (writes DIR/manifest.json, tree/, meta/)
//!   zbin-split pack   <DIR> <OUT.zbin> [--strict]  # rebuild (edits under tree/ allowed unless --strict)
//!   zbin-split verify <DIR>                        # strict rebuild in memory; must match the original
//!   zbin-split status <DIR>                        # list files under tree/ modified/removed/added
//!
//! Unedited, every layer is re-emitted bit-for-bit (the output is the original .zbin). When files
//! under tree/ are edited, deleted or added (a new file joins the innermost tar/zip whose
//! directory contains it), each enclosing layer regenerates its derived fields: DEFLATE is
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
use std::collections::{BTreeMap, HashSet};
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

mod rawdeflate;
#[cfg(test)]
mod tests;

const FORMAT: &str = "huidu-zbin-split/3";
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
    /// tar archive: `parts` are TarMember nodes and raw records (dirs, links); `tail` is the
    /// end-of-archive zeros. `dir` is its directory under tree/ (new files there become members).
    Tar { size: usize, sha256: String, dir: String, parts: Vec<Node>, tail: Box<Node> },
    /// One tar member with data: header (any GNU long-name/PAX records, then the 512-byte header
    /// whose size/checksum get fixed up), body, zero padding to 512.
    TarMember { header: Blob, body: Box<Node> },
    /// ZIP archive, entries in file order; central-directory records in their own order.
    Zip { size: usize, sha256: String, dir: String, entries: Vec<ZipEntry>, cd: Vec<Blob>, eocd_at: usize, tail: Blob },
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

/// Where `child_dir(logical)` lands under tree/ once sanitized (as `Ctx::reserve` does).
fn tree_dir(logical: &str) -> String {
    let d = child_dir(logical);
    d.split(['/', '\\']).filter(|c| !c.is_empty() && *c != ".").map(|c| sanitize(c) + "/").collect()
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
        dir: tree_dir(logical),
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
    type Wrap = Box<dyn FnOnce(Node) -> Node>;
    let (node_fn, plain, used): (Wrap, Vec<u8>, usize) = match preflate(comp) {
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
    let mut pending: Vec<u8> = Vec::new(); // GNU long-name / PAX records owned by the next member
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
                pending.extend_from_slice(h);
                parts.push(Node::TarMember { header: ctx.blob(&pending), body: Box::new(node) });
                pending.clear();
            }
            b'L' | b'K' | b'x' | b'g' => {
                if typ == b'L' {
                    long_name = Some(tar_cstr(body));
                } else if typ == b'x' {
                    if let Some(path) = String::from_utf8_lossy(body).lines().find_map(|l| l.split_once(" path=").map(|x| x.1.to_string())) {
                        long_name = Some(path);
                    }
                }
                pending.extend_from_slice(&data[p..pad_end]);
            }
            b'1'..=b'6' => {
                pending.extend_from_slice(h);
                parts.push(ctx.raw(&pending));
                pending.clear();
            }
            t => bail!("unsupported tar member type {:?} @{p}", t as char),
        }
        p = pad_end;
    }
    ensure!(pending.is_empty(), "tar ends with a dangling long-name record");
    let tail = Box::new(ctx.raw(&data[p..]));
    Ok(Some(Node::Tar { size: data.len(), sha256: sha_hex(data), dir: tree_dir(logical), parts, tail }))
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
    /// new files under tree/, keyed by the (lowercased) tree dir of the tar/zip they join
    additions: BTreeMap<String, Vec<String>>,
    edited: Vec<String>,
    warnings: Vec<String>,
}

/// Changes to tree/ relative to the manifest.
#[derive(Default)]
struct TreeChanges {
    modified: Vec<String>,
    removed: Vec<String>,
    /// lowercased container tree dir -> new file paths (tree-relative)
    added: BTreeMap<String, Vec<String>>,
}

/// OS/editor litter that must never be packed into firmware.
fn is_junk(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    ["thumbs.db", "desktop.ini", ".ds_store"].contains(&n.as_str())
        || n.ends_with('~')
        || n.ends_with(".swp")
        || n.ends_with(".orig")
        || n.ends_with(".rej")
}

/// The on-disk leaf a (zip/tar) member body is, if it is a plain file (possibly under DEFLATE).
fn leaf_of(n: &Node) -> Option<(&str, usize)> {
    match n {
        Node::File { path, parts, .. } => Some((path, *parts)),
        Node::Deflate { plain, .. } | Node::TokenDeflate { plain, .. } => leaf_of(plain),
        _ => None,
    }
}

fn walk_nodes(n: &Node, f: &mut dyn FnMut(&Node)) {
    f(n);
    match n {
        Node::Deflate { plain, .. } | Node::TokenDeflate { plain, .. } => walk_nodes(plain, f),
        Node::Gzip { body, .. } | Node::TarMember { body, .. } => walk_nodes(body, f),
        Node::Tar { parts, tail, .. } => {
            parts.iter().for_each(|p| walk_nodes(p, f));
            walk_nodes(tail, f)
        }
        Node::Zip { entries, .. } => entries.iter().for_each(|e| {
            if let Some(b) = &e.before {
                walk_nodes(b, f);
            }
            walk_nodes(&e.body, f)
        }),
        Node::Vendor { xml, payload, .. } => {
            walk_nodes(xml, f);
            walk_nodes(payload, f)
        }
        Node::File { .. } | Node::Raw { .. } | Node::Zeros { .. } => {}
    }
}

/// `x.part007` -> `x`
fn part_base(path: &str) -> Option<&str> {
    let (base, suf) = path.rsplit_once(".part")?;
    (suf.len() == 3 && suf.bytes().all(|b| b.is_ascii_digit())).then_some(base)
}

/// Compare tree/ against the manifest: modified, removed and newly added files.
fn scan_tree(dir: &Path, root: &Node) -> Result<TreeChanges> {
    // lowercased path -> (path, size, sha256, parts)
    let mut known: BTreeMap<String, (String, usize, String, usize)> = BTreeMap::new();
    let mut containers: Vec<String> = vec![];
    walk_nodes(root, &mut |n| match n {
        Node::File { path, size, sha256, parts } => {
            known.insert(path.to_lowercase(), (path.clone(), *size, sha256.clone(), *parts));
        }
        Node::Tar { dir, .. } | Node::Zip { dir, .. } => containers.push(dir.to_lowercase()),
        _ => {}
    });
    let tree = dir.join("tree");
    let mut ch = TreeChanges::default();
    let l = Loader { dir, strict: false, additions: BTreeMap::new(), edited: vec![], warnings: vec![] };
    for (path, size, sha, parts) in known.values() {
        let full = tree.join(path);
        let exists = if *parts == 0 { full.exists() } else { part_path(&full, 0).exists() };
        if !exists {
            ch.removed.push(path.clone());
            continue;
        }
        let d = l.read_parts(&full, *parts)?;
        if d.len() != *size || sha_hex(&d) != *sha {
            ch.modified.push(path.clone());
        }
    }
    let mut stack = vec![tree.clone()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).with_context(|| format!("list {}", d.display()))? {
            let e = e?;
            let p = e.path();
            if e.file_type()?.is_dir() {
                stack.push(p);
                continue;
            }
            let rel = p.strip_prefix(&tree)?.to_string_lossy().replace('\\', "/");
            let lc = rel.to_lowercase();
            if is_junk(&e.file_name().to_string_lossy()) || known.contains_key(&lc) {
                continue;
            }
            if part_base(&lc).and_then(|b| known.get(b)).is_some_and(|k| k.3 > 0) {
                continue;
            }
            let owner = containers.iter().filter(|c| lc.starts_with(c.as_str())).max_by_key(|c| c.len());
            let owner = owner.ok_or_else(|| anyhow!("tree/{rel} is new but not inside any tar/zip directory"))?;
            ch.added.entry(owner.clone()).or_default().push(rel);
        }
    }
    for v in ch.added.values_mut() {
        v.sort();
    }
    ch.modified.sort();
    ch.removed.sort();
    Ok(ch)
}

fn is_executable(name: &str, data: &[u8]) -> bool {
    name.ends_with(".sh") || data.starts_with(b"\x7fELF") || data.starts_with(b"#!")
}

/// A new tar member modelled on `template` (an existing member's 512-byte header): same owner,
/// group, mtime and format; a GNU long-name record first if the name exceeds 100 bytes.
fn new_tar_member(template: &[u8], name: &str, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let nb = name.as_bytes();
    let mut h = template.to_vec();
    h[0..100].fill(0);
    h[157..257].fill(0); // linkname
    h[345..500].fill(0); // POSIX prefix / GNU atime, ctime, sparse map
    if nb.len() > 100 {
        let mut l = h.clone();
        l[..13].copy_from_slice(b"././@LongLink");
        l[100..108].copy_from_slice(b"0000644\0");
        l[156] = b'L';
        tar_set_size(&mut l, nb.len() + 1);
        out.extend(l);
        let mut body = nb.to_vec();
        body.push(0);
        body.resize(body.len().div_ceil(512) * 512, 0);
        out.extend(body);
        h[..100].copy_from_slice(&nb[..100]);
    } else {
        h[..nb.len()].copy_from_slice(nb);
    }
    let mode: &[u8] = if is_executable(name, data) { b"0000755\0" } else { b"0000644\0" };
    h[100..108].copy_from_slice(mode);
    h[156] = b'0';
    tar_set_size(&mut h, data.len());
    out.extend(h);
    out.extend_from_slice(data);
    out.resize(out.len().div_ceil(512) * 512, 0);
    out
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
            Node::Tar { size, sha256, dir, parts, tail } => {
                let start = out.len();
                let mut dirty = false;
                let mut template = None;
                for p in parts {
                    if let Node::TarMember { header, body } = p {
                        if template.is_none() {
                            let h = self.blob(header)?;
                            template = Some(h[h.len() - 512..].to_vec());
                        }
                        if self.deleted(body)? {
                            dirty = true;
                            continue;
                        }
                    }
                    dirty |= self.eval(p, out)?;
                }
                for rel in self.additions.remove(&dir.to_lowercase()).unwrap_or_default() {
                    let t = template.as_ref().ok_or_else(|| anyhow!("cannot add {rel}: tar {dir} has no member to copy a header from"))?;
                    let data = std::fs::read(self.dir.join("tree").join(&rel))?;
                    out.extend(new_tar_member(t, &rel[dir.len()..], &data));
                    self.edited.push(format!("{rel} (added)"));
                    dirty = true;
                }
                dirty |= self.eval(tail, out)?;
                self.check("tar", &out[start..], dirty, *size, sha256)?;
                dirty
            }
            Node::TarMember { header, body } => {
                let mut h = self.blob(header)?;
                let mut data = Vec::new();
                let dirty = self.eval(body, &mut data)?;
                if dirty {
                    let n = h.len();
                    tar_set_size(&mut h[n - 512..], data.len());
                }
                out.extend(h);
                let pad = data.len().div_ceil(512) * 512 - data.len();
                out.extend(data);
                out.resize(out.len() + pad, 0);
                dirty
            }
            Node::Zip { size, sha256, dir, entries, cd, eocd_at, tail } => self.eval_zip(out, dir, entries, cd, *eocd_at, tail, *size, sha256)?,
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

    /// A member body that is a plain file which the user deleted from tree/.
    fn deleted(&mut self, body: &Node) -> Result<bool> {
        let Some((path, parts)) = leaf_of(body) else { return Ok(false) };
        let full = self.dir.join("tree").join(path);
        let gone = if parts == 0 { !full.exists() } else { !part_path(&full, 0).exists() };
        if gone {
            ensure!(!self.strict, "tree/{path} was deleted (strict mode)");
            self.edited.push(format!("{path} (removed)"));
        }
        Ok(gone)
    }

    #[allow(clippy::too_many_arguments)]
    fn eval_zip(&mut self, out: &mut Vec<u8>, dir: &str, entries: &[ZipEntry], cd: &[Blob], eocd_at: usize, tail: &Blob, size: usize, sha256: &str) -> Result<bool> {
        struct Built {
            name: String,
            comp: Vec<u8>,
            plain: Vec<u8>,
            dirty: bool,
            method: u16,
            /// index into `entries`, or None for a new file
            src: Option<usize>,
        }
        let start = out.len();
        let mut dirty = false;
        // Phase 1: rebuild every surviving entry's data; append new files.
        let mut built: Vec<Built> = Vec::with_capacity(entries.len());
        for (i, e) in entries.iter().enumerate() {
            if self.deleted(&e.body)? {
                dirty = true;
                continue;
            }
            let (comp, plain, d) = match e.method {
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
            };
            dirty |= d;
            built.push(Built { name: e.name.clone(), comp, plain, dirty: d, method: e.method, src: Some(i) });
        }
        for rel in self.additions.remove(&dir.to_lowercase()).unwrap_or_default() {
            let plain = std::fs::read(self.dir.join("tree").join(&rel))?;
            let name = rel[dir.len()..].to_string();
            self.edited.push(format!("{rel} (added)"));
            built.push(Built { name, comp: deflate_fresh(&plain)?, plain, dirty: true, method: 8, src: None });
            dirty = true;
        }

        // Huidu .zbin: fileInfo.xml lists every .bin with its size; keep it truthful.
        if dirty {
            if let Some(fi) = built.iter().position(|b| b.name == "fileInfo.xml") {
                let mut xml = String::from_utf8(built[fi].plain.clone()).context("fileInfo.xml is not UTF-8")?;
                for b in &built {
                    let key = format!("name=\"{}\" size=\"", b.name);
                    if let Some(p) = xml.find(&key) {
                        let vs = p + key.len();
                        let ve = vs + xml[vs..].find('"').ok_or_else(|| anyhow!("bad fileInfo.xml"))?;
                        xml.replace_range(vs..ve, &b.plain.len().to_string());
                    }
                }
                if xml.as_bytes() != built[fi].plain {
                    let b = &mut built[fi];
                    b.plain = xml.into_bytes();
                    b.comp = if b.method == 8 { deflate_fresh(&b.plain)? } else { b.plain.clone() };
                    b.dirty = true;
                    eprintln!("  updated fileInfo.xml sizes");
                }
            }
        }

        // Phase 2: lay out local headers + data, then the central directory with fixed-up fields.
        let tmpl_local = self.blob(&entries[0].local)?;
        let tmpl_cd = self.blob(&cd[entries[0].cd_index])?;
        let mut cd_out: Vec<Option<Vec<u8>>> = vec![None; cd.len()];
        let mut new_cd: Vec<Vec<u8>> = vec![];
        let mut apk_signed = false;
        for b in &built {
            let crc = crc32(&b.plain) as usize;
            let Some(i) = b.src else {
                // New entry: sizes in the local header (no data descriptor), times/attrs from entry 0.
                let off = out.len() - start;
                let nb = b.name.as_bytes();
                let flags: u16 = if b.name.is_ascii() { 0 } else { 0x800 };
                let sizes: Vec<u8> = [crc, b.comp.len(), b.plain.len()].iter().flat_map(|&v| (v as u32).to_le_bytes()).collect();
                let mut l = b"PK\x03\x04".to_vec();
                l.extend(20u16.to_le_bytes());
                l.extend(flags.to_le_bytes());
                l.extend(8u16.to_le_bytes());
                l.extend(&tmpl_local[10..14]);
                l.extend(&sizes);
                l.extend((nb.len() as u16).to_le_bytes());
                l.extend(0u16.to_le_bytes());
                l.extend(nb);
                out.extend(&l);
                out.extend(&b.comp);
                let mut c = b"PK\x01\x02".to_vec();
                c.extend(&tmpl_cd[4..6]); // version made by
                c.extend(20u16.to_le_bytes());
                c.extend(flags.to_le_bytes());
                c.extend(8u16.to_le_bytes());
                c.extend(&tmpl_cd[12..16]);
                c.extend(&sizes);
                c.extend((nb.len() as u16).to_le_bytes());
                c.extend([0u8; 8]); // extra len, comment len, disk, internal attrs
                c.extend(&tmpl_cd[38..42]); // external attrs
                c.extend((off as u32).to_le_bytes());
                c.extend(nb);
                new_cd.push(c);
                continue;
            };
            let e = &entries[i];
            if let Some(bf) = &e.before {
                dirty |= self.eval(bf, out)?;
            }
            let off = out.len() - start;
            let mut local = self.blob(&e.local)?;
            let mut after = self.blob(&e.after)?;
            apk_signed |= after.windows(16).any(|w| w == b"APK Sig Block 42");
            if b.dirty {
                if u16le(&local, 6)? & 8 == 0 {
                    put32(&mut local, 14, crc)?;
                    put32(&mut local, 18, b.comp.len())?;
                    put32(&mut local, 22, b.plain.len())?;
                } else {
                    let d = if after.starts_with(b"PK\x07\x08") { 4 } else { 0 };
                    ensure!(after.len() >= d + 12, "{}: data descriptor missing", e.name);
                    put32(&mut after, d, crc)?;
                    put32(&mut after, d + 4, b.comp.len())?;
                    put32(&mut after, d + 8, b.plain.len())?;
                }
            }
            out.extend(local);
            out.extend(&b.comp);
            out.extend(after);
            let mut r = self.blob(&cd[e.cd_index])?;
            if b.dirty {
                put32(&mut r, 16, crc)?;
                put32(&mut r, 20, b.comp.len())?;
                put32(&mut r, 24, b.plain.len())?;
            }
            if dirty {
                put32(&mut r, 42, off)?;
            }
            cd_out[e.cd_index] = Some(r);
        }
        if dirty && apk_signed {
            self.warnings.push(format!("tree/{dir}: APK contents changed, its v2 signature is now stale; re-sign it before installing"));
        }
        let cd_start = out.len() - start;
        let mut count = 0usize;
        for r in cd_out.into_iter().flatten().chain(new_cd) {
            out.extend(r);
            count += 1;
        }
        let cd_size = out.len() - start - cd_start;
        let mut t = self.blob(tail)?;
        if dirty {
            ensure!(count <= 0xFFFF, "too many zip entries without zip64");
            t[eocd_at + 8..eocd_at + 10].copy_from_slice(&(count as u16).to_le_bytes());
            t[eocd_at + 10..eocd_at + 12].copy_from_slice(&(count as u16).to_le_bytes());
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

/// Rebuild the image; returns (bytes, list of edited/added/removed files).
fn rebuild(dir: &Path, strict: bool) -> Result<(Vec<u8>, Vec<String>)> {
    let m = load_manifest(dir)?;
    let changes = scan_tree(dir, &m.root)?;
    if strict {
        let n: usize = changes.added.values().map(Vec::len).sum();
        ensure!(n == 0, "{n} file(s) added under tree/ (strict mode)");
    }
    let mut l = Loader { dir, strict, additions: changes.added, edited: vec![], warnings: vec![] };
    let mut out = Vec::with_capacity(m.size);
    let dirty = l.eval(&m.root, &mut out)?;
    ensure!(l.additions.is_empty(), "internal: unplaced additions under {:?}", l.additions.keys());
    if !dirty {
        ensure!(out.len() == m.size && sha_hex(&out) == m.sha256, "final image sha256 mismatch");
    }
    for w in &l.warnings {
        eprintln!("  warning: {w}");
    }
    Ok((out, l.edited))
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
            let dir = Path::new(&args[2]);
            let ch = scan_tree(dir, &load_manifest(dir)?.root)?;
            let added: Vec<&String> = ch.added.values().flatten().collect();
            if ch.modified.is_empty() && ch.removed.is_empty() && added.is_empty() {
                eprintln!("clean: tree/ matches the original");
            }
            for c in &ch.modified {
                println!("modified: tree/{c}");
            }
            for c in &ch.removed {
                println!("removed:  tree/{c}");
            }
            for c in added {
                println!("added:    tree/{c}");
            }
            Ok(())
        }
        _ => bail!("{usage}"),
    }
}
