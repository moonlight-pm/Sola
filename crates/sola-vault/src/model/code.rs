//! A one-time code as the record shows it: grouped, with its time left.

use sola_vault_proto::{ItemRecord, remaining_secs};

/// The ring turns warning in the code's last seconds.
pub const WARN_SECS: u32 = 5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeNow {
    pub code: String,
    pub period: u32,
    pub left: u32,
}

impl CodeNow {
    pub fn of(record: &ItemRecord, unix: u64) -> Option<Self> {
        let (code, period) = record.totp_code_at(unix)?;
        Some(Self {
            left: remaining_secs(period, unix),
            code,
            period,
        })
    }

    pub fn warning(&self) -> bool {
        self.left <= WARN_SECS
    }
}

/// "482913" → "482 913"; eight digits in fours; others as they are.
pub fn grouped(code: &str) -> String {
    let n = code.chars().count();
    let size = match n {
        6 | 9 => 3,
        8 => 4,
        _ => return code.to_string(),
    };
    let chars: Vec<char> = code.chars().collect();
    chars
        .chunks(size)
        .map(|c| c.iter().collect::<String>())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_group_for_reading() {
        assert_eq!(grouped("482913"), "482 913");
        assert_eq!(grouped("12345678"), "1234 5678");
        assert_eq!(grouped("12345"), "12345");
    }
}
