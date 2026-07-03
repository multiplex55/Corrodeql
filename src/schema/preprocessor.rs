/// Preprocesses schema text before lexing.
///
/// The returned string keeps the same byte length as the input where possible by
/// replacing comments and SQL Server `GO` batch separators with spaces while
/// preserving newlines. This lets later diagnostics keep line/column positions
/// close to the original source.
pub fn preprocess(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.char_indices().peekable();
    let mut line_start = true;
    let mut line_non_ws = false;

    while let Some((_, ch)) = chars.next() {
        match ch {
            '-' if matches!(chars.peek(), Some((_, '-'))) => {
                chars.next();
                out.push(' ');
                out.push(' ');
                while let Some((_, c)) = chars.next() {
                    if c == '\n' {
                        out.push('\n');
                        line_start = true;
                        line_non_ws = false;
                        break;
                    }
                    out.push(if c == '\r' { '\r' } else { ' ' });
                }
            }
            '/' if matches!(chars.peek(), Some((_, '*'))) => {
                chars.next();
                out.push(' ');
                out.push(' ');
                let mut prev_star = false;
                while let Some((_, c)) = chars.next() {
                    let closes = prev_star && c == '/';
                    if c == '\n' {
                        out.push('\n');
                        line_start = true;
                        line_non_ws = false;
                    } else if c == '\r' {
                        out.push('\r');
                    } else {
                        out.push(' ');
                    }
                    if closes {
                        break;
                    }
                    prev_star = c == '*';
                }
            }
            '\n' => {
                out.push(ch);
                line_start = true;
                line_non_ws = false;
            }
            '\r' => out.push(ch),
            c if c.is_whitespace() => out.push(c),
            c if line_start || !line_non_ws => {
                let mut word = String::new();
                word.push(c);
                while let Some((_, next)) = chars.peek().copied() {
                    if next.is_ascii_alphabetic() {
                        chars.next();
                        word.push(next);
                    } else {
                        break;
                    }
                }
                if word.eq_ignore_ascii_case("go") {
                    let rest_is_ws = chars
                        .clone()
                        .take_while(|(_, n)| *n != '\n')
                        .all(|(_, n)| n.is_whitespace());
                    if rest_is_ws {
                        out.push_str(&" ".repeat(word.len()));
                    } else {
                        line_non_ws = true;
                        out.push_str(&word);
                    }
                } else {
                    line_non_ws = true;
                    out.push_str(&word);
                }
                line_start = false;
            }
            c => {
                line_non_ws = true;
                line_start = false;
                out.push(c);
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_go_batch_separators() {
        let sql = "CREATE TABLE A (Id int);\nGO\nCREATE TABLE B (Id int);";
        let processed = preprocess(sql);
        assert!(!processed.lines().nth(1).unwrap().contains("GO"));
        assert_eq!(processed.lines().count(), sql.lines().count());
    }

    #[test]
    fn preserves_string_literals_containing_go() {
        let sql = "CREATE TABLE A (Name varchar(20) DEFAULT 'GO');";
        assert!(preprocess(sql).contains("'GO'"));
    }
}
