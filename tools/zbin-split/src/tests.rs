//! End-to-end tests on a synthetic .zbin with the same layering as the vendor image:
//! ZIP { HDPLAYER { tar.gz { Plat.tar.gz, upgrade.sh } }, MAGICPLAYER { ZIP }, fileInfo.xml }.
//! Results are checked with independent readers (the `zip` and `tar` crates, flate2, md-5).

use super::*;
use std::io::Read;

fn gz(data: &[u8], level: u32) -> Vec<u8> {
    let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::new(level));
    e.write_all(data).unwrap();
    e.finish().unwrap()
}

fn gunzip(data: &[u8]) -> Vec<u8> {
    let mut v = Vec::new();
    flate2::read::GzDecoder::new(data).read_to_end(&mut v).unwrap();
    v
}

fn tarball(files: &[(&str, &[u8], u32)]) -> Vec<u8> {
    let mut b = tar::Builder::new(Vec::new());
    for (name, data, mode) in files {
        let mut h = tar::Header::new_gnu();
        h.set_size(data.len() as u64);
        h.set_mode(*mode);
        h.set_mtime(1_700_000_000);
        h.set_uid(1011);
        h.set_gid(1011);
        h.set_username("hdplayer").unwrap();
        h.set_groupname("hdplayer").unwrap();
        b.append_data(&mut h, name, *data).unwrap();
    }
    b.into_inner().unwrap()
}

/// name -> (mode, contents)
fn untar(data: &[u8]) -> BTreeMap<String, (u32, Vec<u8>)> {
    let mut a = tar::Archive::new(data);
    let mut m = BTreeMap::new();
    for e in a.entries().unwrap() {
        let mut e = e.unwrap();
        let name = e.path().unwrap().to_string_lossy().into_owned();
        let mode = e.header().mode().unwrap();
        let mut v = Vec::new();
        e.read_to_end(&mut v).unwrap();
        m.insert(name, (mode, v));
    }
    m
}

fn zipfile(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let o = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (name, data) in files {
        w.start_file(*name, o).unwrap();
        w.write_all(data).unwrap();
    }
    w.finish().unwrap().into_inner()
}

/// name -> contents; reading every entry to the end makes the zip crate verify each CRC32.
fn unzip(data: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let mut a = zip::ZipArchive::new(Cursor::new(data)).unwrap();
    let mut m = BTreeMap::new();
    for i in 0..a.len() {
        let mut f = a.by_index(i).unwrap();
        let mut v = Vec::new();
        f.read_to_end(&mut v).unwrap();
        m.insert(f.name().to_string(), v);
    }
    m
}

fn vendor(magic: &str, xml: &str, payload: &[u8]) -> Vec<u8> {
    let mut len_xml = (xml.len() as u32).to_le_bytes().to_vec();
    len_xml.extend(xml.as_bytes());
    let mut v = magic.as_bytes().to_vec();
    v.extend(vendor_md5(magic, &len_xml, payload));
    v.extend(len_xml);
    v.extend(payload);
    v
}

/// (magic, xml, payload) of a vendor .bin, after checking its MD5 the way the device does.
fn open_vendor(magic: &str, d: &[u8]) -> (String, Vec<u8>) {
    assert!(d.starts_with(magic.as_bytes()));
    let lo = magic.len() + 16;
    let xl = u32le(d, lo).unwrap();
    let payload = &d[lo + 4 + xl..];
    let want: [u8; 16] = if magic == "HDPLAYER" { Md5::digest(&d[lo..]).into() } else { Md5::digest(payload).into() };
    assert_eq!(&d[magic.len()..lo], &want, "{magic} MD5");
    (String::from_utf8(d[lo + 4..lo + 4 + xl].to_vec()).unwrap(), payload.to_vec())
}

fn sample_text(n: usize) -> Vec<u8> {
    (0..n).map(|i| format!("line {i}: the quick brown fox {}\n", i % 97)).collect::<String>().into_bytes()
}

