//! # boxupgrade-model
//!
//! A faithful Rust port of the **semantic core** of Huidu's `libBoxUpgrade.so`
//! (`HUpgrade` / `HSystemEnv`). This crate reproduces the *decision* and *format*
//! logic only — the firmware-package format, MD5 verification, version packing,
//! and the run-vs-skip gate. It deliberately does **not** port the Qt / TCP /
//! QIODevice / socket plumbing (our own client already speaks the wire protocol).
//!
//! Everything here is grounded in the unstripped Ghidra decompilation of the
//! RK3288 build of `libBoxUpgrade.so`. Function names cited in the doc comments
//! refer to that decompilation.
//!
//! ## Sources of truth (decompiled functions)
//! * `HUpgrade::GetUpgradeInfo(HUpdatePacket*, QString const&)` — parses a
//!   packaged `.bin` from disk, verifies the MD5, extracts the XML fields.
//! * `HUpgrade::ParseFirewireFile(char const*, char const*)` — the streaming
//!   variant (reads magic=8, md5=16, xml_len=4, then payload) with identical
//!   verification and the same XML field set.
//! * `HUpgrade::RemoveFirewareFileHead(QString const&, QString const&, int)` —
//!   seeks `offset` bytes into the source and copies the remainder to a new
//!   file; `offset == 28 + xml_len`, i.e. it strips the header + XML, leaving the
//!   raw gzip payload.
//! * `HUpgrade::CheckUpgradeVersion(uint, uint)` — the version comparison.
//! * `HUpgrade::UpgradeFireware(QString const&)` — the run-vs-skip decision
//!   (calls GetUpgradeInfo, getMainVersion, getLimitVersion, CheckUpgradeVersion,
//!   then RemoveFirewareFileHead + ProcessUpgrade).
//! * `HUpgrade::ProcessUpgrade(...)` — builds the shell commands
//!   (Decompress `tar zxvf %s -C %s` + Script) and runs them via `system()`.
//! * `HSystemEnv::InitVersionInfo()` — reads the on-box version files and parses
//!   each with `inet_addr()` (dotted-quad!).
//! * `HSystemEnv::{getMainVersion,setMainVersion,getLimitVersion,setLimitVersion}`.

/// HDPLAYER package magic, 8 bytes at file offset 0x00.
///
/// From `GetUpgradeInfo`: after `read(header, 0x1c)` the first 8 bytes are turned
/// into a QString and compared with `QString::operator!=(param_1, "HDPLAYER")`.
pub const MAGIC: &[u8; 8] = b"HDPLAYER";

/// Offset of the 16-byte MD5 field (`GetUpgradeInfo` copies header[0x08..0x18]
/// into `HUpdatePacket+4`).
pub const MD5_OFFSET: usize = 0x08;

/// Offset of the little-endian u32 XML length (`local_4814[0]` in `GetUpgradeInfo`,
/// header[0x18..0x1c]).
pub const XML_LEN_OFFSET: usize = 0x18;

/// Size of the fixed header: 8 (magic) + 16 (md5) + 4 (xml_len) = 28 bytes.
/// `GetUpgradeInfo` reads exactly `0x1c` bytes here.
pub const HEADER_SIZE: usize = 0x1c;

/// The XML document begins immediately after the 28-byte header.
pub const XML_OFFSET: usize = HEADER_SIZE;

/// The byte offset at which the MD5 checksum range begins.
///
/// In `GetUpgradeInfo` the hash is fed, in order:
///   1. the 4-byte `xml_len` field (`addData(local_4814)` — bytes [0x18..0x1c]),
///   2. the XML bytes (`addData(xml)` — [0x1c .. 0x1c+xml_len]),
///   3. the remaining payload in 0x2400 chunks (`addData` in the read loop).
/// So the digest covers **`file[0x18 ..]`** — the xml_len field, the XML, and the
/// gzip payload. It does **not** cover the magic or the stored MD5 field itself.
pub const MD5_RANGE_START: usize = 0x18;

