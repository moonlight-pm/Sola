//! Passwords, passphrases and usernames, by the SDK's generator.

use bitwarden_generators::{
    GeneratorClientsExt, PassphraseGeneratorRequest, PasswordGeneratorRequest, UsernameGeneratorRequest,
};
use sola_vault_proto::{ErrorCode, GeneratorKind, GeneratorOptions};

use super::errors::{Result, err};

/// One value as `options` ask. Needs no account: a client of its own.
pub async fn generate(options: &GeneratorOptions) -> Result<String> {
    let client = bitwarden_core::Client::new(None);
    let generator = client.generator();
    let bad = |e: &dyn std::fmt::Display| err(ErrorCode::Invalid, e.to_string());
    match options.kind {
        GeneratorKind::Password => generator
            .password(password_request(options))
            .map_err(|e| bad(&e)),
        GeneratorKind::Passphrase => generator
            .passphrase(PassphraseGeneratorRequest {
                num_words: options.words.clamp(3, 20),
                word_separator: options.separator.clone(),
                capitalize: options.capitalize,
                include_number: options.include_number,
            })
            .map_err(|e| bad(&e)),
        GeneratorKind::Username => generator
            .username(UsernameGeneratorRequest::Word {
                capitalize: options.capitalize,
                include_number: options.include_number,
            })
            .await
            .map_err(|e| bad(&e)),
    }
}

fn password_request(o: &GeneratorOptions) -> PasswordGeneratorRequest {
    let any = o.lowercase || o.uppercase || o.numbers || o.special;
    PasswordGeneratorRequest {
        // Nothing chosen: letters, as Bitwarden falls back to.
        lowercase: o.lowercase || !any,
        uppercase: o.uppercase,
        numbers: o.numbers,
        special: o.special,
        length: o.length.clamp(5, 128),
        avoid_ambiguous: o.avoid_ambiguous,
        min_lowercase: None,
        min_uppercase: None,
        min_number: o.numbers.then_some(o.min_numbers),
        min_special: o.special.then_some(o.min_special),
        ..PasswordGeneratorRequest::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_defaults_make_a_16_character_password() {
        let p = generate(&GeneratorOptions::default()).await.unwrap();
        assert_eq!(p.chars().count(), 16);
        assert_ne!(p, generate(&GeneratorOptions::default()).await.unwrap());
    }

    #[tokio::test]
    async fn passphrases_and_usernames() {
        let o = GeneratorOptions {
            kind: GeneratorKind::Passphrase,
            words: 5,
            separator: "-".into(),
            ..Default::default()
        };
        assert_eq!(generate(&o).await.unwrap().split('-').count(), 5);
        let o = GeneratorOptions {
            kind: GeneratorKind::Username,
            ..Default::default()
        };
        assert!(!generate(&o).await.unwrap().is_empty());
    }
}
