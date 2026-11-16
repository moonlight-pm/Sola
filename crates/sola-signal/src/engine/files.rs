//! Attachments: files on Signal's file servers, each encrypted under a key
//! of its own that travels in the message (AES-256-CBC, then HMAC-SHA256).

use anyhow::{Context, Result, anyhow, bail};
use prost::Message;
use rand::Rng;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use signal_crypto::{CryptographicMac, aes_256_cbc_decrypt, aes_256_cbc_encrypt};

use super::Shared;
use crate::http;
use crate::net;
use crate::proto::{AttachmentPointer, attachment_pointer::AttachmentIdentifier};
use crate::send::Failure;
use crate::store::chats::Attachment;
use crate::store::now;

/// Signal's file servers, by the number a pointer names.
const SERVERS: [&str; 4] = ["cdn.signal.org", "cdn.signal.org", "cdn2.signal.org", "cdn3.signal.org"];
const IV: usize = 16;
const MAC: usize = 32;

fn mac(key: &[u8], data: &[u8]) -> Result<Vec<u8>> {
    let mut mac = CryptographicMac::new("HmacSha256", key).map_err(|e| anyhow!("hmac: {e}"))?;
    mac.update(data);
    Ok(mac.finalize())
}

/// Fetch and decrypt what a pointer points to.
pub async fn download(pointer: &AttachmentPointer) -> Result<Vec<u8>> {
    let path = match pointer.attachment_identifier.as_ref().context("a pointer to nothing")? {
        AttachmentIdentifier::CdnId(id) => id.to_string(),
        AttachmentIdentifier::CdnKey(key) => key.clone(),
    };
    let server = SERVERS.get(pointer.cdn_number() as usize).copied().unwrap_or(SERVERS[0]);
    let headers = vec![("Content-Type".to_owned(), "application/octet-stream".to_owned())];
    let sealed = http::send("GET", format!("https://{server}/attachments/{path}"), headers, None).await?.ok("attachment")?;
    open(pointer.key(), pointer.digest.as_deref(), pointer.size() as usize, &sealed)
}

/// Check and decrypt a fetched file: its digest, its MAC, then the cipher
/// and the padding after `size`.
pub fn open(keys: &[u8], digest: Option<&[u8]>, size: usize, sealed: &[u8]) -> Result<Vec<u8>> {
    if keys.len() != 64 {
        bail!("an attachment key that is not 64 bytes");
    }
    if sealed.len() < IV + MAC {
        bail!("an attachment too short to be one");
    }
    if let Some(digest) = digest.filter(|digest| !digest.is_empty())
        && Sha256::digest(sealed).as_slice() != digest
    {
        bail!("an attachment that is not what its message says (digest)");
    }
    let (body, tag) = sealed.split_at(sealed.len() - MAC);
    if mac(&keys[32..], body)? != tag {
        bail!("an attachment that was changed on the way (MAC)");
    }
    let mut plain = aes_256_cbc_decrypt(&body[IV..], &keys[..32], &body[..IV]).map_err(|e| anyhow!("attachment: {e}"))?;
    if plain.len() < size {
        bail!("an attachment shorter than its message says");
    }
    if size > 0 {
        plain.truncate(size);
    }
    Ok(plain)
}

