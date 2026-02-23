//! Bundled firmware catalog parsed from FirmwareUpdate.xml.
//!
//! Maps card IDs to their available firmware versions.  Used by the
//! Firmware Update dialog to let the user select a specific version
//! for their card model.

const FIRMWARE_XML: &str = include_str!("../../hdsign-extracted/FirmwareUpdate.xml");

/// A single firmware version entry.
#[derive(Debug, Clone)]
pub struct FirmwareVersion {
    /// Version string, e.g. "HDV4.23"
    pub version: String,
    /// Bundled BIN filename, e.g. "HD_E62_1-23.bin"
    pub filename: String,
    /// Short display label, e.g. "HDV423"
    pub label: String,
}

/// All firmware versions for a particular card model.
#[derive(Debug, Clone)]
pub struct FirmwareEntry {
    /// Card ID matching `HardwareCard::id`
    pub id: u32,
    /// Friendly model name, e.g. "E62"
    pub name: String,
    /// Available versions, oldest → newest
    pub versions: Vec<FirmwareVersion>,
    /// IDs of related/alternate variants for the same physical model
    pub other_ids: Vec<u32>,
}

impl FirmwareEntry {
    /// The latest available version (last in list).
    pub fn latest(&self) -> Option<&FirmwareVersion> {
        self.versions.last()
    }
}

/// Parse FirmwareUpdate.xml and return all firmware entries.
pub fn load_firmware_catalog() -> Vec<FirmwareEntry> {
    parse_firmware(FIRMWARE_XML)
}

/// Find firmware entry for the given card ID (checks `id` and `other_ids`).
pub fn find_firmware(catalog: &[FirmwareEntry], card_id: u32) -> Option<&FirmwareEntry> {
    catalog.iter().find(|e| {
        e.id == card_id || e.other_ids.contains(&card_id)
    })
}

fn get_attr<'a>(xml: &'a str, attr: &str) -> Option<&'a str> {
    for q in &['"', '\''] {
        let needle = format!("{attr}={q}");
        if let Some(s) = xml.find(&needle) {
            let vs = s + needle.len();
            if let Some(e) = xml[vs..].find(*q) {
                return Some(&xml[vs..vs + e]);
            }
        }
        let needle2 = format!("{attr} ={q}");
        if let Some(s) = xml.find(&needle2) {
            let vs = s + needle2.len();
            if let Some(e) = xml[vs..].find(*q) {
                return Some(&xml[vs..vs + e]);
            }
        }
        let needle3 = format!("{attr} = {q}");
        if let Some(s) = xml.find(&needle3) {
            let vs = s + needle3.len();
            if let Some(e) = xml[vs..].find(*q) {
                return Some(&xml[vs..vs + e]);
            }
        }
    }
    None
}

#[allow(dead_code)]
fn get_tag_text<'a>(xml: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    let s = xml.find(&open)? + open.len();
    let e = xml[s..].find(&close)?;
    Some(&xml[s..s + e])
}

fn parse_firmware(xml: &str) -> Vec<FirmwareEntry> {
    let mut entries = Vec::new();
    let mut search = xml;

    while let Some(start) = search.find("<Item ") {
        let end = search[start..]
            .find("</Item>")
            .map(|e| start + e + 7)
            .unwrap_or(search.len());
        let chunk = &search[start..end];

        let id = get_attr(chunk, "id")
            .and_then(|v| v.trim().parse::<u32>().ok())
            .unwrap_or(0);
        let name = get_attr(chunk, "name").unwrap_or("").trim().to_string();
        let other_ids: Vec<u32> = get_attr(chunk, "otherID")
            .unwrap_or("")
            .split(',')
            .filter_map(|s| s.trim().parse::<u32>().ok())
            .collect();

        // Parse <version> children
        let mut versions = Vec::new();
        let mut vs = chunk;
        while let Some(ps) = vs.find("<version ") {
            let pe = vs[ps..].find("</version>").map(|e| ps + e + 10)
                .unwrap_or(vs.len());
            let vtag = &vs[ps..pe];
            let ver_num = get_attr(vtag, "num").unwrap_or("").trim().to_string();
            let filename = get_attr(vtag, "FileName").unwrap_or("").trim().to_string();
            // Inner text is the label
            let label = vtag.find('>')
                .map(|g| {
                    let after = &vtag[g + 1..];
                    let end = after.find('<').unwrap_or(after.len());
                    after[..end].trim().to_string()
                })
                .unwrap_or_default();

            if !ver_num.is_empty() && !filename.is_empty() {
                versions.push(FirmwareVersion {
                    version: ver_num,
                    filename,
                    label: if label.is_empty() { "?".into() } else { label },
                });
            }
            vs = &vs[pe.min(vs.len())..];
        }

        if !name.is_empty() && !versions.is_empty() {
            entries.push(FirmwareEntry { id, name, versions, other_ids });
        }

        search = &search[end.min(search.len())..];
    }

    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn firmware_catalog_non_empty() {
        let cat = load_firmware_catalog();
        assert!(!cat.is_empty(), "Firmware catalog should not be empty");
    }

    #[test]
    fn e62_has_versions() {
        let cat = load_firmware_catalog();
        let e62: Vec<_> = cat.iter().filter(|e| e.name == "E62").collect();
        assert!(!e62.is_empty(), "E62 should have firmware entries");
        for entry in e62 {
            assert!(!entry.versions.is_empty(), "E62 entry should have versions");
        }
    }
}
