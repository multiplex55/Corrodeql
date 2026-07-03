use super::model::{DiagnosticSeverity, SchemaDiagnostic};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SqlBatch {
    pub original_text: String,
    pub line_start: usize,
    pub line_end: usize,
}

pub fn preprocess_bytes(input: &[u8]) -> Result<Vec<SqlBatch>, Vec<SchemaDiagnostic>> {
    let text = if input.starts_with(&[0xFF, 0xFE]) {
        let bytes = &input[2..];
        if bytes.len() % 2 != 0 {
            return Err(vec![diag(
                "UTF-16LE input has an odd number of bytes after the BOM",
                Some(1),
                Some(1),
                None,
            )]);
        }
        let units: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
            .collect();
        match String::from_utf16(&units) {
            Ok(text) => text,
            Err(err) => {
                return Err(vec![diag(
                    &format!("failed to decode UTF-16LE schema input: {err}"),
                    Some(1),
                    Some(1),
                    None,
                )])
            }
        }
    } else {
        match std::str::from_utf8(input) {
            Ok(text) => text.strip_prefix('\u{FEFF}').unwrap_or(text).to_owned(),
            Err(err) => {
                return Err(vec![diag(
                    &format!("failed to decode UTF-8 schema input: {err}"),
                    Some(1),
                    Some(1),
                    None,
                )])
            }
        }
    };

    preprocess(&text)
}

pub fn preprocess(input: &str) -> Result<Vec<SqlBatch>, Vec<SchemaDiagnostic>> {
    Scanner::new(input.strip_prefix('\u{FEFF}').unwrap_or(input)).scan()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Normal,
    String,
    Bracket,
    BlockComment { start_line: usize },
}

struct Scanner<'a> {
    chars: std::iter::Peekable<std::str::Chars<'a>>,
    state: State,
    line: usize,
    batch_start_line: usize,
    batch_text: String,
    line_text: String,
    line_state_start: State,
    block_comment_context: Option<String>,
}

impl<'a> Scanner<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            chars: input.chars().peekable(),
            state: State::Normal,
            line: 1,
            batch_start_line: 1,
            batch_text: String::new(),
            line_text: String::new(),
            line_state_start: State::Normal,
            block_comment_context: None,
        }
    }

    fn scan(mut self) -> Result<Vec<SqlBatch>, Vec<SchemaDiagnostic>> {
        let mut batches = Vec::new();
        while let Some(ch) = self.chars.next() {
            match self.state {
                State::Normal => self.normal(ch, &mut batches),
                State::String => self.string(ch),
                State::Bracket => self.bracket(ch),
                State::BlockComment { .. } => self.block_comment(ch),
            }
        }

        if let State::BlockComment { start_line } = self.state {
            return Err(vec![diag(
                "unterminated block comment",
                Some(start_line),
                Some(1),
                self.block_comment_context.as_deref(),
            )]);
        }

        self.finish_line(&mut batches, true);
        self.push_batch(&mut batches, self.line.saturating_sub(1));
        Ok(batches)
    }

    fn normal(&mut self, ch: char, batches: &mut Vec<SqlBatch>) {
        match ch {
            '\'' => {
                self.state = State::String;
                self.push(ch);
            }
            '[' => {
                self.state = State::Bracket;
                self.push(ch);
            }
            '-' if self.chars.peek() == Some(&'-') => {
                self.chars.next();
                self.push(' ');
                self.push(' ');
                while let Some(c) = self.chars.next() {
                    if c == '\n' {
                        self.newline(batches);
                        break;
                    }
                    self.push(if c == '\r' { '\r' } else { ' ' });
                }
            }
            '/' if self.chars.peek() == Some(&'*') => {
                self.chars.next();
                self.block_comment_context = Some(self.line_text.trim().to_owned());
                self.state = State::BlockComment {
                    start_line: self.line,
                };
                self.push(' ');
                self.push(' ');
            }
            '\n' => self.newline(batches),
            c => self.push(c),
        }
    }

    fn string(&mut self, ch: char) {
        self.push(ch);
        if ch == '\'' {
            if self.chars.peek() == Some(&'\'') {
                let escaped = self.chars.next().unwrap();
                self.push(escaped);
            } else {
                self.state = State::Normal;
            }
        } else if ch == '\n' {
            self.line += 1;
        }
    }

    fn bracket(&mut self, ch: char) {
        self.push(ch);
        if ch == ']' {
            if self.chars.peek() == Some(&']') {
                let escaped = self.chars.next().unwrap();
                self.push(escaped);
            } else {
                self.state = State::Normal;
            }
        } else if ch == '\n' {
            self.line += 1;
        }
    }

    fn block_comment(&mut self, ch: char) {
        if ch == '*' && self.chars.peek() == Some(&'/') {
            self.push(' ');
            self.push(' ');
            self.chars.next();
            self.state = State::Normal;
        } else if ch == '\n' {
            self.newline(&mut Vec::new());
        } else {
            self.push(if ch == '\r' { '\r' } else { ' ' });
        }
    }

    fn push(&mut self, ch: char) {
        self.batch_text.push(ch);
        self.line_text.push(ch);
    }

    fn newline(&mut self, batches: &mut Vec<SqlBatch>) {
        self.push('\n');
        self.finish_line(batches, false);
        self.line += 1;
        self.line_state_start = self.state;
    }

    fn finish_line(&mut self, batches: &mut Vec<SqlBatch>, eof: bool) {
        if self.line_text.is_empty() && eof {
            return;
        }
        if self.line_state_start == State::Normal
            && self.state == State::Normal
            && self.line_text.trim().eq_ignore_ascii_case("go")
        {
            let remove_len = self.line_text.len();
            let keep_len = self.batch_text.len().saturating_sub(remove_len);
            self.batch_text.truncate(keep_len);
            let line_end = self.line.saturating_sub(1);
            self.push_batch(batches, line_end);
            self.batch_start_line = self.line + 1;
            self.batch_text.clear();
        }
        self.line_text.clear();
    }

    fn push_batch(&mut self, batches: &mut Vec<SqlBatch>, line_end: usize) {
        if self.batch_text.trim().is_empty() {
            self.batch_text.clear();
            return;
        }
        batches.push(SqlBatch {
            original_text: std::mem::take(&mut self.batch_text),
            line_start: self.batch_start_line,
            line_end: line_end.max(self.batch_start_line),
        });
    }
}

