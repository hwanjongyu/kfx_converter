use super::*;

#[test]
fn test_p1_7_unclosed_void_html5_tags_in_head_body_and_footnotes() {
    let html = r#"<html>
<head>
  <meta charset="utf-8">
  <link rel="stylesheet" href="s.css">
  <base href="/">
</head>
<body>
  <p>Line 1<br>Line 2<wbr>!</p>
  <hr>
  <img src="fig.png" alt="Tom &amp; Jerry">
  <aside epub:type="footnote" id="fn1">
<p>Note line 1<br>Note line 2</p>
  </aside>
  <p>Sibling paragraph after footnote</p>
</body>
</html>"#;

    let (blocks, footnotes) = parse_xhtml_with_manifest(html, &HashMap::new());
    assert_eq!(
        footnotes.get("fn1").map(String::as_str),
        Some("Note line 1 Note line 2")
    );
    let texts: Vec<String> = blocks.iter().map(Block::plain_text).collect();
    assert!(texts.iter().any(|t| t == "Line 1\nLine 2!"));
    assert!(texts.iter().any(|t| t == "---"));
    assert!(texts.iter().any(|t| t == "Tom & Jerry"));
    assert!(
        texts
            .iter()
            .any(|t| t == "Sibling paragraph after footnote")
    );
}

#[test]
fn test_p1_11_plural_footnote_containers_and_backlinks() {
    let xhtml = r##"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
<body>
  <p>Main claim<a id="fnref1" href="#fn1" epub:type="noteref">1</a> and second<a id="fnref2" href="#fn2">2</a>.</p>
  <section epub:type="footnotes" id="footnotes">
<ol class="endnotes">
  <li id="fn1"><p>First distinct note. <a href="#fnref1" role="doc-backlink">Back</a></p></li>
  <li id="fn2"><p>Second distinct note. <a href="#fnref2" class="footnote-back">Back</a></p></li>
</ol>
  </section>
  <p>After footnotes</p>
</body>
</html>"##;

    let (blocks, footnotes) = parse_xhtml_with_manifest(xhtml, &HashMap::new());
    assert_eq!(footnotes.len(), 2, "footnotes: {:?}", footnotes);
    assert_eq!(
        footnotes.get("fn1").map(String::as_str),
        Some("First distinct note.")
    );
    assert_eq!(
        footnotes.get("fn2").map(String::as_str),
        Some("Second distinct note.")
    );
    assert!(!footnotes.contains_key("footnotes"));
    assert!(!footnotes.contains_key("fnref1"));
    assert!(blocks.iter().any(|b| b.plain_text() == "After footnotes"));
}

