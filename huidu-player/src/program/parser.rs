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
                <analogClock guid="ac-1" dialColor="#222222" handColor="#ffffff" secondColor="#ff4400"/>
              </resources>
            </area>
          </program>
        </screen>
        "##;

        let screen = parse_program_xml(xml).unwrap();
        let area = &screen.programs[0].areas[0];
        if let crate::program::model::ContentItem::AnalogClock(c) = &area.resources.items[0] {
            assert_eq!(c.guid, "ac-1");
            assert_eq!(c.dial_color, "#222222");
            assert_eq!(c.hand_color, "#ffffff");
            assert_eq!(c.second_color, "#ff4400");
        } else {
            panic!("Expected AnalogClock item");
        }
        // Default colors when attributes are absent
        let xml2 = r##"<screen><program guid="p2"><area guid="a2"><rectangle width="32" height="32"/><resources><analogClock guid="ac-2"/></resources></area></program></screen>"##;
        let s2 = parse_program_xml(xml2).unwrap();
        if let crate::program::model::ContentItem::AnalogClock(c) = &s2.programs[0].areas[0].resources.items[0] {
            assert_eq!(c.dial_color, "#0a0a1e");
            assert_eq!(c.hand_color, "#ffffff");
            assert_eq!(c.second_color, "#ff3c3c");
        }
    }

    /// Task #13: scrollDir, scrollSpeed, shadow/outline font attributes
    #[test]
    fn test_parse_text_scroll_and_shadow() {
        let xml = r##"
        <screen>
          <program guid="p1" type="normal">
            <area guid="a1">
              <rectangle x="0" y="0" width="128" height="16"/>
              <resources>
                <text guid="t1" singleLine="true"
                      scrollDir="up" scrollSpeed="75">
                  <string>Scrolling</string>
                  <font size="12" color="#ffffff"
                        shadow="true" shadowColor="#000033" shadowDx="2" shadowDy="2"
                        outline="true" outlineColor="#ff0000"/>
                  <style align="left" valign="middle"/>
                </text>
              </resources>
            </area>
          </program>
        </screen>
        "##;

        let screen = parse_program_xml(xml).unwrap();
        let area = &screen.programs[0].areas[0];
        if let crate::program::model::ContentItem::Text(t) = &area.resources.items[0] {
            assert_eq!(t.scroll_dir, "up");
            assert_eq!(t.scroll_speed, 75);
            let font = t.font.as_ref().unwrap();
            assert!(font.shadow);
            assert_eq!(font.shadow_color, "#000033");
            assert_eq!(font.shadow_dx, 2);
            assert_eq!(font.shadow_dy, 2);
            assert!(font.outline);
            assert_eq!(font.outline_color, "#ff0000");
        } else {
            panic!("Expected Text item");
        }
    }

    /// Task #14: seamless ticker, tickerGap, wordWrap attributes
    #[test]
    fn test_parse_text_seamless_and_wordwrap() {
        let xml = r##"
        <screen>
          <program guid="p1" type="normal">
            <area guid="a1">
              <rectangle x="0" y="0" width="128" height="16"/>
              <resources>
                <text guid="t1" singleLine="true"
                      seamless="true" tickerGap="24" wordWrap="false">
                  <string>Seamless ticker demo</string>
                  <font size="12" color="rainbow"/>
                  <style align="left" valign="middle"/>
                </text>
              </resources>
            </area>
          </program>
        </screen>
        "##;

        let screen = parse_program_xml(xml).unwrap();
        let area = &screen.programs[0].areas[0];
        if let crate::program::model::ContentItem::Text(t) = &area.resources.items[0] {
            assert!(t.seamless);
            assert_eq!(t.ticker_gap, 24);
            assert!(!t.word_wrap);
            assert!(t.single_line);
        } else {
            panic!("Expected Text item");
        }
    }

    /// Task #14: wordWrap="true" on multi-line text
    #[test]
    fn test_parse_text_word_wrap() {
        let xml = r##"
        <screen>
          <program guid="p1" type="normal">
            <area guid="a1">
              <rectangle x="0" y="0" width="128" height="64"/>
              <resources>
                <text guid="t1" singleLine="false" wordWrap="true">
                  <string>This is a long text that should be word wrapped automatically</string>
                  <font size="11" color="#ffffff"/>
                  <style align="center" valign="middle"/>
                </text>
              </resources>
            </area>
          </program>
        </screen>
        "##;

        let screen = parse_program_xml(xml).unwrap();
        let area = &screen.programs[0].areas[0];
        if let crate::program::model::ContentItem::Text(t) = &area.resources.items[0] {
            assert!(t.word_wrap);
            assert!(!t.single_line);
        } else {
            panic!("Expected Text item");
        }
    }

    /// Task #14: weather tempUnit="F" for Fahrenheit display
    #[test]
    fn test_parse_weather_fahrenheit() {
        let xml = r##"
        <screen>
          <program guid="p1" type="normal">
            <area guid="a1">
              <rectangle x="0" y="0" width="128" height="64"/>
              <resources>
                <weather guid="w-1" city="New York" tempUnit="F" updateInterval="15">
                  <font size="12" color="#ffff00"/>
                </weather>
              </resources>
            </area>
          </program>
        </screen>
        "##;

        let screen = parse_program_xml(xml).unwrap();
        let area = &screen.programs[0].areas[0];
        if let crate::program::model::ContentItem::Weather(w) = &area.resources.items[0] {
            assert_eq!(w.city, "New York");
            assert_eq!(w.temp_unit, "F");
            assert_eq!(w.update_interval, 15);
        } else {
            panic!("Expected Weather item");
        }
    }

    /// Default values: missing optional attributes should use their defaults.
    #[test]
    fn test_parse_text_defaults() {
        let xml = r##"
        <screen>
          <program guid="p1" type="normal">
            <area guid="a1">
              <rectangle x="0" y="0" width="128" height="32"/>
              <resources>
                <text guid="t1">
                  <string>Minimal text</string>
                </text>
              </resources>
            </area>
          </program>
        </screen>
        "##;

        let screen = parse_program_xml(xml).unwrap();
        let area = &screen.programs[0].areas[0];
        if let crate::program::model::ContentItem::Text(t) = &area.resources.items[0] {
            // Defaults from Task #13 + #14
            assert_eq!(t.scroll_dir, "left");
            assert_eq!(t.scroll_speed, 50);
            assert!(!t.seamless);
            assert_eq!(t.ticker_gap, 16);
            assert!(!t.word_wrap);
        } else {
            panic!("Expected Text item");
        }
    }

    /// Integration test: the full demo.xml must parse without errors and
    /// contain all expected programs.
    #[test]
    fn test_parse_demo_xml() {
        let xml = include_str!("../../programs/demo.xml");
        let screen = parse_program_xml(xml).unwrap();

        // Collect GUIDs for easy lookup
        let guids: Vec<&str> = screen.programs.iter().map(|p| p.guid.as_str()).collect();

        // Original demos
        assert!(guids.contains(&"demo-001"), "demo-001 missing");
        assert!(guids.contains(&"demo-002"), "demo-002 missing");
        assert!(guids.contains(&"demo-007"), "demo-007 missing");

        // New demos added in tasks 13–14
        assert!(guids.contains(&"demo-007a"), "demo-007a (Text FX) missing");
        assert!(guids.contains(&"demo-008a"), "demo-008a (Live Vars) missing");
        assert!(guids.contains(&"demo-008b"), "demo-008b (Word Wrap) missing");
        assert!(guids.contains(&"demo-009"), "demo-009 (Data Sources) missing");

        // Every program must have at least one area
        for p in &screen.programs {
            assert!(!p.areas.is_empty(), "program {} has no areas", p.guid);
        }
    }

    #[test]
    fn test_parse_web_page() {
        let xml = r##"
        <screen>
          <program guid="p1" type="normal">
            <area guid="a1">
              <rectangle x="0" y="0" width="256" height="32"/>
              <resources>
                <webPage guid="wp-1"
                         url="https://example.com"
                         updateInterval="3"
                         scrollSpeed="60"/>
              </resources>
            </area>
          </program>
        </screen>
        "##;

        let screen = parse_program_xml(xml).unwrap();
        let area = &screen.programs[0].areas[0];
        if let crate::program::model::ContentItem::Web(w) = &area.resources.items[0] {
            assert_eq!(w.url, "https://example.com");
            assert_eq!(w.update_interval, 3);
            assert_eq!(w.scroll_speed, 60);
        } else {
            panic!("Expected Web item");
        }
    }

    #[test]
    fn test_parse_rss_feed() {
        let xml = r##"
        <screen>
          <program guid="p1" type="normal">
            <area guid="a1">
              <rectangle x="0" y="0" width="512" height="32"/>
              <resources>
                <rss guid="rss-1"
                     url="https://feeds.example.com/rss"
                     updateInterval="15"
                     itemCount="5"
                     separator=" — "
                     scrollSpeed="80"/>
              </resources>
            </area>
          </program>
        </screen>
        "##;

        let screen = parse_program_xml(xml).unwrap();
        let area = &screen.programs[0].areas[0];
        if let crate::program::model::ContentItem::Rss(r) = &area.resources.items[0] {
            assert_eq!(r.url, "https://feeds.example.com/rss");
            assert_eq!(r.update_interval, 15);
            assert_eq!(r.item_count, 5);
            assert_eq!(r.separator, " — ");
            assert_eq!(r.scroll_speed, 80);
        } else {
            panic!("Expected Rss item");
        }
    }

    #[test]
    fn test_parse_external_data() {
        let xml = r##"
        <screen>
          <program guid="p1" type="normal">
            <area guid="a1">
              <rectangle x="0" y="0" width="256" height="32"/>
              <resources>
                <externalData guid="ed-1"
                              url="https://api.example.com/data.json"
                              path="current.value"
                              format="Value: {value}"
                              updateInterval="2"/>
              </resources>
            </area>
          </program>
        </screen>
        "##;

        let screen = parse_program_xml(xml).unwrap();
        let area = &screen.programs[0].areas[0];
        if let crate::program::model::ContentItem::ExternalData(e) = &area.resources.items[0] {
            assert_eq!(e.url, "https://api.example.com/data.json");
            assert_eq!(e.path, "current.value");
            assert_eq!(e.format, "Value: {value}");
            assert_eq!(e.update_interval, 2);
        } else {
            panic!("Expected ExternalData item");
        }
    }
}