fn make_zbin() -> Vec<u8> {
    let so: Vec<u8> = (0..5000u32).map(|i| (i.wrapping_mul(2654435761) >> 13) as u8).collect();
    let text = sample_text(4000);
    let plat = gz(&tarball(&[("run.sh", b"#!/bin/sh\necho run\n", 0o755), ("lib/a.so", &so, 0o644), ("data/big.txt", &text, 0o644)]), 6);
    let payload = gz(&tarball(&[("Plat.tar.gz", &plat, 0o664), ("upgrade.sh", b"tar xf Plat.tar.gz\n./run.sh\n", 0o755)]), 9);
    let box_bin = vendor("HDPLAYER", "<?xml version=\"1.0\"?><FirmwareInfo><Version>1.2.3.4</Version></FirmwareInfo>", &payload);
    let magic_bin = vendor(
        "MAGICPLAYER",
        "<FirmwareInfo><Type>MagicPlayer</Type></FirmwareInfo>",
        &zipfile(&[("upgrade.sh", b"echo magic\n"), ("app.sh", b"echo app\n"), ("blob.dat", &text)]),
    );
    let info = format!(
        "<firmwareInfo>\n    <file name=\"Box.bin\" size=\"{}\"></file>\n    <file name=\"Magic.bin\" size=\"{}\"></file>\n</firmwareInfo>",
        box_bin.len(),
        magic_bin.len()
    );
    zipfile(&[("Box.bin", &box_bin), ("Magic.bin", &magic_bin), ("fileInfo.xml", info.as_bytes())])
}

struct Fixture {
    _tmp: tempfile::TempDir,
    zbin: Vec<u8>,
    dir: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let zbin = make_zbin();
        let src = tmp.path().join("test.zbin");
        std::fs::write(&src, &zbin).unwrap();
        let dir = tmp.path().join("unpacked");
        unpack(&src, &dir).unwrap(); // includes a strict byte-identical rebuild check
        Fixture { _tmp: tmp, zbin, dir }
    }
    fn tree(&self, rel: &str) -> PathBuf {
        self.dir.join("tree").join(rel)
    }
}

const PLAT: &str = "Box.bin.d/payload.d/Plat.tar.d/";
const MAGIC: &str = "Magic.bin.d/payload.d/";

#[test]
fn unpack_explodes_every_layer_and_rebuilds_exactly() {
    let f = Fixture::new();
    for rel in [
        "Box.bin.d/_header.xml",
        "Box.bin.d/payload.d/upgrade.sh",
        &format!("{PLAT}run.sh"),
        &format!("{PLAT}lib/a.so"),
        &format!("{PLAT}data/big.txt"),
        "Magic.bin.d/_header.xml",
        &format!("{MAGIC}upgrade.sh"),
        "fileInfo.xml",
    ] {
        assert!(f.tree(rel).is_file(), "missing tree/{rel}");
    }
    let (img, edited) = rebuild(&f.dir, true).unwrap();
    assert!(edited.is_empty());
    assert!(img == f.zbin, "strict rebuild differs");
    let ch = scan_tree(&f.dir, &load_manifest(&f.dir).unwrap().root).unwrap();
    assert!(ch.modified.is_empty() && ch.removed.is_empty() && ch.added.is_empty());
}

