use std::io::{self, Read, Write};

use crate::Event;

/// Write a length-prefixed, postcard-serialized event to a stream.
pub fn write_event(stream: &mut impl Write, event: &Event) -> io::Result<()> {
    let bytes =
        postcard::to_allocvec(event).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let len = bytes.len() as u32;
    stream.write_all(&len.to_le_bytes())?;
    stream.write_all(&bytes)?;
    stream.flush()
}

/// Read a length-prefixed, postcard-serialized event from a stream.
///
/// Returns `None` on clean EOF (0 bytes read for the length prefix).
pub fn read_event(stream: &mut impl Read) -> io::Result<Option<Event>> {
    let mut len_buf = [0u8; 4];
    match stream.read_exact(&mut len_buf) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }

    let len = u32::from_le_bytes(len_buf) as usize;
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf)?;

    let event = postcard::from_bytes(&buf)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    Ok(Some(event))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn roundtrip_no_payload() {
        let event = Event::new("shell:test");
        let mut buf = Vec::new();
        write_event(&mut buf, &event).unwrap();

        let mut cursor = Cursor::new(buf);
        let read_back = read_event(&mut cursor).unwrap().unwrap();
        assert_eq!(read_back.id, event.id);
        assert_eq!(read_back.topic, event.topic);
        assert!(read_back.payload.is_none());
    }

    #[test]
    fn roundtrip_with_payload() {
        let event = Event::with_payload("shell:apps", vec![10, 20, 30]);
        let mut buf = Vec::new();
        write_event(&mut buf, &event).unwrap();

        let mut cursor = Cursor::new(buf);
        let read_back = read_event(&mut cursor).unwrap().unwrap();
        assert_eq!(read_back.id, event.id);
        assert_eq!(read_back.topic, "shell:apps");
        assert_eq!(read_back.payload.unwrap(), vec![10, 20, 30]);
    }

    #[test]
    fn multiple_events_in_stream() {
        let e1 = Event::new("shell:a");
        let e2 = Event::with_payload("shell:b", vec![42]);
        let e3 = Event::new("shell:c");

        let mut buf = Vec::new();
        write_event(&mut buf, &e1).unwrap();
        write_event(&mut buf, &e2).unwrap();
        write_event(&mut buf, &e3).unwrap();

        let mut cursor = Cursor::new(buf);
        let r1 = read_event(&mut cursor).unwrap().unwrap();
        let r2 = read_event(&mut cursor).unwrap().unwrap();
        let r3 = read_event(&mut cursor).unwrap().unwrap();

        assert_eq!(r1.topic, "shell:a");
        assert_eq!(r2.topic, "shell:b");
        assert_eq!(r2.payload.unwrap(), vec![42]);
        assert_eq!(r3.topic, "shell:c");
    }

    #[test]
    fn eof_returns_none() {
        let mut cursor = Cursor::new(Vec::new());
        let result = read_event(&mut cursor).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn truncated_length_returns_none() {
        // Only 2 bytes when 4 are expected for the length prefix
        let mut cursor = Cursor::new(vec![0x05, 0x00]);
        let result = read_event(&mut cursor).unwrap();
        assert!(result.is_none());
    }
}