/// Errors from parsing/verifying a firmware package.
///
/// These mirror the failure branches of `GetUpgradeInfo` / `ParseFirewireFile`,
/// each of which returns 0 (failure) on the corresponding branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// Too short to hold the header (the `read(...) == 0x1c` / `== 8` guards).
    TooShort,
    /// First 8 bytes are not `"HDPLAYER"` (`operator!= "HDPLAYER"`).
    BadMagic,
    /// `xml_len` is 0 (`if (local_4814[0] != 0)` guard) or overruns the buffer.
    BadXmlLen,
    /// Computed MD5 over `file[0x18..]` != the stored 16-byte field
    /// (`memcmp(HUpdatePacket+4, hash, 0x10) == 0`).
    BadMd5,
    /// XML did not parse, root element was not `FirmwareInfo`, or a required
    /// field was missing / empty. In `ParseFirewireFile` the Script and
    /// DeviceType fields are each compared with `operator==("")` and rejected
    /// when empty.
    BadXml,
}

/// A parsed + verified HDPLAYER firmware package.
#[derive(Debug, Clone)]
pub struct FirmwarePackage {
    /// The 16-byte MD5 stored in the header (`HUpdatePacket+4`).
    pub md5: [u8; 16],
    /// Lowercase hex of the MD5 (`HUpdatePacket+0x14`, built digit-by-digit in
    /// `GetUpgradeInfo`; also the QSettings identity key in `UpgradeFireware`).
    pub md5_hex: String,
    /// Length of the embedded XML (header u32 @ 0x18).
    pub xml_len: u32,
    /// Byte offset at which the gzip payload starts (`= 28 + xml_len`,
    /// `HUpdatePacket+0x30`). This is exactly the `offset` fed to
    /// `RemoveFirewareFileHead`.
    pub payload_offset: usize,
    /// The parsed `<FirmwareInfo>` fields.
    pub info: FirmwareInfo,
}

/// The XML fields inside `<FirmwareInfo>` (offsets are into `HUpdatePacket`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FirmwareInfo {
    /// `<Version>` — `HUpdatePacket+0x1c`. Dotted quad, e.g. `7.11.18.0`.
    pub version: String,
    /// `<Decompress>` — `HUpdatePacket+0x20`, e.g.
    /// `"killall -1 BoxDaemon; tar zxvf %s -C %s "`.
    pub decompress: String,
    /// `<Script>` — `HUpdatePacket+0x24` (GetUpgradeInfo) / `this+0x28`
    /// (ParseFirewireFile), e.g. `"upgrade.sh"`. Must be non-empty.
    pub script: String,
    /// All `<Type>` elements, joined in the lib into one string at
    /// `HUpdatePacket+0x18` (the repeated-element loop with `nextSiblingElement`).
    /// e.g. `["FPGA", "BoxPlayer"]`. Used by `ProcessUpgrade` via `indexOf`.
    pub types: Vec<String>,
    /// `<DeviceType>` — a comma-separated list, `HUpdatePacket+0x28` /
    /// `this+0x3c`, e.g. `A3,C15,C35,...`. Must be non-empty.
    pub device_types: Vec<String>,
}

impl FirmwarePackage {
    /// Parse + fully verify a complete package (`GetUpgradeInfo` semantics,
    /// including the MD5 check over `file[0x18..]`).
    pub fn parse(bytes: &[u8]) -> Result<FirmwarePackage, ParseError> {
        let (pkg, _) = Self::parse_inner(bytes, true)?;
        Ok(pkg)
    }

    /// Parse only the header + XML, **without** the MD5 range check. Useful when
    /// you only have the first few hundred bytes (e.g. a header fixture) but not
    /// the multi-hundred-MB payload. Magic, xml_len and XML structure are still
    /// validated; the stored MD5 field is still returned.
    pub fn parse_header(bytes: &[u8]) -> Result<FirmwarePackage, ParseError> {
        let (pkg, _) = Self::parse_inner(bytes, false)?;
        Ok(pkg)
    }

