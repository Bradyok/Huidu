//! XML helpers — thin wrapper around `huidu_protocol::xml`.
pub use huidu_protocol::xml::{
    new_guid, sdk_request, sdk_response, sdk_error_response,
    parse_out_body, extract_method, extract_guid,
    get_attr, get_tag_text,
    xml_escape, xml_unescape,
    parse_value_attr, parse_value_attr_i32, value_elem,
};

/// Parse result code from an SDK XML response, returning the local error type.
pub fn parse_result(xml: &str) -> crate::error::Result<()> {
    huidu_protocol::xml::parse_result(xml).map_err(Into::into)
}
