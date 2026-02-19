/// SDK XML command handler — routes incoming commands to appropriate handlers.
use anyhow::Result;
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::core::player::PlayerCommand;
use crate::program::parser;
use crate::protocol::session::Session;

/// Handle an incoming SDK XML command and return the response XML
pub async fn handle_sdk_command(
    xml: &str,
    session: &Session,
    player_tx: &mpsc::Sender<PlayerCommand>,
    _program_dir: &str,
) -> Result<String> {
    // Extract the method name from: <sdk guid="..."><in method="MethodName">
    let method = extract_method(xml).unwrap_or_default();
    info!("SDK command: {}", method);

    match method.as_str() {
        "QueryIFVersion" | "queryIFVersion" | "GetIFVersion" => {
            // Version negotiation — return session GUID and protocol version
            Ok(format!(
                r#"<?xml version="1.0" encoding="utf-8"?><sdk guid="{}"><out method="QueryIFVersion"><version value="0x1000000"/></out></sdk>"#,
                session.guid
            ))
        }

        "AddProgram" | "addProgram" => {
            // Parse the program XML and send to player
            match parser::parse_program_xml(xml) {
                Ok(screen) => {
                    player_tx.send(PlayerCommand::LoadScreen(screen)).await.ok();
                    Ok(format!(
                        r#"<?xml version="1.0" encoding="utf-8"?><sdk guid="{}"><out method="AddProgram"><result value="0"/></out></sdk>"#,
                        session.guid
                    ))
                }
                Err(e) => {
                    warn!("Failed to parse AddProgram: {}", e);
                    Ok(format!(
                        r#"<?xml version="1.0" encoding="utf-8"?><sdk guid="{}"><out method="AddProgram"><result value="1"/><error message="{}"/></out></sdk>"#,
                        session.guid,
                        e.to_string().replace('"', "'")
                    ))
                }
            }
        }

        "UpdateProgram" | "updateProgram" => {
            // Same as AddProgram for now
            match parser::parse_program_xml(xml) {
                Ok(screen) => {
                    player_tx.send(PlayerCommand::LoadScreen(screen)).await.ok();
                    Ok(format!(
                        r#"<?xml version="1.0" encoding="utf-8"?><sdk guid="{}"><out method="UpdateProgram"><result value="0"/></out></sdk>"#,
                        session.guid
                    ))
                }
                Err(e) => {
                    warn!("Failed to parse UpdateProgram: {}", e);
                    Ok(format!(
                        r#"<?xml version="1.0" encoding="utf-8"?><sdk guid="{}"><out method="UpdateProgram"><result value="1"/></out></sdk>"#,
                        session.guid
                    ))
                }
            }
        }

        "DeleteProgram" | "deleteProgram" => {
            // Clear all programs
            player_tx
                .send(PlayerCommand::LoadScreen(crate::program::model::Screen {
                    timestamps: String::new(),
                    programs: Vec::new(),
                }))
                .await
                .ok();
            Ok(format!(
                r#"<?xml version="1.0" encoding="utf-8"?><sdk guid="{}"><out method="DeleteProgram"><result value="0"/></out></sdk>"#,
                session.guid
            ))
        }

        "OpenScreen" | "openScreen" => {
            player_tx.send(PlayerCommand::ScreenPower(true)).await.ok();
            Ok(format!(
                r#"<?xml version="1.0" encoding="utf-8"?><sdk guid="{}"><out method="OpenScreen"><result value="0"/></out></sdk>"#,
                session.guid
            ))
        }

        "CloseScreen" | "closeScreen" => {
            player_tx
                .send(PlayerCommand::ScreenPower(false))
                .await
                .ok();
            Ok(format!(
                r#"<?xml version="1.0" encoding="utf-8"?><sdk guid="{}"><out method="CloseScreen"><result value="0"/></out></sdk>"#,
                session.guid
            ))
        }

        "GetDeviceInfo" | "getDeviceInfo" => {
            // Return device info
            Ok(format!(
                r#"<?xml version="1.0" encoding="utf-8"?><sdk guid="{}"><out method="GetDeviceInfo"><deviceInfo cpu="RustPlayer" model="huidu-player" fpgaVersion="1.0.0" screenWidth="128" screenHeight="64" deviceID="RUST-001"/></out></sdk>"#,
                session.guid
            ))
        }

        _ => {
            warn!("Unhandled SDK method: {}", method);
            Ok(format!(
                r#"<?xml version="1.0" encoding="utf-8"?><sdk guid="{}"><out method="{}"><result value="0"/></out></sdk>"#,
                session.guid, method
            ))
        }
    }
}

/// Extract the method name from <sdk...><in method="MethodName">
fn extract_method(xml: &str) -> Option<String> {
    // Look for method="..." in the <in> element
    let in_start = xml.find("<in ")?;
    let method_attr = xml[in_start..].find("method=\"")?;
    let start = in_start + method_attr + 8;
    let end = xml[start..].find('"')? + start;
    Some(xml[start..end].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_method() {
        let xml = r#"<sdk guid="abc"><in method="AddProgram"><screen></screen></in></sdk>"#;
        assert_eq!(extract_method(xml), Some("AddProgram".to_string()));
    }

    #[test]
    fn test_extract_method_version() {
        let xml =
            r#"<sdk guid="abc"><in method="QueryIFVersion"><version/></in></sdk>"#;
        assert_eq!(extract_method(xml), Some("QueryIFVersion".to_string()));
    }
}