#[test]
fn test_p1_11_blockquote_p_center_flush_and_non_a_id_anchors() {
    let xhtml = r#"<html xmlns="http://www.w3.org/1999/xhtml">
<body>
  <blockquote>
<p>Quoted one</p>
<p>Quoted two</p>
  </blockquote>
  <p>Open before center<center>Centered text</center></p>
  <section id="sec_anchor">
<h2 id="h_anchor">Heading</h2>
  </section>
  <p id="empty_p"></p>
  <ul><li id="li_anchor">List item</li></ul>
  <p>Inline <span id="span_anchor">target</span> end</p>
</body>
</html>"#;

    let blocks = parse_xhtml_blocks(xhtml);
    // Blockquote <p>s retain blockquote style
    let bq1 = &blocks[0];
    let bq2 = &blocks[1];
    assert_eq!(bq1.plain_text(), "Quoted one");
    assert_eq!(bq2.plain_text(), "Quoted two");
    assert!(bq1.resolved_style().is_blockquote);
    assert!(bq2.resolved_style().is_blockquote);

    // Open paragraph before <center> was flushed instead of discarded
    assert_eq!(blocks[2].plain_text(), "Open before center");
    assert_eq!(blocks[3].plain_text(), "Centered text");
    assert_eq!(
        blocks[3].resolved_style().alignment,
        crate::ir::TextAlignment::Center
    );

    // Non-<a> anchors captured
    let mut collected_anchors = Vec::new();
    for b in &blocks {
        match b {
            Block::Paragraph { spans, .. } | Block::Heading { spans, .. } => {
                for s in spans {
                    if let Some(ref a) = s.anchor_id {
                        collected_anchors.push(a.clone());
                    }
                }
            }
            Block::List { items, .. } => {
                for item in items {
                    for cb in item {
                        if let Block::Paragraph { spans, .. } = cb {
                            for s in spans {
                                if let Some(ref a) = s.anchor_id {
                                    collected_anchors.push(a.clone());
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    for expected in [
        "sec_anchor",
        "h_anchor",
        "empty_p",
        "li_anchor",
        "span_anchor",
    ] {
        assert!(
            collected_anchors.iter().any(|a| a == expected),
            "Missing anchor '{}' in {:?}",
            expected,
            collected_anchors
        );
    }
}

#[test]
fn test_p1_11_nested_inline_styles_and_css_overrides() {
    let mut css = CssStylesheet::new();
    css.add_css(
        "p.epigraph { font-style: italic; text-align: center; } \
         span.accent { color: #003366; font-weight: bold; }",
    );

    let xhtml = r#"<html xmlns="http://www.w3.org/1999/xhtml">
<body>
  <p class="epigraph" style="font-style: normal; text-align: left">Overridden epigraph</p>
  <p><b>outer <b>inner</b> <span style="font-weight: normal">normal</span> still bold</b></p>
  <p><span class="accent">a <i>b</i> c</span></p>
  <p><b>unclosed bold</p>
  <p>clean next paragraph</p>
</body>
</html>"#;

    let (blocks, _) = parse_xhtml_with_css(xhtml, &HashMap::new(), &css);
    assert_eq!(blocks.len(), 5);

    // 1. Cascaded + inline override on <p>
    if let Block::Paragraph { style, .. } = &blocks[0] {
        let st = style.as_deref().unwrap_or("");
        assert!(
            st.split_whitespace().any(|t| t == "align_left"),
            "st: {}",
            st
        );
        assert!(
            !st.split_whitespace().any(|t| t == "align_center"),
            "st: {}",
            st
        );
        assert!(
            st.split_whitespace().any(|t| t == "font_style_normal"),
            "st: {}",
            st
        );
        assert!(!st.split_whitespace().any(|t| t == "italic"), "st: {}", st);
    } else {
        panic!("expected paragraph 0");
    }

    // 2. Nested <b> + font-weight: normal override
    if let Block::Paragraph { spans, .. } = &blocks[1] {
        let find_span = |needle: &str| {
            spans
                .iter()
                .find(|s| s.text == needle)
                .unwrap_or_else(|| panic!("missing span {:?} in {:?}", needle, spans))
        };
        assert!(find_span("outer ").resolved_styles().format.bold);
        assert!(find_span("inner").resolved_styles().format.bold);
        let norm = find_span("normal");
        assert!(
            !norm.resolved_styles().format.bold,
            "font-weight: normal must suppress outer <b>: {:?}",
            norm
        );
        assert!(find_span(" still bold").resolved_styles().format.bold);
    } else {
        panic!("expected paragraph 1");
    }

    // 3. Outer <span> style preserved across inner <i>
    if let Block::Paragraph { spans, .. } = &blocks[2] {
        let b_span = spans.iter().find(|s| s.text == "b").unwrap();
        let b_st = b_span.style.as_deref().unwrap_or("");
        assert!(b_st.contains("color__003366"), "b_st: {}", b_st);
        assert!(
            b_st.split_whitespace().any(|t| t == "bold"),
            "b_st: {}",
            b_st
        );
        assert!(
            b_st.split_whitespace().any(|t| t == "italic"),
            "b_st: {}",
            b_st
        );
        let c_span = spans.iter().find(|s| s.text == " c").unwrap();
        assert!(
            c_span
                .style
                .as_deref()
                .unwrap_or("")
                .contains("color__003366")
        );
    } else {
        panic!("expected paragraph 2");
    }

    // 4. Unclosed <b> does not bleed past </p>
    if let Block::Paragraph { spans, .. } = &blocks[4] {
        assert!(!spans[0].resolved_styles().format.bold);
    } else {
        panic!("expected paragraph 4");
    }
}

#[test]
fn test_link_allowlist_self_closing_pending_anchor_order_and_dropcap_href() {
    // 1. Allowlisted external URIs vs blocked dangerous schemes + control char stripping
    assert_eq!(
        normalize_span_href("  HTTPS://Example.COM/Path?q=a%20b#frag  ", None).as_deref(),
        Some("https://Example.COM/Path?q=a%20b#frag")
    );
    assert_eq!(
        normalize_span_href("HTTP://example.com/ok", None).as_deref(),
        Some("http://example.com/ok")
    );
    assert_eq!(
        normalize_span_href("MAILTO:Reader@Example.com?subject=Hi%20All", None).as_deref(),
        Some("mailto:Reader@Example.com?subject=Hi%20All")
    );
    for bad in [
        "javascript:alert(1)",
        "java\nscript:alert(1)",
        "  \tjava\r\nscript:alert(1)",
        "data:text/html,<script>alert(1)</script>",
        "vbscript:msgbox(1)",
        "file:///etc/passwd",
        "about:blank",
        "blob:https://example.com/123",
        "ftp://example.com/file",
        "//evil.test/steal",
        "http://",
        "https://#onlyfrag",
        "mailto:",
        "   ",
    ] {
        assert_eq!(
            normalize_span_href(bad, None),
            None,
            "Dangerous/empty URI must be rejected: {:?}",
            bad
        );
    }

    // 2. Self-closing <a href="#x"/> emits no span; <a id="x"/> keeps anchor;
    //    nested <section id="footnotes"><ol><li id="fn1"><p>...</p> puts "fn1" at offset 0;
    //    has-dropcap preserves href on trailing remainder span.
    let xhtml = r##"<html xmlns="http://www.w3.org/1999/xhtml">
<body>
  <p>Before<a href="#empty_only"/> middle <a id="kept_empty" href="#target"/> after</p>
  <p><a href="javascript:alert(1)"><b>Unsafe text</b></a></p>
  <p class="has-dropcap"><a id="dc_anc" href="#target">Once upon</a></p>
  <section id="footnotes">
<ol>
  <li id="fn1"><p>First note text</p></li>
</ol>
  </section>
</body>
</html>"##;

    let (blocks, _) = parse_xhtml_with_manifest(xhtml, &HashMap::new());
    // Paragraph 0: no zero-length span carrying href without anchor
    if let Block::Paragraph { spans, .. } = &blocks[0] {
        assert!(
            !spans
                .iter()
                .any(|s| s.text.is_empty() && s.anchor_id.is_none()),
            "Self-closing <a href/> without id must not emit empty span: {:?}",
            spans
        );
        assert!(
            spans
                .iter()
                .any(|s| s.anchor_id.as_deref() == Some("kept_empty") && s.text.is_empty())
        );
    } else {
        panic!("Expected paragraph 0");
    }

    // Paragraph 1: blocked javascript: drops href while keeping inner bold text intact
    if let Block::Paragraph { spans, .. } = &blocks[1] {
        assert_eq!(spans[0].text, "Unsafe text");
        assert_eq!(spans[0].href, None);
        assert!(spans[0].resolved_styles().format.bold);
    } else {
        panic!("Expected paragraph 1");
    }

    // Paragraph 2: dropcap preserves href on both initial letter and remainder
    if let Block::Paragraph { spans, .. } = &blocks[2] {
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].text, "O");
        assert_eq!(spans[0].anchor_id.as_deref(), Some("dc_anc"));
        assert_eq!(spans[0].href.as_deref(), Some("#target"));
        assert_eq!(spans[1].text, "nce upon");
        assert_eq!(spans[1].anchor_id, None);
        assert_eq!(spans[1].href.as_deref(), Some("#target"));
    } else {
        panic!("Expected paragraph 2");
    }

    // List block: "footnotes" and "fn1" both emitted at offset 0 BEFORE "First note text"
    if let Block::List { items, .. } = &blocks[3] {
        if let Block::Paragraph { spans, .. } = &items[0][0] {
            let mut offset = 0usize;
            let mut anc_offsets = Vec::new();
            for s in spans {
                if let Some(ref anc) = s.anchor_id {
                    anc_offsets.push((anc.as_str(), offset));
                }
                offset += s.text.chars().count();
            }
            assert!(
                anc_offsets.contains(&("footnotes", 0)),
                "Expected footnotes at offset 0, got {:?}",
                anc_offsets
            );
            assert!(
                anc_offsets.contains(&("fn1", 0)),
                "Expected fn1 at offset 0 (not end of span), got {:?}",
                anc_offsets
            );
        } else {
            panic!("Expected list item paragraph");
        }
    } else {
        panic!("Expected list block");
    }
}

#[test]
fn test_normalize_span_href_query_stripping_and_standalone_relative() {
    // Standalone mode (link_ctx == None) preserves decoded relative paths + fragments,
    // strips internal ?query suffixes, and returns None for bare "#".
    assert_eq!(
        normalize_span_href("../Text/ch%202.xhtml?ver=1#sec%201", None).as_deref(),
        Some("../Text/ch 2.xhtml#sec 1")
    );
    assert_eq!(normalize_span_href("#", None), None);
    assert_eq!(
        normalize_span_href("#sec%201", None).as_deref(),
        Some("#sec 1")
    );

    // Chapter context resolves relative path after stripping ?query and decoding %20
    let mut href_to_section = HashMap::new();
    href_to_section.insert("OEBPS/Text/ch 2.xhtml".to_string(), "section_2".to_string());
    let ctx = XhtmlLinkContext {
        current_section_id: "section_1",
        current_chapter_dir: "OEBPS/Part1",
        href_to_section: &href_to_section,
        href_lc_to_section: None,
    };
    assert_eq!(
        normalize_span_href("../Text/ch%202.xhtml?cache=bust#sec%201", Some(&ctx)).as_deref(),
        Some("section_2#sec 1")
    );
}

#[test]
fn test_normalize_span_href_rejects_encoded_schemes_and_backslash_protocol_relative() {
    let mut href_to_section = HashMap::new();
    href_to_section.insert("OEBPS/Text/ch1.xhtml".to_string(), "section_1".to_string());
    // Even if a malicious/broken manifest ever mapped these keys, post-decode URI check rejects them
    href_to_section.insert("javascript:alert(1)".to_string(), "section_1".to_string());
    href_to_section.insert("file:///etc/passwd".to_string(), "section_1".to_string());
    href_to_section.insert("data:text/html,hi".to_string(), "section_1".to_string());
    let ctx = XhtmlLinkContext {
        current_section_id: "section_1",
        current_chapter_dir: "OEBPS/Text",
        href_to_section: &href_to_section,
        href_lc_to_section: None,
    };

    let blocked_cases = [
        "javascript%3Aalert(1)",
        "JAVASCRIPT%3aalert(1)",
        "data%3Atext/html,hi",
        "file%3A///etc/passwd",
        r"\\attacker.example\share",
        r"/\attacker.example/share",
        r"\/attacker.example/share",
        "./javascript%3Aalert(1)",
        r"./\\evil.com\path",
        "%2F%2Fevil.com/path",
        "https%3A//evil.com/path",
        "\u{0085}javascript:alert(1)",
        "javascript\u{0085}:alert(1)",
        "mailto:user @example.com",
    ];

    for bad in blocked_cases {
        assert_eq!(
            normalize_span_href(bad, None),
            None,
            "Expected None (link_ctx=None) for blocked href: {:?}",
            bad
        );
        assert_eq!(
            normalize_span_href(bad, Some(&ctx)),
            None,
            "Expected None (link_ctx=Some) for blocked href: {:?}",
            bad
        );
    }
}

#[test]
fn test_p2_15_block_anchor_marks_stack_prevents_li_empty_p_leak() {
    let xhtml = r#"<html xmlns="http://www.w3.org/1999/xhtml">
<body>
  <ul>
<li id="fn1"><p></p></li>
<li>Next</li>
  </ul>
</body>
</html>"#;
    let blocks = parse_xhtml_blocks(xhtml);
    assert_eq!(blocks.len(), 1);
    let Block::List { items, .. } = &blocks[0] else {
        panic!("Expected list block, got {:?}", blocks);
    };
    assert_eq!(items.len(), 2, "items: {:?}", items);

    // First <li> must contain "fn1"
    let li1_anchors: Vec<&str> = items[0]
        .iter()
        .flat_map(|b| match b {
            Block::Paragraph { spans, .. } | Block::Heading { spans, .. } => spans.as_slice(),
            _ => &[],
        })
        .filter_map(|s| s.anchor_id.as_deref())
        .collect();
    assert_eq!(li1_anchors, vec!["fn1"]);

    // Second <li> must NOT inherit leaked "fn1"
    let li2_anchors: Vec<&str> = items[1]
        .iter()
        .flat_map(|b| match b {
            Block::Paragraph { spans, .. } | Block::Heading { spans, .. } => spans.as_slice(),
            _ => &[],
        })
        .filter_map(|s| s.anchor_id.as_deref())
        .collect();
    assert!(
        li2_anchors.is_empty(),
        "fn1 must not leak to next <li>: {:?}",
        items[1]
    );
    assert_eq!(items[1][0].plain_text(), "Next");
}

#[test]
fn test_p2_15_img_in_heading_preserves_heading_and_figure_anchors() {
    let xhtml = r#"<html xmlns="http://www.w3.org/1999/xhtml">
<body>
  <h1 style="text-align: center">Head <img id="i1" src="a.jpg" alt="Pic"/> Tail</h1>
  <figure id="fig1"><img src="b.jpg" alt="Fig1"/></figure>
  <p>After fig1</p>
  <figure id="fig2"><img id="photo2" src="c.jpg" alt="Fig2"/></figure>
  <p>After fig2</p>
</body>
</html>"#;
    let blocks = parse_xhtml_blocks(xhtml);
    assert_eq!(blocks.len(), 8, "blocks: {:#?}", blocks);

    // <h1> split by <img> preserves Block::Heading, level 1, and style on both halves
    match &blocks[0] {
        Block::Heading {
            level,
            spans,
            style,
            ..
        } => {
            assert_eq!(*level, 1);
            assert_eq!(spans[0].text, "Head ");
            assert!(style.as_deref().unwrap_or("").contains("align_center"));
        }
        other => panic!("Expected Heading before img, got {:?}", other),
    }
    match &blocks[1] {
        Block::Image {
            id, resource_id, ..
        } => {
            assert_eq!(id, "i1");
            assert_eq!(resource_id, "a.jpg");
        }
        other => panic!("Expected Image i1, got {:?}", other),
    }
    match &blocks[2] {
        Block::Heading {
            level,
            spans,
            style,
            ..
        } => {
            assert_eq!(*level, 1);
            assert_eq!(spans[0].text, " Tail");
            assert!(style.as_deref().unwrap_or("").contains("align_center"));
        }
        other => panic!("Expected Heading after img, got {:?}", other),
    }

    // <figure id="fig1"><img src="b.jpg"/></figure> binds fig1 directly to Block::Image.id
    match &blocks[3] {
        Block::Image {
            id, resource_id, ..
        } => {
            assert_eq!(id, "fig1");
            assert_eq!(resource_id, "b.jpg");
        }
        other => panic!("Expected Image fig1, got {:?}", other),
    }
    match &blocks[4] {
        Block::Paragraph { spans, .. } => {
            assert_eq!(spans[0].text, "After fig1");
            assert!(spans.iter().all(|s| s.anchor_id.is_none()));
        }
        other => panic!("Expected Paragraph after fig1, got {:?}", other),
    }

    // <figure id="fig2"><img id="photo2" src="c.jpg"/></figure> keeps both anchors before/on image
    match &blocks[5] {
        Block::Paragraph { spans, .. } => {
            assert_eq!(spans.len(), 1);
            assert_eq!(spans[0].text, "");
            assert_eq!(spans[0].anchor_id.as_deref(), Some("fig2"));
        }
        other => panic!("Expected anchor-marker paragraph for fig2, got {:?}", other),
    }
    match &blocks[6] {
        Block::Image { id, .. } => assert_eq!(id, "photo2"),
        other => panic!("Expected Image photo2, got {:?}", other),
    }
    match &blocks[7] {
        Block::Paragraph { spans, .. } => {
            assert_eq!(spans[0].text, "After fig2");
            assert!(spans.iter().all(|s| s.anchor_id.is_none()));
        }
        other => panic!("Expected Paragraph after fig2, got {:?}", other),
    }
}

#[test]
fn test_p2_15_nested_tables_list_routing_anchor_containment_and_eof_flush() {
    // 1. Nested table inside <td>, <td id> containment, empty <td/>, block tags & <img> inside <td>,
    //    and heading flush before <table>.
    let xhtml = r#"<html xmlns="http://www.w3.org/1999/xhtml">
<body>
  <h2>Before table</h2>
  <table id="tbl1">
<tr id="row1">
  <td id="c1"><p>OuterA</p><table><tr><td id="inner_c">Inner1</td><td>Inner2</td></tr></table><p>OuterB</p><img id="td_img" src="x.jpg" alt="Icon"/></td>
  <td/>
  <td>Col3</td>
</tr>
<tr>
  <td>Row2A</td>
  <td>Row2B</td>
</tr>
  </table>
  <p>Post table</p>
  <ul>
<li>Intro<table><tr><td>L1</td><td>L2</td></tr></table></li>
  </ul>
</body>
</html>"#;
    let blocks = parse_xhtml_blocks(xhtml);
    assert_eq!(blocks.len(), 4, "blocks: {:#?}", blocks);
    assert!(matches!(&blocks[0], Block::Heading { level: 2, .. }));
    assert_eq!(blocks[0].plain_text(), "Before table");

    match &blocks[1] {
        Block::Table { id, rows, .. } => {
            assert_eq!(id, "tbl1");
            assert_eq!(rows.len(), 2);
            assert_eq!(rows[0].len(), 3);
            assert_eq!(rows[0][0], "OuterA Inner1 Inner2 OuterB Icon");
            assert_eq!(rows[0][1], "");
            assert_eq!(rows[0][2], "Col3");
            assert_eq!(rows[1], vec!["Row2A", "Row2B"]);
        }
        other => panic!("Expected Table, got {:?}", other),
    }

    // Neither row1, c1, inner_c, nor td_img leaked onto Post table
    match &blocks[2] {
        Block::Paragraph { spans, .. } => {
            assert_eq!(spans[0].text, "Post table");
            assert!(
                spans.iter().all(|s| s.anchor_id.is_none()),
                "Table cell anchors leaked: {:?}",
                spans
            );
        }
        other => panic!("Expected Post table paragraph, got {:?}", other),
    }

    // Table inside <li> routed into Block::List item blocks after Intro paragraph
    match &blocks[3] {
        Block::List { items, .. } => {
            assert_eq!(items.len(), 1);
            assert_eq!(items[0].len(), 2, "list item blocks: {:?}", items[0]);
            assert_eq!(items[0][0].plain_text(), "Intro");
            match &items[0][1] {
                Block::Table { rows, .. } => assert_eq!(rows, &vec![vec!["L1", "L2"]]),
                other => panic!("Expected Table inside list item, got {:?}", other),
            }
        }
        other => panic!("Expected List block, got {:?}", other),
    }

    // 2. Unclosed table at EOF flushes cleanly (and contains <td id>)
    let eof_xhtml = r#"<html><body><table id="unclosed"><tr><td id="eof_td">Cell A</td><td>Cell B"#;
    let eof_blocks = parse_xhtml_blocks(eof_xhtml);
    assert_eq!(eof_blocks.len(), 1, "eof_blocks: {:?}", eof_blocks);
    match &eof_blocks[0] {
        Block::Table { id, rows, .. } => {
            assert_eq!(id, "unclosed");
            assert_eq!(rows, &vec![vec!["Cell A", "Cell B"]]);
        }
        other => panic!("Expected unclosed Table at EOF, got {:?}", other),
    }
}

#[test]
fn test_p2_16_bounded_tag_stacks_deep_nesting_exits_cleanly() {
    let deep_divs_open = "<div>".repeat(1_000);
    let deep_spans_open = "<span><b>".repeat(600);
    let deep_spans_close = "</b></span>".repeat(600);
    let deep_bq_open = "<blockquote>".repeat(400);
    let deep_bq_close = "</blockquote>".repeat(400);

    let xhtml = format!(
        concat!(
            "<html>",
            "<head>{divs}</head>",
            "<body>",
            "<aside epub:type=\"footnote\" id=\"fn1\">{divs}Note body</aside>",
            "<p>{spans}Deep text{spans_close}</p>",
            "<p><b>{spans}Unclosed deep inline</p>",
            "<p>Clean after unclosed</p>",
            "{bq_open}<p>Quoted</p>{bq_close}",
            "<p>Plain after blockquote</p>",
            "</body></html>"
        ),
        divs = deep_divs_open,
        spans = deep_spans_open,
        spans_close = deep_spans_close,
        bq_open = deep_bq_open,
        bq_close = deep_bq_close,
    );

    let (blocks, footnotes) = parse_xhtml_with_manifest(&xhtml, &HashMap::new());
    assert_eq!(
        footnotes.get("fn1").map(String::as_str),
        Some("Note body"),
        "Footnote sentinel must exit cleanly despite 1,000 unclosed inner divs"
    );
    let texts: Vec<String> = blocks.iter().map(Block::plain_text).collect();
    assert!(texts.iter().any(|t| t == "Deep text"), "texts: {:?}", texts);
    assert!(
        texts.iter().any(|t| t == "Clean after unclosed"),
        "texts: {:?}",
        texts
    );
    // Clean paragraph after unclosed deep <b><span> must not inherit bold
    let clean_p = blocks
        .iter()
        .find(|b| b.plain_text() == "Clean after unclosed")
        .unwrap();
    if let Block::Paragraph { spans, .. } = clean_p {
        assert!(!spans[0].resolved_styles().format.bold);
    }
    // Quoted paragraph has blockquote style, and paragraph after all </blockquote>s does not
    let quoted_p = blocks.iter().find(|b| b.plain_text() == "Quoted").unwrap();
    assert!(quoted_p.resolved_style().is_blockquote);
    let plain_p = blocks
        .iter()
        .find(|b| b.plain_text() == "Plain after blockquote")
        .unwrap();
    assert!(!plain_p.resolved_style().is_blockquote);
}

#[test]
fn test_span_unstyled_unknown_classes_do_not_leak_as_kfx_styles() {
    let xhtml = r#"<html xmlns="http://www.w3.org/1999/xhtml">
<body>
  <p><span class="sub">Sub</span> <span class="sup">Sup</span> <span class="code">Code</span> <span class="bold">Bold</span> <span class="italic">Italic</span> <span class="underline">Underline</span> <span class="strike">Strike</span> <span class="noteref">Ref</span></p>
  <p><span class="dropcap">T</span>he <span class="small-caps">Small</span> and <span class="all-caps">Loud</span></p>
</body>
</html>"#;

    let empty_css = CssStylesheet::new();
    let (blocks, _) = parse_xhtml_with_css(xhtml, &HashMap::new(), &empty_css);
    assert_eq!(blocks.len(), 2);

    // 1. Unstyled raw class names must NOT leak into SpanStyles formatting or custom_classes
    match &blocks[0] {
        Block::Paragraph { spans, .. } => {
            for label in [
                "Sub",
                "Sup",
                "Code",
                "Bold",
                "Italic",
                "Underline",
                "Strike",
                "Ref",
            ] {
                let span = spans
                    .iter()
                    .find(|s| s.text.trim() == label)
                    .unwrap_or_else(|| panic!("Missing span {label:?}: {spans:?}"));
                assert_eq!(
                    span.resolved_styles(),
                    crate::ir::SpanStyles::default(),
                    "Unstyled span {label:?} leaked styles: {:?}",
                    span.style
                );
            }
        }
        other => panic!("Expected Paragraph, got {:?}", other),
    }

    // 2. Semantic allowlisted classes (`dropcap`, `small-caps`, `all-caps`) continue working
    match &blocks[1] {
        Block::Paragraph { spans, .. } => {
            let dropcap_span = spans.iter().find(|s| s.text == "T").unwrap();
            assert_eq!(
                dropcap_span.resolved_styles().dropcap,
                crate::ir::DropCapStyle::StandardSingleChar
            );
            let sc_span = spans.iter().find(|s| s.text == "Small").unwrap();
            assert!(sc_span.resolved_styles().format.small_caps);
            let uc_span = spans.iter().find(|s| s.text == "Loud").unwrap();
            assert!(uc_span.resolved_styles().format.uppercase);
        }
        other => panic!("Expected Paragraph, got {:?}", other),
    }

    // 3. CSS-backed `.sub { font-weight: bold; }` applies `bold` without leaking `subscript`
    let mut book_css = CssStylesheet::new();
    book_css.add_css(".sub { font-weight: bold; }");
    let (styled_blocks, _) = parse_xhtml_with_css(
        r#"<html xmlns="http://www.w3.org/1999/xhtml"><body><p><span class="sub">StyledSub</span></p></body></html>"#,
        &HashMap::new(),
        &book_css,
    );
    assert_eq!(styled_blocks.len(), 1);
    match &styled_blocks[0] {
        Block::Paragraph { spans, .. } => {
            let st = spans[0].resolved_styles();
            assert!(st.format.bold, "CSS-backed .sub should be bold: {:?}", st);
            assert!(
                !st.format.subscript,
                "CSS-backed .sub must not leak raw subscript token: {:?}",
                st
            );
        }
        other => panic!("Expected Paragraph, got {:?}", other),
    }
}

#[test]
fn test_parse_xhtml_with_warnings_reports_malformed_xml_and_preserves_prefix() {
    let empty_css = CssStylesheet::new();

    // 1. Well-formed XHTML produces zero warnings
    let valid_xhtml = r#"<html xmlns="http://www.w3.org/1999/xhtml">
<body>
  <p>Clean paragraph.</p>
</body>
</html>"#;
    let ok_res = parse_xhtml_with_warnings(valid_xhtml, &HashMap::new(), &empty_css, None);
    assert!(
        ok_res.warnings.is_empty(),
        "Well-formed XHTML must produce no warnings, got: {:?}",
        ok_res.warnings
    );
    assert_eq!(ok_res.blocks.len(), 1);
    assert_eq!(ok_res.blocks[0].plain_text(), "Clean paragraph.");

    // 2. Malformed XML emits byte-offset warning and preserves already-parsed prefix blocks
    let malformed_xhtml = r#"<html xmlns="http://www.w3.org/1999/xhtml">
<body>
  <p>Valid prefix paragraph.</p>
  <p>Broken <span class="unterminated>trailing text</p>
</body>
</html>"#;
    let err_res = parse_xhtml_with_warnings(malformed_xhtml, &HashMap::new(), &empty_css, None);
    assert!(
        err_res
            .warnings
            .iter()
            .any(|w| w.contains("XHTML parse error at byte offset")),
        "Expected XHTML parse error diagnostic warning, got: {:?}",
        err_res.warnings
    );
    assert!(
        err_res
            .blocks
            .iter()
            .any(|b| b.plain_text() == "Valid prefix paragraph."),
        "Valid blocks before the syntax error must be preserved: {:?}",
        err_res.blocks
    );
}

#[test]
fn test_stage3a_table_rich_spans_formatting_isolation_and_anchor_containment() {
    // 1. Direct trim_cell_spans test: preserves empty-text anchor markers and clears
    //    style/href on whitespace-only anchor spans.
    let mut raw_spans = vec![
        TextSpan {
            text: "   ".to_string(),
            style: Some("bold".to_string()),
            anchor_id: Some("lead_anc".to_string()),
            href: Some("#x".to_string()),
            ruby: None,
        },
        TextSpan {
            text: "  Hello ".to_string(),
            style: Some("italic".to_string()),
            anchor_id: None,
            href: None,
            ruby: None,
        },
        TextSpan {
            text: "World  ".to_string(),
            style: None,
            anchor_id: None,
            href: None,
            ruby: None,
        },
        TextSpan {
            text: " \n ".to_string(),
            style: Some("underline".to_string()),
            anchor_id: Some("trail_anc".to_string()),
            href: Some("#y".to_string()),
            ruby: None,
        },
    ];
    trim_cell_spans(&mut raw_spans);
    assert_eq!(raw_spans.len(), 4);
    assert_eq!(raw_spans[0].text, "");
    assert_eq!(raw_spans[0].anchor_id.as_deref(), Some("lead_anc"));
    assert_eq!(raw_spans[0].style, None);
    assert_eq!(raw_spans[0].href, None);
    assert_eq!(raw_spans[1].text, "Hello ");
    assert_eq!(raw_spans[2].text, "World");
    assert_eq!(raw_spans[3].text, "");
    assert_eq!(raw_spans[3].anchor_id.as_deref(), Some("trail_anc"));
    assert_eq!(raw_spans[3].style, None);
    assert_eq!(raw_spans[3].href, None);

    // 2. Full XHTML table with anchors, formatting, links, ruby, <p>/<hr>, unclosed tags,
    //    empty <td id/>, and nested <table> inside <td>.
    let xhtml = r##"<html xmlns="http://www.w3.org/1999/xhtml">
<body>
  <table id="tbl_rich">
<tr id="tr_hdr">
  <th id="th_1"><b>Header</b> <a href="#ref1">Link</a></th>
  <td id="td_empty"/>
</tr>
<tr>
  <td id="td_unclosed"><b><a href="#bleed">Unclosed</td>
  <td>CleanCell <ruby>漢<rt>かん</rt></ruby></td>
</tr>
<tr>
  <td><p id="inner_p">Before</p><hr/><table id="nested_tbl"><tr><td id="nc1"><i>N1</i></td><td><a href="#n2">N2</a></td></tr></table><a id="empty_a"/>After</td>
  <td><p>P1</p><div>P2</div></td>
</tr>
  </table>
  <p>Post table clean</p>
</body>
</html>"##;

    let blocks = parse_xhtml_blocks(xhtml);
    assert_eq!(blocks.len(), 2, "blocks: {:#?}", blocks);

    let Block::Table {
        id,
        rows,
        cell_spans,
    } = &blocks[0]
    else {
        panic!("Expected Table, got {:?}", blocks[0]);
    };
    assert_eq!(id, "tbl_rich");
    assert_eq!(rows.len(), 3);
    assert_eq!(cell_spans.len(), 3);

    // 1:1 parity between rows[r][c] and cell_spans[r][c]
    for (r_idx, (row, span_row)) in rows.iter().zip(cell_spans.iter()).enumerate() {
        assert_eq!(row.len(), span_row.len(), "row {r_idx} column count");
        for (c_idx, (plain, spans)) in row.iter().zip(span_row.iter()).enumerate() {
            let joined: String = spans.iter().map(|s| s.text.as_str()).collect();
            assert_eq!(plain, &joined, "mismatch at cell ({r_idx}, {c_idx})");
        }
    }

    // Row 0: <tr id="tr_hdr"> and <th id="th_1"> contained in (0, 0); <td id="td_empty"/> in (0, 1)
    let r0c0_anchors: Vec<&str> = cell_spans[0][0]
        .iter()
        .filter_map(|s| s.anchor_id.as_deref())
        .collect();
    assert_eq!(r0c0_anchors, vec!["tr_hdr", "th_1"]);
    assert!(
        cell_spans[0][0]
            .iter()
            .any(|s| s.text == "Header" && s.resolved_styles().format.bold)
    );
    assert!(
        cell_spans[0][0]
            .iter()
            .any(|s| s.text == "Link" && s.href.as_deref() == Some("#ref1"))
    );
    assert_eq!(rows[0][1], "");
    assert_eq!(
        cell_spans[0][1]
            .iter()
            .filter_map(|s| s.anchor_id.as_deref())
            .collect::<Vec<_>>(),
        vec!["td_empty"]
    );

    // Row 1: unclosed <b><a href="#bleed"> does NOT bleed into cell (1, 1)
    assert_eq!(rows[1][0], "Unclosed");
    assert!(
        cell_spans[1][0]
            .iter()
            .any(|s| s.anchor_id.as_deref() == Some("td_unclosed"))
    );
    assert_eq!(rows[1][1], "CleanCell 漢");
    assert!(
        cell_spans[1][1]
            .iter()
            .all(|s| { !s.resolved_styles().format.bold && s.href.is_none() })
    );
    assert!(
        cell_spans[1][1]
            .iter()
            .any(|s| s.text == "漢" && s.ruby.as_deref() == Some("かん"))
    );

    // Row 2: nested table preserves child styles/links/anchors and <hr/> appends ---
    assert_eq!(rows[2][0], "Before --- N1 N2 After");
    assert!(
        cell_spans[2][0]
            .iter()
            .any(|s| s.anchor_id.as_deref() == Some("inner_p"))
    );
    assert!(
        cell_spans[2][0]
            .iter()
            .any(|s| s.anchor_id.as_deref() == Some("nested_tbl"))
    );
    assert!(
        cell_spans[2][0]
            .iter()
            .any(|s| s.anchor_id.as_deref() == Some("nc1")
                && s.text == "N1"
                && s.resolved_styles().format.italic)
    );
    assert!(
        cell_spans[2][0]
            .iter()
            .any(|s| s.text == "N2" && s.href.as_deref() == Some("#n2"))
    );
    assert!(
        cell_spans[2][0]
            .iter()
            .any(|s| s.anchor_id.as_deref() == Some("empty_a"))
    );
    assert_eq!(rows[2][1], "P1 P2");

    // Post-table paragraph has no leaked bold, href, or table anchors
    match &blocks[1] {
        Block::Paragraph { spans, .. } => {
            assert_eq!(spans.len(), 1);
            assert_eq!(spans[0].text, "Post table clean");
            assert!(!spans[0].resolved_styles().format.bold);
            assert!(spans[0].href.is_none());
            assert!(spans[0].anchor_id.is_none());
        }
        other => panic!("Expected clean post-table Paragraph, got {:?}", other),
    }

    // 3. Unclosed table at EOF flushes cell_spans 1:1 with rows and contains anchors
    let eof_blocks = parse_xhtml_blocks(r#"<html><body><table><tr><td id="eof_c"><b>A"#);
    assert_eq!(eof_blocks.len(), 1);
    match &eof_blocks[0] {
        Block::Table {
            rows, cell_spans, ..
        } => {
            assert_eq!(rows, &vec![vec!["A"]]);
            assert_eq!(cell_spans[0][0][0].text, "A");
            assert_eq!(cell_spans[0][0][0].anchor_id.as_deref(), Some("eof_c"));
            assert!(cell_spans[0][0][0].resolved_styles().format.bold);
        }
        other => panic!("Expected Table at EOF, got {:?}", other),
    }
}

#[test]
fn test_stage3b_image_links_svg_xlink_and_in_cell_img() {
    let xhtml = r##"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:xlink="http://www.w3.org/1999/xlink">
<body>
  <p><a href="#t1"><img src="fig1.png" alt="Diagram"/></a></p>
  <p><a id="a_img" href="ch2.xhtml#fig"><img src="fig2.png"/></a></p>
  <svg><a xlink:href="#svg_t"><image xlink:href="vec.svg"/></a></svg>
  <p><a href="javascript:alert(1)"><img src="blocked.png"/></a></p>
  <table>
<tr>
  <td id="c_img"><a id="cell_a" href="#cell_t"><img id="img_id" src="icon.png" alt="Cell Img"/></a></td>
  <td><img id="no_alt_img" src="empty.png" alt=""/></td>
</tr>
  </table>
</body>
</html>"##;

    let blocks = parse_xhtml_blocks(xhtml);
    assert_eq!(blocks.len(), 5, "blocks: {:#?}", blocks);

    match &blocks[0] {
        Block::Image {
            resource_id,
            alt,
            href,
            ..
        } => {
            assert_eq!(resource_id, "fig1.png");
            assert_eq!(alt.as_deref(), Some("Diagram"));
            assert_eq!(href.as_deref(), Some("#t1"));
        }
        other => panic!("Expected linked Image 1, got {:?}", other),
    }

    // <a id="a_img" href="ch2.xhtml#fig"><img src="fig2.png"/></a> adopts a_img without dangling span
    match &blocks[1] {
        Block::Image {
            id,
            resource_id,
            href,
            ..
        } => {
            assert_eq!(id, "a_img");
            assert_eq!(resource_id, "fig2.png");
            assert_eq!(href.as_deref(), Some("ch2.xhtml#fig"));
        }
        other => panic!("Expected linked Image 2, got {:?}", other),
    }

    // SVG <a xlink:href><image xlink:href/></a>
    match &blocks[2] {
        Block::Image {
            resource_id, href, ..
        } => {
            assert_eq!(resource_id, "vec.svg");
            assert_eq!(href.as_deref(), Some("#svg_t"));
        }
        other => panic!("Expected SVG linked Image 3, got {:?}", other),
    }

    // Blocked href degrades cleanly to href: None
    match &blocks[3] {
        Block::Image {
            resource_id, href, ..
        } => {
            assert_eq!(resource_id, "blocked.png");
            assert_eq!(*href, None);
        }
        other => panic!("Expected unlinked blocked Image 4, got {:?}", other),
    }

    // In-cell <img> emits linked TextSpan for alt and retains anchor when alt is empty
    match &blocks[4] {
        Block::Table {
            rows, cell_spans, ..
        } => {
            assert_eq!(rows[0][0], "Cell Img");
            let img_span = cell_spans[0][0]
                .iter()
                .find(|s| s.text == "Cell Img")
                .expect("missing Cell Img span");
            assert_eq!(img_span.href.as_deref(), Some("#cell_t"));
            assert_eq!(img_span.anchor_id.as_deref(), Some("cell_a"));
            let cell0_anchors: Vec<&str> = cell_spans[0][0]
                .iter()
                .filter_map(|s| s.anchor_id.as_deref())
                .collect();
            assert_eq!(cell0_anchors, vec!["c_img", "img_id", "cell_a"]);

            assert_eq!(rows[0][1], "");
            let cell1_anchors: Vec<&str> = cell_spans[0][1]
                .iter()
                .filter_map(|s| s.anchor_id.as_deref())
                .collect();
            assert_eq!(cell1_anchors, vec!["no_alt_img"]);
        }
        other => panic!("Expected Table block 5, got {:?}", other),
    }
}

#[test]
fn test_stage3c_link_pseudo_css_resolution_and_last_wins_cleanup() {
    let mut book_css = CssStylesheet::new();
    book_css.add_css(
        r#"
        a:link { color: #0044AA; text-decoration: underline; font-weight: bold; }
        a:visited { color: purple; font-style: italic; }
        a.plain:link { text-decoration: none; font-weight: normal; color: #006633; }
        a.plain:visited { font-style: normal; color: navy; }
        "#,
    );

    let xhtml = r##"<html xmlns="http://www.w3.org/1999/xhtml">
<body>
  <p><a href="#ch2">Default</a> <a class="plain" href="#ch3"><span style="color: #880000">Plain</span></a> <a id="anc_only">AnchorOnly</a> <a href="javascript:void(0)">Blocked</a></p>
</body>
</html>"##;

    let (blocks, _) = parse_xhtml_with_css(xhtml, &HashMap::new(), &book_css);
    assert_eq!(blocks.len(), 1);
    let Block::Paragraph { spans, .. } = &blocks[0] else {
        panic!("Expected Paragraph, got {:?}", blocks[0]);
    };

    let def_span = spans.iter().find(|s| s.text == "Default").unwrap();
    let def_classes = &def_span.resolved_styles().custom_classes;
    assert!(
        def_classes
            .iter()
            .any(|c| c == "link_unvisited_color_0044AA")
    );
    assert!(def_classes.iter().any(|c| c == "link_unvisited_underline"));
    assert!(def_classes.iter().any(|c| c == "link_unvisited_bold"));
    assert!(def_classes.iter().any(|c| c == "link_visited_color_800080"));
    assert!(def_classes.iter().any(|c| c == "link_visited_italic"));

    // .plain overrides replace prior unvisited/visited tokens cleanly (last-wins),
    // while inner <span style="color: #880000"> keeps its own color token without clobbering link_state colors.
    let plain_span = spans.iter().find(|s| s.text == "Plain").unwrap();
    let plain_classes = &plain_span.resolved_styles().custom_classes;
    assert!(
        plain_classes
            .iter()
            .any(|c| c == "link_unvisited_underline_none")
    );
    assert!(
        !plain_classes
            .iter()
            .any(|c| c == "link_unvisited_underline")
    );
    assert!(
        plain_classes
            .iter()
            .any(|c| c == "link_unvisited_weight_normal")
    );
    assert!(!plain_classes.iter().any(|c| c == "link_unvisited_bold"));
    assert!(
        plain_classes
            .iter()
            .any(|c| c == "link_unvisited_color_006633")
    );
    assert!(
        !plain_classes
            .iter()
            .any(|c| c == "link_unvisited_color_0044AA")
    );
    assert!(
        plain_classes
            .iter()
            .any(|c| c == "link_visited_style_normal")
    );
    assert!(!plain_classes.iter().any(|c| c == "link_visited_italic"));
    assert!(
        plain_classes
            .iter()
            .any(|c| c == "link_visited_color_000080")
    );
    assert!(
        !plain_classes
            .iter()
            .any(|c| c == "link_visited_color_800080")
    );
    assert!(plain_classes.iter().any(|c| c == "color__880000"));

    // Non-href and blocked-href anchors must NOT receive link_unvisited_* / link_visited_* tokens
    for label in ["AnchorOnly", "Blocked"] {
        let s = spans.iter().find(|s| s.text == label).unwrap();
        let classes = &s.resolved_styles().custom_classes;
        assert!(
            classes
                .iter()
                .all(|c| !c.starts_with("link_unvisited_") && !c.starts_with("link_visited_")),
            "Anchor {label} unexpectedly received link-state tokens: {classes:?}"
        );
    }

    // Direct last-wins check across stacked style_stack frames in resolve_active_span_attrs
    let stack = vec![
        StyleFrameKind::Inline {
            tag: b"a".to_vec(),
            bold: false,
            italic: false,
            underline: false,
            strike: false,
            sup: false,
            sub: false,
            code: false,
            css_tokens: vec![
                "link_unvisited_underline".to_string(),
                "link_unvisited_bold".to_string(),
                "link_unvisited_color_0044AA".to_string(),
                "link_visited_italic".to_string(),
                "link_visited_color_800080".to_string(),
            ],
            href: Some("#x".to_string()),
            anchor: None,
            is_noteref: false,
            emitted_span: false,
        },
        StyleFrameKind::Inline {
            tag: b"span".to_vec(),
            bold: false,
            italic: false,
            underline: false,
            strike: false,
            sup: false,
            sub: false,
            code: false,
            css_tokens: vec![
                "link_unvisited_underline_none".to_string(),
                "link_unvisited_weight_normal".to_string(),
                "link_unvisited_color_006633".to_string(),
                "link_visited_style_normal".to_string(),
                "link_visited_color_000080".to_string(),
            ],
            href: None,
            anchor: None,
            is_noteref: false,
            emitted_span: false,
        },
    ];
    let attrs = resolve_active_span_attrs(&stack);
    let tokens: Vec<&str> = attrs
        .style
        .as_deref()
        .unwrap_or("")
        .split_whitespace()
        .collect();
    assert_eq!(
        tokens,
        vec![
            "link_unvisited_underline_none",
            "link_unvisited_weight_normal",
            "link_unvisited_color_006633",
            "link_visited_style_normal",
            "link_visited_color_000080",
        ]
    );
}

#[test]
fn test_quick_xml_general_ref_splitting_and_whitespace_preservation() {
    let xhtml = r##"<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
<body>
  <p>Foo &mdash; bar</p>
  <p>A&nbsp;B</p>
  <p>Tom &amp; Jerry</p>
  <p>&ldquo;Hello&rdquo;</p>
  <p>Dash &#x2014; and&#160;space</p>
  <p>Keep &madeup; verbatim</p>
  <p>&amp;lt;tag&amp;gt; and &amp;#8212;</p>
  <h1>Ch. 1 &mdash; Intro &amp; &#x201C;Scope&#x201D;</h1>
  <table>
<tr>
  <th>Col &amp; 1</th>
  <td>Val &mdash; A&#160;B</td>
</tr>
  </table>
  <p>Base <ruby>A&amp;B<rt>a&#x2014;b</rt></ruby> end</p>
  <ul><li>Item 1 &mdash; A&nbsp;B</li></ul>
  <div>&ldquo;Implicit&rdquo;</div>
  <div>&nbsp;SkippedLeading</div>
  <p>&nbsp;ExplicitLeading</p>
  <aside epub:type="footnote" id="fn_ent">
<p>Note &mdash; see A&amp;B &#160;end.</p>
  </aside>
</body>
</html>"##;

    let empty_css = CssStylesheet::new();
    let parsed = parse_xhtml_with_warnings(xhtml, &HashMap::new(), &empty_css, None);
    assert!(
        parsed.warnings.is_empty(),
        "Unexpected warnings: {:?}",
        parsed.warnings
    );

    let blocks = &parsed.blocks;
    assert_eq!(blocks[0].plain_text(), "Foo — bar");
    assert_eq!(blocks[1].plain_text(), "A\u{00A0}B");
    assert_eq!(blocks[2].plain_text(), "Tom & Jerry");
    assert_eq!(blocks[3].plain_text(), "“Hello”");
    assert_eq!(blocks[4].plain_text(), "Dash — and\u{00A0}space");
    assert_eq!(blocks[5].plain_text(), "Keep &madeup; verbatim");
    assert_eq!(blocks[6].plain_text(), "&lt;tag&gt; and &#8212;");

    // Heading preserves Text -> GeneralRef -> Text
    assert!(matches!(blocks[7], Block::Heading { level: 1, .. }));
    assert_eq!(blocks[7].plain_text(), "Ch. 1 — Intro & “Scope”");

    // Table cells preserve entities and spaces
    let Block::Table { rows, .. } = &blocks[8] else {
        panic!("Expected Table at blocks[8], got {:?}", blocks[8]);
    };
    assert_eq!(rows, &vec![vec!["Col & 1", "Val — A\u{00A0}B"]]);

    // Ruby base and <rt> preserve entities
    let Block::Paragraph { spans, .. } = &blocks[9] else {
        panic!("Expected Paragraph at blocks[9], got {:?}", blocks[9]);
    };
    let ruby_span = spans
        .iter()
        .find(|s| s.text == "A&B")
        .expect("ruby base A&B");
    assert_eq!(ruby_span.ruby.as_deref(), Some("a—b"));
    assert_eq!(blocks[9].plain_text(), "Base A&B end");

    // List item preserves entities & adjacent whitespace
    let Block::List { items, .. } = &blocks[10] else {
        panic!("Expected List at blocks[10], got {:?}", blocks[10]);
    };
    assert_eq!(items[0][0].plain_text(), "Item 1 — A\u{00A0}B");

    // Implicit <p> edge cases inside bare <div> vs explicit <p>
    assert_eq!(blocks[11].plain_text(), "“Implicit”");
    assert_eq!(blocks[12].plain_text(), "SkippedLeading");
    assert_eq!(blocks[13].plain_text(), "\u{00A0}ExplicitLeading");

    // Footnote body preserves Text -> GeneralRef -> Text
    assert_eq!(
        parsed.footnotes.get("fn_ent").map(String::as_str),
        Some("Note — see A&B \u{00A0}end.")
    );
}
