/// The line and column of the byte `offset` in `text`, each counted from one,
/// the column in characters.
#[must_use]
pub fn position(text: &str, offset: usize) -> (usize, usize) {
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

/// Why the file `file` could not be parsed at its `line` and, where told,
/// `column`, named by them alone, so nothing it holds is shown.
#[must_use]
pub fn unparsed(file: &str, line: usize, column: Option<usize>) -> String {
    match column {
        Some(column) => {
            format!("{file} cannot be read at line {line}, column {column}; fix it there")
        }
        None => format!("{file} cannot be read at line {line}; fix it there"),
    }
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

    #[test]
    fn a_file_that_does_not_parse_is_told_by_its_place_alone() {
        assert_eq!(
            unparsed("members.toml", 3, Some(9)),
            "members.toml cannot be read at line 3, column 9; fix it there"
        );
        assert_eq!(
            unparsed("body.md", 2, None),
            "body.md cannot be read at line 2; fix it there"
        );
    }
}
