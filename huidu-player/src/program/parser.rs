/// XML program file parser.
/// Parses program XML from HDPlayer into our data model.
use anyhow::{Context, Result};
use quick_xml::de::from_str;
use std::path::Path;
use tracing::info;

use super::model::Screen;

/// Parse a program XML file from disk
pub fn parse_program_file(path: &Path) -> Result<Screen> {
    let xml = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read program file: {}", path.display()))?;
    parse_program_xml(&xml)
}

/// Parse program XML from a string (e.g. from network)
pub fn parse_program_xml(xml: &str) -> Result<Screen> {
    // The XML may come wrapped in <sdk> tags from the network protocol
    // or standalone as a <screen> element from a file
    let xml_trimmed = xml.trim();

    if xml_trimmed.starts_with("<sdk") {
        // Extract the <screen> element from inside the SDK wrapper
        parse_sdk_wrapped(xml_trimmed)
    } else if xml_trimmed.starts_with("<screen") {
        // Direct screen XML
        let screen: Screen =
            from_str(xml_trimmed).context("Failed to parse <screen> XML")?;
        info!(
            "Parsed screen with {} program(s)",
            screen.programs.len()
        );
        Ok(screen)
    } else if xml_trimmed.starts_with("<?xml") {
        // Has XML declaration, strip it and re-parse
        if let Some(pos) = xml_trimmed.find("?>") {
            let body = xml_trimmed[pos + 2..].trim();
            parse_program_xml(body)
        } else {
            anyhow::bail!("Malformed XML declaration");
        }
    } else {
        anyhow::bail!(
            "Unknown XML format, expected <screen> or <sdk>, got: {}...",
            &xml_trimmed[..xml_trimmed.len().min(50)]
        );
    }
}

