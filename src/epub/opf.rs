//! OPF package document parsing, metadata extraction, manifest items, and spine reading order.

use quick_xml::Reader as XmlReader;
use quick_xml::events::Event;
use std::collections::HashMap;

use crate::epub::EpubError;
use crate::ir::BookMetadata;

#[derive(Debug, Clone)]
pub struct ManifestItem {
    pub id: String,
    pub href: String,
    pub media_type: String,
    pub properties: Option<String>,
}

pub type ParsedOpf = (BookMetadata, HashMap<String, ManifestItem>, Vec<String>);

/// Parses package OPF XML content into metadata, manifest item dictionary, and spine IDs.
pub fn parse_opf(opf_xml: &str) -> Result<ParsedOpf, EpubError> {
    let mut metadata = BookMetadata::default();
    let mut manifest = HashMap::new();
    let mut spine = Vec::new();

    let mut titles: Vec<(Option<String>, Option<String>, String)> = Vec::new();
    let mut title_types: HashMap<String, String> = HashMap::new();
    let mut identifiers: Vec<(Option<String>, String)> = Vec::new();
    let mut unique_identifier_id: Option<String> = None;

    let mut xml_reader = XmlReader::from_str(opf_xml);
    let mut buf = Vec::new();
    let mut tag_stack: Vec<String> = Vec::new();
    let mut current_text = String::new();
    let mut current_title_id: Option<String> = None;
    let mut current_title_type: Option<String> = None;
    let mut current_identifier_id: Option<String> = None;
    let mut current_meta_refines: Option<String> = None;

    fn attr_local_key(key: &str) -> &str {
        key.split_once(':').map_or(key, |(_, local)| local)
    }

    fn is_metadata_value_tag(tag: &str) -> bool {
        matches!(
            tag,
            "title"
                | "identifier"
                | "creator"
                | "language"
                | "publisher"
                | "description"
                | "date"
                | "meta"
        )
    }

    loop {
        let ev = xml_reader.read_event_into(&mut buf);
        let start_or_empty = match &ev {
            Ok(Event::Start(e)) => Some((e, true)),
            Ok(Event::Empty(e)) => Some((e, false)),
            _ => None,
        };
        if let Some((e, is_start)) = start_or_empty {
            let local = e.local_name().as_ref().to_string();
            if is_start {
                tag_stack.push(local.clone());
            }

            match local.as_str() {
                "package" => {
                    for attr in e.attributes().flatten() {
                        if attr_local_key(attr.key.as_ref()) == "unique-identifier" {
                            unique_identifier_id = Some(crate::epub::unescape_attr_value(&attr));
                        }
                    }
                }
                "item" => {
                    let mut id = String::new();
                    let mut href = String::new();
                    let mut media_type = String::new();
                    let mut properties = None;

                    for attr in e.attributes().flatten() {
                        match attr.key.as_ref() {
                            "id" => id = crate::epub::unescape_attr_value(&attr),
                            "href" => href = crate::epub::unescape_attr_value(&attr),
                            "media-type" => media_type = crate::epub::unescape_attr_value(&attr),
                            "properties" => {
                                properties = Some(crate::epub::unescape_attr_value(&attr))
                            }
                            _ => {}
                        }
                    }

                    if !id.is_empty() && !href.is_empty() {
                        if let Some(ref props) = properties
                            && props.contains("cover-image")
                            && metadata.cover_image_id.is_none()
                        {
                            metadata.cover_image_id = Some(id.clone());
                        }
                        manifest.insert(
                            id.clone(),
                            ManifestItem {
                                id,
                                href,
                                media_type,
                                properties,
                            },
                        );
                    }
                }
                "itemref" => {
                    for attr in e.attributes().flatten() {
                        if attr.key.as_ref() == "idref" {
                            spine.push(crate::epub::unescape_attr_value(&attr));
                        }
                    }
                }
                "title" if is_start => {
                    current_text.clear();
                    current_title_id = None;
                    current_title_type = None;
                    for attr in e.attributes().flatten() {
                        let local_key = attr_local_key(attr.key.as_ref());
                        if local_key == "id" {
                            current_title_id = Some(crate::epub::unescape_attr_value(&attr));
                        } else if local_key == "title-type" {
                            current_title_type = Some(crate::epub::unescape_attr_value(&attr));
                        }
                    }
                }
                "identifier" if is_start => {
                    current_text.clear();
                    current_identifier_id = None;
                    for attr in e.attributes().flatten() {
                        if attr_local_key(attr.key.as_ref()) == "id" {
                            current_identifier_id = Some(crate::epub::unescape_attr_value(&attr));
                        }
                    }
                }
                "meta" => {
                    if is_start {
                        current_text.clear();
                        current_meta_refines = None;
                    }
                    let mut is_cover_meta = false;
                    let mut is_title_type_meta = false;
                    let mut refines_val = String::new();
                    let mut content_val = String::new();

                    for attr in e.attributes().flatten() {
                        let local_key = attr_local_key(attr.key.as_ref());
                        let val = crate::epub::unescape_attr_value(&attr);
                        if local_key == "name" && val == "cover" {
                            is_cover_meta = true;
                        } else if local_key == "content" {
                            content_val = val;
                        } else if local_key == "property" && val == "title-type" {
                            is_title_type_meta = true;
                        } else if local_key == "refines" {
                            refines_val = val.trim_start_matches('#').to_string();
                        }
                    }

                    if is_cover_meta && !content_val.is_empty() {
                        metadata.cover_image_id = Some(content_val);
                    } else if is_title_type_meta && !refines_val.is_empty() {
                        if !content_val.is_empty() {
                            title_types.insert(refines_val, content_val);
                        } else if is_start {
                            current_meta_refines = Some(refines_val);
                        }
                    }
                }
                t if is_start && is_metadata_value_tag(t) => {
                    current_text.clear();
                }
                _ => {}
            }
            buf.clear();
            continue;
        }

        match ev {
            Ok(Event::Text(ref e)) => {
                if tag_stack.iter().any(|t| is_metadata_value_tag(t)) {
                    let decoded = crate::epub::unescape_xml_or_html(e);
                    current_text.push_str(&decoded);
                }
            }
            Ok(Event::GeneralRef(ref e)) => {
                if tag_stack.iter().any(|t| is_metadata_value_tag(t)) {
                    let decoded = crate::epub::decode_general_ref(e);
                    current_text.push_str(&decoded);
                }
            }
            Ok(Event::CData(ref e)) => {
                if tag_stack.iter().any(|t| is_metadata_value_tag(t)) {
                    current_text.push_str(&e.html_content());
                }
            }
            Ok(Event::End(ref e)) => {
                let end_local = e.local_name().as_ref().to_string();
                let popped = if let Some(pos) = tag_stack.iter().rposition(|t| t == &end_local) {
                    let popped = tag_stack[pos].clone();
                    tag_stack.truncate(pos);
                    Some(popped)
                } else {
                    None
                };

                if let Some(tag) = popped
                    && is_metadata_value_tag(&tag)
                {
                    let text = current_text.trim().to_string();
                    if !text.is_empty() {
                        match tag.as_str() {
                            "title" => {
                                titles.push((
                                    current_title_id.take(),
                                    current_title_type.take(),
                                    text,
                                ));
                            }
                            "identifier" => {
                                identifiers.push((current_identifier_id.take(), text));
                            }
                            "meta" => {
                                if let Some(target) = current_meta_refines.take() {
                                    title_types.insert(target, text);
                                }
                            }
                            "creator" => metadata.creators.push(text),
                            "language" => {
                                if metadata.language.is_empty() {
                                    metadata.language = text;
                                }
                            }
                            "publisher" => {
                                if metadata.publisher.is_none() {
                                    metadata.publisher = Some(text);
                                }
                            }
                            "description" => {
                                if metadata.description.is_none() {
                                    metadata.description = Some(text);
                                }
                            }
                            "date" if metadata.date.is_none() => {
                                metadata.date = Some(text);
                            }
                            _ => {}
                        }
                    }
                    current_text.clear();
                    if tag == "title" {
                        current_title_id = None;
                        current_title_type = None;
                    } else if tag == "identifier" {
                        current_identifier_id = None;
                    } else if tag == "meta" {
                        current_meta_refines = None;
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(EpubError::Xml(e.to_string())),
            _ => {}
        }
        buf.clear();
    }

    // Title selection hierarchy:
    // 1. Refined title-type="main" or direct title-type="main"
    // 2. id explicitly containing "main" and not "sub"
    // 3. First title in document order not marked as "subtitle"
    // 4. Fallback to first title found
    let mut selected_title = None;
    for (id, direct_type, text) in &titles {
        let is_main = direct_type
            .as_deref()
            .map(|t| t.eq_ignore_ascii_case("main"))
            .unwrap_or(false)
            || id
                .as_ref()
                .and_then(|i| title_types.get(i))
                .map(|t| t.eq_ignore_ascii_case("main"))
                .unwrap_or(false);
        if is_main {
            selected_title = Some(text.clone());
            break;
        }
    }

    if selected_title.is_none() {
        for (id, _, text) in &titles {
            if let Some(i) = id {
                let lower = i.to_ascii_lowercase();
                if lower.contains("main") && !lower.contains("sub") {
                    selected_title = Some(text.clone());
                    break;
                }
            }
        }
    }

    if selected_title.is_none() {
        for (id, direct_type, text) in &titles {
            let is_sub = direct_type
                .as_deref()
                .map(|t| t.eq_ignore_ascii_case("subtitle"))
                .unwrap_or(false)
                || id
                    .as_ref()
                    .and_then(|i| title_types.get(i))
                    .map(|t| t.eq_ignore_ascii_case("subtitle"))
                    .unwrap_or(false)
                || id
                    .as_ref()
                    .map(|i| i.to_ascii_lowercase().contains("sub"))
                    .unwrap_or(false);
            if !is_sub {
                selected_title = Some(text.clone());
                break;
            }
        }
    }

    if selected_title.is_none() && !titles.is_empty() {
        selected_title = Some(titles[0].2.clone());
    }

    if let Some(t) = selected_title {
        metadata.title = t;
    }

    // Identifier selection
    if let Some(uid_name) = unique_identifier_id
        && let Some((_, text)) = identifiers
            .iter()
            .find(|(id, _)| id.as_deref() == Some(&uid_name))
    {
        metadata.identifier = text.clone();
    }
    if metadata.identifier.is_empty() && !identifiers.is_empty() {
        metadata.identifier = identifiers[0].1.clone();
    }

    if metadata.title.is_empty() {
        metadata.title = "Untitled Book".to_string();
    }
    if metadata.language.is_empty() {
        metadata.language = "en".to_string();
    }

    // Validate and resolve cover_image_id to an actual image asset
    let cover_needs_resolution = match metadata.cover_image_id.as_ref() {
        Some(cid) => manifest
            .get(cid)
            .map(|item| !item.media_type.starts_with("image/"))
            .unwrap_or(true),
        None => true,
    };

    if cover_needs_resolution {
        let mut manifest_items: Vec<&ManifestItem> = manifest.values().collect();
        manifest_items.sort_by(|a, b| a.id.cmp(&b.id).then_with(|| a.href.cmp(&b.href)));

        let cover_prop_item = manifest_items.iter().copied().find(|item| {
            item.media_type.starts_with("image/")
                && item
                    .properties
                    .as_deref()
                    .map(|p| p.contains("cover-image"))
                    .unwrap_or(false)
        });
        if let Some(item) = cover_prop_item {
            metadata.cover_image_id = Some(item.id.clone());
        } else {
            let cover_name_item = manifest_items.iter().copied().find(|item| {
                item.media_type.starts_with("image/")
                    && (item.id.to_lowercase().contains("cover")
                        || item.href.to_lowercase().contains("cover"))
            });
            if let Some(item) = cover_name_item {
                metadata.cover_image_id = Some(item.id.clone());
            }
        }
    }

    Ok((metadata, manifest, spine))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_opf_empty_meta_cdata_and_attr_unescape() {
        let opf = r##"<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" unique-identifier="uid&amp;1" version="3.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title id="t1">Part 1 <meta name="empty-marker" content="ignored"/>&amp; Part 2</dc:title>
    <dc:title id="t2"><![CDATA[CDATA Title & More]]></dc:title>
    <meta property="title-type" refines="#t2">main</meta>
    <dc:description><![CDATA[Summary <with> raw & symbols]]></dc:description>
    <dc:creator>Author &mdash; One</dc:creator>
    <dc:language>en</dc:language>
    <dc:identifier id="uid&amp;1">urn:uuid:1234</dc:identifier>
    <meta name="cover" content="cov&amp;img"/>
  </metadata>
  <manifest>
    <item id="cov&amp;img" href="images/cover&amp;1.jpg" media-type="image/jpeg"/>
    <item id="c1" href="text/ch1.xhtml?x=1&amp;y=2" media-type="application/xhtml+xml"/>
  </manifest>
  <spine>
    <itemref idref="c1"/>
  </spine>
</package>"##;

        let (meta, manifest, spine) = parse_opf(opf).expect("parse_opf should succeed");
        assert_eq!(meta.title, "CDATA Title & More");
        assert_eq!(
            meta.description.as_deref(),
            Some("Summary <with> raw & symbols")
        );
        assert_eq!(meta.creators, vec!["Author — One"]);
        assert_eq!(meta.identifier, "urn:uuid:1234");
        assert_eq!(meta.cover_image_id.as_deref(), Some("cov&img"));
        assert_eq!(
            manifest.get("cov&img").map(|i| i.href.as_str()),
            Some("images/cover&1.jpg")
        );
        assert_eq!(
            manifest.get("c1").map(|i| i.href.as_str()),
            Some("text/ch1.xhtml?x=1&y=2")
        );
        assert_eq!(spine, vec!["c1"]);
    }

    #[test]
    fn test_parse_opf_general_ref_entities_in_title_creator_and_description() {
        let opf = r##"<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" unique-identifier="uid" version="3.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Book &mdash; Vol. 1 &amp; 2&#x2019;s Edition&#160;!</dc:title>
    <dc:creator>Jane &amp; John&#x2014;Doe</dc:creator>
    <dc:description>&ldquo;Quoted&rdquo; &amp; &madeup; &#160;text</dc:description>
    <dc:language>en</dc:language>
    <dc:identifier id="uid">urn:uuid:5678</dc:identifier>
  </metadata>
  <manifest>
    <item id="c1" href="text/ch1.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine>
    <itemref idref="c1"/>
  </spine>
</package>"##;

        let (meta, _, _) = parse_opf(opf).expect("parse_opf should succeed");
        assert_eq!(meta.title, "Book — Vol. 1 & 2’s Edition\u{00A0}!");
        assert_eq!(meta.creators, vec!["Jane & John—Doe"]);
        assert_eq!(
            meta.description.as_deref(),
            Some("“Quoted” & &madeup; \u{00A0}text")
        );
    }
}
