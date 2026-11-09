//! VideoToolbox hands back NAL units behind their lengths; the wire and
//! NVDEC want Annex B, start codes between them.

const START: [u8; 4] = [0, 0, 0, 1];

/// Append one parameter set (or any bare NAL unit) to `out`.
pub fn push_nal(out: &mut Vec<u8>, nal: &[u8]) {
    out.extend_from_slice(&START);
    out.extend_from_slice(nal);
}

/// Append the NAL units of `sample`, each behind a big-endian length of
/// `header` bytes. `None` when a length runs past the end.
pub fn push_sample(out: &mut Vec<u8>, sample: &[u8], header: usize) -> Option<()> {
    if !(1..=4).contains(&header) {
        return None;
    }
    let mut at = 0;
    while at < sample.len() {
        let len = sample
            .get(at..at + header)?
            .iter()
            .fold(0usize, |n, b| (n << 8) | usize::from(*b));
        at += header;
        push_nal(out, sample.get(at..at + len)?);
        at += len;
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lengths_become_start_codes() {
        let sample = [0, 0, 0, 2, 0xaa, 0xbb, 0, 0, 0, 1, 0xcc];
        let mut out = Vec::new();
        push_sample(&mut out, &sample, 4).unwrap();
        assert_eq!(out, [0, 0, 0, 1, 0xaa, 0xbb, 0, 0, 0, 1, 0xcc]);
    }

    #[test]
    fn a_length_past_the_end_is_refused() {
        let mut out = Vec::new();
        assert!(push_sample(&mut out, &[0, 0, 0, 9, 0xaa], 4).is_none());
        assert!(push_sample(&mut out, &[0, 0], 4).is_none());
        assert!(push_sample(&mut out, &[], 0).is_none());
    }
}
