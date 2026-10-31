//! What names an item in a row: a login's first letter on an enamel hashed
//! from its name, or a grey plate with its kind's glyph. A plate names a
//! thing; it never reports a state.

use sola_vault_proto::ItemKind;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plate {
    /// The letter, and the seed its enamel is hashed from.
    Letter { letter: char, seed: String },
    /// A lucide icon on the grey plate.
    Glyph(&'static str),
}

pub fn plate(kind: ItemKind, name: &str) -> Plate {
    match kind {
        ItemKind::Login => Plate::Letter {
            letter: letter(name),
            seed: seed(name),
        },
        other => Plate::Glyph(kind_icon(other)),
    }
}

/// A kind's glyph (plates, the sidebar, New Item).
pub fn kind_icon(kind: ItemKind) -> &'static str {
    match kind {
        ItemKind::Login => "lucide/globe",
        ItemKind::Card => "lucide/credit-card",
        ItemKind::Identity => "lucide/user",
        ItemKind::SecureNote => "lucide/sticky-note",
        ItemKind::SshKey => "lucide/key-round",
        ItemKind::BankAccount => "lucide/landmark",
        ItemKind::DriversLicense => "lucide/id-card",
        ItemKind::Passport => "lucide/book-open",
    }
}

/// The first letter or digit, upper case; `?` for a name without one.
pub fn letter(name: &str) -> char {
    name.chars()
        .find(|c| c.is_alphanumeric())
        .map(|c| c.to_uppercase().next().unwrap_or(c))
        .unwrap_or('?')
}

/// The name's first word, lower case: "GitHub" and "GitHub — work" share
/// a plate, as one site's logins should.
pub fn seed(name: &str) -> String {
    name.split(|c: char| !c.is_alphanumeric())
        .find(|w| !w.is_empty())
        .unwrap_or(name)
        .to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logins_get_a_letter_others_a_glyph() {
        assert_eq!(
            plate(ItemKind::Login, "github"),
            Plate::Letter {
                letter: 'G',
                seed: "github".into()
            }
        );
        assert_eq!(plate(ItemKind::Card, "Visa"), Plate::Glyph("lucide/credit-card"));
        assert_eq!(plate(ItemKind::SecureNote, "x"), Plate::Glyph("lucide/sticky-note"));
    }

    #[test]
    fn one_site_one_plate() {
        assert_eq!(seed("GitHub"), seed("GitHub — work"));
        assert_ne!(seed("Google Cloud"), seed("GitHub"));
        let a = sola_kit::components::select::enamel(&seed("GitHub"));
        let b = sola_kit::components::select::enamel(&seed("github (old)"));
        assert_eq!(a, b);
    }

    #[test]
    fn the_letter_skips_punctuation() {
        assert_eq!(letter("  (old) bank"), 'O');
        assert_eq!(letter("1password"), '1');
        assert_eq!(letter("—"), '?');
        assert_eq!(letter("élan"), 'É');
    }
}