#[test]
fn edit_add_remove_produces_valid_image() {
    let f = Fixture::new();
    // modify, remove, add (incl. a >100-byte name needing a GNU long-name record), junk ignored
    std::fs::write(f.tree(&format!("{PLAT}run.sh")), b"#!/bin/sh\necho patched run\n").unwrap();
    std::fs::remove_file(f.tree(&format!("{PLAT}lib/a.so"))).unwrap();
    let long = format!("custom/{}/file.txt", "d".repeat(120));
    std::fs::create_dir_all(f.tree(&format!("{PLAT}custom/{}", "d".repeat(120)))).unwrap();
    std::fs::write(f.tree(&format!("{PLAT}{long}")), b"long name\n").unwrap();
    std::fs::write(f.tree(&format!("{PLAT}custom/hello.sh")), b"#!/bin/sh\necho hi\n").unwrap();
    std::fs::write(f.tree(&format!("{PLAT}custom/Thumbs.db")), b"junk").unwrap();
    std::fs::write(f.tree(&format!("{MAGIC}upgrade.sh")), b"echo magic patched\n").unwrap();
    std::fs::remove_file(f.tree(&format!("{MAGIC}app.sh"))).unwrap();
    std::fs::write(f.tree(&format!("{MAGIC}extra.txt")), b"new entry\n").unwrap();
    let hdr = f.tree("Box.bin.d/_header.xml");
    let xml = std::fs::read_to_string(&hdr).unwrap().replace("1.2.3.4", "1.2.3.99-custom");
    std::fs::write(&hdr, &xml).unwrap();

    let ch = scan_tree(&f.dir, &load_manifest(&f.dir).unwrap().root).unwrap();
    assert_eq!(ch.modified.len(), 3);
    assert_eq!(ch.removed.len(), 2);
    assert_eq!(ch.added.values().map(Vec::len).sum::<usize>(), 3);
    assert!(rebuild(&f.dir, true).is_err(), "strict mode must refuse edits");

    let (img, edited) = rebuild(&f.dir, false).unwrap();
    assert_eq!(edited.len(), 8);

    let outer = unzip(&img);
    for (name, size) in [("Box.bin", outer["Box.bin"].len()), ("Magic.bin", outer["Magic.bin"].len())] {
        let info = String::from_utf8(outer["fileInfo.xml"].clone()).unwrap();
        assert!(info.contains(&format!("name=\"{name}\" size=\"{size}\"")), "fileInfo.xml size for {name}");
    }
    let (bxml, payload) = open_vendor("HDPLAYER", &outer["Box.bin"]);
    assert!(bxml.contains("1.2.3.99-custom"));
    let top = untar(&gunzip(&payload));
    let plat = untar(&gunzip(&top["Plat.tar.gz"].1));
    assert_eq!(plat["run.sh"], (0o755, b"#!/bin/sh\necho patched run\n".to_vec()));
    assert!(!plat.contains_key("lib/a.so"));
    assert_eq!(plat["data/big.txt"].1, sample_text(4000));
    assert_eq!(plat["custom/hello.sh"], (0o755, b"#!/bin/sh\necho hi\n".to_vec()));
    assert_eq!(plat[&long], (0o644, b"long name\n".to_vec()));
    assert!(!plat.keys().any(|k| k.contains("Thumbs")));
    let (_, mpay) = open_vendor("MAGICPLAYER", &outer["Magic.bin"]);
    let m = unzip(&mpay);
    assert_eq!(m.keys().cloned().collect::<Vec<_>>(), ["blob.dat", "extra.txt", "upgrade.sh"]);
    assert_eq!(m["upgrade.sh"], b"echo magic patched\n");
    assert_eq!(m["extra.txt"], b"new entry\n");
}

#[test]
fn reverting_edits_restores_the_exact_image() {
    let f = Fixture::new();
    let p = f.tree(&format!("{PLAT}run.sh"));
    let orig = std::fs::read(&p).unwrap();
    std::fs::write(&p, b"changed").unwrap();
    assert!(rebuild(&f.dir, false).unwrap().0 != f.zbin);
    std::fs::write(&p, orig).unwrap();
    assert!(rebuild(&f.dir, true).unwrap().0 == f.zbin);
}

#[test]
fn big_leaves_are_split_and_rejoined() {
    // one leaf just over PART_SIZE, inside a stored zip entry
    let big: Vec<u8> = (0..PART_SIZE + 1234).map(|i| (i * 7 % 251) as u8).collect();
    let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let o = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    w.start_file("big.dat", o).unwrap();
    w.write_all(&big).unwrap();
    let zbin = w.finish().unwrap().into_inner();
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("b.zbin"), &zbin).unwrap();
    let dir = tmp.path().join("u");
    unpack(&tmp.path().join("b.zbin"), &dir).unwrap();
    assert!(dir.join("tree/big.dat.part000").is_file() && dir.join("tree/big.dat.part001").is_file());
    assert!(!dir.join("tree/big.dat").exists());
    assert!(std::fs::metadata(dir.join("tree/big.dat.part000")).unwrap().len() as usize == PART_SIZE);
    assert!(rebuild(&dir, true).unwrap().0 == zbin);
}

#[test]
fn rawdeflate_round_trips_every_zlib_level() {
    let mut data = sample_text(3000);
    data.extend((0..20000u32).map(|i| (i.wrapping_mul(2654435761) >> 7) as u8));
    for level in 0..=9 {
        let mut e = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::new(level));
        e.write_all(&data).unwrap();
        let comp = e.finish().unwrap();
        let (script, plain, used) = rawdeflate::analyze(&comp).unwrap();
        assert_eq!(plain, data);
        assert_eq!(used, comp.len());
        assert_eq!(rawdeflate::rebuild(&script, &plain).unwrap(), comp, "level {level}");
    }
}

#[test]
fn sanitize_makes_windows_safe_names() {
    assert_eq!(sanitize("a:b?.txt"), "a_b_.txt");
    assert_eq!(sanitize("CON.txt"), "_CON.txt");
    assert_eq!(sanitize(".gitignore"), "_.gitignore");
    assert_eq!(sanitize("trail."), "trail_");
    assert_eq!(sanitize(".."), "_up");
}
