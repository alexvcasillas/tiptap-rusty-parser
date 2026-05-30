//! HTML serialization tests.

use tiptap_rusty_parser::{
    Document, HtmlOptions, SelfClosingStyle, UnknownMarkPolicy, UnknownNodePolicy,
};

fn html(json: &str) -> String {
    Document::from_json_str(json).unwrap().to_html()
}

#[test]
fn paragraph_and_headings() {
    assert_eq!(
        html(
            r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"hi"}]}]}"#
        ),
        "<p>hi</p>"
    );
    assert_eq!(
        html(
            r#"{"type":"doc","content":[{"type":"heading","attrs":{"level":1},"content":[{"type":"text","text":"A"}]}]}"#
        ),
        "<h1>A</h1>"
    );
    assert_eq!(
        html(r#"{"type":"heading","attrs":{"level":6},"content":[{"type":"text","text":"B"}]}"#),
        "<h6>B</h6>"
    );
    // clamp >6 and default when missing
    assert_eq!(
        html(r#"{"type":"heading","attrs":{"level":9}}"#),
        "<h6></h6>"
    );
    assert_eq!(html(r#"{"type":"heading"}"#), "<h1></h1>");
}

#[test]
fn marks_nesting_and_link() {
    assert_eq!(
        html(r#"{"type":"text","text":"x","marks":[{"type":"bold"},{"type":"italic"}]}"#),
        "<strong><em>x</em></strong>"
    );
    assert_eq!(
        html(r#"{"type":"text","text":"x","marks":[{"type":"bold"}]}"#),
        "<strong>x</strong>"
    );
    assert_eq!(
        html(
            r#"{"type":"text","text":"go","marks":[{"type":"link","attrs":{"href":"/a","target":"_blank","rel":"noopener"}}]}"#
        ),
        r#"<a href="/a" target="_blank" rel="noopener">go</a>"#
    );
    // link without href still emits <a>
    assert_eq!(
        html(r#"{"type":"text","text":"go","marks":[{"type":"link"}]}"#),
        "<a>go</a>"
    );
}

#[test]
fn lists_codeblock_breaks_image() {
    assert_eq!(
        html(
            r#"{"type":"bulletList","content":[{"type":"listItem","content":[{"type":"text","text":"a"}]}]}"#
        ),
        "<ul><li>a</li></ul>"
    );
    assert_eq!(
        html(r#"{"type":"orderedList","attrs":{"start":3},"content":[{"type":"listItem"}]}"#),
        r#"<ol start="3"><li></li></ol>"#
    );
    // start == 1 omits the attribute
    assert_eq!(
        html(r#"{"type":"orderedList","attrs":{"start":1},"content":[]}"#),
        "<ol></ol>"
    );
    assert_eq!(
        html(
            r#"{"type":"codeBlock","attrs":{"language":"rust"},"content":[{"type":"text","text":"fn main(){}"}]}"#
        ),
        r#"<pre><code class="language-rust">fn main(){}</code></pre>"#
    );
    // marks inside code blocks are ignored (raw escaped text)
    assert_eq!(
        html(
            r#"{"type":"codeBlock","content":[{"type":"text","text":"a<b","marks":[{"type":"bold"}]}]}"#
        ),
        "<pre><code>a&lt;b</code></pre>"
    );
    assert_eq!(html(r#"{"type":"hardBreak"}"#), "<br>");
    assert_eq!(html(r#"{"type":"horizontalRule"}"#), "<hr>");
    assert_eq!(
        html(r#"{"type":"image","attrs":{"src":"/i.png","alt":"pic","title":"T"}}"#),
        r#"<img src="/i.png" alt="pic" title="T">"#
    );
    assert_eq!(
        html(r#"{"type":"image","attrs":{"src":"/i.png"}}"#),
        r#"<img src="/i.png">"#
    );
}

#[test]
fn text_align() {
    assert_eq!(
        html(
            r#"{"type":"paragraph","attrs":{"textAlign":"center"},"content":[{"type":"text","text":"c"}]}"#
        ),
        r#"<p style="text-align:center">c</p>"#
    );
}

#[test]
fn escaping_text_and_attribute_injection() {
    assert_eq!(
        html(r#"{"type":"paragraph","content":[{"type":"text","text":"a < b & c > d"}]}"#),
        "<p>a &lt; b &amp; c &gt; d</p>"
    );
    // attribute value can't break out of the quoted context (fully escaped)
    assert_eq!(
        html(r#"{"type":"image","attrs":{"src":"\"><script>alert(1)</script>"}}"#),
        r#"<img src="&quot;&gt;&lt;script&gt;alert(1)&lt;/script&gt;">"#
    );
    assert_eq!(
        html(
            r#"{"type":"text","text":"x","marks":[{"type":"link","attrs":{"href":"\" onclick=\"x"}}]}"#
        ),
        r#"<a href="&quot; onclick=&quot;x">x</a>"#
    );
}

#[test]
fn unknown_node_and_mark_policies() {
    let doc =
        r#"{"type":"doc","content":[{"type":"widget","content":[{"type":"text","text":"hi"}]}]}"#;
    // default: Transparent (children rendered, no wrapper)
    assert_eq!(html(doc), "hi");

    let wrap = HtmlOptions {
        unknown_node: UnknownNodePolicy::DataTypeDiv,
        ..Default::default()
    };
    assert_eq!(
        Document::from_json_str(doc).unwrap().to_html_with(&wrap),
        r#"<div data-type="widget">hi</div>"#
    );

    let skip = HtmlOptions {
        unknown_node: UnknownNodePolicy::Skip,
        ..Default::default()
    };
    assert_eq!(
        Document::from_json_str(doc).unwrap().to_html_with(&skip),
        ""
    );

    // unknown mark: default Transparent drops the wrapper, keeps text
    let marked = r#"{"type":"text","text":"x","marks":[{"type":"blink"}]}"#;
    assert_eq!(html(marked), "x");
    let span = HtmlOptions {
        unknown_mark: UnknownMarkPolicy::DataMarkSpan,
        ..Default::default()
    };
    assert_eq!(
        Document::from_json_str(marked).unwrap().to_html_with(&span),
        r#"<span data-mark="blink">x</span>"#
    );
}

#[test]
fn empty_docs() {
    assert_eq!(html(r#"{"type":"doc"}"#), "");
    assert_eq!(html(r#"{"type":"doc","content":[]}"#), "");
}

#[test]
fn determinism() {
    let doc = Document::from_json_str(
        r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"x","marks":[{"type":"bold"},{"type":"italic"}]}]}]}"#,
    )
    .unwrap();
    assert_eq!(doc.to_html(), doc.to_html());
}

#[test]
fn options_overrides() {
    let doc = r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"hi","marks":[{"type":"highlight"}]}]}]}"#;
    let mut opts = HtmlOptions::default();
    opts.node_tags.insert("paragraph".into(), "div".into());
    opts.mark_tags.insert("highlight".into(), "mark".into());
    assert_eq!(
        Document::from_json_str(doc).unwrap().to_html_with(&opts),
        "<div><mark>hi</mark></div>"
    );

    // xhtml self-closing
    let xhtml = HtmlOptions {
        self_closing: SelfClosingStyle::Xhtml,
        ..Default::default()
    };
    assert_eq!(
        Document::from_json_str(r#"{"type":"hardBreak"}"#)
            .unwrap()
            .to_html_with(&xhtml),
        "<br/>"
    );

    // spread_attrs on/off for a custom attribute
    let custom = r#"{"type":"paragraph","attrs":{"data-foo":"bar"},"content":[]}"#;
    assert_eq!(html(custom), "<p></p>"); // off by default
    let spread = HtmlOptions {
        spread_attrs: true,
        ..Default::default()
    };
    assert_eq!(
        Document::from_json_str(custom)
            .unwrap()
            .to_html_with(&spread),
        r#"<p data-foo="bar"></p>"#
    );
}

#[test]
fn kitchen_sink() {
    let doc = r#"{"type":"doc","content":[
        {"type":"heading","attrs":{"level":2},"content":[{"type":"text","text":"Title"}]},
        {"type":"paragraph","content":[
            {"type":"text","text":"Hello "},
            {"type":"text","text":"world","marks":[{"type":"bold"}]},
            {"type":"hardBreak"},
            {"type":"text","text":"line2"}
        ]},
        {"type":"horizontalRule"}
    ]}"#;
    assert_eq!(
        html(doc),
        "<h2>Title</h2><p>Hello <strong>world</strong><br>line2</p><hr>"
    );
}
