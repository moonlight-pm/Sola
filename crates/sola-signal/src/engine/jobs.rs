//! What is fetched beside the conversation itself: profiles, group states,
//! the phone's contacts, attachments. One at a time and off the engine's
//! loop, so a slow answer holds nothing up; each result goes into the
//! store and the engine is told what changed.

use std::rc::Rc;
use std::time::Duration;

use anyhow::{Context, Result};
use prost::Message;
use tokio::sync::mpsc;

use super::inbox::{self, Effects};
use super::{Input, Shared, files, groups, outgoing, profile};
use crate::proto::AttachmentPointer;
use crate::store::chats::{Fetch, Id};
use crate::store::{Store, now};

/// A profile is not asked for again within this.
const FRESH: u64 = 3600 * 1000;
/// And is asked for again after this.
const STALE: u64 = 7 * 24 * 3600 * 1000;

pub enum Job {
    Profile(Id),
    Group(Id),
    Contacts(Box<AttachmentPointer>),
    /// The account's record of contacts and groups on Signal's storage service.
    Storage,
    /// Queue the profiles that have not been looked at for long.
    Stale,
}

pub fn start(shared: Rc<Shared>, inputs: mpsc::UnboundedSender<Input>) -> mpsc::UnboundedSender<Job> {
    let (jobs, mut waiting) = mpsc::unbounded_channel();
    let more = jobs.clone();
    tokio::task::spawn_local(async move {
        while let Some(job) = waiting.recv().await {
            match run(&shared, job, &more).await {
                Ok(effects) => drop(inputs.send(Input::Done(effects))),
                Err(e) => tracing::warn!("{e:#}"),
            }
        }
    });
    jobs
}

async fn run(shared: &Shared, job: Job, more: &mpsc::UnboundedSender<Job>) -> Result<Effects> {
    let store = &shared.store;
    let mut fx = Effects::default();
    match job {
        Job::Profile(id) => {
            let recipient = store.recipient(id)?;
            let (Some(aci), Some(key)) = (recipient.aci, recipient.profile_key) else { return Ok(fx) };
            if recipient.profile_at + FRESH > now() {
                return Ok(fx);
            }
            let wire = shared.wire.borrow().clone().context("not connected")?;
            let name = profile::name(&wire, aci, &key).await.context("a profile")?;
            store.set_profile_name(id, name.as_deref(), now())?;
            fx.people = true;
            // Signal limits how fast profiles may be asked for.
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        Job::Stale => {
            let old = now().saturating_sub(STALE);
            for recipient in store.recipients()? {
                if recipient.profile_key.is_some() && recipient.profile_at < old {
                    let _ = more.send(Job::Profile(recipient.id));
                }
            }
        }
        Job::Group(id) => {
            let conversation = group(shared, id, more).await.context("a group")?;
            fx.changed.insert(conversation);
            fx.people = true;
        }
        Job::Contacts(pointer) => {
            let blob = files::download(&pointer).await.context("the phone's contacts")?;
            for contact in files::contacts(&blob)? {
                let Some(aci) = inbox::aci(contact.aci_binary.as_deref(), contact.aci.as_deref()) else { continue };
                let id = store.recipient_for(aci)?;
                store.set_contact(id, contact.number.as_deref(), contact.name.as_deref())?;
            }
            fx.people = true;
        }
        Job::Storage => return super::storage::sync(shared, more).await.context("the storage service"),
    }
    Ok(fx)
}

/// Read a group's state from the server into the store. Answers with its
/// conversation.
pub async fn group(shared: &Shared, id: Id, more: &mpsc::UnboundedSender<Job>) -> Result<Id> {
    let store = &shared.store;
    let state = groups::fetch(shared, &store.group(id)?.master_key).await?;
    let mut members = Vec::new();
    for (aci, key) in &state.members {
        let member = store.recipient_for(*aci)?;
        if let Some(key) = key
            && member != shared.me.get()
            && store.set_profile_key(member, key)?
        {
            let _ = more.send(Job::Profile(member));
        }
        members.push(member);
    }
    store.set_group(id, state.revision, &state.title, &members)?;
    let conversation = store.conversation_in(id)?;
    store.set_timer(conversation, state.timer, 1)?;
    Ok(conversation)
}

/// Fetch an attachment's file, by itself: files do not wait for each other.
pub fn fetch(shared: Rc<Shared>, inputs: mpsc::UnboundedSender<Input>, attachment: Id) {
    tokio::task::spawn_local(async move {
        let store = &shared.store;
        let fetched = async {
            let attachment = store.attachment(attachment)?;
            if attachment.state == Fetch::Fetching || attachment.state == Fetch::Here {
                return Ok(None);
            }
            let (conversation, ..) = store.message_address(attachment.message)?;
            let pointer = attachment.pointer.as_deref().context("an attachment with nothing to fetch it by")?;
            let pointer = AttachmentPointer::decode(pointer).context("attachment")?;
            store.set_attachment(attachment.id, Fetch::Fetching, None)?;
            let mut started = Effects::default();
            started.changed.insert(conversation);
            let _ = inputs.send(Input::Done(started));

            let kept = match files::download(&pointer).await.and_then(|plain| keep(&attachment.content_type, &plain)) {
                Ok(path) => store.set_attachment(attachment.id, Fetch::Here, Some(&path)),
                Err(e) => {
                    tracing::warn!("an attachment was not fetched: {e:#}");
                    store.set_attachment(attachment.id, Fetch::Failed, None)
                }
            };
            kept.map(|()| Some(conversation))
        };
        let fetched: Result<Option<Id>> = fetched.await;
        match fetched {
            Ok(Some(conversation)) => {
                let mut fx = Effects::default();
                fx.changed.insert(conversation);
                let _ = inputs.send(Input::Done(fx));
            }
            Ok(None) => {}
            Err(e) => tracing::warn!("an attachment: {e:#}"),
        }
    });
}

/// Write a fetched file beside the store, readable by its owner only.
fn keep(content_type: &str, plain: &[u8]) -> Result<String> {
    use std::os::unix::fs::OpenOptionsExt;
    let dir = Store::files_dir();
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.{}", uuid::Uuid::new_v4(), outgoing::extension(content_type)));
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&path)?;
    std::io::Write::write_all(&mut file, plain)?;
    Ok(path.to_string_lossy().into_owned())
}
