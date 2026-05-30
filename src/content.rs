//! ProseMirror **content expressions** — cardinality + ordering for schema
//! validation (e.g. `paragraph+`, `heading{1,3}`, `(text | image)*`, `block+`).
//!
//! A [`NodeSpec`](crate::NodeSpec)'s `content` is a [`ContentRule`]: either a
//! set of allowed child types (the array form, order/count-insensitive) or a
//! [`ContentExpr`] parsed from a content-expression string. Expressions compile
//! to an NFA and are matched against a node's child-type sequence; a name in an
//! expression matches a child whose `type` equals it **or** whose
//! [`NodeSpec::group`](crate::NodeSpec) contains it.
//!
//! Supported grammar (the full ProseMirror content-expression language):
//! ```text
//! Choice  := Seq ('|' Seq)*
//! Seq     := Postfix*                  // whitespace-separated; empty allowed
//! Postfix := Atom ('*' | '+' | '?' | '{' n (',' m?)? '}')?
//! Atom    := '(' Choice ')' | Name     // Name = [A-Za-z0-9_-]+
//! ```

use crate::node::Node;
use crate::schema::Schema;
use serde::de::{self, Deserializer, Visitor};
use serde::{Deserialize, Serialize, Serializer};
use std::collections::HashSet;
use std::fmt;

/// Largest explicit repeat bound allowed in `{n}`/`{n,m}` (guards NFA blowup).
const MAX_REPEAT: u32 = 1000;
/// Hard ceiling on compiled NFA states (guards pathological expressions).
const MAX_NFA_STATES: usize = 100_000;

/// Error parsing (or compiling) a content expression.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid content expression at byte {pos}: {msg}")]
pub struct ParseExprError {
    /// Byte offset into the source string where the problem was detected.
    pub pos: usize,
    /// Human-readable reason.
    pub msg: String,
}

// ---- AST ----------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
enum Expr {
    Empty,
    Name(String),
    Seq(Vec<Expr>),
    Choice(Vec<Expr>),
    Star(Box<Expr>),
    Plus(Box<Expr>),
    Opt(Box<Expr>),
    Range {
        min: u32,
        max: Option<u32>,
        inner: Box<Expr>,
    },
}

fn is_name_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '-'
}

