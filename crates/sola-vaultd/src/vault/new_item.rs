//! New, empty items ready to fill and encrypt.

use bitwarden_vault::{
    BankAccountView, CardView, CipherRepromptType, CipherView, DriversLicenseView, IdentityView,
    LoginUriView, LoginView, PassportView, SecureNoteType, SecureNoteView, SshKeyView, UriMatchType,
};
use sola_vault_proto::ItemKind;

use super::convert;

/// A personal item of `kind` with every field empty and no id.
pub fn new_view(kind: ItemKind) -> CipherView {
    let now = chrono::Utc::now();
    let mut view = CipherView {
        id: None,
        organization_id: None,
        folder_id: None,
        collection_ids: Vec::new(),
        key: None,
        name: kind.label().to_string(),
        notes: None,
        r#type: convert::cipher_type(kind),
        login: None,
        identity: None,
        card: None,
        secure_note: None,
        ssh_key: None,
        bank_account: None,
        drivers_license: None,
        passport: None,
        favorite: false,
        reprompt: CipherRepromptType::None,
        organization_use_totp: false,
        edit: true,
        permissions: None,
        view_password: true,
        local_data: None,
        attachments: None,
        attachment_decryption_failures: None,
        fields: None,
        password_history: None,
        creation_date: now,
        deleted_date: None,
        revision_date: now,
        archived_date: None,
    };
    match kind {
        ItemKind::Login => view.login = Some(empty_login()),
        ItemKind::Card => view.card = Some(empty_card()),
        ItemKind::Identity => view.identity = Some(empty_identity()),
        ItemKind::SecureNote => {
            view.secure_note = Some(SecureNoteView {
                r#type: SecureNoteType::Generic,
            })
        }
        ItemKind::SshKey => {
            view.ssh_key = Some(SshKeyView {
                private_key: String::new(),
                public_key: String::new(),
                fingerprint: String::new(),
            })
        }
        ItemKind::BankAccount => view.bank_account = Some(BankAccountView::default()),
        ItemKind::DriversLicense => view.drivers_license = Some(DriversLicenseView::default()),
        ItemKind::Passport => view.passport = Some(PassportView::default()),
    }
    view
}

/// A personal login (a passkey's new home): `name` or "Login", and a
/// Domain website when given.
pub fn new_login_view(
    name: String,
    username: Option<String>,
    password: Option<String>,
    uri: Option<String>,
) -> CipherView {
    let mut view = new_view(ItemKind::Login);
    let name = name.trim();
    if !name.is_empty() {
        view.name = name.to_string();
    }
    let login = view.login.as_mut().expect("a login");
    login.username = username.filter(|s| !s.is_empty());
    login.password = password.filter(|s| !s.is_empty());
    if let Some(uri) = uri.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
        login.uris = Some(vec![LoginUriView {
            uri: Some(uri),
            r#match: Some(UriMatchType::Domain),
            uri_checksum: None,
        }]);
    }
    login.generate_checksums();
    view
}

pub fn empty_login() -> LoginView {
    LoginView {
        username: None,
        password: None,
        password_revision_date: None,
        uris: None,
        totp: None,
        autofill_on_page_load: None,
        fido2_credentials: None,
    }
}

pub fn empty_card() -> CardView {
    CardView {
        cardholder_name: None,
        exp_month: None,
        exp_year: None,
        code: None,
        brand: None,
        number: None,
    }
}

pub fn empty_identity() -> IdentityView {
    IdentityView {
        title: None,
        first_name: None,
        middle_name: None,
        last_name: None,
        address1: None,
        address2: None,
        address3: None,
        city: None,
        state: None,
        postal_code: None,
        country: None,
        company: None,
        email: None,
        phone: None,
        ssn: None,
        username: None,
        passport_number: None,
        license_number: None,
    }
}