    fn parse_inner(bytes: &[u8], verify_md5: bool) -> Result<(FirmwarePackage, ()), ParseError> {
        if bytes.len() < HEADER_SIZE {
            return Err(ParseError::TooShort);
        }
        // magic: header[0x00..0x08] != "HDPLAYER" -> reject
        if &bytes[0..8] != &MAGIC[..] {
            return Err(ParseError::BadMagic);
        }
        // md5 field: header[0x08..0x18] -> HUpdatePacket+4
        let mut md5 = [0u8; 16];
        md5.copy_from_slice(&bytes[MD5_OFFSET..MD5_OFFSET + 16]);
        // xml_len: little-endian u32 @ 0x18
        let xml_len = u32::from_le_bytes([
            bytes[XML_LEN_OFFSET],
            bytes[XML_LEN_OFFSET + 1],
            bytes[XML_LEN_OFFSET + 2],
            bytes[XML_LEN_OFFSET + 3],
        ]);
        if xml_len == 0 {
            return Err(ParseError::BadXmlLen);
        }
        let xml_end = XML_OFFSET
            .checked_add(xml_len as usize)
            .ok_or(ParseError::BadXmlLen)?;
        if xml_end > bytes.len() {
            return Err(ParseError::BadXmlLen);
        }
        let payload_offset = xml_end; // = 28 + xml_len == HUpdatePacket+0x30

        // md5_hex, built to lowercase hex (GetUpgradeInfo appends QString::number
        // of each byte; we normalize to canonical 2-digit lowercase hex).
        let md5_hex = md5.iter().map(|b| format!("{:02x}", b)).collect::<String>();

        if verify_md5 {
            // digest covers file[0x18..] : xml_len(4) + XML + payload
            let digest = md5::compute(&bytes[MD5_RANGE_START..]);
            if digest.0 != md5 {
                return Err(ParseError::BadMd5);
            }
        }

        let xml = std::str::from_utf8(&bytes[XML_OFFSET..xml_end]).map_err(|_| ParseError::BadXml)?;
        let info = FirmwareInfo::from_xml(xml)?;

        Ok((
            FirmwarePackage {
                md5,
                md5_hex,
                xml_len,
                payload_offset,
                info,
            },
            (),
        ))
    }
}

impl FirmwareInfo {
    /// Extract the `<FirmwareInfo>` fields from the XML text.
    ///
    /// Root element must be `FirmwareInfo` (`GetUpgradeInfo` compares the
    /// document element tagName; `ParseFirewireFile` does the same at 0x25f).
    /// Script and DeviceType must be non-empty (`operator==("")` -> reject).
    fn from_xml(xml: &str) -> Result<FirmwareInfo, ParseError> {
        if !xml.contains("<FirmwareInfo") {
            return Err(ParseError::BadXml);
        }
        let version = first_tag(xml, "Version").ok_or(ParseError::BadXml)?;
        let decompress = first_tag(xml, "Decompress").unwrap_or_default();
        let script = first_tag(xml, "Script").ok_or(ParseError::BadXml)?;
        if script.is_empty() {
            return Err(ParseError::BadXml);
        }
        let types = all_tags(xml, "Type");
        let dev = first_tag(xml, "DeviceType").ok_or(ParseError::BadXml)?;
        if dev.trim().is_empty() {
            return Err(ParseError::BadXml);
        }
        let device_types = dev
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        Ok(FirmwareInfo {
            version,
            decompress,
            script,
            types,
            device_types,
        })
    }

    /// True if `unit` (the box's device type, e.g. "C15") is a member of the
    /// `<DeviceType>` list. See [`upgrade_decision`] for where this is applied.
    pub fn device_type_matches(&self, unit: &str) -> bool {
        self.device_types.iter().any(|d| d.eq_ignore_ascii_case(unit))
    }
}

/// Extract the text of the first `<Tag>...</Tag>` (no attributes handled — the
/// real firmware XML uses bare tags).
fn first_tag(xml: &str, tag: &str) -> Option<String> {
    all_tags(xml, tag).into_iter().next()
}

/// Extract the text of every `<Tag>...</Tag>` occurrence.
fn all_tags(xml: &str, tag: &str) -> Vec<String> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(i) = rest.find(&open) {
        let after = &rest[i + open.len()..];
        if let Some(j) = after.find(&close) {
            out.push(after[..j].to_string());
            rest = &after[j + close.len()..];
        } else {
            break;
        }
    }
    out
}

