//! Query language: see [`Query`](crate::Query).

use std::cmp::Ordering;
use std::fmt;

use serde_json::Value;

use crate::node::Node;

/// A compiled query: a small, schema-agnostic query language over [`Node`]s.
///
/// [`Query::parse`] compiles a query string once into an AST; [`Query::matches`]
/// evaluates it against one node without allocating, and [`Query::find_paths`]
/// returns the index paths of every match in document (pre-)order.
///
/// # Grammar
///
/// ```text
/// query   := or
/// or      := and (("||" | "or") and)*
/// and     := unary (("&&" | "and")? unary)*      adjacent terms are ANDed
/// unary   := ("!" | "not") unary | "(" or ")" | term
/// term    := WORD | STRING                       bare word / quoted string
///          | "type" OP value
///          | "text" [OP value]
///          | "attrs" | "attrs." PATH [OP value]
///          | "marks" | "marks." NAME ["." PATH] [OP value]
///          | "mark" OP value
///          | "has" "mark" value | "has" "attr" PATH
///          | "children" [OP NUMBER]
///          | "depth" OP NUMBER
/// OP      := "==" | "=" | "!=" | "<" | "<=" | ">" | ">=" | "~"
/// value   := WORD | STRING        WORD: number, true, false, null, or text
/// ```
///
/// Semantics:
///
/// - A bare word or quoted string matches when it is a case-insensitive
///   substring of the node's own `text` or of its `type`.
/// - `~` is a case-insensitive substring test; `==` is exact (case-sensitive).
///   `<`/`>`… compare numbers numerically and strings lexicographically.
/// - `!=` is always the negation of `==`, so a missing attr is `!=` anything.
/// - A field without an operator tests existence: `attrs.level` (attr present),
///   `text` (has own text), `children` (has children), `marks.link` (has a
///   `link` mark), `attrs` / `marks` (non-empty).
/// - Attr paths descend into objects (`attrs.meta.source`) and arrays by index
///   (`attrs.tags.0`); comparing against an array matches if any element does.
/// - `depth` is the length of the node's index path (root = 0). It needs
///   context, so it is only meaningful through [`Query::find_paths`] or
///   [`Query::matches_at`]; [`Query::matches`] evaluates it at depth 0.
/// - Keywords (`type`, `text`, `has`, `and`…) are fields, not bare words:
///   quote them to search for the word itself (`"type"`).
///
/// ```
/// use tiptap_rusty_parser::{Document, Query};
///
/// let doc = Document::from_json_str(r#"{"type":"doc","content":[
///   {"type":"heading","attrs":{"level":1},"content":[{"type":"text","text":"Title"}]},
///   {"type":"heading","attrs":{"level":2},"content":[{"type":"text","text":"Usage"}]}
/// ]}"#).unwrap();
///
/// let q = Query::parse("type == heading && attrs.level > 1").unwrap();
/// assert_eq!(q.find_paths(doc.root()), vec![vec![1]]);
/// ```
///
/// # Examples per construct
///
/// Bare words and quoted strings (own text or type, case-insensitive):
///
/// ```
/// use tiptap_rusty_parser::{Node, Query};
/// let n = Node::text("The diff engine");
/// assert!(Query::parse("ENGINE").unwrap().matches(&n));
/// assert!(Query::parse("\"diff engine\"").unwrap().matches(&n));
/// assert!(Query::parse("tex").unwrap().matches(&n)); // type "text"
/// assert!(!Query::parse("\"engine diff\"").unwrap().matches(&n));
/// ```
///
/// `type`:
///
/// ```
/// use tiptap_rusty_parser::{Node, Query};
/// let n = Node::element("bulletList");
/// assert!(Query::parse("type == bulletList").unwrap().matches(&n));
/// assert!(Query::parse("type != text").unwrap().matches(&n));
/// assert!(Query::parse("type ~ list").unwrap().matches(&n));
/// ```
///
/// `attrs.KEY` comparisons, existence and dotted paths:
///
/// ```
/// use tiptap_rusty_parser::{Node, Query};
/// use serde_json::json;
/// let n = Node::element("callout")
///     .with_attr("priority", 2)
///     .with_attr("collapsed", false)
///     .with_attr("variant", "info")
///     .with_attr("meta", json!({"source": "import", "tags": ["docs"]}));
/// let m = |q: &str| Query::parse(q).unwrap().matches(&n);
/// assert!(m("attrs.priority >= 2") && m("attrs.priority < 3") && !m("attrs.priority != 2"));
/// assert!(m("attrs.collapsed == false") && m("attrs.variant ~ NF"));
/// assert!(m("attrs.meta.source == import") && m("attrs.meta.tags == docs"));
/// assert!(m("attrs.variant") && !m("attrs.missing"));
/// ```
///
/// `text`, marks:
///
/// ```
/// use tiptap_rusty_parser::{Mark, Node, Query};
/// let n = Node::text_with_marks(
///     "see repo",
///     [Mark::new("bold"), Mark::new("link").attr("href", "https://github.com/x")],
/// );
/// let m = |q: &str| Query::parse(q).unwrap().matches(&n);
/// assert!(m("text ~ \"REPO\"") && m("text == \"see repo\"") && m("text"));
/// assert!(m("has mark bold") && m("mark == link") && !m("mark == italic"));
/// assert!(m("marks.link") && m("marks.link.href ~ github"));
/// ```
///
/// `children` and `depth`:
///
/// ```
/// use tiptap_rusty_parser::{Document, Node, Query};
/// let list = Node::element("bulletList")
///     .with_children((0..3).map(|_| Node::element("listItem")));
/// assert!(Query::parse("children > 2").unwrap().matches(&list));
/// assert!(Query::parse("children").unwrap().matches(&list));
///
/// let doc = Document::new(tiptap_rusty_parser::doc([list]));
/// let q = Query::parse("depth == 2").unwrap();
/// assert_eq!(q.find_paths(doc.root()), vec![vec![0, 0], vec![0, 1], vec![0, 2]]);
/// ```
///
/// Boolean logic: `&&`/`and`, `||`/`or`, `!`/`not`, parentheses:
///
/// ```
/// use tiptap_rusty_parser::{Node, Query};
/// let n = Node::element("heading").with_attr("level", 2);
/// let m = |q: &str| Query::parse(q).unwrap().matches(&n);
/// assert!(m("type == heading && attrs.level == 2"));
/// assert!(m("type == paragraph or type == heading"));
/// assert!(m("not (type == text || attrs.level == 1)"));
/// assert!(m("!text and heading")); // adjacent terms AND
/// ```
///
/// Errors carry a byte offset:
///
/// ```
/// use tiptap_rusty_parser::Query;
/// let e = Query::parse("type == heading &&").unwrap_err();
/// assert_eq!(e.offset, 18);
/// assert_eq!(Query::parse("a & b").unwrap_err().offset, 2);
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct Query {
    expr: Expr,
}

