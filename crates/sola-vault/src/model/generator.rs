//! The generator's options as the sheet edits them, kept in Bitwarden's
//! bounds, and the line that says what a result is made of.

use sola_vault_proto::{GeneratorKind, GeneratorOptions};

pub const LENGTH: (u8, u8) = (5, 128);
pub const WORDS: (u8, u8) = (3, 20);

/// Options a server would take: a length in range, at least one set,
/// minimums that fit.
pub fn clamp(mut o: GeneratorOptions) -> GeneratorOptions {
    o.length = o.length.clamp(LENGTH.0, LENGTH.1);
    o.words = o.words.clamp(WORDS.0, WORDS.1);
    if !(o.lowercase || o.uppercase || o.numbers || o.special) {
        o.lowercase = true;
    }
    o.min_numbers = if o.numbers { o.min_numbers.min(9) } else { 0 };
    o.min_special = if o.special { o.min_special.min(9) } else { 0 };
    let floor = o.min_numbers.saturating_add(o.min_special);
    if o.length < floor {
        o.length = floor.max(LENGTH.0);
    }
    o
}

/// "16 characters, letters, digits and symbols"; "4 words, a hyphen
/// between"; "A username".
pub fn describe(o: &GeneratorOptions) -> String {
    match o.kind {
        GeneratorKind::Password => {
            let mut sets: Vec<&str> = Vec::new();
            match (o.lowercase, o.uppercase) {
                (true, true) => sets.push("letters"),
                (true, false) => sets.push("lower-case letters"),
                (false, true) => sets.push("capitals"),
                (false, false) => {}
            }
            if o.numbers {
                sets.push("digits");
            }
            if o.special {
                sets.push("symbols");
            }
            format!("{} characters, {}", o.length, and_list(&sets))
        }
        GeneratorKind::Passphrase => {
            let sep = match o.separator.as_str() {
                "-" => "a hyphen".to_string(),
                " " => "a space".to_string(),
                "" => "nothing".to_string(),
                s => format!("“{s}”"),
            };
            let mut s = format!("{} words, {sep} between", o.words);
            if o.capitalize {
                s.push_str(", capitalized");
            }
            if o.include_number {
                s.push_str(", with a number");
            }
            s
        }
        GeneratorKind::Username => "A random word".into(),
    }
}

fn and_list(parts: &[&str]) -> String {
    match parts {
        [] => String::new(),
        [one] => (*one).to_string(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounds_and_one_set_at_least() {
        let o = clamp(GeneratorOptions {
            length: 2,
            words: 40,
            lowercase: false,
            uppercase: false,
            numbers: false,
            special: false,
            ..GeneratorOptions::default()
        });
        assert_eq!(o.length, 5);
        assert_eq!(o.words, 20);
        assert!(o.lowercase);
        assert_eq!(o.min_numbers, 0);
    }

    #[test]
    fn minimums_fit_the_length() {
        let o = clamp(GeneratorOptions {
            length: 5,
            min_numbers: 4,
            min_special: 4,
            ..GeneratorOptions::default()
        });
        assert_eq!(o.length, 8);
    }

    #[test]
    fn says_what_it_makes() {
        let o = GeneratorOptions::default();
        assert_eq!(describe(&o), "16 characters, letters, digits and symbols");
        let p = GeneratorOptions {
            kind: GeneratorKind::Passphrase,
            capitalize: true,
            ..GeneratorOptions::default()
        };
        assert_eq!(describe(&p), "4 words, a hyphen between, capitalized");
    }
}