/// A firmware version, packed the way the device compares them.
///
/// ## Packing (VERIFIED from the code)
/// The device never packs a dotted string with a hand-rolled `(a<<24)|...`. In
/// `HSystemEnv::InitVersionInfo` every version file is parsed with **`inet_addr()`**
/// — i.e. the running/limit versions are literally treated as IPv4 dotted-quads.
/// `inet_addr("a.b.c.d")` yields an `in_addr_t` in **network byte order**, so in
/// memory the bytes are `[a][b][c][d]` (a = most-significant octet on the wire).
/// The incoming XML `<Version>` is likewise fed through `inet_addr` in
/// `UpgradeFireware` (`inet_addr(local_a0)`), so both operands share this format.
///
/// `HUpgrade::CheckUpgradeVersion(uint p1, uint p2)` then byte-swaps each operand:
/// ```text
/// return  (p2<<24 | (p2>>8 &0xff)<<16 | (p2>>16 &0xff)<<8 | p2>>24)
///      <= (p1<<24 | (p1>>8 &0xff)<<16 | (p1>>16 &0xff)<<8 | p1>>24);
/// ```
/// The byte-swap converts the network-order bytes `[a][b][c][d]` into the host
/// integer `(a<<24)|(b<<16)|(c<<8)|d`, so the comparison is an ordinary unsigned
/// compare with **a** most-significant and **d** least-significant. The 4th octet
/// participates (as the lowest-priority byte).
///
/// We therefore store the **natural** value `(a<<24)|(b<<16)|(c<<8)|d` and get a
/// correct `Ord` for free.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version(pub u32);

impl Version {
    /// Build from four octets: `(a<<24)|(b<<16)|(c<<8)|d`.
    pub const fn new(a: u8, b: u8, c: u8, d: u8) -> Version {
        Version(((a as u32) << 24) | ((b as u32) << 16) | ((c as u32) << 8) | (d as u32))
    }

    /// Parse a dotted version string `"a.b.c.d"` (missing trailing octets => 0),
    /// exactly matching `inet_addr` semantics for the common 4-part form.
    pub fn parse(s: &str) -> Option<Version> {
        let mut oct = [0u8; 4];
        let parts: Vec<&str> = s.trim().split('.').collect();
        if parts.is_empty() || parts.len() > 4 {
            return None;
        }
        for (i, p) in parts.iter().enumerate() {
            oct[i] = p.parse::<u8>().ok()?;
        }
        Some(Version::new(oct[0], oct[1], oct[2], oct[3]))
    }

    /// The `in_addr_t` value as `inet_addr` would return it and as the device
    /// stores it (network-order bytes `[a][b][c][d]`, read back as a host u32).
    /// This is the *raw stored* form on which the equality gate operates.
    pub fn to_inet_addr(self) -> u32 {
        self.0.swap_bytes()
    }

    /// Reconstruct a [`Version`] from a raw `inet_addr`/stored value.
    pub fn from_inet_addr(raw: u32) -> Version {
        Version(raw.swap_bytes())
    }

    /// The four octets, most-significant first.
    pub fn octets(self) -> (u8, u8, u8, u8) {
        (
            (self.0 >> 24) as u8,
            (self.0 >> 16) as u8,
            (self.0 >> 8) as u8,
            self.0 as u8,
        )
    }
}

/// Faithful port of `HUpgrade::CheckUpgradeVersion(incoming, limit)`.
///
/// Returns `true` (the lib's non-zero result, which lets the upgrade proceed)
/// **iff `incoming >= limit`** as unsigned natural versions. The decompiled body
/// byte-swaps both operands and returns `natural(limit) <= natural(incoming)`.
pub fn check_upgrade_version(incoming: Version, limit: Version) -> bool {
    // natural(limit) <= natural(incoming)   <=>   incoming >= limit
    limit.0 <= incoming.0
}

