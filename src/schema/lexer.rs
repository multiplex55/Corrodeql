/// A lexical token from schema input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub lexeme: String,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenKind {
    Keyword(Keyword),
    Identifier,
    String,
    Number,
    Symbol(char),
    Operator(String),
    Eof,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keyword {
    Create,
    Table,
    Alter,
    Constraint,
    Primary,
    Key,
    Not,
    Null,
    Default,
    Index,
    Unique,
    Clustered,
    NonClustered,
    On,
    Add,
    Foreign,
    References,
    Check,
    Int,
    BigInt,
    SmallInt,
    TinyInt,
    Bit,
    Decimal,
    Numeric,
    Money,
    Float,
    Real,
    Date,
    Time,
    DateTime,
    DateTime2,
    SmallDateTime,
    UniqueIdentifier,
    Char,
    VarChar,
    NChar,
    NVarChar,
    Text,
    NText,
    Binary,
    VarBinary,
    Xml,
    Max,
}

pub fn lex(input: &str) -> Vec<Token> {
    let mut lexer = Lexer {
        input,
        pos: 0,
        line: 1,
        column: 1,
    };
    lexer.lex_all()
}

struct Lexer<'a> {
    input: &'a str,
    pos: usize,
    line: usize,
    column: usize,
}

impl<'a> Lexer<'a> {
    fn lex_all(&mut self) -> Vec<Token> {
        let mut tokens = Vec::new();
        while let Some(c) = self.peek() {
            if c.is_whitespace() {
                self.bump();
                continue;
            }
            let line = self.line;
            let column = self.column;
            if c == '[' {
                tokens.push(self.bracketed_identifier(line, column));
            } else if c == '\'' {
                tokens.push(self.string(line, column));
            } else if c.is_ascii_digit() {
                tokens.push(self.number(line, column));
            } else if is_ident_start(c) {
                tokens.push(self.word(line, column));
            } else if "(),.;".contains(c) {
                self.bump();
                tokens.push(Token {
                    kind: TokenKind::Symbol(c),
                    lexeme: c.to_string(),
                    line,
                    column,
                });
            } else {
                tokens.push(self.operator(line, column));
            }
        }
        tokens.push(Token {
            kind: TokenKind::Eof,
            lexeme: String::new(),
            line: self.line,
            column: self.column,
        });
        tokens
    }
    fn peek(&self) -> Option<char> {
        self.input[self.pos..].chars().next()
    }
    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        if c == '\n' {
            self.line += 1;
            self.column = 1
        } else {
            self.column += 1
        };
        Some(c)
    }
    fn word(&mut self, line: usize, column: usize) -> Token {
        let mut s = String::new();
        while let Some(c) = self.peek() {
            if is_ident_continue(c) {
                s.push(c);
                self.bump();
            } else {
                break;
            }
        }
        let kind = keyword(&s)
            .map(TokenKind::Keyword)
            .unwrap_or(TokenKind::Identifier);
        Token {
            kind,
            lexeme: s,
            line,
            column,
        }
    }
    fn number(&mut self, line: usize, column: usize) -> Token {
        let mut s = String::new();
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() || c == '.' {
                s.push(c);
                self.bump();
            } else {
                break;
            }
        }
        Token {
            kind: TokenKind::Number,
            lexeme: s,
            line,
            column,
        }
    }
    fn string(&mut self, line: usize, column: usize) -> Token {
        let mut s = String::new();
        s.push(self.bump().unwrap());
        while let Some(c) = self.bump() {
            s.push(c);
            if c == '\'' {
                if self.peek() == Some('\'') {
                    s.push(self.bump().unwrap());
                } else {
                    break;
                }
            }
        }
        Token {
            kind: TokenKind::String,
            lexeme: s,
            line,
            column,
        }
    }
    fn bracketed_identifier(&mut self, line: usize, column: usize) -> Token {
        let mut s = String::new();
        self.bump();
        while let Some(c) = self.bump() {
            if c == ']' {
                if self.peek() == Some(']') {
                    s.push(']');
                    self.bump();
                } else {
                    break;
                }
            } else {
                s.push(c);
            }
        }
        Token {
            kind: TokenKind::Identifier,
            lexeme: s,
            line,
            column,
        }
    }
    fn operator(&mut self, line: usize, column: usize) -> Token {
        let mut s = String::new();
        if let Some(c) = self.bump() {
            s.push(c);
        }
        if matches!(self.peek(), Some('=' | '>' | '<')) {
            if matches!(s.as_str(), "<" | ">" | "!" | "=") {
                s.push(self.bump().unwrap());
            }
        }
        Token {
            kind: TokenKind::Operator(s.clone()),
            lexeme: s,
            line,
            column,
        }
    }
}
fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_' || c == '#'
}
fn is_ident_continue(c: char) -> bool {
    is_ident_start(c) || c.is_ascii_digit() || c == '$'
}
fn keyword(s: &str) -> Option<Keyword> {
    use Keyword::*;
    Some(match s.to_ascii_uppercase().as_str() {
        "CREATE" => Create,
        "TABLE" => Table,
        "ALTER" => Alter,
        "CONSTRAINT" => Constraint,
        "PRIMARY" => Primary,
        "KEY" => Key,
        "NOT" => Not,
        "NULL" => Null,
        "DEFAULT" => Default,
        "INDEX" => Index,
        "UNIQUE" => Unique,
        "CLUSTERED" => Clustered,
        "NONCLUSTERED" => NonClustered,
        "ON" => On,
        "ADD" => Add,
        "FOREIGN" => Foreign,
        "REFERENCES" => References,
        "CHECK" => Check,
        "INT" => Int,
        "BIGINT" => BigInt,
        "SMALLINT" => SmallInt,
        "TINYINT" => TinyInt,
        "BIT" => Bit,
        "DECIMAL" => Decimal,
        "NUMERIC" => Numeric,
        "MONEY" => Money,
        "FLOAT" => Float,
        "REAL" => Real,
        "DATE" => Date,
        "TIME" => Time,
        "DATETIME" => DateTime,
        "DATETIME2" => DateTime2,
        "SMALLDATETIME" => SmallDateTime,
        "UNIQUEIDENTIFIER" => UniqueIdentifier,
        "CHAR" => Char,
        "VARCHAR" => VarChar,
        "NCHAR" => NChar,
        "NVARCHAR" => NVarChar,
        "TEXT" => Text,
        "NTEXT" => NText,
        "BINARY" => Binary,
        "VARBINARY" => VarBinary,
        "XML" => Xml,
        "MAX" => Max,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lexes_bracketed_identifiers() {
        let t = lex("[dbo].[Customer]");
        assert_eq!(t[0].lexeme, "dbo");
        assert_eq!(t[1].kind, TokenKind::Symbol('.'));
        assert_eq!(t[2].lexeme, "Customer");
    }
    #[test]
    fn lexes_string_defaults() {
        let t = lex("DEFAULT 'x''y'");
        assert!(t
            .iter()
            .any(|t| t.kind == TokenKind::String && t.lexeme == "'x''y'"));
    }
}
