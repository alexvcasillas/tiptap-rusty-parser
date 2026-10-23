//! Markdown -> JSONContent.
//!
//! Single pass over [`pulldown_cmark`] events with a stack of open nodes; no
//! intermediate AST. Text stays borrowed until it lands in a text node and
//! adjacent text runs with identical marks are merged in place.
//!
//! # Mapping (Tiptap StarterKit names)
//!
//! | Markdown | JSONContent |
//! |---|---|
//! | document | `doc` (empty input: one empty `paragraph`) |
//! | paragraph | `paragraph` |
//! | `#`..`######` | `heading{level}` |
//! | `>` | `blockquote` |
//! | `-`/`*`/`+` list | `bulletList > listItem` |
//! | `1.` list | `orderedList{start}` (`start` only when != 1) `> listItem` |
//! | fenced/indented code | `codeBlock{language}` (first info-string word or `null`), one text child without the final newline, no child when empty |
//! | `---` | `horizontalRule` |
//! | hard break | `hardBreak` (soft break: a single space merged into text) |
//! | `**`/`*`/`~~`/`` ` `` | marks `bold`/`italic`/`strike`/`code` |
//! | `[t](u "x")`, `<u>` | mark `link{href, title?}` (`title` only when non-empty; email autolinks get `mailto:`) |
//! | `![a](s "t")` | `image{src, alt, title?}` |
//! | GFM table | `table > tableRow > tableHeader/tableCell > paragraph` (alignment ignored) |
//! | GFM task list | `taskList > taskItem{checked}` |
//!
//! Details:
//! - Marks are ordered outermost first; a mark already active is not repeated
//!   (`**a **b** c**` yields one `bold`).
//! - `listItem`/`taskItem` always start with a `paragraph`: inline runs of
//!   tight lists are wrapped, and an item starting with another block (or
//!   empty) gets an empty leading paragraph. `blockquote`, `tableCell` and
//!   `tableHeader` are never empty (`block+`).
//! - Images: a paragraph holding only unmarked images (whitespace and hard
//!   breaks ignored) is replaced by the images as block-level siblings,
//!   except when it is the first child of a list item (which must start with
//!   a paragraph). Otherwise images stay inline in their paragraph/heading
//!   and carry the active marks (e.g. a linked badge keeps its `link`).
//! - Task lists: a list becomes a `taskList` only when *every* item has a
//!   task marker (ordered task lists lose `start`). A mixed list stays a
//!   `bulletList`/`orderedList` and the markers are kept as literal leading
//!   text (`[ ] `/`[x] `) so nothing is lost.
//! - Autolinks: CommonMark `<https://…>` / `<a@b.c>` become links. GFM
//!   extended autolinks (bare `www.…`/`https://…`) are not supported by
//!   pulldown-cmark 0.13 and stay plain text.
//! - Raw HTML follows [`HtmlPolicy`]. Footnotes, math, metadata blocks and
//!   GFM alerts are not enabled (their syntax stays text).

use pulldown_cmark::{CodeBlockKind, CowStr, Event, LinkType, Options, Parser, Tag, TagEnd};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tiptap_rusty_parser::{Document, Mark, Node};

/// What to do with raw HTML found in Markdown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HtmlPolicy {
    /// Keep it as plain text (visible, schema-valid): inline HTML is merged
    /// into the surrounding text, an HTML block becomes a paragraph.
    #[default]
    Text,
    /// Drop it. Paragraphs left empty by dropping are removed.
    Drop,
}

/// Import options.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ParseOptions {
    /// GFM extensions: tables, task lists, strikethrough. (CommonMark
    /// `<…>` autolinks are always on; GFM bare-URL autolinks are unsupported.)
    pub gfm: bool,
    /// Raw HTML handling.
    pub html: HtmlPolicy,
}

impl Default for ParseOptions {
    fn default() -> Self {
        Self {
            gfm: true,
            html: HtmlPolicy::Text,
        }
    }
}

