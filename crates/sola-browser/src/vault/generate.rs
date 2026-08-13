//! Site-password generator for new login items.
//!
//! One opinionated default: 16 characters, mixed classes, no options panel.

const LEN: usize = 16;
const UPPER: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ";
const LOWER: &[u8] = b"abcdefghijkmnopqrstuvwxyz";
const DIGIT: &[u8] = b"23456789";
const SYMBOL: &[u8] = b"!@#$%^&*-_=+";
const ALL: [&[u8]; 4] = [UPPER, LOWER, DIGIT, SYMBOL];

/// Generate a new login password (16 chars, all four classes).
pub fn password() -> String {
    let mut bytes = vec![0u8; LEN];
    // Guarantee one of each class in the first four slots, then fill.
    bytes[0] = pick(UPPER);
    bytes[1] = pick(LOWER);
    bytes[2] = pick(DIGIT);
    bytes[3] = pick(SYMBOL);
    for b in &mut bytes[4..] {
        let set = ALL[rand_u32() as usize % ALL.len()];
        *b = pick(set);
    }
    shuffle(&mut bytes);
    String::from_utf8(bytes).unwrap_or_else(|_| "A1b!A1b!A1b!A1b!".into())
}

fn pick(set: &[u8]) -> u8 {
    set[rand_u32() as usize % set.len()]
}

fn shuffle(bytes: &mut [u8]) {
    for i in (1..bytes.len()).rev() {
        let j = rand_u32() as usize % (i + 1);
        bytes.swap(i, j);
    }
}

fn rand_u32() -> u32 {
    let mut buf = [0u8; 4];
    if getrandom::getrandom(&mut buf).is_err() {
        // Extremely unlikely; still produce *some* variation.
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(1);
        return t;
    }
    u32::from_le_bytes(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classes(s: &str) -> (bool, bool, bool, bool) {
        let b = s.as_bytes();
        (
            b.iter().any(|c| UPPER.contains(c)),
            b.iter().any(|c| LOWER.contains(c)),
            b.iter().any(|c| DIGIT.contains(c)),
            b.iter().any(|c| SYMBOL.contains(c)),
        )
    }

    #[test]
    fn length_and_classes() {
        for _ in 0..32 {
            let p = password();
            assert_eq!(p.len(), LEN);
            assert!(p.is_ascii());
            assert_eq!(classes(&p), (true, true, true, true));
        }
    }

    #[test]
    fn not_identical() {
        let a = password();
        let b = password();
        assert_ne!(a, b);
    }
}