/// The outcome of the run-vs-skip decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpgradeDecision {
    /// Proceed: strip the header (`RemoveFirewareFileHead`) and run
    /// `ProcessUpgrade` (Decompress + Script).
    Run,
    /// Incoming version **equals** the running main version — the device logs and
    /// does nothing (`if (getMainVersion() == inet_addr(Version)) -> skip`).
    SkipNotNewer,
    /// Incoming version is **below** the limit version — rejected by
    /// `CheckUpgradeVersion(incoming, limit) == 0`.
    RejectBelowLimit,
    /// The box's device type is not present in `<DeviceType>`.
    ///
    /// NOTE: `libBoxUpgrade` itself only checks that `<DeviceType>` is *non-empty*;
    /// the actual membership test against the running unit is performed downstream
    /// (the packaged `upgrade.sh` / higher layers). We model it here because a
    /// mismatched unit is, in practice, a hard no-go for a real upgrade.
    RejectBadDeviceType,
    /// The package failed structural/MD5 verification (surfaced from
    /// [`FirmwarePackage::parse`]); included for completeness.
    RejectBadPackage,
}

/// Faithful port of the run-vs-skip core of `HUpgrade::UpgradeFireware`.
///
/// Assumes the package already parsed & MD5-verified (that is `GetUpgradeInfo`
/// returning non-zero). The decompiled ordering is:
///
/// 1. `GetUpgradeInfo` OK (else reject — [`RejectBadPackage`]).
/// 2. QSettings identity guard: skip if the stored md5-hex equals this package's
///    (modeled by the caller — a *different* image always passes).
/// 3. `if (getMainVersion() == inet_addr(Version))` -> **skip** ([`SkipNotNewer`]).
///    This is an **equality** test, not `<` — see BOTTOM LINE.
/// 4. `if (CheckUpgradeVersion(inet_addr(Version), getLimitVersion()) == 0)` ->
///    **reject** ([`RejectBelowLimit`]) (i.e. incoming < limit).
/// 5. otherwise -> `RemoveFirewareFileHead` + `ProcessUpgrade` -> **[`Run`]**.
///
/// Note there is **no** `incoming > running` check: an incoming version that is
/// merely *different* from the running one (even older) will run, provided it is
/// `>= limit`.
///
/// [`RejectBadPackage`]: UpgradeDecision::RejectBadPackage
/// [`SkipNotNewer`]: UpgradeDecision::SkipNotNewer
/// [`RejectBelowLimit`]: UpgradeDecision::RejectBelowLimit
/// [`Run`]: UpgradeDecision::Run
pub fn upgrade_decision(
    incoming: &FirmwareInfo,
    running_version: Version,
    limit_version: Version,
    device_type: &str,
) -> UpgradeDecision {
    let inc = match Version::parse(&incoming.version) {
        Some(v) => v,
        None => return UpgradeDecision::RejectBadPackage,
    };

    // (downstream) device-type membership — see RejectBadDeviceType docs.
    if !incoming.device_type_matches(device_type) {
        return UpgradeDecision::RejectBadDeviceType;
    }

    // 3. equality gate: getMainVersion() == inet_addr(Version)
    //    (raw stored values; equality is byte-order independent).
    if inc == running_version {
        return UpgradeDecision::SkipNotNewer;
    }

    // 4. limit gate: CheckUpgradeVersion(incoming, limit) must be true.
    if !check_upgrade_version(inc, limit_version) {
        return UpgradeDecision::RejectBelowLimit;
    }

    // 5. run.
    UpgradeDecision::Run
}

#[cfg(test)]
mod tests {
    use super::*;

    const REAL_MD5_HEX: &str = "efecab6cbd765b50da3670626582344e";

    // ---- Real header fixture (first 700 bytes of BoxPlayer_7_11_18_0.bin) ----

