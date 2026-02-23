//! Bundled weather city database parsed from Yahoo_Weather_*_City_Code.xml.
//!
//! Maps country names to lists of (city_name, city_code) pairs.
//! These codes are used by the Weather content item to display weather data.

// Embed the country XML files at compile time.
// The 1.7 MB America file is omitted; a small curated US list is provided instead.
const AUSTRALIA_XML:   &str = include_str!("../../hdsign-extracted/Yahoo_Weather_Australia_City_Code.xml");
const CANADA_XML:      &str = include_str!("../../hdsign-extracted/Yahoo_Weather_Canada_City_Code.xml");
const CHINA_XML:       &str = include_str!("../../hdsign-extracted/Yahoo_Weather_China_City_Code.xml");
const INDIA_XML:       &str = include_str!("../../hdsign-extracted/Yahoo_Weather_India_City_Code.xml");
const INDONESIA_XML:   &str = include_str!("../../hdsign-extracted/Yahoo_Weather_Indonesia_City_Code.xml");
const IRAN_XML:        &str = include_str!("../../hdsign-extracted/Yahoo_Weather_Iran_City_Code.xml");
const JAPAN_XML:       &str = include_str!("../../hdsign-extracted/Yahoo_Weather_Japan_City_Code.xml");
const KOREA_XML:       &str = include_str!("../../hdsign-extracted/Yahoo_Weather_Korea_City_Code.xml");
const MALAYSIA_XML:    &str = include_str!("../../hdsign-extracted/Yahoo_Weather_Malaysia_City_Code.xml");
const PAKISTAN_XML:    &str = include_str!("../../hdsign-extracted/Yahoo_Weather_Pakistan_City_Code.xml");
const PERU_XML:        &str = include_str!("../../hdsign-extracted/Yahoo_Weather_Peru_City_Code.xml");
const POLAND_XML:      &str = include_str!("../../hdsign-extracted/Yahoo_Weather_Poland_City_Code.xml");
const RUSSIA_XML:      &str = include_str!("../../hdsign-extracted/Yahoo_Weather_Russia_City_Code.xml");
const SPAIN_XML:       &str = include_str!("../../hdsign-extracted/Yahoo_Weather_Spain_City_Code.xml");
const THAILAND_XML:    &str = include_str!("../../hdsign-extracted/Yahoo_Weather_Thailand_City_Code.xml");
const TURKEY_XML:      &str = include_str!("../../hdsign-extracted/Yahoo_Weather_Turkey_City_Code.xml");
const UAE_XML:         &str = include_str!("../../hdsign-extracted/Yahoo_Weather_United Arab Emirates_City_Code.xml");
const VIETNAM_XML:     &str = include_str!("../../hdsign-extracted/Yahoo_Weather_Vietnam_City_Code.xml");

/// Curated list of major US cities (name, Yahoo city code).
const AMERICA_CITIES: &[(&str, &str)] = &[
    ("New York",       "USNY0996"),
    ("Los Angeles",    "USCA0638"),
    ("Chicago",        "USIL0225"),
    ("Houston",        "USTX0617"),
    ("Phoenix",        "USAZ0166"),
    ("Philadelphia",   "USPA1095"),
    ("San Antonio",    "USTX1916"),
    ("San Diego",      "USCA1082"),
    ("Dallas",         "USTX0457"),
    ("San Jose",       "USCA1064"),
    ("Austin",         "USTX0061"),
    ("Jacksonville",   "USFL0351"),
    ("Fort Worth",     "USTX0521"),
    ("Columbus",       "USOH0169"),
    ("Charlotte",      "USNC0080"),
    ("Indianapolis",   "USIN0317"),
    ("San Francisco",  "USCA0987"),
    ("Seattle",        "USWA0395"),
    ("Denver",         "USCO0105"),
    ("Washington DC",  "USDC0001"),
    ("Nashville",      "USTN0400"),
    ("Oklahoma City",  "USOK0440"),
    ("El Paso",        "USTX0476"),
    ("Boston",         "USMA0043"),
    ("Portland",       "USOR0275"),
    ("Las Vegas",      "USNV0049"),
    ("Memphis",        "USTN0413"),
    ("Louisville",     "USKY0228"),
    ("Baltimore",      "USMD0018"),
    ("Milwaukee",      "USWI0478"),
    ("Albuquerque",    "USNM0007"),
    ("Tucson",         "USAZ0373"),
    ("Atlanta",        "USGA0028"),
    ("Miami",          "USFL0316"),
    ("Minneapolis",    "USMN0503"),
    ("New Orleans",    "USLA0376"),
    ("Detroit",        "USMI0202"),
    ("Honolulu",       "USHI0013"),
    ("Anchorage",      "USAK0012"),
];

