//! JSONContent -> Markdown (`to_markdown`).
//!
//! Tiptap StarterKit names (`paragraph`, `heading`, `bulletList`, ...) map to
//! CommonMark + GFM. Schema-agnostic like the rest of the workspace: unknown
//! nodes follow [`UnknownNodePolicy`], unknown marks and unknown attrs are
//! ignored, malformed input never panics.
//!
//! Design notes:
//! - One output `String`; line prefixes (`> `, list indentation) live on a
//!   prefix stack and are applied when a newline is emitted. No per-line
//!   buffers, no post-processing passes, no node clones.
//! - Marks are opened/closed across a whole inline run (so adjacent text nodes
//!   sharing a mark yield one `**...**`), and whitespace at mark boundaries is
//!   moved outside the delimiters.
//! - Text is escaped so the output re-parses to the same text; line-start
//!   escapes (`#`, `-`, `1.`, ...) are only applied where they matter.
//!
//! ```
//! use tiptap_rusty_markdown::{to_markdown, Document};
//! let doc = Document::from_json_str(
//!     r#"{"type":"doc","content":[
//!         {"type":"heading","attrs":{"level":2},"content":[{"type":"text","text":"Hi"}]},
//!         {"type":"paragraph","content":[
//!             {"type":"text","text":"a ","marks":[]},
//!             {"type":"text","text":"b","marks":[{"type":"bold"}]},
//!             {"type":"text","text":"c","marks":[{"type":"bold"}]}]}]}"#,
//! ).unwrap();
//! assert_eq!(to_markdown(&doc), "## Hi\n\na **bc**\n");
//! ```

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt::Write;
use tiptap_rusty_parser::{HtmlOptions, Mark, Node};

/// How to render a node with no Markdown equivalent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UnknownNodePolicy {
    /// Render its children only.
    #[default]
    Transparent,
    /// Emit the node as HTML via the parser's `to_html`.
    Html,
    /// Drop it and its children.
    Skip,
}

/// Export options.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RenderOptions {
    /// How to render unknown node types.
    pub unknown_node: UnknownNodePolicy,
    /// Bullet list marker: `'-'` (default), `'*'` or `'+'`. Anything else
    /// falls back to `'-'`. Adjacent bullet lists alternate with another
    /// marker so they do not merge on re-parse.
    pub bullet: char,
    /// Italic delimiter: `'*'` (default) or `'_'`. Anything else falls back
    /// to `'*'`.
    pub emphasis: char,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            unknown_node: UnknownNodePolicy::Transparent,
            bullet: '-',
            emphasis: '*',
        }
    }
}

/// Render `node` to Markdown with default options.
///
/// ```
/// use tiptap_rusty_markdown::{to_markdown, Document};
/// let doc = Document::from_json_str(
///     r#"{"type":"doc","content":[{"type":"bulletList","content":[
///         {"type":"listItem","content":[{"type":"paragraph","content":[
///             {"type":"text","text":"1. not a list"}]}]}]}]}"#,
/// ).unwrap();
/// assert_eq!(to_markdown(&doc), "- 1\\. not a list\n");
/// ```
pub fn to_markdown(node: &Node) -> String {
    to_markdown_with(node, &RenderOptions::default())
}

/// Render `node` to Markdown with `opts`.
///
/// The output is empty for an empty document, otherwise it ends with exactly
/// one `"\n"`.
pub fn to_markdown_with(node: &Node, opts: &RenderOptions) -> String {
    let mut r = Renderer::new(opts, approx_size(node));
    let mut st = St::default();
    match node.node_type.as_deref() {
        None | Some("doc") => r.blocks(node.children(), Sep::Blank, &mut st),
        _ => r.blocks(std::slice::from_ref(node), Sep::Blank, &mut st),
    }
    let keep = r.out.trim_end_matches('\n').len();
    r.out.truncate(keep);
    if !r.out.is_empty() {
        r.out.push('\n');
    }
    r.out
}

// ---- state --------------------------------------------------------------

/// Block separator inside a container.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Sep {
    /// Blank line between blocks.
    Blank,
    /// Single newline (tight list items).
    Tight,
}

/// Kind of the previous sibling block (for separators and list alternation).
#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum Prev {
    #[default]
    None,
    Bullet,
    Ordered,
    Other,
}

#[derive(Default)]
struct St {
    prev: Prev,
    /// Whether the previous sibling list used the alternate marker.
    alt: bool,
    /// Task item: a first block other than a paragraph must start on the
    /// next line (`- [x] - a` would be text).
    task_line: bool,
    /// Inside a list item (its first paragraph keeps lone images inline).
    item: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Normal paragraph text.
    Normal,
    /// Single-line context (heading, image alt): newlines become spaces.
    Flat,
    /// Table cell: single line, `|` escaped, hard break is `<br>`.
    Cell,
}

struct Renderer<'a> {
    opts: &'a RenderOptions,
    bullet: char,
    emphasis: char,
    out: String,
    /// Continuation prefix applied after every emitted newline.
    prefix: String,
    /// Byte offset where the current line's content starts (after prefix/marker).
    cs: usize,
    /// Content start of the current heading / table cell (leading whitespace
    /// there is stripped on re-parse, like at a line start).
    bol: usize,
    mode: Mode,
    /// Marks currently open in the inline run, outermost first.
    active: Vec<&'a Mark>,
    /// Pending content of an open `code` mark.
    code_buf: String,
    /// Whitespace held back until we know whether marks close first.
    ws: String,
    /// Hard break held back (dropped at the end of a run).
    hb: bool,
    /// Marks of the held-back hard break.
    hb_marks: &'a [Mark],
    /// Whitespace-only text held back after the held-back hard break.
    ws_after: String,
    /// Span (start, end, cs before) of the last run of emitted hard breaks;
    /// `out.len() == end` means nothing followed it yet.
    hb_run: (usize, usize, usize),
    /// Delimiter of the open `italic` mark (differs from `emphasis` when the
    /// opener would abut a closing `*`/`_` run and merge with it).
    em_open: char,
    /// Delimiter of the open `bold` mark (`_` when `**` would merge with a
    /// preceding `*` closer).
    bold_open: char,
    /// `out.len()` right after the last emphasis closer.
    closed_at: usize,
}

