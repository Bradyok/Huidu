//! XML helpers for building and parsing Huidu SDK XML messages.

use uuid::Uuid;
use crate::error::{DeviceError, Error, Result};

// ── GUID ─────────────────────────────────────────────────────────────────────

/// Generate a new GUID in Huidu format (uppercase UUID v4).
pub fn new_guid() -> String {
    Uuid::new_v4().to_string().to_uppercase()
}

// ── Request / response envelope builders ─────────────────────────────────────

/// Wrap an XML method call in the client-side SDK request envelope.
///
/// ```xml
/// <?xml version='1.0' encoding='utf-8'?>
/// <sdk guid="{guid}">
///   <in method="{method}">{body}</in>
/// </sdk>
/// ```
pub fn sdk_request(guid: &str, method: &str, body: &str) -> String {
    format!(
        "<?xml version='1.0' encoding='utf-8'?>\
<sdk guid=\"{guid}\">\
<in method=\"{method}\">{body}</in>\
</sdk>"
    )
}

/// Build a device-side (server) SDK success response envelope.
///
/// ```xml
/// <?xml version='1.0' encoding='utf-8'?>
/// <sdk guid="{guid}">
///   <out method="{method}">{body}<result value="0"/></out>
/// </sdk>
/// ```
pub fn sdk_response(guid: &str, method: &str, body: &str) -> String {
    format!(
        "<?xml version='1.0' encoding='utf-8'?>\
<sdk guid=\"{guid}\">\
<out method=\"{method}\">{body}<result value=\"0\"/></out>\
</sdk>"
    )
}

/// Build a device-side SDK error response.
pub fn sdk_error_response(guid: &str, method: &str, code: i32) -> String {
    format!(
        "<?xml version='1.0' encoding='utf-8'?>\
<sdk guid=\"{guid}\">\
<out method=\"{method}\"><result value=\"{code}\"/></out>\
</sdk>"
    )
}

// ── Parsing helpers ───────────────────────────────────────────────────────────

/// Parse a result code from an SDK XML response.
///
/// Looks for `<result value="N"/>` and returns:
/// - `Ok(())` if `N == 0` (or the tag is absent — some methods omit it)
/// - `Err(Error::Device { code, message })` for any non-zero code
pub fn parse_result(xml: &str) -> Result<()> {
    if let Some(start) = xml.find("result value=\"") {
        let rest = &xml[start + 14..];
        if let Some(end) = rest.find('"') {
            let code: i32 = rest[..end].parse().unwrap_or(-1);
            if code == 0 {
                return Ok(());
            }
            let de = DeviceError::from_code(code);
            return Err(Error::Device { code, message: de.message().into() });
        }
    }
    Ok(())
}

/// Extract the content of the `<out method="...">...</out>` block.
pub fn parse_out_body(xml: &str) -> Option<String> {
    let open = xml.find("<out ")?;
    let close_tag_start = xml[open..].find('>')? + open + 1;
    let close = xml.rfind("</out>")?;
    Some(xml[close_tag_start..close].to_string())
}

/// Extract the `method` attribute value from an SDK XML request or response.
///
/// Works on both `<in method="...">` and `<out method="...">` envelopes.
pub fn extract_method(xml: &str) -> Option<&str> {
    let needle = "method=\"";
    let start = xml.find(needle)? + needle.len();
    let end = xml[start..].find('"')?;
    Some(&xml[start..start + end])
}

/// Extract the `guid` attribute from the `<sdk>` envelope.
pub fn extract_guid(xml: &str) -> Option<&str> {
    get_attr(xml, "guid")
}

/// Extract an attribute value by name from an XML tag.
///
/// Scans for the first occurrence of `attr="..."` in `xml`.
pub fn get_attr<'a>(xml: &'a str, attr: &str) -> Option<&'a str> {
    let needle = format!("{}=\"", attr);
    let start = xml.find(&needle)? + needle.len();
    let end = xml[start..].find('"')?;
    Some(&xml[start..start + end])
}

/// Extract text content between `<tag>` and `</tag>` delimiters.
pub fn get_tag_text<'a>(xml: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    let start = xml.find(&open)? + open.len();
    let end = xml.find(&close)?;
    Some(&xml[start..end])
}