/// Encrypt a file to upload: its keys, the bytes to store, their digest.
pub fn seal(plain: &[u8]) -> Result<(Vec<u8>, Vec<u8>, Vec<u8>)> {
    let mut keys = vec![0u8; 64];
    let mut iv = [0u8; IV];
    rand::rng().fill(keys.as_mut_slice());
    rand::rng().fill(&mut iv);
    // Padded up to a step of 5 %, so the size says little about the file.
    let steps = ((plain.len().max(1) as f64).ln() / 1.05f64.ln()).ceil();
    let padded = (1.05f64.powf(steps).floor() as usize).max(541).max(plain.len());
    let mut body = plain.to_vec();
    body.resize(padded, 0);
    let mut sealed = iv.to_vec();
    sealed.extend(aes_256_cbc_encrypt(&body, &keys[..32], &iv).map_err(|e| anyhow!("attachment: {e}"))?);
    let tag = mac(&keys[32..], &sealed)?;
    sealed.extend(tag);
    let digest = Sha256::digest(&sealed).to_vec();
    Ok((keys, sealed, digest))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Form {
    cdn: u32,
    key: String,
    headers: std::collections::HashMap<String, String>,
    signed_upload_location: String,
}

/// Upload a file of ours; the pointer is what recipients fetch it by.
pub async fn upload(shared: &Shared, attachment: &Attachment) -> Result<AttachmentPointer, Failure> {
    let never = Failure::Never;
    let path = attachment.path.as_deref().context("an attachment with no file").map_err(never)?;
    let plain = std::fs::read(path).with_context(|| path.to_owned()).map_err(never)?;
    let (keys, sealed, digest) = seal(&plain).map_err(never)?;

    let wire = shared.wire.borrow().clone().ok_or_else(|| Failure::Later(anyhow!("not connected")))?;
    let ask = format!("/v4/attachments/form/upload?uploadLength={}", sealed.len());
    let form = match net::request(&wire.chat, ::http::Method::GET, &ask, None, None).await {
        Ok(response) => response,
        Err(e @ net::Refused::Status { .. }) => return Err(never(e.into())),
        Err(e) => return Err(Failure::Later(e.into())),
    };
    let form: Form = serde_json::from_slice(form.body.as_deref().unwrap_or_default()).context("upload form").map_err(never)?;

    let later = |e: anyhow::Error| Failure::Later(e);
    let mut headers: Vec<(String, String)> = form.headers.into_iter().collect();
    if form.cdn == 3 {
        // One request of the tus protocol: create the upload with its bytes.
        use base64::Engine;
        headers.push(("Tus-Resumable".into(), "1.0.0".into()));
        headers.push(("Upload-Length".into(), sealed.len().to_string()));
        let name = base64::engine::general_purpose::STANDARD.encode(&form.key);
        headers.push(("Upload-Metadata".into(), format!("filename {name}")));
        headers.push(("Content-Type".into(), "application/offset+octet-stream".into()));
        http::send("POST", form.signed_upload_location, headers, Some(sealed)).await.map_err(later)?.ok("upload").map_err(never)?;
    } else {
        // Ask where to put it, then put it there.
        headers.push(("Content-Type".into(), "application/octet-stream".into()));
        let begun = http::send("POST", form.signed_upload_location, headers, None).await.map_err(later)?;
        let location = begun.location.clone();
        begun.ok("upload").map_err(never)?;
        let location = location.context("the file server named no place to upload to").map_err(never)?;
        let headers = vec![("Content-Type".to_owned(), "application/octet-stream".to_owned())];
        http::send("PUT", location, headers, Some(sealed)).await.map_err(later)?.ok("upload").map_err(never)?;
    }

    Ok(AttachmentPointer {
        attachment_identifier: Some(AttachmentIdentifier::CdnKey(form.key)),
        cdn_number: Some(form.cdn),
        content_type: Some(attachment.content_type.clone()),
        key: Some(keys),
        size: Some(plain.len() as u32),
        digest: Some(digest),
        file_name: attachment.file_name.clone(),
        width: (attachment.width > 0).then_some(attachment.width),
        height: (attachment.height > 0).then_some(attachment.height),
        upload_timestamp: Some(now()),
        ..Default::default()
    })
}

/// The phone's address book as it sends it: each contact a length, a
/// `ContactDetails`, then the bytes of its picture if it has one.
pub fn contacts(mut blob: &[u8]) -> Result<Vec<crate::proto::ContactDetails>> {
    let mut contacts = Vec::new();
    while !blob.is_empty() {
        let length = prost::encoding::decode_varint(&mut blob).context("contacts")? as usize;
        if length > blob.len() {
            bail!("contacts: a record longer than what is left");
        }
        let contact = crate::proto::ContactDetails::decode(&blob[..length]).context("contacts")?;
        blob = &blob[length..];
        let picture = contact.avatar.as_ref().map_or(0, |avatar| avatar.length() as usize);
        blob = blob.get(picture..).context("contacts: a picture longer than what is left")?;
        contacts.push(contact);
    }
    Ok(contacts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_is_sealed_opens_and_a_changed_byte_does_not() {
        let plain: Vec<u8> = (0..2000u32).map(|n| n as u8).collect();
        let (keys, mut sealed, digest) = seal(&plain).unwrap();
        assert_eq!(open(&keys, Some(&digest), plain.len(), &sealed).unwrap(), plain);
        // Without the digest the MAC still catches a change.
        sealed[40] ^= 1;
        assert!(open(&keys, Some(&digest), plain.len(), &sealed).is_err());
        assert!(open(&keys, None, plain.len(), &sealed).is_err());
    }

    #[test]
    fn a_small_file_is_padded_to_the_least_size() {
        let (_, sealed, _) = seal(b"hello").unwrap();
        // 541 bytes of padded plaintext, to a whole cipher block more, with the IV and MAC.
        assert_eq!(sealed.len(), IV + 544 + MAC);
    }
}