const CODE_RANK: u8 = 9;
const SPACES: &str = "                                ";

impl<'a> Renderer<'a> {
    fn new(opts: &'a RenderOptions, cap: usize) -> Self {
        Self {
            opts,
            bullet: if matches!(opts.bullet, '-' | '*' | '+') {
                opts.bullet
            } else {
                '-'
            },
            emphasis: if opts.emphasis == '_' { '_' } else { '*' },
            out: String::with_capacity(cap),
            prefix: String::new(),
            cs: 0,
            bol: usize::MAX,
            mode: Mode::Normal,
            active: Vec::new(),
            code_buf: String::new(),
            ws: String::new(),
            hb: false,
            hb_marks: &[],
            ws_after: String::new(),
            hb_run: (0, usize::MAX, 0),
            em_open: '*',
            bold_open: '*',
            closed_at: usize::MAX,
        }
    }

    // ---- lines ----------------------------------------------------------

    fn newline(&mut self) {
        self.out.push('\n');
        self.out.push_str(&self.prefix);
        self.cs = self.out.len();
    }

    /// Empty line; prefix trailing spaces trimmed (`>` not `> `).
    fn blank_line(&mut self) {
        self.out.push('\n');
        self.out.push_str(self.prefix.trim_end());
    }

    /// Newline, but without trailing prefix whitespace when the next line is empty.
    fn newline_for(&mut self, next_empty: bool) {
        if next_empty {
            self.blank_line();
            self.cs = self.out.len();
        } else {
            self.newline();
        }
    }

    fn at_line_start(&self) -> bool {
        self.out
            .as_bytes()
            .get(self.cs..)
            .is_some_and(|t| t.iter().all(|&b| b == b' '))
    }

    fn separate(&mut self, sep: Sep) {
        if sep == Sep::Blank {
            self.blank_line();
        }
        self.newline();
    }

    fn begin(&mut self, sep: Sep, st: &St) {
        if st.prev != Prev::None {
            self.separate(sep);
        } else if st.task_line {
            if self.out.ends_with(' ') {
                self.out.pop();
            }
            self.newline();
        }
    }

    /// Write multi-line raw text, applying the prefix on each line.
    fn raw(&mut self, s: &str) {
        for (i, line) in s.split('\n').enumerate() {
            if i > 0 {
                self.newline_for(line.is_empty());
            }
            self.out.push_str(line);
        }
    }

    fn html(&self, n: &Node) -> String {
        n.to_html_with(&HtmlOptions {
            unknown_node: tiptap_rusty_parser::UnknownNodePolicy::DataTypeDiv,
            ..HtmlOptions::default()
        })
    }

    // ---- blocks ---------------------------------------------------------

    fn blocks(&mut self, nodes: &'a [Node], sep: Sep, st: &mut St) {
        let mut i = 0;
        while i < nodes.len() {
            if is_inline_kind(&nodes[i]) {
                // a block-level image is its own paragraph
                let mut j = i + 1;
                if nodes[i].node_type.as_deref() != Some("image") {
                    while j < nodes.len() && is_run_kind(&nodes[j]) {
                        j += 1;
                    }
                }
                self.paragraph(&nodes[i..j], sep, st);
                i = j;
            } else {
                self.block(&nodes[i], sep, st);
                i += 1;
            }
        }
    }

    /// An inline run as a paragraph; leaves no trace when it renders to nothing.
    fn paragraph(&mut self, nodes: &'a [Node], sep: Sep, st: &mut St) {
        if nodes.is_empty() {
            return;
        }
        // The importer turns a paragraph of only images (and breaks /
        // whitespace) into block images, except as an item's first block:
        // write them as such so a re-export is stable.
        if nodes.len() > 1 && !(st.item && st.prev == Prev::None) && is_hoistable_images(nodes) {
            for n in nodes {
                if n.node_type.as_deref() == Some("image") {
                    self.paragraph(std::slice::from_ref(n), sep, st);
                }
            }
            return;
        }
        let (len, cs) = (self.out.len(), self.cs);
        if st.prev != Prev::None {
            self.separate(sep);
        }
        let at = self.out.len();
        self.inline(nodes);
        if self.out.len() == at {
            self.out.truncate(len);
            self.cs = cs;
        } else {
            st.prev = Prev::Other;
        }
    }

