//! An invented account with invented conversations, to look at the window
//! with no one's messages in it (`sola-signal demo`, then run with
//! `SOLA_SIGNAL_OFFLINE=1`). Refuses a store that holds a real account.

use anyhow::{Result, bail};
use libsignal_core::{Aci, DeviceId};
use libsignal_protocol::IdentityKeyPair;

use crate::account::{Account, Identity, IdentityKeys};
use crate::link::{Linked, first_pre_keys};
use crate::store::chats::{Id, NewAttachment, NewMessage, Quote, Status};
use crate::store::{Store, now};

const MINUTE: u64 = 60_000;

pub fn seed(store: &Store) -> Result<()> {
    if store.account()?.is_some() {
        bail!("this store holds an account; the demo is for an empty one");
    }
    let keys = IdentityKeys { key_pair: IdentityKeyPair::generate(&mut rand::rng()), registration_id: 1 };
    let pre_keys = first_pre_keys(Identity::Aci, &keys.key_pair)?;
    let account = Account {
        aci: Aci::from(uuid::Uuid::new_v4()),
        device_id: DeviceId::new(2).expect("a device id"),
        password: "demo".into(),
        aci_keys: keys,
        pni: None,
        number: Some("+15550100".into()),
        profile_key: vec![0; 32],
        account_entropy_pool: None,
        media_root_backup_key: None,
        ephemeral_backup_key: None,
        read_receipts: true,
    };
    let me = store.recipient_for(account.aci)?;
    store.save_link(&Linked { account, pre_keys: vec![pre_keys] })?;
    store.set_contact(me, None, Some("You"))?;

    let person = |name: &str| -> Result<Id> {
        let id = store.recipient_for(Aci::from(uuid::Uuid::new_v4()))?;
        store.set_contact(id, None, Some(name))?;
        Ok(id)
    };
    let (ada, grace, linus, margaret) = (person("Ada Lovelace")?, person("Grace Hopper")?, person("Linus")?, person("Margaret Hamilton")?);
    let start = now() - 26 * 60 * MINUTE;
    let say = |conversation: Id, sender: Id, minutes: u64, body: &str, more: NewMessage| -> Result<Id> {
        let at = start + minutes * MINUTE;
        let outgoing = sender == me;
        let new = NewMessage {
            conversation,
            sender,
            sent_at: at,
            received_at: at,
            body: body.into(),
            outgoing,
            status: Some(more.status.unwrap_or(Status::Read)),
            unread: more.unread,
            quote: more.quote,
            attachments: more.attachments,
            note: more.note,
            ..Default::default()
        };
        Ok(store.add_message(&new)?.expect("a new message"))
    };
    let plain = NewMessage::default;

    let with_ada = store.conversation_with(ada)?;
    let first = say(with_ada, ada, 0, "Did the engine finish the table of Bernoulli numbers?", plain())?;
    say(with_ada, me, 3, "It did, last night. The notes are at https://example.org/notes if you want to check them.", plain())?;
    store.set_reaction(first, me, Some("👍"), start)?;
    let picture = picture()?;
    say(with_ada, ada, 1500, "", NewMessage { attachments: vec![picture], ..plain() })?;
    let quoted = say(with_ada, ada, 1501, "Here is the diagram. Tell me what you make of the loop in the middle.", plain())?;
    say(
        with_ada,
        me,
        1540,
        "The loop is the clever part: it feeds its own result back in.",
        NewMessage {
            quote: Some(Quote { sent_at: start + 1501 * MINUTE, author: ada, text: "Here is the diagram. Tell me what you make of the loop in the middle.".into() }),
            status: Some(Status::Delivered),
            ..plain()
        },
    )?;
    store.set_reaction(quoted, me, Some("❤️"), start)?;
    say(with_ada, ada, 1555, "Exactly so. Shall we write it up tomorrow?", NewMessage { unread: true, ..plain() })?;

    let group = store.group_for(&[7; 32], &[8; 32])?;
    store.set_group(group, 3, "Compiler Club", &[me, grace, linus, margaret])?;
    let club = store.conversation_in(group)?;
    say(club, grace, 1400, "I found an actual bug in the relay. A moth.", plain())?;
    let moth = say(club, linus, 1402, "Tape it into the log book.", plain())?;
    store.set_reaction(moth, grace, Some("😂"), start)?;
    store.set_reaction(moth, margaret, Some("😂"), start)?;
    say(club, margaret, 1410, "We should add a check before launch so that can never happen in flight.", plain())?;
    say(club, me, 1420, "Agreed. I will write the test.", NewMessage { status: Some(Status::Sent), ..plain() })?;
    say(club, grace, 1550, "Meeting moved to Thursday.", NewMessage { unread: true, ..plain() })?;
    say(club, grace, 1551, "Bring the log book.", NewMessage { unread: true, ..plain() })?;

    let with_linus = store.conversation_with(linus)?;
    store.set_timer(with_linus, 604_800, 2)?;
    say(with_linus, linus, 900, "Linus set messages to disappear after 1 week.", NewMessage { note: true, ..plain() })?;
    say(with_linus, me, 905, "Patch sent.", NewMessage { status: Some(Status::Failed), ..plain() })?;

    let myself = store.conversation_with(me)?;
    say(myself, me, 600, "Buy more punched cards.", plain())?;
    Ok(())
}

/// A small invented picture, so a thread has one to show.
fn picture() -> Result<NewAttachment> {
    let dir = Store::files_dir();
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("demo.png");
    let (width, height) = (480u32, 300u32);
    let pixels = image::RgbImage::from_fn(width, height, |x, y| {
        let ring = (((x as f32 - 240.0).hypot(y as f32 - 150.0) / 18.0).sin() * 60.0) as i32;
        image::Rgb([(70 + ring) as u8, (110 + x / 4) as u8, (150 + y / 3) as u8])
    });
    pixels.save(&path)?;
    Ok(NewAttachment {
        content_type: "image/png".into(),
        file_name: Some("diagram.png".into()),
        size: std::fs::metadata(&path)?.len(),
        width,
        height,
        flags: 0,
        pointer: None,
        path: Some(path.to_string_lossy().into_owned()),
    })
}