/// A parse error with the byte offset in the query string where it occurred.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryError {
    /// Byte offset into the query string.
    pub offset: usize,
    /// What went wrong.
    pub kind: QueryErrorKind,
}

/// Kinds of [`QueryError`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum QueryErrorKind {
    /// The query is empty or only whitespace.
    Empty,
    /// The query ended where `expected` was required.
    UnexpectedEnd {
        /// What the parser was looking for.
        expected: &'static str,
    },
    /// A token appeared where `expected` was required.
    UnexpectedToken {
        /// The offending token text.
        found: String,
        /// What the parser was looking for.
        expected: &'static str,
    },
    /// A quoted string has no closing quote.
    UnterminatedString,
    /// A single `&` or `|`.
    LoneOperator(char),
    /// A word followed by an operator that is not a known field.
    UnknownField(String),
    /// The field takes a number (`children`, `depth`).
    ExpectedNumber,
    /// The operator is not valid for this field (e.g. `children ~ 2`).
    InvalidOperator,
    /// An empty segment in a dotted path (`attrs.`, `attrs..x`).
    EmptyPath,
}

impl fmt::Display for QueryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            QueryErrorKind::Empty => write!(f, "empty query"),
            QueryErrorKind::UnexpectedEnd { expected } => {
                write!(f, "expected {expected} at end of query")
            }
            QueryErrorKind::UnexpectedToken { found, expected } => {
                write!(f, "expected {expected}, found `{found}`")
            }
            QueryErrorKind::UnterminatedString => write!(f, "unterminated string"),
            QueryErrorKind::LoneOperator(c) => write!(f, "expected `{c}{c}`"),
            QueryErrorKind::UnknownField(w) => {
                write!(f, "unknown field `{w}` (try attrs.{w})")
            }
            QueryErrorKind::ExpectedNumber => write!(f, "expected a number"),
            QueryErrorKind::InvalidOperator => write!(f, "operator not valid here"),
            QueryErrorKind::EmptyPath => write!(f, "empty path segment"),
        }?;
        write!(f, " (at byte {})", self.offset)
    }
}