    fn block(&mut self, n: &'a Node, sep: Sep, st: &mut St) {
        let ty = match n.node_type.as_deref() {
            Some(t) => t,
            None => return self.blocks(n.children(), sep, st),
        };
        match ty {
            "paragraph" => {
                self.paragraph(n.children(), sep, st);
            }
            "heading" => {
                self.begin(sep, st);
                let level = attr_u64(n, "level").unwrap_or(1).clamp(1, 6) as usize;
                for _ in 0..level {
                    self.out.push('#');
                }
                self.out.push(' ');
                let content_at = self.out.len();
                self.bol = content_at;
                let prev = std::mem::replace(&mut self.mode, Mode::Flat);
                self.inline(n.children());
                self.mode = prev;
                self.bol = usize::MAX;
                if self.out.len() == content_at {
                    self.out.pop();
                } else if self.out.ends_with('#') && !self.out.ends_with("\\#") {
                    // a trailing `#` run would read as a closing sequence
                    let l = self.out.len() - 1;
                    self.out.insert(l, '\\');
                }
                st.prev = Prev::Other;
            }
            "blockquote" => {
                self.begin(sep, st);
                self.out.push_str("> ");
                self.cs = self.out.len();
                let at = self.out.len();
                let pl = self.prefix.len();
                self.prefix.push_str("> ");
                let mut inner = St::default();
                self.blocks(n.children(), Sep::Blank, &mut inner);
                self.prefix.truncate(pl);
                if self.out.len() == at {
                    self.out.pop();
                }
                st.prev = Prev::Other;
            }
            "horizontalRule" => {
                self.begin(sep, st);
                // `- ---` / `* * **` read as one thematic break: use a rule
                // char absent from the markers on this line.
                let line = &self.out.as_bytes()[self.out.rfind('\n').map_or(0, |i| i + 1)..];
                let rule = if !line.contains(&b'-') {
                    "---"
                } else if !line.contains(&b'*') {
                    "***"
                } else {
                    "___"
                };
                self.out.push_str(rule);
                st.prev = Prev::Other;
            }
            "codeBlock" => {
                self.begin(sep, st);
                self.code_block(n);
                st.prev = Prev::Other;
            }
            "bulletList" | "taskList" => {
                if n.children().is_empty() {
                    return;
                }
                self.begin(list_sep(n, false, sep), st);
                let alt = st.prev == Prev::Bullet && !st.alt;
                self.list(n, false, alt);
                (st.prev, st.alt) = (Prev::Bullet, alt);
            }
            "orderedList" => {
                if n.children().is_empty() {
                    return;
                }
                self.begin(list_sep(n, true, sep), st);
                let alt = st.prev == Prev::Ordered && !st.alt;
                self.list(n, true, alt);
                (st.prev, st.alt) = (Prev::Ordered, alt);
            }
            "table" => {
                if n.children().is_empty() {
                    return;
                }
                self.begin(sep, st);
                if pipe_table_ok(n) {
                    self.table(n);
                } else {
                    let h = table_html(n);
                    self.raw(&h);
                }
                st.prev = Prev::Other;
            }
            // structural containers outside their parent: just their content
            "doc" | "listItem" | "taskItem" | "tableRow" | "tableCell" | "tableHeader" => {
                self.blocks(n.children(), sep, st)
            }
            _ => match self.opts.unknown_node {
                UnknownNodePolicy::Skip => {}
                UnknownNodePolicy::Transparent => self.blocks(n.children(), sep, st),
                UnknownNodePolicy::Html => {
                    self.begin(sep, st);
                    let h = self.html(n);
                    self.raw(&h);
                    st.prev = Prev::Other;
                }
            },
        }
    }

    fn code_block(&mut self, n: &Node) {
        // longest backtick / tilde run across the content (carried over piece borders)
        let (mut cur, mut max, mut any) = ([0usize; 2], [0usize; 2], false);
        walk_text(n, &mut |s| {
            any |= !s.is_empty();
            for b in s.bytes() {
                let k = match b {
                    b'`' => 0,
                    b'~' => 1,
                    _ => {
                        cur = [0; 2];
                        continue;
                    }
                };
                cur[k] += 1;
                cur[1 - k] = 0;
                max[k] = max[k].max(cur[k]);
            }
        });
        // the importer keeps the first info-string word only
        let lang = match n.attr("language") {
            Some(Value::String(l)) => l.split_whitespace().next().unwrap_or(""),
            _ => "",
        };
        // a backtick fence's info string cannot contain backticks
        let (fc, run) = if lang.contains('`') {
            ('~', max[1])
        } else {
            ('`', max[0])
        };
        let fence = (run + 1).max(3);
        for _ in 0..fence {
            self.out.push(fc);
        }
        let lb = lang.as_bytes();
        for (i, c) in lang.char_indices() {
            match c {
                '\n' | '\r' => {}
                '\\' => self.out.push_str("\\\\"),
                '&' if entity_like(&lb[i + 1..]) => self.out.push_str("\\&"),
                _ => self.out.push(c),
            }
        }
        self.newline();
        walk_text(n, &mut |s| {
            for (i, seg) in s.split('\n').enumerate() {
                if i > 0 {
                    self.newline_for(seg.is_empty());
                }
                self.out.push_str(seg);
            }
        });
        if any {
            self.newline();
        }
        for _ in 0..fence {
            self.out.push(fc);
        }
    }

    // ---- lists ----------------------------------------------------------

    fn list(&mut self, n: &'a Node, ordered: bool, alt: bool) {
        let items = n.children();
        let sep = if is_tight(items) {
            Sep::Tight
        } else {
            Sep::Blank
        };
        let mut num = attr_u64(n, "start").unwrap_or(1);
        let bullet = if alt {
            if self.bullet == '-' {
                '*'
            } else {
                '-'
            }
        } else {
            self.bullet
        };
        // `- - -` (nested lists opened on one line) reads as a thematic break
        let bullet = if ordered {
            bullet
        } else {
            let line = &self.out.as_bytes()[self.out.rfind('\n').map_or(0, |i| i + 1)..];
            let count = |c: char| line.iter().filter(|&&b| b == c as u8).count();
            [bullet, '-', '*', '+']
                .into_iter()
                .find(|&c| count(c) < 2)
                .unwrap_or(bullet)
        };
        let delim = if alt { ')' } else { '.' };
        for (i, item) in items.iter().enumerate() {
            if i > 0 {
                self.separate(sep);
            }
            let kids: &[Node] = if is_item(item) {
                item.children()
            } else {
                std::slice::from_ref(item)
            };
            let before = self.out.len();
            if ordered {
                let _ = write!(self.out, "{num}{delim} ");
                num = num.wrapping_add(1);
            } else {
                self.out.push(bullet);
                self.out.push(' ');
            }
            let width = self.out.len() - before;
            let marker_end = self.out.len();
            if item.node_type.as_deref() == Some("taskItem") {
                let checked = item.attr("checked").and_then(Value::as_bool) == Some(true);
                self.out.push_str(if checked { "[x] " } else { "[ ] " });
            }
            self.cs = self.out.len();
            let pl = self.prefix.len();
            for _ in 0..width / SPACES.len() {
                self.prefix.push_str(SPACES);
            }
            self.prefix.push_str(&SPACES[..width % SPACES.len()]);
            let mut inner = St {
                task_line: item.node_type.as_deref() == Some("taskItem"),
                item: true,
                ..St::default()
            };
            self.blocks(kids, sep, &mut inner);
            self.prefix.truncate(pl);
            if self.out.len() == marker_end {
                self.out.pop(); // empty item: `-` without trailing space
            }
        }
    }

