//! Markdown written by others, as a closed tree of the kinds a view may show (threat model §3.7 points 1,
//! 3, 5 and 6). The parser runs with raw HTML off: `Html` and `InlineHtml` events are text, and an HTML
//! block is shown as code. Nothing in the tree carries an attribute taken from the content: no code
//! language, table alignment, list start, heading id or class, link title or image source. Extensions that
//! would add a kind the tree does not have (strikethrough, task lists, footnotes, math) stay off, so their
//! markup reads as the text it is.

use pulldown_cmark::{Event, Options, Parser, Tag};
use serde::Serialize;

use crate::link::{LinkTarget, check_link, image_host};
use crate::text::normalize_document;

/// How deep blocks and spans nest. A deeper container hands its content to the one above, so a thousand
/// `>` make one quote of text, not a thousand nested elements.
pub const DEPTH_LIMIT: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Document {
    pub blocks: Vec<Block>,
    /// The text was longer than the cap (`text::DOCUMENT_LIMIT`), and its end is missing.
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Block {
    Paragraph { content: Vec<Inline> },
    /// 1 to 6, as written.
    Heading { level: u8, content: Vec<Inline> },
    Quote { blocks: Vec<Block> },
    /// A fenced or indented code block, or an HTML block shown as it was written.
    Code { text: String },
    List { ordered: bool, items: Vec<Vec<Block>> },
    Table { head: Vec<Vec<Inline>>, rows: Vec<Vec<Vec<Inline>>> },
    Rule,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Inline {
    Text { text: String },
    Emphasis { content: Vec<Inline> },
    Strong { content: Vec<Inline> },
    Code { text: String },
    Break,
    Link { content: Vec<Inline>, target: LinkTarget },
    /// Not shown (§3.7 point 5): a placeholder with its alternative text and the host it would load from.
    Image { alt: String, host: Option<String> },
}

/// Normalizes the text (`text::normalize_document`), then parses it into the tree.
pub fn parse_document(written: &str) -> Document {
    let normalized = normalize_document(written);
    let mut builder = Builder { stack: vec![Frame::Blocks { blocks: Vec::new(), pending: Vec::new(), kind: Holder::Root }] };
    for event in Parser::new_ext(&normalized.text, Options::ENABLE_TABLES) {
        builder.event(event);
    }
    Document { blocks: builder.finish(), truncated: normalized.truncated }
}

#[derive(Debug, Clone, Copy)]
enum Holder {
    Root,
    Quote,
    Item,
}

#[derive(Debug, Clone, Copy)]
enum Span {
    Paragraph,
    Heading(u8),
    Cell,
    Emphasis,
    Strong,
}

enum Frame {
    /// Holds blocks; inline content that arrives directly (a tight list's item) waits in `pending` for
    /// its paragraph.
    Blocks { kind: Holder, blocks: Vec<Block>, pending: Vec<Inline> },
    Inlines { kind: Span, content: Vec<Inline> },
    Link { target: LinkTarget, content: Vec<Inline> },
    Image { alt: String, host: Option<String> },
    Code { text: String },
    List { ordered: bool, items: Vec<Vec<Block>> },
    Table { head: Vec<Vec<Inline>>, rows: Vec<Vec<Vec<Inline>>> },
    Row { head: bool, cells: Vec<Vec<Inline>> },
    /// Too deep, or a kind the tree does not have: its content goes to the frame above.
    Transparent,
}

struct Builder {
    stack: Vec<Frame>,
}

impl Builder {
    fn depth(&self) -> usize {
        self.stack.iter().filter(|frame| !matches!(frame, Frame::Transparent)).count()
    }

    fn event(&mut self, event: Event<'_>) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(_) => self.end(),
            Event::Text(text) | Event::Html(text) | Event::InlineHtml(text) => self.text(&text),
            Event::Code(text) => self.inline(Inline::Code { text: text.into_string() }),
            Event::SoftBreak => self.text("\n"),
            Event::HardBreak => self.inline(Inline::Break),
            Event::Rule => self.block(Block::Rule),
            // Off in the options; read as text if a parser version emits them anyway.
            Event::FootnoteReference(name) => self.text(&format!("[^{name}]")),
            Event::TaskListMarker(done) => self.text(if done { "[x] " } else { "[ ] " }),
            Event::InlineMath(text) | Event::DisplayMath(text) => self.inline(Inline::Code { text: text.into_string() }),
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        let frame = match tag {
            _ if self.depth() >= DEPTH_LIMIT => Frame::Transparent,
            Tag::Paragraph => Frame::Inlines { kind: Span::Paragraph, content: Vec::new() },
            Tag::Heading { level, .. } => Frame::Inlines { kind: Span::Heading(level as u8), content: Vec::new() },
            Tag::BlockQuote(_) => Frame::Blocks { kind: Holder::Quote, blocks: Vec::new(), pending: Vec::new() },
            Tag::CodeBlock(_) | Tag::HtmlBlock => Frame::Code { text: String::new() },
            Tag::List(start) => Frame::List { ordered: start.is_some(), items: Vec::new() },
            Tag::Item => Frame::Blocks { kind: Holder::Item, blocks: Vec::new(), pending: Vec::new() },
            Tag::Table(_) => Frame::Table { head: Vec::new(), rows: Vec::new() },
            Tag::TableHead => Frame::Row { head: true, cells: Vec::new() },
            Tag::TableRow => Frame::Row { head: false, cells: Vec::new() },
            Tag::TableCell => Frame::Inlines { kind: Span::Cell, content: Vec::new() },
            Tag::Emphasis => Frame::Inlines { kind: Span::Emphasis, content: Vec::new() },
            Tag::Strong => Frame::Inlines { kind: Span::Strong, content: Vec::new() },
            Tag::Link { dest_url, .. } => Frame::Link { target: check_link(&dest_url), content: Vec::new() },
            Tag::Image { dest_url, .. } => Frame::Image { alt: String::new(), host: image_host(&dest_url) },
            _ => Frame::Transparent,
        };
        self.stack.push(frame);
    }

    fn end(&mut self) {
        // The root never ends: the parser balances its own tags.
        if self.stack.len() == 1 {
            return;
        }
        match self.stack.pop().expect("a frame") {
            Frame::Transparent => {}
            Frame::Blocks { kind: Holder::Item, mut blocks, pending } => {
                flush(&mut blocks, pending);
                if let Some(Frame::List { items, .. }) = self.stack.last_mut() {
                    items.push(blocks);
                } else {
                    blocks.into_iter().for_each(|block| self.block(block));
                }
            }
            Frame::Blocks { mut blocks, pending, .. } => {
                flush(&mut blocks, pending);
                self.block(Block::Quote { blocks });
            }
            Frame::Inlines { kind: Span::Paragraph, content } => self.block(Block::Paragraph { content }),
            Frame::Inlines { kind: Span::Heading(level), content } => self.block(Block::Heading { level, content }),
            Frame::Inlines { kind: Span::Cell, content } => {
                if let Some(Frame::Row { cells, .. }) = self.stack.last_mut() {
                    cells.push(content);
                } else {
                    content.into_iter().for_each(|inline| self.inline(inline));
                }
            }
            Frame::Inlines { kind: Span::Emphasis, content } => self.inline(Inline::Emphasis { content }),
            Frame::Inlines { kind: Span::Strong, content } => self.inline(Inline::Strong { content }),
            Frame::Link { target, content } => self.inline(Inline::Link { content, target }),
            Frame::Image { alt, host } => self.inline(Inline::Image { alt, host }),
            Frame::Code { text } => self.block(Block::Code { text }),
            Frame::List { ordered, items } => self.block(Block::List { ordered, items }),
            Frame::Table { head, rows } => self.block(Block::Table { head, rows }),
            Frame::Row { head, cells } => match self.stack.last_mut() {
                Some(Frame::Table { head: table_head, .. }) if head => *table_head = cells,
                Some(Frame::Table { rows, .. }) => rows.push(cells),
                _ => cells.into_iter().flatten().for_each(|inline| self.inline(inline)),
            },
        }
    }

    /// The frame content goes to: the innermost one that is not transparent.
    fn target(&mut self) -> &mut Frame {
        self.stack.iter_mut().rev().find(|frame| !matches!(frame, Frame::Transparent)).expect("the root")
    }

    fn text(&mut self, text: &str) {
        match self.target() {
            Frame::Code { text: code } => code.push_str(text),
            Frame::Image { alt, .. } => alt.push_str(text),
            _ => self.inline(Inline::Text { text: text.to_owned() }),
        }
    }

    fn inline(&mut self, inline: Inline) {
        match self.target() {
            Frame::Blocks { pending: content, .. }
            | Frame::Inlines { content, .. }
            | Frame::Link { content, .. } => push_inline(content, inline),
            Frame::Code { text } => text.push_str(&inline_text(&inline)),
            Frame::Image { alt, .. } => alt.push_str(&inline_text(&inline)),
            Frame::List { items, .. } => items.push(vec![Block::Paragraph { content: vec![inline] }]),
            Frame::Row { cells, .. } => cells.push(vec![inline]),
            Frame::Table { rows, .. } => rows.push(vec![vec![inline]]),
            Frame::Transparent => unreachable!("target skips transparent frames"),
        }
    }

    fn block(&mut self, block: Block) {
        match self.target() {
            Frame::Blocks { blocks, pending, .. } => {
                flush(blocks, std::mem::take(pending));
                blocks.push(block);
            }
            Frame::List { items, .. } => items.push(vec![block]),
            Frame::Code { text } => text.push_str(&block_text(&block)),
            // A block where only spans fit: its text, so nothing written is lost.
            _ => {
                let text = block_text(&block);
                self.inline(Inline::Text { text });
            }
        }
    }

    fn finish(mut self) -> Vec<Block> {
        while self.stack.len() > 1 {
            self.end();
        }
        match self.stack.pop() {
            Some(Frame::Blocks { mut blocks, pending, .. }) => {
                flush(&mut blocks, pending);
                blocks
            }
            _ => unreachable!("the root holds blocks"),
        }
    }
}

/// Adjacent text joins, so a paragraph of plain text is one node.
fn push_inline(content: &mut Vec<Inline>, inline: Inline) {
    if let (Some(Inline::Text { text: last }), Inline::Text { text }) = (content.last_mut(), &inline) {
        last.push_str(text);
    } else {
        content.push(inline);
    }
}

fn flush(blocks: &mut Vec<Block>, pending: Vec<Inline>) {
    if !pending.is_empty() {
        blocks.push(Block::Paragraph { content: pending });
    }
}

fn inline_text(inline: &Inline) -> String {
    match inline {
        Inline::Text { text } | Inline::Code { text } => text.clone(),
        Inline::Emphasis { content } | Inline::Strong { content } | Inline::Link { content, .. } => {
            content.iter().map(inline_text).collect()
        }
        Inline::Break => "\n".to_owned(),
        Inline::Image { alt, .. } => alt.clone(),
    }
}

fn block_text(block: &Block) -> String {
    let inlines = |content: &[Inline]| content.iter().map(inline_text).collect::<String>();
    match block {
        Block::Paragraph { content } | Block::Heading { content, .. } => inlines(content),
        Block::Quote { blocks } => blocks.iter().map(block_text).collect::<Vec<_>>().join("\n"),
        Block::Code { text } => text.clone(),
        Block::List { items, .. } => {
            items.iter().flatten().map(block_text).collect::<Vec<_>>().join("\n")
        }
        Block::Table { head, rows } => std::iter::once(head)
            .chain(rows)
            .map(|row| row.iter().map(|cell| inlines(cell)).collect::<Vec<_>>().join(" | "))
            .collect::<Vec<_>>()
            .join("\n"),
        Block::Rule => String::new(),
    }
}

/// The hostile corpus of the threat model's V07, the one source for the core's tests, the app's smoke run
/// and the frontend's DOM proof. Test data: compiled in only with the `corpus` feature.
#[cfg(any(test, feature = "corpus"))]
pub mod corpus {
    use serde::Deserialize;

    const SOURCE: &str = include_str!("../corpus/hostile.json");

    #[derive(Debug, Clone, Deserialize)]
    #[serde(deny_unknown_fields)]
    pub struct Case {
        pub name: String,
        pub markdown: String,
        /// The markdown is this many copies of itself: the 1 MiB case.
        #[serde(default)]
        pub repeat: Option<usize>,
        /// Text the tree must show as text, markup included.
        #[serde(default)]
        pub shows: Vec<String>,
        /// The links the tree must have, in order: `external`, `confirm` (external, asks first), or
        /// `refused:<reason>`.
        #[serde(default)]
        pub links: Vec<String>,
    }

    impl Case {
        pub fn text(&self) -> String {
            self.markdown.repeat(self.repeat.unwrap_or(1))
        }
    }

    pub fn cases() -> Vec<Case> {
        serde_json::from_str(SOURCE).expect("corpus/hostile.json")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::link::LinkRefusal;

    fn walk_inlines<'a>(content: &'a [Inline], out: &mut Vec<&'a Inline>) {
        for inline in content {
            out.push(inline);
            if let Inline::Emphasis { content } | Inline::Strong { content } | Inline::Link { content, .. } = inline {
                walk_inlines(content, out);
            }
        }
    }

    fn walk<'a>(blocks: &'a [Block], depth: usize, out: &mut Vec<&'a Inline>, deepest: &mut usize) {
        *deepest = (*deepest).max(depth);
        for block in blocks {
            match block {
                Block::Paragraph { content } | Block::Heading { content, .. } => walk_inlines(content, out),
                Block::Quote { blocks } => walk(blocks, depth + 1, out, deepest),
                Block::List { items, .. } => items.iter().for_each(|item| walk(item, depth + 1, out, deepest)),
                Block::Table { head, rows } => {
                    head.iter().chain(rows.iter().flatten()).for_each(|cell| walk_inlines(cell, out))
                }
                Block::Code { .. } | Block::Rule => {}
            }
        }
    }

    fn all_text(document: &Document) -> String {
        document.blocks.iter().map(block_text).collect::<Vec<_>>().join("\n")
    }

    fn links(document: &Document) -> Vec<String> {
        let (mut inlines, mut deepest) = (Vec::new(), 0);
        walk(&document.blocks, 0, &mut inlines, &mut deepest);
        inlines
            .into_iter()
            .filter_map(|inline| match inline {
                Inline::Link { target: LinkTarget::External { confirm: false, .. }, .. } => Some("external".to_owned()),
                Inline::Link { target: LinkTarget::External { confirm: true, .. }, .. } => Some("confirm".to_owned()),
                Inline::Link { target: LinkTarget::Refused { reason }, .. } => {
                    Some(format!("refused:{}", serde_json::to_value(reason).unwrap().as_str().unwrap()))
                }
                _ => None,
            })
            .collect()
    }

    /// V07 on the tree: what a view could turn into markup, a request or a hidden character. Every link
    /// that may open is https with a host and no credentials; no text holds a control, an escape or a bidi
    /// control; nothing nests deeper than the limit.
    fn tree_problems(document: &Document) -> Vec<String> {
        let (mut inlines, mut deepest) = (Vec::new(), 0);
        walk(&document.blocks, 0, &mut inlines, &mut deepest);
        let mut problems = Vec::new();
        if deepest > DEPTH_LIMIT {
            problems.push(format!("blocks nest {deepest} deep"));
        }
        for inline in inlines {
            if let Inline::Link { target: LinkTarget::External { url, host, .. }, .. } = inline {
                let authority = url.strip_prefix("https://").unwrap_or_default().split('/').next().unwrap_or_default();
                if !url.starts_with("https://") || authority.contains('@') || url.len() > crate::link::URL_LIMIT {
                    problems.push(format!("a link that may open: {url}"));
                }
                if host.is_empty() || !host.is_ascii() {
                    problems.push(format!("a host that is not ASCII: {host}"));
                }
            }
        }
        let text = all_text(document);
        if let Some(c) = text.chars().find(|c| {
            (c.is_control() && !matches!(c, '\n' | '\t')) || ('\u{202A}'..='\u{202E}').contains(c) || ('\u{2066}'..='\u{2069}').contains(c)
        }) {
            problems.push(format!("text holds U+{:04X}", u32::from(c)));
        }
        problems
    }

    #[test]
    fn the_hostile_corpus_becomes_text_and_safe_links() {
        let cases = corpus::cases();
        assert!(cases.len() >= 20, "the corpus is read");
        for case in cases {
            let document = parse_document(&case.text());
            assert_eq!(tree_problems(&document), Vec::<String>::new(), "{}", case.name);
            let text = all_text(&document);
            for shown in &case.shows {
                assert!(text.contains(shown.as_str()), "{}: {shown:?} is not shown as text", case.name);
            }
            assert_eq!(links(&document), case.links, "{}", case.name);
            if case.repeat.is_some() {
                assert!(document.truncated, "{}: the cap holds", case.name);
            }
        }
    }

    #[test]
    fn v07_on_the_tree_fails_on_each_alteration() {
        let link = |url: &str, host: &str| Document {
            blocks: vec![Block::Paragraph {
                content: vec![Inline::Link {
                    content: vec![Inline::Text { text: "x".into() }],
                    target: LinkTarget::External { url: url.into(), host: host.into(), confirm: true },
                }],
            }],
            truncated: false,
        };
        let text = |text: &str| Document {
            blocks: vec![Block::Paragraph { content: vec![Inline::Text { text: text.into() }] }],
            truncated: false,
        };
        let mut deep = text("x");
        for _ in 0..=DEPTH_LIMIT {
            deep = Document { blocks: vec![Block::Quote { blocks: deep.blocks }], truncated: false };
        }
        let altered = [
            ("javascript", link("javascript:window.__pwned=1", "x")),
            ("http", link("http://example.invalid/", "example.invalid")),
            ("credentials", link("https://user:pass@example.invalid/", "example.invalid")),
            ("a unicode host", link("https://g\u{456}thub.com/", "g\u{456}thub.com")),
            ("an escape", text("\u{1B}[31mred")),
            ("a bidi override", text("a\u{202E}b")),
            ("too deep", deep),
        ];
        for (name, document) in altered {
            assert!(!tree_problems(&document).is_empty(), "V07 on the tree misses {name}");
        }
    }

    #[test]
    fn the_tree_has_the_kinds_of_the_model() {
        let document = parse_document(
            "# Title {#id .class}\n\nSome *em* and **strong** with `code`  \nand a break.\n\n> quoted\n\n\
             1. one\n2. two\n\n- tight\n- list\n\n| a | b |\n|:-|-:|\n| 1 | 2 |\n\n---\n\n```rust\nfn x() {}\n```\n",
        );
        let kinds: Vec<&str> = document
            .blocks
            .iter()
            .map(|block| match block {
                Block::Paragraph { .. } => "paragraph",
                Block::Heading { .. } => "heading",
                Block::Quote { .. } => "quote",
                Block::Code { .. } => "code",
                Block::List { .. } => "list",
                Block::Table { .. } => "table",
                Block::Rule => "rule",
            })
            .collect();
        assert_eq!(kinds, ["heading", "paragraph", "quote", "list", "list", "table", "rule", "code"]);
        // Heading attributes stay off: the braces are text, not an id and a class.
        assert_eq!(
            document.blocks[0],
            Block::Heading { level: 1, content: vec![Inline::Text { text: "Title {#id .class}".into() }] }
        );
        assert_eq!(document.blocks[7], Block::Code { text: "fn x() {}\n".into() });
        let Block::List { ordered: true, items } = &document.blocks[3] else { panic!("{:?}", document.blocks[3]) };
        assert_eq!(items.len(), 2);
        let Block::Table { head, rows } = &document.blocks[5] else { panic!("{:?}", document.blocks[5]) };
        assert_eq!((head.len(), rows.len()), (2, 1));
    }

    #[test]
    fn markup_the_tree_has_no_kind_for_reads_as_text() {
        let document = parse_document("~~struck~~ - [ ] task [^1]\n\n[^1]: note");
        assert!(all_text(&document).contains("~~struck~~"), "{document:?}");
        assert!(all_text(&document).contains("[ ] task"), "{document:?}");
        let refused = parse_document("[a](javascript:alert(1)) and [b](https://example.invalid/)");
        assert_eq!(links(&refused), ["refused:scheme", "confirm"]);
        let Block::Paragraph { content } = &refused.blocks[0] else { panic!() };
        assert!(matches!(&content[0], Inline::Link { target: LinkTarget::Refused { reason: LinkRefusal::Scheme }, .. }));
    }

    #[test]
    fn nesting_stops_at_the_limit() {
        let document = parse_document(&format!("{}deep", ">".repeat(1000)));
        let (mut inlines, mut deepest) = (Vec::new(), 0);
        walk(&document.blocks, 0, &mut inlines, &mut deepest);
        assert!(deepest <= DEPTH_LIMIT, "{deepest}");
        assert!(all_text(&document).contains("deep"));
    }
}