impl std::error::Error for QueryError {}

// ---- AST ------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Tilde,
}

impl Op {
    #[inline]
    fn ord(self, o: Ordering) -> bool {
        match self {
            Op::Eq => o == Ordering::Equal,
            Op::Ne => o != Ordering::Equal,
            Op::Lt => o == Ordering::Less,
            Op::Le => o != Ordering::Greater,
            Op::Gt => o == Ordering::Greater,
            Op::Ge => o != Ordering::Less,
            Op::Tilde => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum LitKind {
    Num(f64),
    Bool(bool),
    Null,
    Str,
}

#[derive(Debug, Clone, PartialEq)]
struct Lit {
    raw: Box<str>,
    lower: Box<str>,
    kind: LitKind,
}

impl Lit {
    fn new(raw: String, quoted: bool) -> Self {
        let kind = if quoted {
            LitKind::Str
        } else {
            match raw.as_str() {
                "true" => LitKind::Bool(true),
                "false" => LitKind::Bool(false),
                "null" => LitKind::Null,
                s => s.parse::<f64>().map(LitKind::Num).unwrap_or(LitKind::Str),
            }
        };
        Lit {
            lower: raw.to_lowercase().into_boxed_str(),
            raw: raw.into_boxed_str(),
            kind,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Seg {
    key: Box<str>,
    idx: Option<usize>,
}

type Cmp = Option<(Op, Lit)>;

#[derive(Debug, Clone, PartialEq)]
enum Pred {
    Word(Box<str>),
    Type(Op, Lit),
    Text(Cmp),
    AnyAttrs,
    Attr(Vec<Seg>, Cmp),
    AnyMarks,
    Mark(Op, Lit),
    MarkAttr(Box<str>, Vec<Seg>, Cmp),
    Children(Option<(Op, f64)>),
    Depth(Op, f64),
}

#[derive(Debug, Clone, PartialEq)]
enum Expr {
    And(Vec<Expr>),
    Or(Vec<Expr>),
    Not(Box<Expr>),
    Pred(Pred),
}

// ---- matching ---------------------------------------------------------------------

impl Query {
    /// Parse `src` into a reusable query.
    ///
    /// ```
    /// use tiptap_rusty_parser::{Query, QueryErrorKind};
    /// assert!(Query::parse("type == heading").is_ok());
    /// let err = Query::parse("attrs.level >").unwrap_err();
    /// assert_eq!(err.offset, 13);
    /// assert!(matches!(err.kind, QueryErrorKind::UnexpectedEnd { .. }));
    /// ```
    pub fn parse(src: &str) -> Result<Query, QueryError> {
        let tokens = lex(src)?;
        if tokens.is_empty() {
            return Err(QueryError {
                offset: 0,
                kind: QueryErrorKind::Empty,
            });
        }
        let mut p = Parser {
            tokens: &tokens,
            pos: 0,
            end: src.len(),
        };
        let expr = p.or()?;
        if let Some(t) = p.peek() {
            return Err(p.unexpected(t, "an operator or end of query"));
        }
        Ok(Query { expr })
    }

    /// Does `node` match? `depth` predicates are evaluated at depth 0.
    ///
    /// ```
    /// use tiptap_rusty_parser::{Node, Query};
    /// let q = Query::parse("roundtrip").unwrap();
    /// assert!(q.matches(&Node::text("Faithful RoundTrip")));
    /// assert!(!q.matches(&Node::text("nope")));
    /// ```
    #[inline]
    pub fn matches(&self, node: &Node) -> bool {
        self.expr.eval(node, 0)
    }

    /// Does `node`, found at index-path length `depth`, match?
    ///
    /// ```
    /// use tiptap_rusty_parser::{Node, Query};
    /// let q = Query::parse("depth >= 2").unwrap();
    /// assert!(q.matches_at(&Node::text("x"), 2));
    /// assert!(!q.matches_at(&Node::text("x"), 1));
    /// ```
    #[inline]
    pub fn matches_at(&self, node: &Node, depth: usize) -> bool {
        self.expr.eval(node, depth)
    }

    /// Index paths (root = `[]`) of every node under `root` (incl. itself)
    /// that matches, in document order.
    ///
    /// ```
    /// use tiptap_rusty_parser::{Document, Query};
    /// let doc = Document::from_json_str(r#"{"type":"doc","content":[
    ///   {"type":"paragraph","content":[{"type":"text","text":"a"}]},
    ///   {"type":"paragraph"}]}"#).unwrap();
    /// let q = Query::parse("type == paragraph").unwrap();
    /// assert_eq!(q.find_paths(doc.root()), vec![vec![0], vec![1]]);
    /// ```
    pub fn find_paths(&self, root: &Node) -> Vec<Vec<usize>> {
        let mut out = Vec::new();
        let mut path = Vec::new();
        self.collect(root, &mut path, &mut out);
        out
    }

    fn collect(&self, node: &Node, path: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
        if self.expr.eval(node, path.len()) {
            out.push(path.clone());
        }
        if let Some(children) = &node.content {
            for (i, c) in children.iter().enumerate() {
                path.push(i);
                self.collect(c, path, out);
                path.pop();
            }
        }
    }
}

impl Expr {
    fn eval(&self, n: &Node, depth: usize) -> bool {
        match self {
            Expr::And(xs) => xs.iter().all(|x| x.eval(n, depth)),
            Expr::Or(xs) => xs.iter().any(|x| x.eval(n, depth)),
            Expr::Not(x) => !x.eval(n, depth),
            Expr::Pred(p) => p.eval(n, depth),
        }
    }
}

impl Pred {
    fn eval(&self, n: &Node, depth: usize) -> bool {
        match self {
            Pred::Word(w) => {
                n.node_type.as_deref().is_some_and(|t| contains_ci(t, w))
                    || n.text.as_deref().is_some_and(|t| contains_ci(t, w))
            }
            Pred::Type(op, lit) => opt_str(n.node_type.as_deref(), *op, lit),
            Pred::Text(None) => n.text.is_some(),
            Pred::Text(Some((op, lit))) => opt_str(n.text.as_deref(), *op, lit),
            Pred::AnyAttrs => n.attrs.as_ref().is_some_and(|a| !a.is_empty()),
            Pred::Attr(path, cmp) => {
                let v = n.attrs.as_ref().and_then(|a| {
                    let (first, rest) = path.split_first()?;
                    resolve(a.get(&*first.key)?, rest)
                });
                opt_value(v, cmp)
            }
            Pred::AnyMarks => n.marks.as_ref().is_some_and(|m| !m.is_empty()),
            Pred::Mark(op, lit) => {
                let marks = n.marks.as_deref().unwrap_or(&[]);
                if *op == Op::Ne {
                    !marks.iter().any(|m| cmp_str(&m.mark_type, Op::Eq, lit))
                } else {
                    marks.iter().any(|m| cmp_str(&m.mark_type, *op, lit))
                }
            }
            Pred::MarkAttr(name, path, cmp) => {
                let mut marks = n
                    .marks
                    .as_deref()
                    .unwrap_or(&[])
                    .iter()
                    .filter(|m| *m.mark_type == **name);
                if path.is_empty() {
                    return marks.next().is_some();
                }
                let hit = marks.any(|m| {
                    let v = m.attrs.as_ref().and_then(|a| {
                        let (first, rest) = path.split_first()?;
                        resolve(a.get(&*first.key)?, rest)
                    });
                    match cmp {
                        Some((Op::Ne, lit)) => v.is_some_and(|v| cmp_value(v, Op::Eq, lit)),
                        _ => opt_value(v, cmp),
                    }
                });
                // `!=` = no `link` mark whose attr equals the value.
                if matches!(cmp, Some((Op::Ne, _))) {
                    !hit
                } else {
                    hit
                }
            }
            Pred::Children(None) => n.content.as_ref().is_some_and(|c| !c.is_empty()),
            Pred::Children(Some((op, x))) => cmp_num(n.child_count() as f64, *op, *x),
            Pred::Depth(op, x) => cmp_num(depth as f64, *op, *x),
        }
    }
}

fn resolve<'a>(mut v: &'a Value, rest: &[Seg]) -> Option<&'a Value> {
    for seg in rest {
        v = match v {
            Value::Object(m) => m.get(&*seg.key)?,
            Value::Array(a) => a.get(seg.idx?)?,
            _ => return None,
        };
    }
    Some(v)
}

#[inline]
fn opt_value(v: Option<&Value>, cmp: &Cmp) -> bool {
    match (v, cmp) {
        (None, None) => false,
        (Some(_), None) => true,
        (None, Some((op, _))) => *op == Op::Ne,
        (Some(v), Some((op, lit))) => cmp_value(v, *op, lit),
    }
}

#[inline]
fn opt_str(s: Option<&str>, op: Op, lit: &Lit) -> bool {
    match s {
        Some(s) => cmp_str(s, op, lit),
        None => op == Op::Ne,
    }
}

#[inline]
fn cmp_num(a: f64, op: Op, b: f64) -> bool {
    a.partial_cmp(&b).is_some_and(|o| op.ord(o))
}

#[inline]
fn cmp_str(s: &str, op: Op, lit: &Lit) -> bool {
    match op {
        Op::Tilde => contains_ci(s, &lit.lower),
        _ => op.ord(s.cmp(&lit.raw)),
    }
}

fn cmp_value(v: &Value, op: Op, lit: &Lit) -> bool {
    if op == Op::Ne {
        return !cmp_value(v, Op::Eq, lit);
    }
    match v {
        Value::Array(a) => a.iter().any(|x| cmp_value(x, op, lit)),
        Value::String(s) => cmp_str(s, op, lit),
        Value::Number(x) => match (&lit.kind, x.as_f64()) {
            (LitKind::Num(b), Some(a)) if op != Op::Tilde => cmp_num(a, op, *b),
            _ => false,
        },
        Value::Bool(b) => op == Op::Eq && lit.kind == LitKind::Bool(*b),
        Value::Null => op == Op::Eq && lit.kind == LitKind::Null,
        Value::Object(_) => false,
    }
}

/// Case-insensitive substring test; `needle` is already lowercase. No allocation.
fn contains_ci(hay: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    if needle.is_ascii() {
        let (h, n) = (hay.as_bytes(), needle.as_bytes());
        if n.len() > h.len() {
            return false;
        }
        let first = n[0];
        return h
            .windows(n.len())
            .any(|w| w[0].to_ascii_lowercase() == first && w.eq_ignore_ascii_case(n));
    }
    hay.char_indices()
        .any(|(i, _)| starts_with_ci(&hay[i..], needle))
}

fn starts_with_ci(hay: &str, needle: &str) -> bool {
    let mut n = needle.chars();
    for c in hay.chars().flat_map(char::to_lowercase) {
        match n.next() {
            None => return true,
            Some(x) if x == c => {}
            Some(_) => return false,
        }
    }
    n.next().is_none()
}

// ---- lexer ------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Word(String),
    Str(String),
    Op(Op),
    And,
    Or,
    Not,
    LParen,
    RParen,
}

#[derive(Debug)]
struct Token {
    tok: Tok,
    start: usize,
}

const SPECIAL: &[u8] = b"()&|!=<>~\"'";

fn lex(src: &str) -> Result<Vec<Token>, QueryError> {
    let b = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        let start = i;
        let next = b.get(i + 1).copied();
        let (tok, len) = match c {
            b'(' => (Tok::LParen, 1),
            b')' => (Tok::RParen, 1),
            b'&' | b'|' if next == Some(c) => (if c == b'&' { Tok::And } else { Tok::Or }, 2),
            b'&' | b'|' => {
                return Err(QueryError {
                    offset: start,
                    kind: QueryErrorKind::LoneOperator(c as char),
                })
            }
            b'!' if next == Some(b'=') => (Tok::Op(Op::Ne), 2),
            b'!' => (Tok::Not, 1),
            b'=' if next == Some(b'=') => (Tok::Op(Op::Eq), 2),
            b'=' => (Tok::Op(Op::Eq), 1),
            b'<' if next == Some(b'=') => (Tok::Op(Op::Le), 2),
            b'<' => (Tok::Op(Op::Lt), 1),
            b'>' if next == Some(b'=') => (Tok::Op(Op::Ge), 2),
            b'>' => (Tok::Op(Op::Gt), 1),
            b'~' => (Tok::Op(Op::Tilde), 1),
            b'"' | b'\'' => {
                let mut s = String::new();
                let mut j = i + 1;
                let mut seg = j;
                loop {
                    match b.get(j) {
                        None => {
                            return Err(QueryError {
                                offset: start,
                                kind: QueryErrorKind::UnterminatedString,
                            })
                        }
                        Some(&q) if q == c => break,
                        Some(b'\\') if j + 1 < b.len() => {
                            s.push_str(&src[seg..j]);
                            // Keep the escaped char (may be multi-byte).
                            let ch = src[j + 1..].chars().next().unwrap_or('\\');
                            s.push(ch);
                            j += 1 + ch.len_utf8();
                            seg = j;
                        }
                        Some(_) => j += 1,
                    }
                }
                s.push_str(&src[seg..j]);
                (Tok::Str(s), j + 1 - i)
            }
            _ => {
                let mut j = i;
                while j < b.len() && !b[j].is_ascii_whitespace() && !SPECIAL.contains(&b[j]) {
                    j += 1;
                }
                let w = &src[i..j];
                let tok = if w.eq_ignore_ascii_case("and") {
                    Tok::And
                } else if w.eq_ignore_ascii_case("or") {
                    Tok::Or
                } else if w.eq_ignore_ascii_case("not") {
                    Tok::Not
                } else {
                    Tok::Word(w.to_string())
                };
                (tok, j - i)
            }
        };
        i += len;
        out.push(Token { tok, start });
    }
    Ok(out)
}

// ---- parser -----------------------------------------------------------------------

struct Parser<'a> {
    tokens: &'a [Token],
    pos: usize,
    end: usize,
}

impl<'a> Parser<'a> {
    #[inline]
    fn peek(&self) -> Option<&'a Token> {
        self.tokens.get(self.pos)
    }