    #[test]
    fn parses_real_header() {
        let bytes = include_bytes!("../tests/header_7_11_18_0.bin");
        let pkg = FirmwarePackage::parse_header(bytes).expect("header parses");
        assert_eq!(&bytes[0..8], b"HDPLAYER");
        assert_eq!(pkg.xml_len, 650, "xml_len @0x18");
        assert_eq!(pkg.md5_hex, REAL_MD5_HEX, "stored md5 field");
        assert_eq!(pkg.payload_offset, 28 + 650); // 678
        assert_eq!(pkg.info.version, "7.11.18.0");
        assert_eq!(pkg.info.script, "upgrade.sh");
        assert_eq!(
            pkg.info.decompress,
            "killall -1 BoxDaemon; tar zxvf %s -C %s "
        );
        assert_eq!(pkg.info.types, vec!["FPGA", "BoxPlayer"]);
        // DeviceType membership includes our real units:
        assert!(pkg.info.device_type_matches("C15"));
        assert!(pkg.info.device_type_matches("D15"));
        assert!(pkg.info.device_type_matches("c15")); // case-insensitive
        assert!(!pkg.info.device_type_matches("Z99"));
        // version parses to 7.11.18.0
        assert_eq!(Version::parse(&pkg.info.version).unwrap(), Version::new(7, 11, 18, 0));
    }

    #[test]
    fn rejects_bad_magic() {
        let mut b = include_bytes!("../tests/header_7_11_18_0.bin").to_vec();
        b[0] = b'X';
        assert_eq!(FirmwarePackage::parse_header(&b).unwrap_err(), ParseError::BadMagic);
    }

    // ---- MD5 range proof: synthesize a full package and verify md5(file[0x18..]) ----

    #[test]
    fn md5_covers_from_0x18_to_eof() {
        let xml = b"<?xml version=\"1.0\" encoding=\"UTF-8\"?><FirmwareInfo>\
            <Version>1.2.3.4</Version><Decompress>tar zxvf %s -C %s </Decompress>\
            <Script>upgrade.sh</Script><Type>FPGA</Type>\
            <DeviceType>C15,D15</DeviceType></FirmwareInfo>";
        let payload = b"\x1f\x8b\x08\x00 fake gzip payload bytes \x00\x01\x02\x03";
        let xml_len = xml.len() as u32;

        // Assemble: magic + [md5 placeholder] + xml_len + xml + payload
        let mut body = Vec::new();
        body.extend_from_slice(&xml_len.to_le_bytes()); // starts at 0x18
        body.extend_from_slice(xml);
        body.extend_from_slice(payload);
        let digest = md5::compute(&body); // md5(file[0x18..])

        let mut file = Vec::new();
        file.extend_from_slice(MAGIC);
        file.extend_from_slice(&digest.0); // stored md5 field @0x08
        file.extend_from_slice(&body); // xml_len@0x18, xml, payload
        assert_eq!(&file[0x18..], &body[..]);

        let pkg = FirmwarePackage::parse(&file).expect("valid package");
        assert_eq!(pkg.xml_len, xml_len);
        assert_eq!(pkg.info.version, "1.2.3.4");
        assert_eq!(pkg.payload_offset, 0x1c + xml_len as usize);

        // Corrupt one payload byte -> BadMd5.
        let mut bad = file.clone();
        *bad.last_mut().unwrap() ^= 0xff;
        assert_eq!(FirmwarePackage::parse(&bad).unwrap_err(), ParseError::BadMd5);

        // Corrupt the xml_len field (inside the md5 range) -> BadMd5 (or BadXmlLen
        // if it changes structure); flipping a high bit keeps length valid-ish but
        // breaks the digest. Flip a byte of the XML instead to stay well-defined:
        let mut bad2 = file.clone();
        bad2[0x1c + 60] ^= 0x01; // somewhere inside the XML text
        assert_eq!(FirmwarePackage::parse(&bad2).unwrap_err(), ParseError::BadMd5);
    }

    // ---- Version packing truth table: does the 4th octet matter? ----