    // ---- tables ---------------------------------------------------------

    fn table(&mut self, n: &'a Node) {
        let rows = n.children();
        let cols = rows.iter().map(|r| r.children().len()).max().unwrap_or(0);
        for (ri, row) in rows.iter().enumerate() {
            if ri > 0 {
                self.newline();
            }
            self.out.push('|');
            let cells = row.children();
            for ci in 0..cols {
                self.out.push(' ');
                if let Some(cell) = cells.get(ci) {
                    self.bol = self.out.len();
                    let prev = std::mem::replace(&mut self.mode, Mode::Cell);
                    if let Some(p) = cell.children().first() {
                        self.inline(p.children());
                    }
                    self.mode = prev;
                    self.bol = usize::MAX;
                }
                self.out.push_str(" |");
            }
            if ri == 0 {
                self.newline();
                self.out.push('|');
                for _ in 0..cols {
                    self.out.push_str(" --- |");
                }
            }
        }
    }

    // ---- inline runs ----------------------------------------------------

    fn inline(&mut self, nodes: &'a [Node]) {
        self.active.clear();
        self.ws.clear();
        self.ws_after.clear();
        self.hb = false;
        self.items(nodes);
        self.close_not_in(&[], true);
        // trailing whitespace is stripped on re-parse, except non-ASCII (NBSP, ...)
        let ascii_ws = [' ', '\t', '\n', '\r'];
        let keep = self.ws_after.trim_end_matches(ascii_ws).len();
        if self.hb && keep > 0 {
            self.ws_after.truncate(keep);
            self.flush_pending();
        } else {
            let keep = self.ws.trim_end_matches(ascii_ws).len();
            if keep > 0 {
                self.ws.truncate(keep);
                self.flush_ws();
            }
        }
        // breaks at the end of the run would leave a literal `\\`
        self.take_hb_run();
        self.ws.clear();
        self.ws_after.clear();
        self.hb = false;
    }

    fn items(&mut self, nodes: &'a [Node]) {
        let mut i = 0;
        while i < nodes.len() {
            let n = &nodes[i];
            i += 1;
            match n.node_type.as_deref() {
                Some("text") => {
                    // Adjacent texts with equal marks re-import as one node:
                    // escape them as one (line-start / `<` / `&` lookahead).
                    // (In a single-line context a break is a space: absorb it.)
                    let marks = n.marks.as_deref().unwrap_or(&[]);
                    let flat = self.mode == Mode::Flat;
                    let same = |m: &Node| {
                        matches!(m.node_type.as_deref(), Some("text"))
                            || (flat && m.node_type.as_deref() == Some("hardBreak"))
                    } && m.marks.as_deref().unwrap_or(&[]) == marks;
                    let j = i + nodes[i..].iter().take_while(|m| same(m)).count();
                    if j == i {
                        self.text_item(n.text.as_deref().unwrap_or(""), marks);
                    } else {
                        let mut s = String::from(n.text.as_deref().unwrap_or(""));
                        for m in &nodes[i..j] {
                            s.push_str(m.text.as_deref().unwrap_or(" "));
                        }
                        self.text_item(&s, marks);
                        i = j;
                    }
                }
                Some("hardBreak") => self.hard_break(n),
                Some("image") => self.image(n),
                None => self.items(n.children()),
                Some(ty) if is_known_container(ty) => self.items(n.children()),
                Some(_) => match self.opts.unknown_node {
                    UnknownNodePolicy::Skip => {}
                    UnknownNodePolicy::Transparent => self.items(n.children()),
                    UnknownNodePolicy::Html => {
                        self.close_not_in(&[], false);
                        self.flush_pending();
                        let h = self.html(n);
                        let prev = self.mode;
                        if prev == Mode::Normal {
                            self.raw(&h);
                        } else {
                            self.out.push_str(&h.replace('\n', " "));
                        }
                    }
                },
            }
        }
    }

    fn text_item(&mut self, text: &str, marks: &'a [Mark]) {
        if text.is_empty() {
            return;
        }
        if marks.iter().any(|m| m.mark_type == "code") {
            self.close_not_in(marks, true);
            // code is innermost: a mark opening now must not land inside it
            if self.active.last().is_some_and(|a| a.mark_type == "code")
                && marks.iter().any(|m| {
                    m.mark_type != "code"
                        && mark_rank(m).is_some()
                        && !self.active.iter().any(|a| a.mark_type == m.mark_type)
                })
            {
                if let Some(c) = self.active.pop() {
                    self.close_mark(c);
                }
            }
            self.open_after_pending(marks, true, "", '`');
            self.code_buf.extend(
                text.chars()
                    .map(|c| if matches!(c, '\n' | '\r') { ' ' } else { c }),
            );
            return;
        }
        let body = text.trim_start_matches(is_ws);
        let lead = &text[..text.len() - body.len()];
        let core = body.trim_end_matches(is_ws);
        self.close_not_in(marks, true);
        if core.is_empty() {
            // `[ ](u)` is still a link: open it around the whitespace
            let link = marks
                .iter()
                .find(|m| m.mark_type == "link" && mark_rank(m).is_some());
            if let Some(link) = link {
                if !self.active.iter().any(|a| a.mark_type == "link") {
                    self.open_after_pending(std::slice::from_ref(link), true, "", '[');
                    self.escape_text(text);
                    return;
                }
            }
            if self.hb {
                self.ws_after.push_str(text);
            } else {
                self.ws.push_str(text);
            }
            return;
        }
        self.open_after_pending(marks, true, lead, core.chars().next().unwrap_or(' '));
        self.escape_text(core);
        self.ws.push_str(&body[core.len()..]);
    }

