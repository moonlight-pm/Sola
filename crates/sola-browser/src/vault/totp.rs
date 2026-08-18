//! TOTP from a Bitwarden login `totp` string (`otpauth://` or raw secret).
//!
//! Official defaults: SHA-1, 6 digits, 30 s. Steam (`steam://`) is ignored.

use hmac::{Hmac, Mac};
use sha1::Sha1;

type HmacSha1 = Hmac<Sha1>;

const DEFAULT_DIGITS: u32 = 6;
const DEFAULT_PERIOD: u32 = 30;

/// Parsed TOTP parameters (no secret after [`Self::code`] is called — the
/// secret lives only long enough to compute).
#[derive(Debug, Clone)]
pub struct TotpSpec {
    secret: Vec<u8>,
    pub digits: u32,
    pub period: u32,
}

impl TotpSpec {
    pub fn parse(raw: &str) -> Option<Self> {
        let t = raw.trim();
        if t.is_empty() || t.to_ascii_lowercase().starts_with("steam://") {
            return None;
        }
        if let Some(rest) = t.strip_prefix("otpauth://") {
            return parse_otpauth(rest);
        }
        let secret = decode_base32(t)?;
        Some(Self {
            secret,
            digits: DEFAULT_DIGITS,
            period: DEFAULT_PERIOD,
        })
    }

    pub fn code_at(&self, unix_secs: u64) -> String {
        let counter = unix_secs / u64::from(self.period.max(1));
        hotp(&self.secret, counter, self.digits.max(1).min(10))
    }

}

/// Wall-clock remaining seconds for a standard 30 s window (chrome display).
pub fn remaining_secs(period: u32, unix_secs: u64) -> u32 {
    let p = period.max(1);
    p - (unix_secs % u64::from(p)) as u32
}

fn parse_otpauth(rest: &str) -> Option<TotpSpec> {
    // totp/Label?secret=…  (ignore hotp)
    let after_type = rest.strip_prefix("totp/")?;
    let q = after_type.find('?')?;
    let query = &after_type[q + 1..];
    let mut secret = None;
    let mut digits = DEFAULT_DIGITS;
    let mut period = DEFAULT_PERIOD;
    for pair in query.split('&') {
        let (k, v) = pair.split_once('=')?;
        match k.to_ascii_lowercase().as_str() {
            "secret" => secret = decode_base32(&url_decode(v)),
            "digits" => {
                if let Ok(n) = v.parse::<u32>() {
                    digits = n;
                }
            }
            "period" => {
                if let Ok(n) = v.parse::<u32>() {
                    period = n;
                }
            }
            "algorithm" if !v.eq_ignore_ascii_case("sha1") => return None,
            _ => {}
        }
    }
    Some(TotpSpec {
        secret: secret?,
        digits,
        period,
    })
}

fn url_decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let hex = &s[i + 1..i + 3];
            if let Ok(v) = u8::from_str_radix(hex, 16) {
                out.push(v as char);
                i += 3;
                continue;
            }
        }
        out.push(b[i] as char);
        i += 1;
    }
    out
}

fn hotp(secret: &[u8], counter: u64, digits: u32) -> String {
    let mut mac = HmacSha1::new_from_slice(secret).expect("hmac key");
    mac.update(&counter.to_be_bytes());
    let hash = mac.finalize().into_bytes();
    let offset = (hash[19] & 0x0f) as usize;
    let bin = ((u32::from(hash[offset]) & 0x7f) << 24)
        | (u32::from(hash[offset + 1]) << 16)
        | (u32::from(hash[offset + 2]) << 8)
        | u32::from(hash[offset + 3]);
    let modulus = 10u32.pow(digits);
    format!("{:0width$}", bin % modulus, width = digits as usize)
}

/// RFC 4648 base32 (no padding required). Spaces / dashes ignored.
fn decode_base32(s: &str) -> Option<Vec<u8>> {
    const TBL: [i8; 256] = {
        let mut t = [-1i8; 256];
        let mut i = 0u8;
        while i < 26 {
            t[(b'A' + i) as usize] = i as i8;
            t[(b'a' + i) as usize] = i as i8;
            i += 1;
        }
        i = 0;
        while i < 6 {
            t[(b'2' + i) as usize] = (26 + i) as i8;
            i += 1;
        }
        t
    };
    let mut acc: u32 = 0;
    let mut bits: u32 = 0;
    let mut out = Vec::new();
    let mut saw = false;
    for &b in s.as_bytes() {
        if b == b' ' || b == b'-' || b == b'=' {
            continue;
        }
        let v = TBL[b as usize];
        if v < 0 {
            return None;
        }
        saw = true;
        acc = (acc << 5) | v as u32;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    if saw && !out.is_empty() {
        Some(out)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc4226_hotp_sha1() {
        // RFC 4226 appendix D — secret "12345678901234567890", counter 0 → 755224
        let secret = b"12345678901234567890";
        assert_eq!(hotp(secret, 0, 6), "755224");
        assert_eq!(hotp(secret, 1, 6), "287082");
    }

    #[test]
    fn remaining_wraps_on_period() {
        assert_eq!(remaining_secs(30, 0), 30);
        assert_eq!(remaining_secs(30, 29), 1);
        assert_eq!(remaining_secs(30, 30), 30);
    }

    #[test]
    fn parse_raw_secret_and_otpauth() {
        let raw = TotpSpec::parse("JBSWY3DPEHPK3PXP").expect("raw");
        assert_eq!(raw.period, 30);
        assert_eq!(raw.digits, 6);
        let url = TotpSpec::parse(
            "otpauth://totp/Example:user?secret=JBSWY3DPEHPK3PXP&issuer=Example&period=30&digits=6",
        )
        .expect("otpauth");
        assert_eq!(url.secret, raw.secret);
        assert!(TotpSpec::parse("steam://XXXX").is_none());
        assert!(TotpSpec::parse("").is_none());
    }

    #[test]
    fn same_window_same_code() {
        let spec = TotpSpec::parse("JBSWY3DPEHPK3PXP").unwrap();
        let a = spec.code_at(1_700_000_000);
        let b = spec.code_at(1_700_000_005);
        assert_eq!(a, b);
        let c = spec.code_at(1_700_000_030);
        assert_ne!(a, c);
    }
}
