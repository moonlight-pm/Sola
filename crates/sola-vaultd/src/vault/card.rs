//! How a card reads in a list: brand, last digits, expiry.

/// Last 4 digits, or 5 for Amex (34/37). `None` if the number is too short.
pub fn last4(number: Option<&str>) -> Option<String> {
    let digits: String = number?.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() < 4 {
        return None;
    }
    let take = if digits.starts_with("34") || digits.starts_with("37") {
        5.min(digits.len())
    } else {
        4
    };
    Some(digits[digits.len() - take..].to_string())
}

pub fn subtitle(brand: Option<&str>, last4: Option<&str>) -> String {
    match (
        brand.filter(|s| !s.is_empty()),
        last4.filter(|s| !s.is_empty()),
    ) {
        (Some(b), Some(n)) => format!("{b} · •••• {n}"),
        (Some(b), None) => b.to_string(),
        (None, Some(n)) => format!("•••• {n}"),
        (None, None) => String::new(),
    }
}

/// `MM/YY`, when both parts are there.
pub fn exp_display(month: Option<&str>, year: Option<&str>) -> Option<String> {
    let m = month.map(str::trim).filter(|s| !s.is_empty())?;
    let y = year.map(str::trim).filter(|s| !s.is_empty())?;
    let mm = if m.len() == 1 { format!("0{m}") } else { m.to_string() };
    let yy = if y.len() >= 4 { &y[y.len() - 2..] } else { y };
    Some(format!("{mm}/{yy}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn last4_visa_and_amex() {
        assert_eq!(last4(Some("4111111111111111")).as_deref(), Some("1111"));
        assert_eq!(last4(Some("3782 822463 10005")).as_deref(), Some("10005"));
        assert_eq!(last4(Some("12")), None);
        assert_eq!(last4(None), None);
    }

    #[test]
    fn subtitle_and_exp() {
        assert_eq!(subtitle(Some("Visa"), Some("1111")), "Visa · •••• 1111");
        assert_eq!(subtitle(None, Some("4444")), "•••• 4444");
        assert_eq!(exp_display(Some("3"), Some("2028")).as_deref(), Some("03/28"));
        assert_eq!(exp_display(Some("12"), Some("28")).as_deref(), Some("12/28"));
        assert_eq!(exp_display(Some("12"), None), None);
    }
}