/// Parse Markdown with default options (GFM on, HTML kept as text).
///
/// ```
/// use tiptap_rusty_markdown::from_markdown;
///
/// let doc = from_markdown("# Hi\n\nSome **bold** text");
/// let json = doc.to_value().unwrap();
/// assert_eq!(json["content"][0]["type"], "heading");
/// assert_eq!(json["content"][0]["attrs"]["level"], 1);
/// assert_eq!(json["content"][1]["content"][1]["text"], "bold");
/// assert_eq!(json["content"][1]["content"][1]["marks"][0]["type"], "bold");
/// ```
pub fn from_markdown(src: &str) -> Document {
    from_markdown_with(src, &ParseOptions::default())
}

/// Parse Markdown with `opts`. Never panics; the result always has at least
/// one block.
///
/// ```
/// use tiptap_rusty_markdown::{from_markdown_with, HtmlPolicy, ParseOptions};
///
/// let opts = ParseOptions { gfm: false, html: HtmlPolicy::Drop };
/// let doc = from_markdown_with("a <b>b</b>", &opts);
/// let json = doc.to_value().unwrap();
/// assert_eq!(json["content"][0]["content"][0]["text"], "a b");
/// ```
pub fn from_markdown_with(src: &str, opts: &ParseOptions) -> Document {
    let mut flags = Options::empty();
    if opts.gfm {
        flags |= Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS | Options::ENABLE_STRIKETHROUGH;
    }
    let mut b = Builder::new(opts.html);
    for ev in Parser::new_ext(src, flags) {
        b.event(ev);
    }
    Document::new(b.finish())
}

// ---- builder ---------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Doc,
    Para,
    /// Paragraph opened to wrap bare inline content (tight list items, cells).
    ImplicitPara,
    Heading(u8),
    Quote,
    List(Option<u64>),
    Item,
    Code,
    Html,
    Table,
    Row {
        header: bool,
    },
    Cell {
        header: bool,
    },
}

struct Frame {
    kind: Kind,
    children: Vec<Node>,
    /// Code / HTML block text.
    buf: String,
    /// Code block attrs.
    attrs: Option<Map<String, Value>>,
    /// Item: task marker. List: one entry per closed item.
    task: Option<bool>,
    tasks: Vec<Option<bool>>,
    /// Paragraph: some inline HTML was dropped.
    dropped_html: bool,
}

impl Frame {
    #[inline]
    fn new(kind: Kind) -> Self {
        Frame {
            kind,
            children: Vec::new(),
            buf: String::new(),
            attrs: None,
            task: None,
            tasks: Vec::new(),
            dropped_html: false,
        }
    }
}

struct PendingImage {
    src: String,
    title: String,
    alt: String,
    marks: Vec<Mark>,
}

struct Builder {
    html: HtmlPolicy,
    stack: Vec<Frame>,
    marks: Vec<Mark>,
    /// Per open inline mark tag: the rank of the mark it pushed onto `marks`
    /// (`None` if the type was already active).
    mark_pushed: Vec<Option<u8>>,
    image: Option<PendingImage>,
    image_depth: u32,
}

#[inline]
fn element(ty: &str, attrs: Option<Map<String, Value>>, children: Vec<Node>) -> Node {
    Node {
        node_type: Some(ty.to_owned()),
        attrs,
        content: if children.is_empty() {
            None
        } else {
            Some(children)
        },
        ..Default::default()
    }
}

#[inline]
fn one_attr(k: &str, v: Value) -> Option<Map<String, Value>> {
    let mut m = Map::new();
    m.insert(k.to_owned(), v);
    Some(m)
}

#[inline]
fn is_type(n: &Node, ty: &str) -> bool {
    n.node_type.as_deref() == Some(ty)
}

fn mark_rank(m: &Mark) -> u8 {
    match m.mark_type.as_str() {
        "link" => 0,
        "strike" => 1,
        "bold" => 2,
        "italic" => 3,
        _ => 4,
    }
}

/// `existing == stack (+ code)` without allocating.
#[inline]
fn marks_eq(existing: Option<&[Mark]>, stack: &[Mark], code: bool) -> bool {
    let existing = existing.unwrap_or(&[]);
    if code {
        match existing.split_last() {
            Some((last, rest)) => last.mark_type == "code" && last.attrs.is_none() && rest == stack,
            None => false,
        }
    } else {
        existing == stack
    }
}