    fn bump(&mut self) -> Option<&'a Token> {
        let t = self.tokens.get(self.pos);
        self.pos += 1;
        t
    }

    fn eof(&self, expected: &'static str) -> QueryError {
        QueryError {
            offset: self.end,
            kind: QueryErrorKind::UnexpectedEnd { expected },
        }
    }

    fn unexpected(&self, t: &Token, expected: &'static str) -> QueryError {
        QueryError {
            offset: t.start,
            kind: QueryErrorKind::UnexpectedToken {
                found: self.text(t),
                expected,
            },
        }
    }

    fn text(&self, t: &Token) -> String {
        match &t.tok {
            Tok::Word(w) => w.clone(),
            Tok::Str(s) => format!("\"{s}\""),
            Tok::Op(op) => match op {
                Op::Eq => "==",
                Op::Ne => "!=",
                Op::Lt => "<",
                Op::Le => "<=",
                Op::Gt => ">",
                Op::Ge => ">=",
                Op::Tilde => "~",
            }
            .into(),
            Tok::And => "&&".into(),
            Tok::Or => "||".into(),
            Tok::Not => "!".into(),
            Tok::LParen => "(".into(),
            Tok::RParen => ")".into(),
        }
    }

    fn or(&mut self) -> Result<Expr, QueryError> {
        let mut xs = vec![self.and()?];
        while matches!(self.peek(), Some(Token { tok: Tok::Or, .. })) {
            self.pos += 1;
            xs.push(self.and()?);
        }
        Ok(if xs.len() == 1 {
            xs.pop().unwrap()
        } else {
            Expr::Or(xs)
        })
    }

    fn and(&mut self) -> Result<Expr, QueryError> {
        let mut xs = vec![self.unary()?];
        loop {
            match self.peek().map(|t| &t.tok) {
                Some(Tok::And) => {
                    self.pos += 1;
                    xs.push(self.unary()?);
                }
                // Implicit AND between adjacent terms.
                Some(Tok::Word(_) | Tok::Str(_) | Tok::Not | Tok::LParen) => xs.push(self.unary()?),
                _ => break,
            }
        }
        Ok(if xs.len() == 1 {
            xs.pop().unwrap()
        } else {
            Expr::And(xs)
        })
    }

    fn unary(&mut self) -> Result<Expr, QueryError> {
        let Some(t) = self.bump() else {
            return Err(self.eof("a term"));
        };
        match &t.tok {
            Tok::Not => Ok(Expr::Not(Box::new(self.unary()?))),
            Tok::LParen => {
                let e = self.or()?;
                match self.bump() {
                    Some(Token {
                        tok: Tok::RParen, ..
                    }) => Ok(e),
                    Some(t) => Err(self.unexpected(t, "`)`")),
                    None => Err(self.eof("`)`")),
                }
            }
            Tok::Str(s) => Ok(Expr::Pred(Pred::Word(s.to_lowercase().into()))),
            Tok::Word(w) => self.term(w, t).map(Expr::Pred),
            _ => Err(self.unexpected(t, "a term")),
        }
    }

    fn peek_op(&self) -> Option<Op> {
        match self.peek() {
            Some(Token {
                tok: Tok::Op(op), ..
            }) => Some(*op),
            _ => None,
        }
    }

    /// Optional `OP value`.
    fn cmp(&mut self) -> Result<Cmp, QueryError> {
        let Some(op) = self.peek_op() else {
            return Ok(None);
        };
        self.pos += 1;
        Ok(Some((op, self.value()?)))
    }

    /// Required `OP value`.
    fn cmp_req(&mut self) -> Result<(Op, Lit), QueryError> {
        match self.cmp()? {
            Some(c) => Ok(c),
            None => Err(match self.peek() {
                Some(t) => self.unexpected(t, "an operator"),
                None => self.eof("an operator"),
            }),
        }
    }

    fn value(&mut self) -> Result<Lit, QueryError> {
        match self.bump() {
            Some(Token {
                tok: Tok::Word(w), ..
            }) => Ok(Lit::new(w.clone(), false)),
            Some(Token {
                tok: Tok::Str(s), ..
            }) => Ok(Lit::new(s.clone(), true)),
            Some(t) => Err(self.unexpected(t, "a value")),
            None => Err(self.eof("a value")),
        }
    }

    fn num_cmp(&mut self, required: bool) -> Result<Option<(Op, f64)>, QueryError> {
        let op_tok = self.peek();
        let Some(op) = self.peek_op() else {
            if required {
                return Err(match op_tok {
                    Some(t) => self.unexpected(t, "an operator"),
                    None => self.eof("an operator"),
                });
            }
            return Ok(None);
        };
        let op_tok = op_tok.unwrap();
        if op == Op::Tilde {
            return Err(QueryError {
                offset: op_tok.start,
                kind: QueryErrorKind::InvalidOperator,
            });
        }
        self.pos += 1;
        let at = self.peek().map_or(self.end, |t| t.start);
        let lit = self.value()?;
        match lit.kind {
            LitKind::Num(x) => Ok(Some((op, x))),
            _ => Err(QueryError {
                offset: at,
                kind: QueryErrorKind::ExpectedNumber,
            }),
        }
    }

    fn path(&self, s: &str, offset: usize) -> Result<Vec<Seg>, QueryError> {
        let mut segs = Vec::new();
        let mut at = offset;
        for part in s.split('.') {
            if part.is_empty() {
                return Err(QueryError {
                    offset: at,
                    kind: QueryErrorKind::EmptyPath,
                });
            }
            segs.push(Seg {
                key: part.into(),
                idx: part.parse().ok(),
            });
            at += part.len() + 1;
        }
        Ok(segs)
    }

    fn term(&mut self, w: &str, t: &Token) -> Result<Pred, QueryError> {
        match w {
            "type" => {
                let (op, lit) = self.cmp_req()?;
                Ok(Pred::Type(op, lit))
            }
            "text" => Ok(Pred::Text(self.cmp()?)),
            "mark" => {
                let (op, lit) = self.cmp_req()?;
                Ok(Pred::Mark(op, lit))
            }
            "children" => Ok(Pred::Children(self.num_cmp(false)?)),
            "depth" => {
                let (op, x) = self.num_cmp(true)?.expect("required");
                Ok(Pred::Depth(op, x))
            }
            "attrs" if self.peek_op().is_none() => Ok(Pred::AnyAttrs),
            "marks" if self.peek_op().is_none() => Ok(Pred::AnyMarks),
            "has" => {
                let what = match self.bump() {
                    Some(Token {
                        tok: Tok::Word(k), ..
                    }) if k == "mark" || k == "attr" => k.as_str(),
                    Some(t) => return Err(self.unexpected(t, "`mark` or `attr`")),
                    None => return Err(self.eof("`mark` or `attr`")),
                };
                if what == "mark" {
                    Ok(Pred::Mark(Op::Eq, self.value()?))
                } else {
                    match self.bump() {
                        Some(
                            t @ Token {
                                tok: Tok::Word(k), ..
                            },
                        ) => Ok(Pred::Attr(self.path(k, t.start)?, None)),
                        Some(t) => Err(self.unexpected(t, "an attr name")),
                        None => Err(self.eof("an attr name")),
                    }
                }
            }
            _ => {
                if let Some(rest) = w.strip_prefix("attrs.") {
                    let path = self.path(rest, t.start + 6)?;
                    return Ok(Pred::Attr(path, self.cmp()?));
                }
                if let Some(rest) = w.strip_prefix("marks.") {
                    let (name, path) = match rest.split_once('.') {
                        Some((n, p)) => (n, self.path(p, t.start + 7 + n.len())?),
                        None => (rest, Vec::new()),
                    };
                    if name.is_empty() {
                        return Err(QueryError {
                            offset: t.start + 6,
                            kind: QueryErrorKind::EmptyPath,
                        });
                    }
                    let cmp = self.cmp()?;
                    if path.is_empty() && cmp.is_some() {
                        return Err(QueryError {
                            offset: self.tokens[self.pos - 2].start,
                            kind: QueryErrorKind::InvalidOperator,
                        });
                    }
                    return Ok(Pred::MarkAttr(name.into(), path, cmp));
                }
                if self.peek_op().is_some() {
                    return Err(QueryError {
                        offset: t.start,
                        kind: QueryErrorKind::UnknownField(w.to_string()),
                    });
                }
                Ok(Pred::Word(w.to_lowercase().into()))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contains_ci_paths() {
        assert!(contains_ci("Hello World", "world"));
        assert!(!contains_ci("Hello", "world"));
        assert!(contains_ci("ÉCOLE", "école"));
        assert!(contains_ci("x", ""));
        assert!(!contains_ci("ab", "abc"));
    }

    #[test]
    fn lexes_escapes_and_offsets() {
        let t = lex(r#"a "b\"c" =="#).unwrap();
        assert_eq!(t[1].tok, Tok::Str("b\"c".into()));
        assert_eq!((t[1].start, t[2].start), (2, 9));
        assert_eq!(t[2].tok, Tok::Op(Op::Eq));
    }
}
