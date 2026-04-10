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