fn push_text(children: &mut Vec<Node>, marks: &[Mark], code: bool, t: CowStr<'_>) {
    if t.is_empty() {
        return;
    }
    if let Some(last) = children.last_mut() {
        if is_type(last, "text") && marks_eq(last.marks.as_deref(), marks, code) {
            if let Some(s) = last.text.as_mut() {
                s.push_str(&t);
                return;
            }
        }
    }
    let n = marks.len() + code as usize;
    let marks = if n == 0 {
        None
    } else {
        let mut m = Vec::with_capacity(n);
        m.extend_from_slice(marks);
        if code {
            m.push(Mark::new("code"));
        }
        Some(m)
    };
    children.push(Node {
        node_type: Some("text".to_owned()),
        text: Some(t.into_string()),
        marks,
        ..Default::default()
    });
}

/// The children of the current inline container, opening an implicit
/// paragraph in block containers. `None` where inline content can't go.
fn inline_target(stack: &mut Vec<Frame>) -> Option<&mut Vec<Node>> {
    match stack.last()?.kind {
        Kind::Para | Kind::ImplicitPara | Kind::Heading(_) => {}
        Kind::Doc | Kind::Quote | Kind::Item | Kind::Cell { .. } => {
            stack.push(Frame::new(Kind::ImplicitPara))
        }
        _ => return None,
    }
    stack.last_mut().map(|f| &mut f.children)
}

fn ensure_para_first(children: &mut Vec<Node>) {
    if !children.first().is_some_and(|c| is_type(c, "paragraph")) {
        children.insert(0, element("paragraph", None, Vec::new()));
    }
}

fn ensure_nonempty(children: &mut Vec<Node>) {
    if children.is_empty() {
        children.push(element("paragraph", None, Vec::new()));
    }
}

/// Mixed task list: keep the marker as literal leading text.
fn prepend_marker(item: &mut Node, checked: bool) {
    let marker = if checked { "[x] " } else { "[ ] " };
    let Some(para) = item.content.as_mut().and_then(|c| c.first_mut()) else {
        return;
    };
    let content = para.content.get_or_insert_with(Vec::new);
    if let Some(first) = content.first_mut() {
        if is_type(first, "text") && first.marks.is_none() {
            if let Some(s) = first.text.as_mut() {
                s.insert_str(0, marker);
                return;
            }
        }
    }
    content.insert(0, Node::text(marker));
}

fn is_hoistable_images(children: &[Node]) -> bool {
    let mut any = false;
    for c in children {
        match c.node_type.as_deref() {
            Some("image") if c.marks.is_none() => any = true,
            Some("hardBreak") => {}
            Some("text")
                if c.marks.is_none() && c.text.as_deref().is_some_and(|t| t.trim().is_empty()) => {}
            _ => return false,
        }
    }
    any
}

impl Builder {
    fn new(html: HtmlPolicy) -> Self {
        let mut stack = Vec::with_capacity(16);
        stack.push(Frame::new(Kind::Doc));
        Builder {
            html,
            stack,
            marks: Vec::new(),
            mark_pushed: Vec::new(),
            image: None,
            image_depth: 0,
        }
    }

    fn event(&mut self, ev: Event<'_>) {
        if self.image_depth > 0 {
            self.image_event(ev);
            return;
        }
        match ev {
            Event::Start(tag) => self.start(tag),
            Event::End(end) => self.end(end),
            Event::Text(t) => self.text(t),
            Event::Code(t) => {
                if let Some(ch) = inline_target(&mut self.stack) {
                    push_text(ch, &self.marks, true, t);
                }
            }
            Event::Html(t) | Event::InlineHtml(t) => self.raw_html(t),
            Event::SoftBreak => self.text(CowStr::Borrowed(" ")),
            Event::HardBreak => {
                if let Some(ch) = inline_target(&mut self.stack) {
                    let mut br = element("hardBreak", None, Vec::new());
                    if !self.marks.is_empty() {
                        // inline nodes carry the surrounding marks, like images
                        br.marks = Some(self.marks.clone());
                    }
                    ch.push(br);
                }
            }
            Event::Rule => self.push_block(element("horizontalRule", None, Vec::new())),
            Event::TaskListMarker(checked) => {
                if let Some(f) = self
                    .stack
                    .iter_mut()
                    .rev()
                    .take(2)
                    .find(|f| f.kind == Kind::Item)
                {
                    f.task = Some(checked);
                }
            }
            // Not enabled; keep their source text if ever emitted.
            Event::FootnoteReference(t) | Event::InlineMath(t) | Event::DisplayMath(t) => {
                self.text(t)
            }
        }
    }

