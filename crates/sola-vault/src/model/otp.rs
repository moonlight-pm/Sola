//! An authenticator code in six boxes. One field takes the typing (so
//! Backspace, a paste and the Edit menu behave as in any field); the boxes
//! show its digits, the next one marked.

pub const LEN: usize = 6;

/// What the code keeps of the field's text: its digits, at most six — a
/// pasted "482 913" is the whole code.
pub fn accept(input: &str) -> String {
    input.chars().filter(char::is_ascii_digit).take(LEN).collect()
}

/// The digit box `i` shows.
pub fn digit(code: &str, i: usize) -> Option<char> {
    code.chars().nth(i)
}

/// The box typing goes into next (none when all are full).
pub fn next_box(code: &str) -> Option<usize> {
    let n = code.chars().count();
    (n < LEN).then_some(n)
}

pub fn complete(code: &str) -> bool {
    code.chars().count() == LEN
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digits_only_six_at_most() {
        assert_eq!(accept("48"), "48");
        assert_eq!(accept("482 913"), "482913");
        assert_eq!(accept("4829137"), "482913");
        assert_eq!(accept("a4-8"), "48");
    }

    #[test]
    fn the_boxes_read_the_code() {
        assert_eq!(digit("48", 1), Some('8'));
        assert_eq!(digit("48", 2), None);
        assert_eq!(next_box("48"), Some(2));
        assert_eq!(next_box("482913"), None);
        assert!(complete("482913"));
        assert!(!complete("48291"));
    }
}
