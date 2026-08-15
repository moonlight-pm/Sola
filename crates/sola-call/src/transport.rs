//! Length-prefixed JSON frames (u32 LE + UTF-8).

use std::io::{self, Read, Write};

use crate::protocol::Wire;

pub fn write_msg(stream: &mut impl Write, msg: &Wire) -> io::Result<()> {
    let bytes = serde_json::to_vec(msg).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let len = u32::try_from(bytes.len()).map_err(|_| {
        io::Error::new(io::ErrorKind::InvalidData, "call frame larger than u32")
    })?;
    stream.write_all(&len.to_le_bytes())?;
    stream.write_all(&bytes)?;
    stream.flush()
}

pub fn read_msg(stream: &mut impl Read) -> io::Result<Option<Wire>> {
    let mut len_buf = [0u8; 4];
    match stream.read_exact(&mut len_buf) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let len = u32::from_le_bytes(len_buf) as usize;
    if len > 16 * 1024 * 1024 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("call frame too large ({len} bytes)"),
        ));
    }
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf)?;
    let msg = serde_json::from_slice(&buf)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    Ok(Some(msg))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{Role, Wire};
    use std::io::Cursor;

    #[test]
    fn roundtrip_hello() {
        let msg = Wire::Hello {
            role: Role::Caller,
            app_id: "solactl".into(),
            owner: None,
        };
        let mut buf = Vec::new();
        write_msg(&mut buf, &msg).unwrap();
        let read = read_msg(&mut Cursor::new(buf)).unwrap().unwrap();
        assert_eq!(read, msg);
    }

    #[test]
    fn eof_is_none() {
        let mut cursor = Cursor::new(Vec::<u8>::new());
        assert!(read_msg(&mut cursor).unwrap().is_none());
    }
}