/// All country entries: (display_name, xml_data_or_none).
/// `None` means use the manual list (AMERICA_CITIES).
static COUNTRY_DATA: &[(&str, Option<&str>)] = &[
    ("America",             None),
    ("Australia",           Some(AUSTRALIA_XML)),
    ("Canada",              Some(CANADA_XML)),
    ("China",               Some(CHINA_XML)),
    ("India",               Some(INDIA_XML)),
    ("Indonesia",           Some(INDONESIA_XML)),
    ("Iran",                Some(IRAN_XML)),
    ("Japan",               Some(JAPAN_XML)),
    ("Korea",               Some(KOREA_XML)),
    ("Malaysia",            Some(MALAYSIA_XML)),
    ("Pakistan",            Some(PAKISTAN_XML)),
    ("Peru",                Some(PERU_XML)),
    ("Poland",              Some(POLAND_XML)),
    ("Russia",              Some(RUSSIA_XML)),
    ("Spain",               Some(SPAIN_XML)),
    ("Thailand",            Some(THAILAND_XML)),
    ("Turkey",              Some(TURKEY_XML)),
    ("United Arab Emirates",Some(UAE_XML)),
    ("Vietnam",             Some(VIETNAM_XML)),
];

/// All available country names.
pub fn weather_countries() -> &'static [&'static str] {
    // Use a static slice; COUNTRY_DATA always has these in order.
    &[
        "America", "Australia", "Canada", "China", "India", "Indonesia",
        "Iran", "Japan", "Korea", "Malaysia", "Pakistan", "Peru", "Poland",
        "Russia", "Spain", "Thailand", "Turkey", "United Arab Emirates", "Vietnam",
    ]
}

/// Return (city_name, city_code) pairs for the given country.
pub fn get_weather_cities(country: &str) -> Vec<(String, String)> {
    if country == "America" {
        return AMERICA_CITIES.iter()
            .map(|(n, id)| (n.to_string(), id.to_string()))
            .collect();
    }
    let xml = match COUNTRY_DATA.iter().find(|(c, _)| *c == country) {
        Some((_, Some(xml))) => xml,
        _ => return Vec::new(),
    };
    parse_weather_xml(xml)
}

fn parse_weather_xml(xml: &str) -> Vec<(String, String)> {
    let mut cities = Vec::new();
    let mut search = xml;
    while let Some(start) = search.find("<City ") {
        let end = search[start..].find("/>")
            .map(|e| start + e + 2)
            .unwrap_or(search.len());
        let tag = &search[start..end];
        if let (Some(name), Some(id)) = (get_attr(tag, "Name"), get_attr(tag, "ID")) {
            cities.push((name.to_string(), id.to_string()));
        }
        search = &search[end.min(search.len())..];
    }
    cities
}

fn get_attr<'a>(xml: &'a str, attr: &str) -> Option<&'a str> {
    let needle = format!("{attr}=\"");
    let s = xml.find(&needle)? + needle.len();
    let e = xml[s..].find('"')?;
    Some(&xml[s..s + e])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indonesia_cities_non_empty() {
        let cities = get_weather_cities("Indonesia");
        assert!(!cities.is_empty(), "Indonesia should have weather cities");
        assert!(cities.iter().any(|(n, _)| n == "Jakarta"), "Jakarta should be present");
    }

    #[test]
    fn america_cities_non_empty() {
        let cities = get_weather_cities("America");
        assert!(!cities.is_empty());
        assert!(cities.iter().any(|(n, _)| n == "New York"));
    }

    #[test]
    fn all_countries_load() {
        for country in weather_countries() {
            let cities = get_weather_cities(country);
            assert!(!cities.is_empty(), "Country {country} should have cities");
        }
    }
}