    #[test]
    fn version_packing_and_fourth_octet() {
        // natural packing (a<<24)|(b<<16)|(c<<8)|d
        assert_eq!(Version::new(7, 11, 18, 0).0, 0x070B1200);
        assert_eq!(Version::parse("7.11.18.0").unwrap().0, 0x070B1200);

        // inet_addr / stored form is byte-swapped: bytes [07][0B][12][00]
        assert_eq!(Version::new(7, 11, 18, 0).to_inet_addr(), 0x00120B07);
        assert_eq!(Version::from_inet_addr(0x00120B07), Version::new(7, 11, 18, 0));

        // The 4th octet DOES participate (lowest priority):
        assert!(Version::parse("7.11.18.1").unwrap() > Version::parse("7.11.18.0").unwrap());
        assert!(Version::parse("7.11.18.99").unwrap() > Version::parse("7.11.18.0").unwrap());

        // Ordering across octets:
        let v = |s: &str| Version::parse(s).unwrap();
        assert!(v("7.11.18.0") < v("7.11.18.99"));
        assert!(v("7.11.18.99") < v("7.11.20.0"));
        assert!(v("7.11.20.0") < v("7.12.0.0"));
        assert!(v("7.11.17.255") < v("7.11.18.0")); // 3rd octet dominates 4th
        assert!(v("7.11.18.0") != v("7.11.18.99")); // distinct => not "skip"
    }

    #[test]
    fn check_upgrade_version_is_incoming_ge_limit() {
        let v = |s: &str| Version::parse(s).unwrap();
        // incoming >= limit -> true (proceed)
        assert!(check_upgrade_version(v("7.11.18.0"), v("7.11.18.0")));
        assert!(check_upgrade_version(v("7.11.18.99"), v("7.11.18.0")));
        assert!(check_upgrade_version(v("7.12.0.0"), v("7.11.18.0")));
        // incoming < limit -> false (reject)
        assert!(!check_upgrade_version(v("7.11.17.0"), v("7.11.18.0")));
        assert!(!check_upgrade_version(v("6.0.0.0"), v("7.11.18.0")));
    }

    // ---- The run-vs-skip decision ----

    fn info(version: &str) -> FirmwareInfo {
        FirmwareInfo {
            version: version.to_string(),
            decompress: "tar zxvf %s -C %s ".to_string(),
            script: "upgrade.sh".to_string(),
            types: vec!["FPGA".to_string(), "BoxPlayer".to_string()],
            device_types: vec!["C15".to_string(), "D15".to_string()],
        }
    }

    #[test]
    fn decision_truth_table() {
        let running = Version::new(7, 11, 18, 0);
        let limit = Version::new(7, 0, 0, 0); // a low limit, as on-box
        let dt = "C15";

        // incoming == running -> SkipNotNewer (the equality no-op)
        assert_eq!(
            upgrade_decision(&info("7.11.18.0"), running, limit, dt),
            UpgradeDecision::SkipNotNewer
        );
        // bump the 4th octet only -> distinct -> RUN
        assert_eq!(
            upgrade_decision(&info("7.11.18.99"), running, limit, dt),
            UpgradeDecision::Run
        );
        // bump 3rd octet -> RUN
        assert_eq!(
            upgrade_decision(&info("7.11.20.0"), running, limit, dt),
            UpgradeDecision::Run
        );
        // bump 2nd octet -> RUN
        assert_eq!(
            upgrade_decision(&info("7.12.0.0"), running, limit, dt),
            UpgradeDecision::Run
        );
    }

    #[test]
    fn decision_below_limit_is_rejected() {
        let running = Version::new(7, 11, 18, 0);
        let limit = Version::new(7, 11, 18, 50);
        // incoming (7.11.18.10) < limit (7.11.18.50) and != running -> reject
        assert_eq!(
            upgrade_decision(&info("7.11.18.10"), running, limit, "C15"),
            UpgradeDecision::RejectBelowLimit
        );
    }

    #[test]
    fn decision_older_but_different_still_runs_if_above_limit() {
        // Demonstrates there is NO "strictly newer than running" rule:
        // 7.11.17.0 is OLDER than running 7.11.18.0, but >= limit and != running.
        let running = Version::new(7, 11, 18, 0);
        let limit = Version::new(7, 0, 0, 0);
        assert_eq!(
            upgrade_decision(&info("7.11.17.0"), running, limit, "C15"),
            UpgradeDecision::Run
        );
    }

    #[test]
    fn decision_bad_device_type() {
        let running = Version::new(7, 11, 18, 0);
        let limit = Version::new(7, 0, 0, 0);
        assert_eq!(
            upgrade_decision(&info("7.11.20.0"), running, limit, "Z99"),
            UpgradeDecision::RejectBadDeviceType
        );
    }
}