    fn hard_break(&mut self, n: &'a Node) {
        let marks = n.marks.as_deref().unwrap_or(&[]);
        self.close_not_in(marks, false);
        match self.mode {
            Mode::Normal => {
                if self.hb {
                    // consecutive breaks: emit the earlier one (and the
                    // whitespace before it) now
                    self.flush_pending();
                }
                // a break inside link text opens the link (no flanking rules)
                let link = marks
                    .iter()
                    .find(|m| m.mark_type == "link" && mark_rank(m).is_some());
                if let Some(link) = link {
                    if !self.active.iter().any(|a| a.mark_type == "link") {
                        self.open_after_pending(std::slice::from_ref(link), false, "", '[');
                    }
                }
                self.hb = true;
                self.hb_marks = marks;
            }
            // nothing before it: the space would only be stripped
            Mode::Flat if self.out.len() == self.bol && self.ws.is_empty() => {}
            Mode::Flat => self.ws.push(' '),
            Mode::Cell => {
                self.flush_pending();
                self.out.push_str("<br>");
            }
        }
    }

    fn image(&mut self, n: &'a Node) {
        let marks = n.marks.as_deref().unwrap_or(&[]);
        self.close_not_in(marks, false);
        self.open_after_pending(marks, false, "", '!');
        self.out.push_str("![");
        if let Some(alt) = n.attr("alt").and_then(Value::as_str) {
            let prev = self.mode;
            if prev == Mode::Normal {
                self.mode = Mode::Flat;
            }
            self.escape_text(alt);
            self.mode = prev;
        }
        self.out.push_str("](");
        write_dest(
            &mut self.out,
            n.attr("src").and_then(Value::as_str).unwrap_or(""),
        );
        if let Some(t) = n.attr("title").and_then(Value::as_str) {
            write_title(&mut self.out, t);
        }
        self.out.push(')');
    }

    /// Flush held-back whitespace / hard break, then `lead` and the opens of
    /// `marks` (`next` = first char after them). A held-back hard break that
    /// carries every newly opened mark goes inside them (`**\\\nb**`).
    fn open_after_pending(&mut self, marks: &'a [Mark], text: bool, lead: &str, next: char) {
        self.flush_ws();
        let from = self.collect_opens(marks, text);
        let mut mid = from;
        if self.hb {
            // new marks the break also carries open (outermost) before it;
            // delimiter nesting order is free, import re-canonicalizes it
            let hbm = self.hb_marks;
            let carried = |m: &Mark| m.mark_type != "code" && hbm.contains(m);
            let new = &mut self.active[from..];
            if new.iter().any(|m| carried(m)) {
                new.sort_by_key(|m| !carried(m));
                mid += new.iter().take_while(|m| carried(m)).count();
                self.write_opens(from, mid, '\\');
            }
        }
        self.flush_hb();
        if !lead.is_empty() {
            self.escape_text(lead);
        }
        self.write_opens(mid, self.active.len(), next);
    }

    fn flush_pending(&mut self) {
        self.flush_ws();
        self.flush_hb();
    }

    fn flush_ws(&mut self) {
        if !self.ws.is_empty() {
            let w = std::mem::take(&mut self.ws);
            self.escape_text(&w);
            self.ws = w;
            self.ws.clear();
        }
    }

    fn flush_hb(&mut self) {
        self.emit_hb(false);
    }

    /// Emit the held-back break as `\\` + newline (or two trailing spaces).
    fn emit_hb(&mut self, spaces: bool) {
        if self.hb {
            self.hb = false;
            if self.out.len() != self.hb_run.1 {
                self.hb_run.0 = self.out.len();
                self.hb_run.2 = self.cs;
            }
            self.out.push_str(if spaces { "  " } else { "\\" });
            self.newline();
            self.hb_run.1 = self.out.len();
            if !self.ws_after.is_empty() {
                let w = std::mem::take(&mut self.ws_after);
                self.escape_text(&w);
                self.ws_after = w;
                self.ws_after.clear();
            }
        }
    }

    // ---- marks ----------------------------------------------------------

    /// Remove a run of hard breaks nothing followed yet; returns it.
    fn take_hb_run(&mut self) -> Option<String> {
        if self.out.len() != self.hb_run.1 {
            return None;
        }
        let tail = self.out.split_off(self.hb_run.0);
        self.cs = self.hb_run.2;
        self.hb_run.1 = usize::MAX;
        Some(tail)
    }

    /// Close open marks (innermost first) from the first one `marks` lacks.
    fn close_not_in(&mut self, marks: &[Mark], text: bool) {
        let mut i = 0;
        while i < self.active.len() {
            let a = self.active[i];
            if (text || a.mark_type != "code") && marks.iter().any(|m| m == a) {
                i += 1;
            } else {
                break;
            }
        }
        // a delimiter closer at a line start (right after a break) would not
        // close: close before the breaks (`](u)` / `</u>` / code are fine)
        let delim = self
            .active
            .last()
            .is_some_and(|m| matches!(m.mark_type.as_str(), "bold" | "italic" | "strike"));
        let breaks = if self.active.len() > i && delim {
            self.take_hb_run()
        } else {
            None
        };
        while self.active.len() > i {
            if let Some(m) = self.active.pop() {
                if m.mark_type == "link" {
                    // whitespace (and a break) held back from inside the link
                    // text stays inside: `[a ](u)` is fine, unlike `**a **`.
                    // A `\\` break right before `]` after a delimiter closer is
                    // read as text, so newlines become spaces / the break uses
                    // trailing spaces.
                    if self.ws.contains(['\n', '\r']) {
                        self.ws = self.ws.replace(['\n', '\r'], " ");
                    }
                    self.flush_ws();
                    if self.hb && self.hb_marks.contains(m) {
                        let spaces = self.out.len() == self.closed_at;
                        self.emit_hb(spaces);
                    }
                }
                self.close_mark(m);
            }
        }
        if let Some(b) = breaks {
            self.hb_run.0 = self.out.len();
            self.out.push_str(&b);
            self.cs = self.out.len();
            self.hb_run.1 = self.out.len();
        }
    }