struct Parser<'a> {
    input: &'a str,
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str) -> Self {
        Self { input, pos: 0 }
    }
    fn peek(&self) -> Option<char> {
        self.input[self.pos..].chars().next()
    }
    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        Some(c)
    }
    fn skip_ws(&mut self) {
        while let Some(c) = self.peek() {
            if c.is_whitespace() {
                self.pos += c.len_utf8();
            } else {
                break;
            }
        }
    }
    fn err(&self, msg: impl Into<String>) -> ParseExprError {
        ParseExprError {
            pos: self.pos,
            msg: msg.into(),
        }
    }

    fn parse_choice(&mut self) -> Result<Expr, ParseExprError> {
        let mut opts = vec![self.parse_seq()?];
        loop {
            self.skip_ws();
            if self.peek() == Some('|') {
                self.bump();
                opts.push(self.parse_seq()?);
            } else {
                break;
            }
        }
        Ok(if opts.len() == 1 {
            opts.pop().unwrap()
        } else {
            Expr::Choice(opts)
        })
    }

    fn parse_seq(&mut self) -> Result<Expr, ParseExprError> {
        let mut items = Vec::new();
        loop {
            self.skip_ws();
            match self.peek() {
                Some(c) if c == '(' || is_name_char(c) => items.push(self.parse_postfix()?),
                _ => break,
            }
        }
        Ok(match items.len() {
            0 => Expr::Empty,
            1 => items.pop().unwrap(),
            _ => Expr::Seq(items),
        })
    }

    fn parse_postfix(&mut self) -> Result<Expr, ParseExprError> {
        let atom = self.parse_atom()?;
        self.skip_ws();
        match self.peek() {
            Some('*') => {
                self.bump();
                Ok(Expr::Star(Box::new(atom)))
            }
            Some('+') => {
                self.bump();
                Ok(Expr::Plus(Box::new(atom)))
            }
            Some('?') => {
                self.bump();
                Ok(Expr::Opt(Box::new(atom)))
            }
            Some('{') => {
                self.bump();
                let (min, max) = self.parse_range()?;
                Ok(Expr::Range {
                    min,
                    max,
                    inner: Box::new(atom),
                })
            }
            _ => Ok(atom),
        }
    }

    fn parse_atom(&mut self) -> Result<Expr, ParseExprError> {
        self.skip_ws();
        match self.peek() {
            Some('(') => {
                self.bump();
                let e = self.parse_choice()?;
                self.skip_ws();
                if self.peek() != Some(')') {
                    return Err(self.err("expected ')'"));
                }
                self.bump();
                Ok(e)
            }
            Some(c) if is_name_char(c) => {
                let start = self.pos;
                while self.peek().is_some_and(is_name_char) {
                    self.bump();
                }
                Ok(Expr::Name(self.input[start..self.pos].to_string()))
            }
            _ => Err(self.err("expected a name or '('")),
        }
    }

    /// Parse a `{n}` / `{n,}` / `{n,m}` body (the opening `{` already consumed).
    fn parse_range(&mut self) -> Result<(u32, Option<u32>), ParseExprError> {
        self.skip_ws();
        let min = self.parse_num()?;
        self.skip_ws();
        let max = match self.peek() {
            Some(',') => {
                self.bump();
                self.skip_ws();
                match self.peek() {
                    Some('}') => None,
                    Some(c) if c.is_ascii_digit() => Some(self.parse_num()?),
                    _ => return Err(self.err("expected a number or '}' in range")),
                }
            }
            Some('}') => Some(min),
            _ => return Err(self.err("expected ',' or '}' in range")),
        };
        self.skip_ws();
        if self.peek() != Some('}') {
            return Err(self.err("expected '}'"));
        }
        self.bump();
        if min > MAX_REPEAT || max.is_some_and(|m| m > MAX_REPEAT) {
            return Err(self.err(format!("repeat count exceeds cap of {MAX_REPEAT}")));
        }
        if max.is_some_and(|m| m < min) {
            return Err(self.err("range maximum is less than minimum"));
        }
        Ok((min, max))
    }

    fn parse_num(&mut self) -> Result<u32, ParseExprError> {
        let start = self.pos;
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.bump();
        }
        if self.pos == start {
            return Err(self.err("expected a number"));
        }
        self.input[start..self.pos]
            .parse()
            .map_err(|_| self.err("number too large"))
    }
}

fn parse(input: &str) -> Result<Expr, ParseExprError> {
    let mut p = Parser::new(input);
    let e = p.parse_choice()?;
    p.skip_ws();
    if p.pos != input.len() {
        return Err(p.err("unexpected trailing input"));
    }
    Ok(e)
}

// ---- NFA ----------------------------------------------------------------

#[derive(Debug, Clone)]
struct State {
    eps: Vec<usize>,
    edges: Vec<(String, usize)>,
}

#[derive(Debug, Clone)]
struct Nfa {
    states: Vec<State>,
    start: usize,
    accept: usize,
}

struct Builder {
    states: Vec<State>,
}

impl Builder {
    fn new_state(&mut self) -> Result<usize, ParseExprError> {
        if self.states.len() >= MAX_NFA_STATES {
            return Err(ParseExprError {
                pos: 0,
                msg: format!("content expression too large (> {MAX_NFA_STATES} states)"),
            });
        }
        self.states.push(State {
            eps: Vec::new(),
            edges: Vec::new(),
        });
        Ok(self.states.len() - 1)
    }
    fn eps(&mut self, from: usize, to: usize) {
        self.states[from].eps.push(to);
    }

