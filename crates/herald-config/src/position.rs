/// The line and column of the byte `offset` in `text`, each counted from one,
/// the column in characters.
pub(crate) fn position(text: &str, offset: usize) -> (usize, usize) {
    let mut line = 1;
    let mut column = 1;
    for (index, character) in text.char_indices() {
        if index >= offset {
            break;
        }
        if character == '\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }
    (line, column)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_offset_is_its_line_and_column_counted_from_one_in_characters() {
        let text = "a = 1\n📦 = 2\nc";
        assert_eq!(position(text, 0), (1, 1));
        assert_eq!(position(text, 4), (1, 5));
        assert_eq!(position(text, 6), (2, 1));
        assert_eq!(position(text, 11), (2, 3));
        assert_eq!(position(text, 99), (3, 2));
    }
}