    /// Push marks of the node not yet open; returns the index of the first new one.
    fn collect_opens(&mut self, marks: &'a [Mark], text: bool) -> usize {
        let from = self.active.len();
        for m in marks {
            let Some(rank) = mark_rank(m) else { continue };
            if rank == CODE_RANK && !text {
                continue;
            }
            if self.active.iter().any(|a| a.mark_type == m.mark_type) {
                continue;
            }
            self.active.push(m);
        }
        if self.active.len() - from > 1 {
            self.active[from..].sort_by_key(|m| mark_rank(m).unwrap_or(0));
        }
        from
    }

    /// Open `active[from..to]`; `next` is the first char written after them.
    fn write_opens(&mut self, from: usize, to: usize, next: char) {
        if from >= to {
            return;
        }
        let start = self.out.len();
        for i in from..to {
            let m = self.active[i];
            self.open_mark(m);
        }
        // The leading delimiter run must be left-flanking (and for `_` not
        // also right-flanking): a word char before it breaks that when the
        // run is followed by punctuation, or always for `_`. Encode that char.
        let b = &self.out.as_bytes()[start..];
        let Some(&c) = b.first().filter(|c| matches!(c, b'*' | b'_' | b'~')) else {
            return;
        };
        let run = b.iter().take_while(|&&x| x == c).count();
        let n = b.get(run).map_or(next, |&x| x as char);
        if let Some(p) = self.out[..start].chars().next_back() {
            if maybe_word(p) && (maybe_punct(n) || c == b'_') {
                let mut e = String::with_capacity(12);
                let _ = write!(e, "&#{};", p as u32);
                self.out.replace_range(start - p.len_utf8()..start, &e);
                self.closed_at = usize::MAX;
            }
        }
    }

    fn open_mark(&mut self, m: &Mark) {
        match m.mark_type.as_str() {
            "link" => {
                // `!` + `[` would read as an image
                if ends_unescaped(&self.out, b'!') {
                    let l = self.out.len() - 1;
                    self.out.insert(l, '\\');
                }
                self.out.push('[');
            }
            "bold" => {
                // `*a*` + `**b**` would merge into one `***` run
                let c = if self.out.len() == self.closed_at && self.out.ends_with('*') {
                    '_'
                } else {
                    '*'
                };
                self.bold_open = c;
                self.out.push(c);
                self.out.push(c);
            }
            "italic" => {
                // `**a*b****c*`: a closer run directly followed by an opener
                // of the same char merges into one run; switch delimiter.
                let mut c = self.emphasis;
                if self.out.len() == self.closed_at && self.out.ends_with(c) {
                    c = if c == '*' { '_' } else { '*' };
                }
                self.em_open = c;
                self.out.push(c);
            }
            "strike" => self.out.push_str("~~"),
            "underline" => self.out.push_str("<u>"),
            "subscript" => self.out.push_str("<sub>"),
            "superscript" => self.out.push_str("<sup>"),
            "code" => self.code_buf.clear(),
            _ => {}
        }
    }

    fn close_mark(&mut self, m: &Mark) {
        match m.mark_type.as_str() {
            "link" => {
                self.out.push_str("](");
                write_dest(&mut self.out, mark_attr(m, "href").unwrap_or(""));
                if let Some(t) = mark_attr(m, "title") {
                    write_title(&mut self.out, t);
                }
                self.out.push(')');
            }
            "bold" => {
                self.out.push(self.bold_open);
                self.out.push(self.bold_open);
                self.closed_at = self.out.len();
            }
            "italic" => {
                self.out.push(self.em_open);
                self.closed_at = self.out.len();
            }
            "strike" => {
                self.out.push_str("~~");
                self.closed_at = self.out.len();
            }
            "underline" => self.out.push_str("</u>"),
            "subscript" => self.out.push_str("</sub>"),
            "superscript" => self.out.push_str("</sup>"),
            "code" => self.flush_code(),
            _ => {}
        }
    }

    fn flush_code(&mut self) {
        let s = std::mem::take(&mut self.code_buf);
        // smallest backtick count that does not equal any run in the content
        let (mut run, mut seen) = (0u32, 0u64);
        for b in s.bytes().chain(std::iter::once(b' ')) {
            if b == b'`' {
                run += 1;
            } else {
                if run > 0 && run < 64 {
                    seen |= 1 << run;
                }
                run = 0;
            }
        }
        let n = (1..64).find(|k| seen & (1 << k) == 0).unwrap_or(1);
        let pad = s.starts_with('`')
            || s.ends_with('`')
            || (s.starts_with(' ') && s.ends_with(' ') && s.bytes().any(|b| b != b' '));
        for _ in 0..n {
            self.out.push('`');
        }
        if pad {
            self.out.push(' ');
        }
        if self.mode == Mode::Cell && s.contains('|') {
            self.out.push_str(&s.replace('|', "\\|"));
        } else {
            self.out.push_str(&s);
        }
        if pad {
            self.out.push(' ');
        }
        for _ in 0..n {
            self.out.push('`');
        }
        self.code_buf = s;
        self.code_buf.clear();
    }

    // ---- text -----------------------------------------------------------

