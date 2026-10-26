//! Windows time-zone names (Outlook / Exchange `TZID`) → IANA.
//!
//! Primary zone per Windows name from CLDR `windowsZones.xml` (territory
//! `001`). Unlisted names fall back to the caller's default.

const ZONES: &[(&str, &str)] = &[
    ("Dateline Standard Time", "Etc/GMT+12"),
    ("UTC-11", "Etc/GMT+11"),
    ("Hawaiian Standard Time", "Pacific/Honolulu"),
    ("Alaskan Standard Time", "America/Anchorage"),
    ("Pacific Standard Time", "America/Los_Angeles"),
    ("Pacific Standard Time (Mexico)", "America/Tijuana"),
    ("US Mountain Standard Time", "America/Phoenix"),
    ("Mountain Standard Time", "America/Denver"),
    ("Mountain Standard Time (Mexico)", "America/Mazatlan"),
    ("Central America Standard Time", "America/Guatemala"),
    ("Central Standard Time", "America/Chicago"),
    ("Central Standard Time (Mexico)", "America/Mexico_City"),
    ("Canada Central Standard Time", "America/Regina"),
    ("SA Pacific Standard Time", "America/Bogota"),
    ("Eastern Standard Time", "America/New_York"),
    ("Eastern Standard Time (Mexico)", "America/Cancun"),
    ("US Eastern Standard Time", "America/Indiana/Indianapolis"),
    ("Atlantic Standard Time", "America/Halifax"),
    ("Venezuela Standard Time", "America/Caracas"),
    ("SA Western Standard Time", "America/La_Paz"),
    ("Pacific SA Standard Time", "America/Santiago"),
    ("Newfoundland Standard Time", "America/St_Johns"),
    ("E. South America Standard Time", "America/Sao_Paulo"),
    ("Argentina Standard Time", "America/Argentina/Buenos_Aires"),
    ("SA Eastern Standard Time", "America/Cayenne"),
    ("UTC-02", "Etc/GMT+2"),
    ("Azores Standard Time", "Atlantic/Azores"),
    ("Cape Verde Standard Time", "Atlantic/Cape_Verde"),
    ("UTC", "Etc/UTC"),
    ("GMT Standard Time", "Europe/London"),
    ("Greenwich Standard Time", "Atlantic/Reykjavik"),
    ("W. Europe Standard Time", "Europe/Berlin"),
    ("Central Europe Standard Time", "Europe/Budapest"),
    ("Romance Standard Time", "Europe/Paris"),
    ("Central European Standard Time", "Europe/Warsaw"),
    ("W. Central Africa Standard Time", "Africa/Lagos"),
    ("GTB Standard Time", "Europe/Bucharest"),
    ("E. Europe Standard Time", "Europe/Chisinau"),
    ("FLE Standard Time", "Europe/Helsinki"),
    ("Israel Standard Time", "Asia/Jerusalem"),
    ("South Africa Standard Time", "Africa/Johannesburg"),
    ("Egypt Standard Time", "Africa/Cairo"),
    ("Turkey Standard Time", "Europe/Istanbul"),
    ("Arabic Standard Time", "Asia/Baghdad"),
    ("Arab Standard Time", "Asia/Riyadh"),
    ("Russian Standard Time", "Europe/Moscow"),
    ("E. Africa Standard Time", "Africa/Nairobi"),
    ("Iran Standard Time", "Asia/Tehran"),
    ("Arabian Standard Time", "Asia/Dubai"),
    ("Pakistan Standard Time", "Asia/Karachi"),
    ("India Standard Time", "Asia/Kolkata"),
    ("Sri Lanka Standard Time", "Asia/Colombo"),
    ("Nepal Standard Time", "Asia/Kathmandu"),
    ("Bangladesh Standard Time", "Asia/Dhaka"),
    ("SE Asia Standard Time", "Asia/Bangkok"),
    ("China Standard Time", "Asia/Shanghai"),
    ("Singapore Standard Time", "Asia/Singapore"),
    ("Taipei Standard Time", "Asia/Taipei"),
    ("W. Australia Standard Time", "Australia/Perth"),
    ("Tokyo Standard Time", "Asia/Tokyo"),
    ("Korea Standard Time", "Asia/Seoul"),
    ("Cen. Australia Standard Time", "Australia/Adelaide"),
    ("AUS Central Standard Time", "Australia/Darwin"),
    ("E. Australia Standard Time", "Australia/Brisbane"),
    ("AUS Eastern Standard Time", "Australia/Sydney"),
    ("Tasmania Standard Time", "Australia/Hobart"),
    ("West Pacific Standard Time", "Pacific/Port_Moresby"),
    ("New Zealand Standard Time", "Pacific/Auckland"),
];

/// IANA zone for a Windows zone name, if it is one we know.
pub fn iana(windows: &str) -> Option<&'static str> {
    let name = windows.trim();
    ZONES
        .iter()
        .find(|(w, _)| w.eq_ignore_ascii_case(name))
        .map(|(_, iana)| *iana)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_zone_parses() {
        for (w, iana) in ZONES {
            assert!(iana.parse::<chrono_tz::Tz>().is_ok(), "{w} → {iana}");
        }
    }

    #[test]
    fn maps_outlook_names() {
        assert_eq!(iana("Pacific Standard Time"), Some("America/Los_Angeles"));
        assert_eq!(iana("eastern standard time"), Some("America/New_York"));
        assert_eq!(iana("America/Chicago"), None);
    }
}