/// Extract <screen> from SDK-wrapped XML:
/// <sdk guid="..."><in method="AddProgram"><screen>...</screen></in></sdk>
fn parse_sdk_wrapped(xml: &str) -> Result<Screen> {
    // Find the <screen element within the XML
    let screen_start = xml
        .find("<screen")
        .context("No <screen> element found inside SDK XML")?;

    // Find matching </screen>
    let screen_end = xml
        .rfind("</screen>")
        .context("No closing </screen> tag found")?;

    let screen_xml = &xml[screen_start..screen_end + "</screen>".len()];
    let screen: Screen =
        from_str(screen_xml).context("Failed to parse <screen> from SDK XML")?;

    info!(
        "Parsed SDK-wrapped screen with {} program(s)",
        screen.programs.len()
    );
    Ok(screen)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_program() {
        let xml = r##"
        <screen timeStamps="12345">
          <program guid="abc-123" name="Test" type="normal">
            <area guid="area-1" name="Main" alpha="255">
              <rectangle x="0" y="0" width="128" height="64"/>
              <resources>
                <text guid="txt-1" singleLine="true">
                  <string>Hello World</string>
                  <effect in="0" out="0" inSpeed="0" outSpeed="0" duration="50"/>
                  <font size="12" color="#ff0000"/>
                  <style align="center" valign="middle"/>
                </text>
              </resources>
            </area>
          </program>
        </screen>
        "##;

        let screen = parse_program_xml(xml).unwrap();
        assert_eq!(screen.programs.len(), 1);
        let prog = &screen.programs[0];
        assert_eq!(prog.guid, "abc-123");
        assert_eq!(prog.areas.len(), 1);

        let area = &prog.areas[0];
        assert_eq!(area.rectangle.width, 128);
        assert_eq!(area.rectangle.height, 64);
        assert_eq!(area.resources.items.len(), 1);
    }

    #[test]
    fn test_parse_sdk_wrapped() {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
        <sdk guid="session-guid">
          <in method="AddProgram">
            <screen>
              <program guid="prog-1" name="NewProgram" type="normal">
                <area guid="area-1">
                  <rectangle width="128" height="64" x="0" y="0"/>
                  <resources>
                    <image guid="img-1" fit="stretch">
                      <effect in="17" out="17" inSpeed="3" outSpeed="3" duration="50"/>
                      <file name="logo.png"/>
                    </image>
                  </resources>
                </area>
              </program>
            </screen>
          </in>
        </sdk>
        "#;

        let screen = parse_program_xml(xml).unwrap();
        assert_eq!(screen.programs.len(), 1);
    }

    #[test]
    fn test_parse_clock() {
        let xml = r##"
        <screen>
          <program guid="p1" type="normal">
            <area guid="a1">
              <rectangle x="0" y="0" width="128" height="64"/>
              <resources>
                <clock guid="clk-1" type="digital" timezone="+8:00">
                  <date format="1" color="#00ff00" display="true"/>
                  <time format="1" color="#ffffff" display="true"/>
                  <week format="2" color="#ffff00" display="true"/>
                </clock>
              </resources>
            </area>
          </program>
        </screen>
        "##;

        let screen = parse_program_xml(xml).unwrap();
        assert_eq!(screen.programs.len(), 1);
        assert_eq!(screen.programs[0].areas[0].resources.items.len(), 1);
    }

    #[test]
    fn test_parse_text_rich() {
        let xml = r##"
        <screen>
          <program guid="p1" type="normal">
            <area guid="a1">
              <rectangle x="0" y="0" width="128" height="32"/>
              <resources>
                <text guid="t1" singleLine="false" background="#000000">
                  <string>Hello\nWorld</string>
                  <font size="14" color="rainbow" bold="true" italic="true" underline="false"/>
                  <style align="center" valign="top"/>
                </text>
              </resources>
            </area>
          </program>
        </screen>
        "##;

        let screen = parse_program_xml(xml).unwrap();
        let area = &screen.programs[0].areas[0];
        if let crate::program::model::ContentItem::Text(t) = &area.resources.items[0] {
            let font = t.font.as_ref().unwrap();
            assert!(font.bold);
            assert!(font.italic);
            assert_eq!(font.color, "rainbow");
        } else {
            panic!("Expected Text item");
        }
    }

    #[test]
    fn test_parse_table() {
        let xml = r##"
        <screen>
          <program guid="p1" type="normal">
            <area guid="a1">
              <rectangle x="0" y="0" width="200" height="100"/>
              <resources>
                <table guid="tbl-1" rows="2" cols="3">
                  <style fontColor="#ffffff" borderColor="#444444"
                         fontSize="10" headerBg="#222266"/>
                  <row>
                    <cell>Score</cell><cell>Team A</cell><cell>Team B</cell>
                  </row>
                  <row>
                    <cell>1</cell><cell>32</cell><cell>28</cell>
                  </row>
                </table>
              </resources>
            </area>
          </program>
        </screen>
        "##;

        let screen = parse_program_xml(xml).unwrap();
        let area = &screen.programs[0].areas[0];
        if let crate::program::model::ContentItem::Table(t) = &area.resources.items[0] {
            assert_eq!(t.rows, 2);
            assert_eq!(t.cols, 3);
        } else {
            panic!("Expected Table item");
        }
    }

    #[test]
    fn test_parse_countdown() {
        let xml = r##"
        <screen>
          <program guid="p1" type="normal">
            <area guid="a1">
              <rectangle x="0" y="0" width="128" height="32"/>
              <resources>
                <countdownTimer guid="ct-1"
                                target="2030-01-01 00:00:00"
                                format="D:H:M:S"
                                label="New Year in"
                                color="#00ff88"
                                urgentSecs="60"
                                urgentColor="#ff2200"
                                fontSize="14"/>
              </resources>
            </area>
          </program>
        </screen>
        "##;

        let screen = parse_program_xml(xml).unwrap();
        let area = &screen.programs[0].areas[0];
        if let crate::program::model::ContentItem::Countdown(c) = &area.resources.items[0] {
            assert_eq!(c.format, "D:H:M:S");
            assert_eq!(c.urgent_secs, 60);
            assert_eq!(c.label, "New Year in");
        } else {
            panic!("Expected Countdown item");
        }
    }

    #[test]
    fn test_parse_weather() {
        let xml = r##"
        <screen>
          <program guid="p1" type="normal">
            <area guid="a1">
              <rectangle x="0" y="0" width="128" height="64"/>
              <resources>
                <weather guid="w-1" city="London" updateInterval="2">
                  <font size="10" color="#ffffff"/>
                </weather>
              </resources>
            </area>
          </program>
        </screen>
        "##;

        let screen = parse_program_xml(xml).unwrap();
        let area = &screen.programs[0].areas[0];
        if let crate::program::model::ContentItem::Weather(w) = &area.resources.items[0] {
            assert_eq!(w.city, "London");
            assert_eq!(w.update_interval, 2);
        } else {
            panic!("Expected Weather item");
        }
    }

    #[test]
    fn test_parse_analog_clock() {
        let xml = r##"
        <screen>
          <program guid="p1" type="normal">
            <area guid="a1">
              <rectangle x="0" y="0" width="64" height="64"/>
              <resources>
                <analogClock guid="ac-1" dialColor="#222222" handColor="#ffffff"/>
              </resources>
            </area>
          </program>
        </screen>
        "##;

        let screen = parse_program_xml(xml).unwrap();
        let area = &screen.programs[0].areas[0];
        assert!(matches!(
            &area.resources.items[0],
            crate::program::model::ContentItem::AnalogClock(_)
        ));
    }
}
