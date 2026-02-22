//! XML helpers for building and parsing Huidu SDK XML messages.

use uuid::Uuid;
use crate::error::{Error, Result};

/// Generate a new GUID in Huidu format (uppercase UUID v4).
pub fn new_guid() -> String {
    Uuid::new_v4().to_string().to_uppercase()
}

/// Wrap an XML method call in the SDK envelope.
///
/// ```xml
/// <?xml version='1.0' encoding='utf-8'?>
/// <sdk guid="{GUID}">
///   <in method="{method}">
///     {body}
///   </in>
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

/// Parse result code from an SDK XML response.
/// Returns Ok(()) if result value="0", Err with code otherwise.
pub fn parse_result(xml: &str) -> Result<()> {
    // Look for <result value="N"/>
    if let Some(start) = xml.find("result value=\"") {
        let rest = &xml[start + 14..];
        if let Some(end) = rest.find('"') {
            let val_str = &rest[..end];
            let code: i32 = val_str.parse().unwrap_or(-1);
            if code == 0 {
                return Ok(());
            } else {
                return Err(Error::Device {
                    code,
                    message: crate::error::DeviceError::from_code(code).message().into(),
                });
            }
        }
    }
    // If no result tag, assume success (some methods omit it)
    Ok(())
}

/// Extract the content of the `<out method="...">` block.
pub fn parse_out_body(xml: &str) -> Option<String> {
    let open = xml.find("<out ")?;
    let close_tag_start = xml[open..].find('>')? + open + 1;
    let end_tag = "</out>";
    let close = xml.rfind(end_tag)?;
    Some(xml[close_tag_start..close].to_string())
}

/// Extract an attribute value by name from an XML tag.
pub fn get_attr<'a>(xml: &'a str, attr: &str) -> Option<&'a str> {
    let needle = format!("{}=\"", attr);
    let start = xml.find(&needle)? + needle.len();
    let end = xml[start..].find('"')?;
    Some(&xml[start..start + end])
}

/// Extract text content between an XML tag pair.
pub fn get_tag_text<'a>(xml: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    let start = xml.find(&open)? + open.len();
    let end = xml.find(&close)?;
    Some(&xml[start..end])
}

/// Escape XML special characters.
pub fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
     .replace('<', "&lt;")
     .replace('>', "&gt;")
     .replace('"', "&quot;")
     .replace('\'', "&apos;")
}

/// Unescape XML entities back to their character equivalents.
///
/// Handles the five predefined XML entities:
/// `&amp;` → `&`, `&lt;` → `<`, `&gt;` → `>`, `&quot;` → `"`, `&apos;` → `'`
///
/// Returns the input unchanged (without allocation) if no `&` is present.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sdk_request() {
        let xml = sdk_request("GUID-1234", "GetDeviceInfo", "");
        assert!(xml.contains("method=\"GetDeviceInfo\""));
        assert!(xml.contains("guid=\"GUID-1234\""));
    }

    #[test]
    fn test_parse_result_success() {
        let xml = "<sdk><out method=\"test\"><result value=\"0\"/></out></sdk>";
        assert!(parse_result(xml).is_ok());
    }

    #[test]
    fn test_parse_result_error() {
        let xml = "<sdk><out method=\"test\"><result value=\"18\"/></out></sdk>";
        assert!(parse_result(xml).is_err());
    }

    // ── xml_unescape ─────────────────────────────────────────────────────────

    #[test]
    fn test_xml_unescape_no_entities_passthrough() {
        assert_eq!(xml_unescape("Hello World"), "Hello World");
        assert_eq!(xml_unescape(""), "");
        assert_eq!(xml_unescape("LED Sign v2"), "LED Sign v2");
    }

    #[test]
    fn test_xml_unescape_amp() {
        assert_eq!(xml_unescape("A &amp; B"), "A & B");
    }

    #[test]
    fn test_xml_unescape_lt_gt() {
        assert_eq!(xml_unescape("&lt;tag&gt;"), "<tag>");
    }

    #[test]
    fn test_xml_unescape_quot() {
        assert_eq!(xml_unescape("say &quot;hello&quot;"), "say \"hello\"");
    }

    #[test]
    fn test_xml_unescape_apos() {
        assert_eq!(xml_unescape("it&apos;s"), "it's");
    }

    #[test]
    fn test_xml_unescape_multiple_entities() {
        assert_eq!(
            xml_unescape("&lt;name value=&quot;A &amp; B&quot;/&gt;"),
            "<name value=\"A & B\"/>"
        );
    }

    #[test]
    fn test_xml_escape_unescape_roundtrip() {
        let original = "LED & Sign <v2> \"test\" it's";
        assert_eq!(xml_unescape(&xml_escape(original)), original);
    }
}