    fn escape_text(&mut self, s: &str) {
        let b = s.as_bytes();
        let mut ls = self.at_line_start();
        // leading whitespace would be stripped: encode the first char
        let mut lead = ls || self.out.len() == self.bol;
        let (mut last, mut i) = (0, 0);
        if self.out.len() == self.closed_at {
            // The delimiter run just closed must stay right-flanking (and for
            // `_` not also left-flanking): a word char after it breaks that
            // when the run follows punctuation, or always for `_`.
            if let Some(n) = s.chars().next().filter(|&n| maybe_word(n)) {
                let ob = self.out.as_bytes();
                let c = ob[ob.len() - 1];
                let run = ob.iter().rev().take_while(|&&x| x == c).count();
                let p = self.out[..ob.len() - run].chars().next_back();
                if c == b'_' || p.is_some_and(maybe_punct) {
                    let _ = write!(self.out, "&#{};", n as u32);
                    (last, i, lead) = (n.len_utf8(), n.len_utf8(), false);
                }
            }
        }
        while i < b.len() {
            let c = b[i];
            if lead && self.mode != Mode::Normal && (c == b'\n' || c == b'\r') {
                // would be a stripped space here
                self.out.push_str(&s[last..i]);
                last = i + 1;
                i += 1;
                continue;
            }
            if lead && (c == b' ' || c == b'\t') {
                self.out.push_str(&s[last..i]);
                self.out.push_str(if c == b' ' { "&#32;" } else { "&#9;" });
                (last, ls, lead) = (i + 1, false, false);
                i += 1;
                continue;
            }
            lead = false;
            let esc = match c {
                b'\\' | b'*' | b'`' | b'[' | b']' | b'~' => true,
                b'_' => {
                    let w = |x: Option<&u8>| x.is_some_and(|x| x.is_ascii_alphanumeric());
                    !(i > 0 && w(b.get(i - 1)) && w(b.get(i + 1)))
                }
                // at the end of the piece the next char is unknown (it may
                // come from a node with marks that render no delimiter)
                b'<' => b
                    .get(i + 1)
                    .is_none_or(|x| x.is_ascii_alphabetic() || matches!(x, b'/' | b'!' | b'?')),
                b'&' => i + 1 == b.len() || entity_like(&b[i + 1..]),
                b'|' => self.mode == Mode::Cell,
                b'#' if ls => {
                    let mut j = i;
                    while j < b.len() && b[j] == b'#' {
                        j += 1;
                    }
                    j - i <= 6 && b.get(j).is_none_or(|x| matches!(x, b' ' | b'\t'))
                }
                b'>' if ls => true,
                b'-' | b'+' if ls => b
                    .get(i + 1)
                    .is_none_or(|x| matches!(x, b' ' | b'\t') || (c == b'-' && *x == b'-')),
                b'=' if ls => b
                    .get(i + 1)
                    .is_none_or(|x| matches!(x, b' ' | b'\t' | b'=')),
                b'0'..=b'9' if ls => {
                    let mut j = i;
                    while j < b.len() && b[j].is_ascii_digit() {
                        j += 1;
                    }
                    if j - i <= 9
                        && matches!(b.get(j), Some(b'.' | b')'))
                        && b.get(j + 1).is_none_or(|x| matches!(x, b' ' | b'\t'))
                    {
                        self.out.push_str(&s[last..j]);
                        self.out.push('\\');
                        last = j;
                    }
                    ls = false;
                    i = j;
                    continue;
                }
                b'\r' if b.get(i + 1) == Some(&b'\n') => {
                    // `\r\n`: one line break, emitted at the `\n`
                    self.out.push_str(&s[last..i]);
                    last = i + 1;
                    i += 1;
                    continue;
                }
                b'\n' | b'\r' => {
                    self.out.push_str(&s[last..i]);
                    last = i + 1;
                    if self.mode == Mode::Normal {
                        // a bare newline would reimport as a space
                        self.out.push('\\');
                        self.newline();
                        (ls, lead) = (true, true);
                    } else {
                        self.out.push(' ');
                    }
                    i += 1;
                    continue;
                }
                _ => false,
            };
            if esc {
                self.out.push_str(&s[last..i]);
                self.out.push('\\');
                last = i;
            }
            if c != b' ' && c != b'\t' {
                ls = false;
            }
            i += 1;
        }
        self.out.push_str(&s[last..]);
    }
}

// ---- helpers ------------------------------------------------------------

/// `out` ends with `c` not preceded by an odd number of backslashes.
fn ends_unescaped(out: &str, c: u8) -> bool {
    let b = out.as_bytes();
    let Some((&last, rest)) = b.split_last() else {
        return false;
    };
    last == c && rest.iter().rev().take_while(|&&x| x == b'\\').count() % 2 == 0
}

/// Not certainly whitespace or punctuation (for delimiter flanking).
fn maybe_word(c: char) -> bool {
    !c.is_whitespace() && !c.is_ascii_punctuation()
}

/// Not certainly whitespace or alphanumeric (for delimiter flanking).
fn maybe_punct(c: char) -> bool {
    !c.is_whitespace() && !c.is_alphanumeric()
}

/// Whitespace for delimiter flanking (Unicode, like CommonMark).
fn is_ws(c: char) -> bool {
    c.is_whitespace()
}

/// Unmarked images, hard breaks and whitespace only, with at least one image
/// (mirrors the importer's hoisting rule).
fn is_hoistable_images(nodes: &[Node]) -> bool {
    let mut any = false;
    for c in nodes {
        match c.node_type.as_deref() {
            Some("image") if c.marks.as_ref().is_none_or(|m| m.is_empty()) => any = true,
            Some("hardBreak") => {}
            Some("text") if c.text.as_deref().is_none_or(|t| t.trim().is_empty()) => {}
            _ => return false,
        }
    }
    any
}

fn is_inline_kind(n: &Node) -> bool {
    matches!(n.node_type.as_deref(), Some("text" | "hardBreak" | "image"))
}

/// Kinds that continue a paragraph run (everything inline except images).
fn is_run_kind(n: &Node) -> bool {
    matches!(n.node_type.as_deref(), Some("text" | "hardBreak"))
}

/// Real HTML table for tables GFM cannot express (spans, block cells).
fn table_html(n: &Node) -> String {
    let mut o = HtmlOptions {
        unknown_node: tiptap_rusty_parser::UnknownNodePolicy::DataTypeDiv,
        spread_attrs: true,
        ..HtmlOptions::default()
    };
    for (k, v) in [
        ("table", "table"),
        ("tableRow", "tr"),
        ("tableCell", "td"),
        ("tableHeader", "th"),
    ] {
        o.node_tags.insert(k.into(), v.into());
    }
    n.to_html_with(&o)
}