    fn image_event(&mut self, ev: Event<'_>) {
        match ev {
            Event::Start(Tag::Image { .. }) => self.image_depth += 1,
            Event::End(TagEnd::Image) => {
                self.image_depth -= 1;
                if self.image_depth == 0 {
                    self.finish_image();
                }
            }
            Event::Text(t) | Event::Code(t) => {
                if let Some(img) = self.image.as_mut() {
                    img.alt.push_str(&t);
                }
            }
            Event::InlineHtml(t) | Event::Html(t) if self.html == HtmlPolicy::Text => {
                if let Some(img) = self.image.as_mut() {
                    img.alt.push_str(&t);
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if let Some(img) = self.image.as_mut() {
                    img.alt.push(' ');
                }
            }
            _ => {}
        }
    }

    fn finish_image(&mut self) {
        let Some(img) = self.image.take() else {
            return;
        };
        let mut attrs = Map::new();
        attrs.insert("src".to_owned(), Value::String(img.src));
        attrs.insert("alt".to_owned(), Value::String(img.alt));
        if !img.title.is_empty() {
            attrs.insert("title".to_owned(), Value::String(img.title));
        }
        let node = Node {
            node_type: Some("image".to_owned()),
            attrs: Some(attrs),
            marks: if img.marks.is_empty() {
                None
            } else {
                Some(img.marks)
            },
            ..Default::default()
        };
        if let Some(ch) = inline_target(&mut self.stack) {
            ch.push(node);
        }
    }

    fn text(&mut self, t: CowStr<'_>) {
        let Some(top) = self.stack.last_mut() else {
            return;
        };
        if matches!(top.kind, Kind::Code | Kind::Html) {
            if top.kind == Kind::Code || self.html == HtmlPolicy::Text {
                top.buf.push_str(&t);
            }
            return;
        }
        if let Some(ch) = inline_target(&mut self.stack) {
            push_text(ch, &self.marks, false, t);
        }
    }

    fn raw_html(&mut self, t: CowStr<'_>) {
        match self.html {
            HtmlPolicy::Text => self.text(t),
            HtmlPolicy::Drop => {
                if let Some(top) = self.stack.last_mut() {
                    top.dropped_html = true;
                }
            }
        }
    }

    fn push_block(&mut self, node: Node) {
        self.close_implicit();
        if let Some(top) = self.stack.last_mut() {
            top.children.push(node);
        }
    }

    fn open_block(&mut self, kind: Kind) {
        self.close_implicit();
        self.stack.push(Frame::new(kind));
    }

    /// Marks are kept in canonical nesting order (link > strike > bold >
    /// italic), so equal formatting always yields the same mark list however
    /// the source nested the delimiters.
    fn push_mark(&mut self, m: Mark) {
        if self.marks.iter().any(|x| x.mark_type == m.mark_type) {
            self.mark_pushed.push(None);
        } else {
            let r = mark_rank(&m);
            let at = self.marks.partition_point(|x| mark_rank(x) <= r);
            self.marks.insert(at, m);
            self.mark_pushed.push(Some(r));
        }
    }

    fn pop_mark(&mut self) {
        if let Some(Some(r)) = self.mark_pushed.pop() {
            if let Some(at) = self.marks.iter().position(|x| mark_rank(x) == r) {
                self.marks.remove(at);
            }
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph => self.open_block(Kind::Para),
            Tag::Heading { level, .. } => self.open_block(Kind::Heading(level as u8)),
            Tag::BlockQuote(_) => self.open_block(Kind::Quote),
            Tag::CodeBlock(kind) => {
                self.open_block(Kind::Code);
                let lang = match &kind {
                    CodeBlockKind::Fenced(info) => info
                        .split_whitespace()
                        .next()
                        .map_or(Value::Null, |l| Value::String(l.to_owned())),
                    CodeBlockKind::Indented => Value::Null,
                };
                if let Some(top) = self.stack.last_mut() {
                    top.attrs = one_attr("language", lang);
                }
            }
            Tag::HtmlBlock => self.open_block(Kind::Html),
            Tag::List(start) => self.open_block(Kind::List(start)),
            Tag::Item => self.open_block(Kind::Item),
            Tag::Table(_) => self.open_block(Kind::Table),
            Tag::TableHead => self.stack.push(Frame::new(Kind::Row { header: true })),
            Tag::TableRow => self.stack.push(Frame::new(Kind::Row { header: false })),
            Tag::TableCell => {
                let header = matches!(
                    self.stack.last().map(|f| f.kind),
                    Some(Kind::Row { header: true })
                );
                self.stack.push(Frame::new(Kind::Cell { header }));
            }
            Tag::Emphasis => self.push_mark(Mark::new("italic")),
            Tag::Strong => self.push_mark(Mark::new("bold")),
            Tag::Strikethrough => self.push_mark(Mark::new("strike")),
            Tag::Link {
                link_type,
                dest_url,
                title,
                ..
            } => {
                let href = if link_type == LinkType::Email {
                    let mut s = String::with_capacity(7 + dest_url.len());
                    s.push_str("mailto:");
                    s.push_str(&dest_url);
                    s
                } else {
                    dest_url.into_string()
                };
                let mut m = Mark::new("link").attr("href", href);
                if !title.is_empty() {
                    m = m.attr("title", title.into_string());
                }
                self.push_mark(m);
            }
            Tag::Image {
                dest_url, title, ..
            } => {
                self.image_depth = 1;
                self.image = Some(PendingImage {
                    src: dest_url.into_string(),
                    title: title.into_string(),
                    alt: String::new(),
                    marks: self.marks.clone(),
                });
            }
            // Not enabled: transparent.
            Tag::FootnoteDefinition(_)
            | Tag::DefinitionList
            | Tag::DefinitionListTitle
            | Tag::DefinitionListDefinition
            | Tag::Superscript
            | Tag::Subscript
            | Tag::MetadataBlock(_) => {}
        }
    }

    fn end(&mut self, end: TagEnd) {
        match end {
            TagEnd::Paragraph
            | TagEnd::Heading(_)
            | TagEnd::BlockQuote(_)
            | TagEnd::CodeBlock
            | TagEnd::HtmlBlock
            | TagEnd::List(_)
            | TagEnd::Item
            | TagEnd::Table
            | TagEnd::TableHead
            | TagEnd::TableRow
            | TagEnd::TableCell => {
                self.close_implicit();
                self.close_top();
            }
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough | TagEnd::Link => {
                self.pop_mark()
            }
            TagEnd::Image
            | TagEnd::FootnoteDefinition
            | TagEnd::DefinitionList
            | TagEnd::DefinitionListTitle
            | TagEnd::DefinitionListDefinition
            | TagEnd::Superscript
            | TagEnd::Subscript
            | TagEnd::MetadataBlock(_) => {}
        }
    }

    fn close_implicit(&mut self) {
        if self
            .stack
            .last()
            .is_some_and(|f| f.kind == Kind::ImplicitPara)
        {
            self.close_top();
        }
    }

    /// Pop the top frame and attach its node(s) to the parent. Never pops `doc`.
    fn close_top(&mut self) {
        if self.stack.len() <= 1 {
            return;
        }
        let Some(f) = self.stack.pop() else {
            return;
        };
        let html = self.html;
        let Some(parent) = self.stack.last_mut() else {
            return;
        };
        let mut children = f.children;
        match f.kind {
            Kind::Doc => {}
            Kind::Para | Kind::ImplicitPara => {
                if children.is_empty()
                    || (f.dropped_html
                        && children.iter().all(|c| {
                            is_type(c, "text")
                                && c.text.as_deref().is_some_and(|t| t.trim().is_empty())
                        }))
                {
                    return;
                }
                // Cells stay `cell > paragraph > image` (pipe tables need it).
                let keep_para = match parent.kind {
                    Kind::Item => parent.children.is_empty(),
                    Kind::Cell { .. } => true,
                    _ => false,
                };
                if !keep_para && is_hoistable_images(&children) {
                    parent
                        .children
                        .extend(children.into_iter().filter(|c| is_type(c, "image")));
                } else {
                    parent.children.push(element("paragraph", None, children));
                }
            }
            Kind::Heading(level) => {
                parent.children.push(element(
                    "heading",
                    one_attr("level", Value::from(level)),
                    children,
                ));
            }
            Kind::Quote => {
                ensure_nonempty(&mut children);
                parent.children.push(element("blockquote", None, children));
            }
            Kind::Code => {
                let mut text = f.buf;
                if text.ends_with('\n') {
                    text.pop();
                }
                let content = if text.is_empty() {
                    Vec::new()
                } else {
                    vec![Node::text(text)]
                };
                let attrs = f.attrs.or_else(|| one_attr("language", Value::Null));
                parent.children.push(element("codeBlock", attrs, content));
            }
            Kind::Html => {
                if html == HtmlPolicy::Text {
                    let mut text = f.buf;
                    let len = text.trim_end_matches(['\n', '\r']).len();
                    text.truncate(len);
                    if !text.is_empty() {
                        // Source line breaks become hard breaks: a raw `\n`
                        // in a text node would not survive an export.
                        let mut nodes = Vec::new();
                        for (i, line) in text.split('\n').enumerate() {
                            if i > 0 {
                                nodes.push(element("hardBreak", None, Vec::new()));
                            }
                            if !line.is_empty() {
                                nodes.push(Node::text(line));
                            }
                        }
                        parent.children.push(element("paragraph", None, nodes));
                    }
                }
            }
            Kind::List(start) => {
                let tasks = f.tasks;
                let all_task = !children.is_empty()
                    && tasks.len() == children.len()
                    && tasks.iter().all(Option::is_some);
                if all_task {
                    for (item, t) in children.iter_mut().zip(&tasks) {
                        item.node_type = Some("taskItem".to_owned());
                        item.attrs = one_attr("checked", Value::Bool(t.unwrap_or(false)));
                    }
                    parent.children.push(element("taskList", None, children));
                } else {
                    if tasks.iter().any(Option::is_some) {
                        for (item, t) in children.iter_mut().zip(&tasks) {
                            if let Some(checked) = *t {
                                prepend_marker(item, checked);
                            }
                        }
                    }
                    let node = match start {
                        None => element("bulletList", None, children),
                        Some(n) => element(
                            "orderedList",
                            if n == 1 {
                                None
                            } else {
                                one_attr("start", Value::from(n))
                            },
                            children,
                        ),
                    };
                    parent.children.push(node);
                }
            }
            Kind::Item => {
                ensure_para_first(&mut children);
                parent.children.push(element("listItem", None, children));
                parent.tasks.push(f.task);
            }
            Kind::Table => {
                if !children.is_empty() {
                    parent.children.push(element("table", None, children));
                }
            }
            Kind::Row { .. } => parent.children.push(element("tableRow", None, children)),
            Kind::Cell { header } => {
                ensure_nonempty(&mut children);
                let ty = if header { "tableHeader" } else { "tableCell" };
                parent.children.push(element(ty, None, children));
            }
        }
    }

    fn finish(mut self) -> Node {
        // Close anything left open (defensive; pulldown balances its events).
        while self.stack.len() > 1 {
            self.close_top();
        }
        let mut children = self.stack.pop().map(|f| f.children).unwrap_or_default();
        ensure_nonempty(&mut children);
        Node {
            node_type: Some("doc".to_owned()),
            content: Some(children),
            ..Default::default()
        }
    }
}
