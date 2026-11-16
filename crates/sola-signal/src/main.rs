//! Build proof: links libsignal's protocol, network and provisioning crates.

use libsignal_protocol::IdentityKeyPair;

fn main() {
    let identity = IdentityKeyPair::generate(&mut rand::rng());
    let link = libsignal_provisioning::RecvProvisionCiphertextState::generate(&mut rand::rng());
    println!(
        "identity {} bytes, link key {} bytes, chat host {}",
        identity.public_key().serialize().len(),
        link.public_key().len(),
        libsignal_net::env::PROD.chat_domain_config.connect.hostname,
    );
}