fn is_item(n: &Node) -> bool {
    matches!(n.node_type.as_deref(), Some("listItem" | "taskItem"))
}

fn is_list(n: &Node) -> bool {
    matches!(
        n.node_type.as_deref(),
        Some("bulletList" | "orderedList" | "taskList")
    )
}

/// Node types with built-in block handling (flattened when met inside an inline run).
fn is_known_container(ty: &str) -> bool {
    matches!(
        ty,
        "doc"
            | "paragraph"
            | "heading"
            | "blockquote"
            | "bulletList"
            | "orderedList"
            | "taskList"
            | "listItem"
            | "taskItem"
            | "codeBlock"
            | "table"
            | "tableRow"
            | "tableCell"
            | "tableHeader"
    )
}

/// Tight: every item is at most one paragraph (first) followed only by nested lists.
fn is_tight(items: &[Node]) -> bool {
    items.iter().all(|item| {
        let kids: &[Node] = if is_item(item) {
            item.children()
        } else {
            return false;
        };
        kids.iter()
            .enumerate()
            .all(|(i, k)| is_list(k) || (i == 0 && k.node_type.as_deref() == Some("paragraph")))
    })
}

/// Separator before a list: a list starting with an empty item or (ordered)
/// not at 1 cannot interrupt a paragraph, so it needs a blank line.
fn list_sep(list: &Node, ordered: bool, sep: Sep) -> Sep {
    if sep == Sep::Blank {
        return sep;
    }
    let empty_first = list.children().first().is_some_and(|it| {
        it.node_type.as_deref() == Some("listItem")
            && match it.children() {
                [] => true,
                [p] => p.node_type.as_deref() == Some("paragraph") && p.children().is_empty(),
                _ => false,
            }
    });
    if empty_first || (ordered && attr_u64(list, "start").unwrap_or(1) != 1) {
        Sep::Blank
    } else {
        sep
    }
}

fn pipe_table_ok(table: &Node) -> bool {
    let span_ok = |c: &Node| {
        ["colspan", "rowspan"]
            .iter()
            .all(|k| attr_u64(c, k).is_none_or(|v| v <= 1))
    };
    table.children().iter().all(|row| {
        row.node_type.as_deref() == Some("tableRow")
            && !row.children().is_empty()
            && row.children().iter().all(|cell| {
                matches!(cell.node_type.as_deref(), Some("tableCell" | "tableHeader"))
                    && span_ok(cell)
                    && match cell.children() {
                        [] => true,
                        [p] => {
                            p.node_type.as_deref() == Some("paragraph")
                                && p.children().iter().all(is_inline_kind)
                        }
                        _ => false,
                    }
            })
    })
}

fn attr_u64(n: &Node, key: &str) -> Option<u64> {
    match n.attr(key)? {
        Value::Number(x) => x.as_u64().or_else(|| x.as_f64().map(|f| f.max(0.0) as u64)),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

fn mark_attr<'m>(m: &'m Mark, key: &str) -> Option<&'m str> {
    m.attrs
        .as_ref()?
        .get(key)?
        .as_str()
        .filter(|s| !s.is_empty())
}

/// Nesting order (outermost first). `None` = unknown mark, rendered transparently.
fn mark_rank(m: &Mark) -> Option<u8> {
    Some(match m.mark_type.as_str() {
        "link" => {
            // `[a]()` is a link too: only a missing / non-string href is not
            m.attrs.as_ref()?.get("href")?.as_str()?;
            0
        }
        "underline" => 1,
        "subscript" | "superscript" => 2,
        "strike" => 3,
        "bold" => 4,
        "italic" => 5,
        "code" => CODE_RANK,
        _ => return None,
    })
}

/// Visit descendant text (raw, marks ignored).
fn walk_text(n: &Node, f: &mut impl FnMut(&str)) {
    if let Some(t) = &n.text {
        f(t);
    }
    for c in n.children() {
        walk_text(c, f);
    }
}

fn approx_size(n: &Node) -> usize {
    fn go(n: &Node) -> usize {
        8 + n.text.as_ref().map_or(0, String::len) + n.children().iter().map(go).sum::<usize>()
    }
    go(n) + go(n) / 8 + 16
}

/// `&` followed by something that reads as a character reference.
fn entity_like(rest: &[u8]) -> bool {
    for (n, &x) in rest.iter().take(33).enumerate() {
        if x == b';' {
            return n > 0;
        }
        if !(x.is_ascii_alphanumeric() || (n == 0 && x == b'#')) {
            return false;
        }
    }
    false
}

fn write_dest(out: &mut String, href: &str) {
    let mut depth = 0i32;
    let mut balanced = true;
    for b in href.bytes() {
        match b {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                balanced &= depth >= 0;
            }
            _ => {}
        }
    }
    // a bare destination cannot start with `<` (it would open the `<…>` form)
    let angle = href.is_empty()
        || href.starts_with('<')
        || depth != 0
        || !balanced
        || href.bytes().any(|b| b <= b' ' || b == 0x7f);
    if angle {
        out.push('<');
        for c in href.chars() {
            match c {
                '<' | '>' | '\\' => {
                    out.push('\\');
                    out.push(c);
                }
                '\n' => out.push_str("%0A"),
                '\r' => out.push_str("%0D"),
                _ => out.push(c),
            }
        }
        out.push('>');
    } else {
        for c in href.chars() {
            if c == '\\' {
                out.push('\\');
            }
            out.push(c);
        }
    }
}

fn write_title(out: &mut String, t: &str) {
    if t.is_empty() {
        return;
    }
    out.push_str(" \"");
    for c in t.chars() {
        match c {
            '"' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            '\n' | '\r' => out.push(' '),
            _ => out.push(c),
        }
    }
    out.push('"');
}