// ── HDSet XML helpers — value-attribute style ─────────────────────────────────

/// Parse the `value` attribute from a `<Tag value="N"/>` element.
///
/// Returns `None` if the tag is not found or the value cannot be parsed.
pub fn parse_value_attr(xml: &str, tag: &str) -> Option<u32> {
    let tag_str = format!("<{}", tag);
    let pos = xml.find(&tag_str)?;
    get_attr(&xml[pos..], "value")?.parse().ok()
}

/// Parse the `value` attribute from a `<Tag value="N"/>` element as `i32`.
pub fn parse_value_attr_i32(xml: &str, tag: &str) -> Option<i32> {
    let tag_str = format!("<{}", tag);
    let pos = xml.find(&tag_str)?;
    get_attr(&xml[pos..], "value")?.parse().ok()
}

/// Build a `<Tag value="N"/>` element.
pub fn value_elem(tag: &str, value: impl std::fmt::Display) -> String {
    format!("<{} value=\"{}\"/>", tag, value)
}

// ── Escaping ─────────────────────────────────────────────────────────────────

/// Escape the five predefined XML special characters.
pub fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
     .replace('<', "&lt;")
     .replace('>', "&gt;")
     .replace('"', "&quot;")
     .replace('\'', "&apos;")
}

/// Unescape the five predefined XML entities back to their characters.
///
/// Returns the input string unchanged (without allocation) if no `&` is present.
pub fn xml_unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    s.replace("&amp;", "&")
     .replace("&lt;", "<")
     .replace("&gt;", ">")
     .replace("&quot;", "\"")
     .replace("&apos;", "'")
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sdk_request_envelope() {
        let xml = sdk_request("GUID-1234", "GetDeviceInfo", "");
        assert!(xml.contains("method=\"GetDeviceInfo\""));
        assert!(xml.contains("guid=\"GUID-1234\""));
        assert!(xml.contains("<in "));
    }

    #[test]
    fn test_sdk_response_envelope() {
        let xml = sdk_response("GUID-1234", "GetDeviceInfo", "<foo/>");
        assert!(xml.contains("<out method=\"GetDeviceInfo\">"));
        assert!(xml.contains("<result value=\"0\"/>"));
        assert!(xml.contains("<foo/>"));
    }

    #[test]
    fn test_parse_result_success() {
        let xml = "<sdk><out method=\"test\"><result value=\"0\"/></out></sdk>";
        assert!(parse_result(xml).is_ok());
    }

    #[test]
    fn test_parse_result_absent_treated_as_success() {
        let xml = "<sdk><out method=\"test\"><data/></out></sdk>";
        assert!(parse_result(xml).is_ok());
    }

    #[test]
    fn test_parse_result_error() {
        let xml = "<sdk><out method=\"test\"><result value=\"18\"/></out></sdk>";
        let err = parse_result(xml).unwrap_err();
        assert!(matches!(err, Error::Device { code: 18, .. }));
    }

    #[test]
    fn test_extract_method() {
        let xml = "<?xml version='1.0'?><sdk guid=\"G\"><in method=\"SetBrightness\"><b/></in></sdk>";
        assert_eq!(extract_method(xml), Some("SetBrightness"));
    }

    #[test]
    fn test_get_attr() {
        let xml = "<tag foo=\"bar\" baz=\"qux\"/>";
        assert_eq!(get_attr(xml, "foo"), Some("bar"));
        assert_eq!(get_attr(xml, "baz"), Some("qux"));
        assert_eq!(get_attr(xml, "missing"), None);
    }

    #[test]
    fn test_parse_value_attr() {
        let xml = "<GrayLevel value=\"256\"/>";
        assert_eq!(parse_value_attr(xml, "GrayLevel"), Some(256u32));
        assert_eq!(parse_value_attr(xml, "Missing"), None);
    }

    #[test]
    fn test_xml_escape_unescape_roundtrip() {
        let s = "LED & Sign <v2> \"test\" it's";
        assert_eq!(xml_unescape(&xml_escape(s)), s);
    }

    #[test]
    fn test_xml_unescape_no_entities_no_alloc_indicator() {
        // No '&' → should just return a clone (no entity processing)
        let s = "Hello World";
        assert_eq!(xml_unescape(s), s);
    }
}