    /// Build a Thompson fragment for `e`, returning its `(start, out)` states.
    fn build(&mut self, e: &Expr) -> Result<(usize, usize), ParseExprError> {
        match e {
            Expr::Empty => {
                let (s, o) = (self.new_state()?, self.new_state()?);
                self.eps(s, o);
                Ok((s, o))
            }
            Expr::Name(n) => {
                let (s, o) = (self.new_state()?, self.new_state()?);
                self.states[s].edges.push((n.clone(), o));
                Ok((s, o))
            }
            Expr::Seq(items) => {
                let s = self.new_state()?;
                let mut cur = s;
                for it in items {
                    let (fs, fo) = self.build(it)?;
                    self.eps(cur, fs);
                    cur = fo;
                }
                Ok((s, cur))
            }
            Expr::Choice(opts) => {
                let (s, o) = (self.new_state()?, self.new_state()?);
                for opt in opts {
                    let (fs, fo) = self.build(opt)?;
                    self.eps(s, fs);
                    self.eps(fo, o);
                }
                Ok((s, o))
            }
            Expr::Star(inner) => {
                let (s, o) = (self.new_state()?, self.new_state()?);
                let (fs, fo) = self.build(inner)?;
                self.eps(s, fs);
                self.eps(s, o);
                self.eps(fo, fs);
                self.eps(fo, o);
                Ok((s, o))
            }
            Expr::Plus(inner) => {
                let (s, o) = (self.new_state()?, self.new_state()?);
                let (fs, fo) = self.build(inner)?;
                self.eps(s, fs);
                self.eps(fo, fs);
                self.eps(fo, o);
                Ok((s, o))
            }
            Expr::Opt(inner) => {
                let (s, o) = (self.new_state()?, self.new_state()?);
                let (fs, fo) = self.build(inner)?;
                self.eps(s, fs);
                self.eps(s, o);
                self.eps(fo, o);
                Ok((s, o))
            }
            Expr::Range { min, max, inner } => {
                let s = self.new_state()?;
                let mut cur = s;
                for _ in 0..*min {
                    let (fs, fo) = self.build(inner)?;
                    self.eps(cur, fs);
                    cur = fo;
                }
                match max {
                    None => {
                        // open `{n,}` => append `inner*` (loop-back, no expansion)
                        let (fs, fo) = self.build(inner)?;
                        let (ss, so) = (self.new_state()?, self.new_state()?);
                        self.eps(ss, fs);
                        self.eps(ss, so);
                        self.eps(fo, fs);
                        self.eps(fo, so);
                        self.eps(cur, ss);
                        cur = so;
                    }
                    Some(m) => {
                        // `(m - min)` optional copies
                        for _ in *min..*m {
                            let (fs, fo) = self.build(inner)?;
                            let (os, oo) = (self.new_state()?, self.new_state()?);
                            self.eps(os, fs);
                            self.eps(os, oo);
                            self.eps(fo, oo);
                            self.eps(cur, os);
                            cur = oo;
                        }
                    }
                }
                Ok((s, cur))
            }
        }
    }
}

fn compile(ast: &Expr) -> Result<Nfa, ParseExprError> {
    let mut b = Builder { states: Vec::new() };
    let (start, accept) = b.build(ast)?;
    Ok(Nfa {
        states: b.states,
        start,
        accept,
    })
}

/// A label matches a child if it equals the child's type or one of its groups.
fn label_matches(label: &str, child_type: &str, schema: &Schema) -> bool {
    label == child_type
        || schema
            .nodes
            .get(child_type)
            .and_then(|spec| spec.group.as_deref())
            .is_some_and(|g| g.split_whitespace().any(|grp| grp == label))
}

impl Nfa {
    fn eps_closure(&self, set: &mut [bool], stack: &mut Vec<usize>) {
        while let Some(s) = stack.pop() {
            for &t in &self.states[s].eps {
                if !set[t] {
                    set[t] = true;
                    stack.push(t);
                }
            }
        }
    }

    fn matches(&self, children: &[Node], schema: &Schema) -> bool {
        let n = self.states.len();
        let mut current = vec![false; n];
        let mut stack = vec![self.start];
        current[self.start] = true;
        self.eps_closure(&mut current, &mut stack);

        for child in children {
            let Some(ct) = child.node_type.as_deref() else {
                return false; // an untyped child can't satisfy a named slot
            };
            let mut next = vec![false; n];
            let mut nstack = Vec::new();
            for (s, &active) in current.iter().enumerate() {
                if active {
                    for (label, dst) in &self.states[s].edges {
                        if !next[*dst] && label_matches(label, ct, schema) {
                            next[*dst] = true;
                            nstack.push(*dst);
                        }
                    }
                }
            }
            self.eps_closure(&mut next, &mut nstack);
            if !next.iter().any(|&b| b) {
                return false; // dead — no state survives this child
            }
            current = next;
        }
        current[self.accept]
    }
}

// ---- public types -------------------------------------------------------

/// A compiled ProseMirror content expression (e.g. `paragraph+`).
///
/// Parse with [`ContentExpr::parse`]; (de)serializes as its source string.
#[derive(Debug, Clone)]
pub struct ContentExpr {
    raw: String,
    ast: Expr,
    nfa: Nfa,
}

