//! `.ics` files opened from Mail or `xdg-open`. The file's event becomes a
//! new-event draft, so you pick a calendar you can edit before it saves.

use std::path::Path;

use chrono::Duration;

use crate::ics::{self, RawEvent};
use crate::model::{CalEvent, new_id};
use crate::timeutil::{pretty_date, today};

/// An event read from a file, not yet in any calendar.
pub struct Invite {
    pub event: CalEvent,
    /// Draft caption: the file, and anything that was left out.
    pub note: String,
}

pub fn read(path: &Path) -> Result<Invite, String> {
    let file = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    let bytes = std::fs::read(path).map_err(|e| format!("Can't read {file}: {e}"))?;
    parse(&String::from_utf8_lossy(&bytes), &file)
}

fn parse(text: &str, file: &str) -> Result<Invite, String> {
    if ics::method(text).is_some_and(|m| m.eq_ignore_ascii_case("CANCEL")) {
        return Err(format!("{file} cancels an event — nothing to add"));
    }
    let raws = ics::parse_vevents(text);
    // A series' own entry, not an override of one of its days.
    let raw = raws
        .iter()
        .find(|r| r.dtstart.is_some() && !r.recurrence_id)
        .or_else(|| raws.iter().find(|r| r.dtstart.is_some()))
        .ok_or_else(|| format!("No event in {file}"))?;
    if raw.cancelled {
        return Err(format!("{file} is a cancelled event — nothing to add"));
    }
    let event = to_event(raw).ok_or_else(|| format!("No event in {file}"))?;

    let mut note = format!("From {file}.");
    if raw.rrule.is_some() {
        let day = event.start_date.unwrap_or_else(|| {
            event.start.with_timezone(&chrono::Local).date_naive()
        });
        note.push_str(&format!(" It repeats; only {} is added.", pretty_date(day)));
    }
    let mut uids: Vec<&str> = raws.iter().filter_map(|r| r.uid.as_deref()).collect();
    uids.sort_unstable();
    uids.dedup();
    let others = uids.len().saturating_sub(1);
    if others > 0 {
        let s = if others == 1 { "" } else { "s" };
        note.push_str(&format!(" {others} more event{s} in the file are not added."));
    }
    Ok(Invite { event, note })
}

/// The one occurrence to draft: a series' next date from today (its first
/// if it has ended), else the event itself.
fn to_event(raw: &RawEvent) -> Option<CalEvent> {
    let start = raw.dtstart?.date;
    let next = raw.rrule.as_ref().and_then(|_| {
        let from = today();
        ics::to_cal_events(raw.clone(), "", None, None, (from, from + Duration::days(366)))
            .into_iter()
            .next()
    });
    let single = || {
        let once = RawEvent {
            rrule: None,
            ..raw.clone()
        };
        ics::to_cal_events(once, "", None, None, (start, start))
            .into_iter()
            .next()
    };
    let mut ev = next.or_else(single)?;
    ev.id = new_id("ev");
    ev.remote_id = None;
    ev.href = None;
    ev.etag = None;
    ev.read_only = false;
    ev.rrule = None;
    ev.master_id = None;
    Some(ev)
}

#[cfg(test)]
mod tests {
    use super::*;

    const INVITE: &str = "BEGIN:VCALENDAR\r\nMETHOD:REQUEST\r\nBEGIN:VEVENT\r\nUID:abc@google.com\r\nSUMMARY:Design review\r\nDESCRIPTION:Join: https://meet.example/x\r\nLOCATION:Room 4\r\nDTSTART:20261027T170000Z\r\nDTEND:20261027T180000Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

    #[test]
    fn invite_becomes_a_fresh_event() {
        let inv = parse(INVITE, "invite.ics").unwrap();
        assert_eq!(inv.event.title, "Design review");
        assert_eq!(inv.event.location, "Room 4");
        assert!(inv.event.notes.contains("meet.example"));
        assert_eq!(inv.event.end - inv.event.start, Duration::hours(1));
        assert!(inv.event.remote_id.is_none());
        assert!(!inv.event.read_only);
        assert_eq!(inv.note, "From invite.ics.");
    }

    #[test]
    fn cancellation_is_refused() {
        let cancel = INVITE.replace("METHOD:REQUEST", "METHOD:CANCEL");
        assert!(parse(&cancel, "invite.ics").is_err());
        let status = INVITE.replace("LOCATION:Room 4", "STATUS:CANCELLED");
        assert!(parse(&status, "invite.ics").is_err());
    }

    #[test]
    fn multi_day_all_day_keeps_its_span() {
        let ics = "BEGIN:VEVENT\r\nUID:c@x\r\nSUMMARY:Conference\r\nDTSTART;VALUE=DATE:20261102\r\nDTEND;VALUE=DATE:20261105\r\nEND:VEVENT\r\n";
        let ev = parse(ics, "conf.ics").unwrap().event;
        assert!(ev.all_day);
        let (s, e) = (ev.start_date.unwrap(), ev.end_date.unwrap());
        assert_eq!((e - s).num_days(), 3);
    }

    #[test]
    fn series_takes_one_date_and_says_so() {
        let ics = "BEGIN:VEVENT\r\nUID:s@x\r\nSUMMARY:Standup\r\nDTSTART:20200106T160000Z\r\nDTEND:20200106T161500Z\r\nRRULE:FREQ=WEEKLY;COUNT=3\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:s@x\r\nRECURRENCE-ID:20200113T160000Z\r\nSUMMARY:Standup (moved)\r\nDTSTART:20200113T170000Z\r\nEND:VEVENT\r\n";
        let inv = parse(ics, "standup.ics").unwrap();
        // Series ended in 2020: falls back to its first date.
        assert_eq!(inv.event.title, "Standup");
        assert!(inv.event.rrule.is_none());
        assert!(inv.note.contains("It repeats; only January 6, 2020 is added."));
        assert!(!inv.note.contains("more event"));
    }
}