fn diag(
    message: &str,
    line: Option<usize>,
    column: Option<usize>,
    context: Option<&str>,
) -> SchemaDiagnostic {
    SchemaDiagnostic {
        severity: DiagnosticSeverity::Error,
        message: match context {
            Some(context) => format!("{message} near line context: {context}"),
            None => message.into(),
        },
        line,
        column,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_go_batches_with_line_ranges() {
        let batches = preprocess("CREATE TABLE A (Id int);\nGO\nCREATE TABLE B (Id int);").unwrap();
        assert_eq!(batches.len(), 2);
        assert_eq!((batches[0].line_start, batches[0].line_end), (1, 1));
        assert_eq!((batches[1].line_start, batches[1].line_end), (3, 3));
    }

    #[test]
    fn does_not_split_go_inside_string() {
        let batches = preprocess("SELECT 'this is not a batch GO separator';").unwrap();
        assert_eq!(batches.len(), 1);
    }

    #[test]
    fn does_not_split_go_inside_bracketed_identifier() {
        let batches = preprocess("CREATE TABLE [GO] (Id int);").unwrap();
        assert_eq!(batches.len(), 1);
    }

    #[test]
    fn strips_line_comments_preserving_newlines() {
        let batches =
            preprocess("CREATE TABLE A (Id int); -- comment\nGO\nCREATE TABLE B (Id int);")
                .unwrap();
        assert!(!batches[0].original_text.contains("comment"));
        assert_eq!(batches[1].line_start, 3);
    }

    #[test]
    fn strips_block_comments_preserving_line_count() {
        let batches = preprocess("CREATE /* one\ntwo */ TABLE A (Id int);").unwrap();
        assert!(!batches[0].original_text.contains("one"));
        assert_eq!(batches[0].original_text.lines().count(), 2);
    }

    #[test]
    fn handles_utf8_bom() {
        let batches = preprocess_bytes("\u{FEFF}CREATE TABLE A (Id int);".as_bytes()).unwrap();
        assert!(batches[0].original_text.starts_with("CREATE"));
    }

    #[test]
    fn handles_utf16le_bom() {
        let mut bytes = vec![0xFF, 0xFE];
        for unit in "CREATE TABLE A (Id int);".encode_utf16() {
            bytes.extend(unit.to_le_bytes());
        }
        let batches = preprocess_bytes(&bytes).unwrap();
        assert!(batches[0].original_text.starts_with("CREATE"));
    }

    #[test]
    fn reports_unterminated_block_comment_with_line_context() {
        let err = preprocess("CREATE TABLE A (Id int);\n/* unterminated").unwrap_err();
        assert_eq!(err[0].line, Some(2));
        assert!(err[0].message.contains("line context"));
    }
}