impl ContentExpr {
    /// Parse and compile a content-expression string.
    ///
    /// ```
    /// use tiptap_rusty_parser::ContentExpr;
    /// assert!(ContentExpr::parse("paragraph+").is_ok());
    /// assert!(ContentExpr::parse("(a |").is_err());
    /// ```
    pub fn parse(s: &str) -> Result<Self, ParseExprError> {
        let ast = parse(s)?;
        let nfa = compile(&ast)?;
        Ok(Self {
            raw: s.to_string(),
            ast,
            nfa,
        })
    }

    /// The original expression source.
    pub fn as_str(&self) -> &str {
        &self.raw
    }

    /// Whether `children`'s type sequence satisfies this expression.
    pub(crate) fn matches(&self, children: &[Node], schema: &Schema) -> bool {
        self.nfa.matches(children, schema)
    }
}

impl PartialEq for ContentExpr {
    fn eq(&self, other: &Self) -> bool {
        self.ast == other.ast // compare structure, not the compiled NFA
    }
}

impl Serialize for ContentExpr {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.raw)
    }
}

impl<'de> Deserialize<'de> for ContentExpr {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        ContentExpr::parse(&s).map_err(de::Error::custom)
    }
}

/// A node's allowed content: a set of child types (array form) or an ordered
/// content [expression](ContentExpr) (string form).
#[derive(Debug, Clone, PartialEq)]
pub enum ContentRule {
    /// Allowed child types, any count/order. Emits `DisallowedChild`.
    Types(HashSet<String>),
    /// A content expression (cardinality + ordering). Emits `InvalidContent`.
    Expr(ContentExpr),
}

impl Serialize for ContentRule {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            ContentRule::Types(set) => set.serialize(s), // JSON array
            ContentRule::Expr(e) => e.serialize(s),      // JSON string
        }
    }
}

impl<'de> Deserialize<'de> for ContentRule {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct RuleVisitor;
        impl<'de> Visitor<'de> for RuleVisitor {
            type Value = ContentRule;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("an array of child type names or a content-expression string")
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<ContentRule, E> {
                ContentExpr::parse(v)
                    .map(ContentRule::Expr)
                    .map_err(E::custom)
            }
            fn visit_string<E: de::Error>(self, v: String) -> Result<ContentRule, E> {
                self.visit_str(&v)
            }
            fn visit_seq<A: de::SeqAccess<'de>>(self, mut seq: A) -> Result<ContentRule, A::Error> {
                let mut set = HashSet::new();
                while let Some(s) = seq.next_element::<String>()? {
                    set.insert(s);
                }
                Ok(ContentRule::Types(set))
            }
        }
        d.deserialize_any(RuleVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(s: &str) -> Expr {
        Expr::Name(s.to_string())
    }

    #[test]
    fn precedence_and_shape() {
        // `|` binds loosest; sequence by whitespace; postfix binds to its atom.
        assert_eq!(
            parse("a b | c").unwrap(),
            Expr::Choice(vec![Expr::Seq(vec![name("a"), name("b")]), name("c")])
        );
        assert_eq!(
            parse("a b+").unwrap(),
            Expr::Seq(vec![name("a"), Expr::Plus(Box::new(name("b")))])
        );
        assert_eq!(
            parse("(a b)+").unwrap(),
            Expr::Plus(Box::new(Expr::Seq(vec![name("a"), name("b")])))
        );
        assert_eq!(parse("").unwrap(), Expr::Empty);
        assert_eq!(
            parse("h{2,3}").unwrap(),
            Expr::Range {
                min: 2,
                max: Some(3),
                inner: Box::new(name("h")),
            }
        );
        assert_eq!(
            parse("h{2,}").unwrap(),
            Expr::Range {
                min: 2,
                max: None,
                inner: Box::new(name("h")),
            }
        );
    }

    #[test]
    fn range_cap_and_errors() {
        assert!(parse("a{2000}").is_err());
        assert!(parse("a{3,1}").is_err());
        assert!(parse("a**").is_err());
        // serialized form round-trips through ContentExpr
        let e = ContentExpr::parse("(a | b) c*").unwrap();
        assert_eq!(e.as_str(), "(a | b) c*");
    }
}
