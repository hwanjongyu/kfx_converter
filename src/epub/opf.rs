//! OPF package document parsing, metadata extraction, manifest items, and spine reading order.

use std::collections::HashMap;
use quick_xml::events::Event;
use quick_xml::Reader as XmlReader;

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
    let mut current_tag = String::new();
    let mut current_text = String::new();
    let mut current_title_id: Option<String> = None;
    let mut current_title_type: Option<String> = None;
    let mut current_identifier_id: Option<String> = None;
    let mut current_meta_refines: Option<String> = None;

    loop {
        match xml_reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                current_text.clear();
                let local = String::from_utf8_lossy(e.local_name().as_ref()).to_string();

                if local == "package" {
                    for attr in e.attributes().flatten() {
                        let key = attr.key.as_ref();
                        let local_key = if let Some(pos) = key.iter().position(|&b| b == b':') {
                            &key[pos + 1..]
                        } else {
                            key
                        };
                        if local_key == b"unique-identifier" {
                            unique_identifier_id =
                                Some(String::from_utf8_lossy(&attr.value).to_string());
                        }
                    }
                } else if local == "item" {
                    let mut id = String::new();
                    let mut href = String::new();
                    let mut media_type = String::new();
                    let mut properties = None;

                    for attr in e.attributes().flatten() {
                        match attr.key.as_ref() {
                            b"id" => id = String::from_utf8_lossy(&attr.value).to_string(),
                            b"href" => href = String::from_utf8_lossy(&attr.value).to_string(),
                            b"media-type" => {
                                media_type = String::from_utf8_lossy(&attr.value).to_string()
                            }
                            b"properties" => {
                                properties = Some(String::from_utf8_lossy(&attr.value).to_string())
                            }
                            _ => {}
                        }
                    }

                    if !id.is_empty() && !href.is_empty() {
                        if let Some(ref props) = properties
                            && props.contains("cover-image") && metadata.cover_image_id.is_none() {
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
                } else if local == "itemref" {
                    for attr in e.attributes().flatten() {
                        if attr.key.as_ref() == b"idref" {
                            spine.push(String::from_utf8_lossy(&attr.value).to_string());
                        }
                    }
                } else if local == "title" {
                    current_tag = "title".to_string();
                    current_title_id = None;
                    current_title_type = None;
                    for attr in e.attributes().flatten() {
                        let key = attr.key.as_ref();
                        let local_key = if let Some(pos) = key.iter().position(|&b| b == b':') {
                            &key[pos + 1..]
                        } else {
                            key
                        };
                        if local_key == b"id" {
                            current_title_id =
                                Some(String::from_utf8_lossy(&attr.value).to_string());
                        } else if local_key == b"title-type" {
                            current_title_type =
                                Some(String::from_utf8_lossy(&attr.value).to_string());
                        }
                    }
                } else if local == "identifier" {
                    current_tag = "identifier".to_string();
                    current_identifier_id = None;
                    for attr in e.attributes().flatten() {
                        let key = attr.key.as_ref();
                        let local_key = if let Some(pos) = key.iter().position(|&b| b == b':') {
                            &key[pos + 1..]
                        } else {
                            key
                        };
                        if local_key == b"id" {
                            current_identifier_id =
                                Some(String::from_utf8_lossy(&attr.value).to_string());
                        }
                    }
                } else if local == "meta" {
                    let mut is_cover_meta = false;
                    let mut is_title_type_meta = false;
                    let mut refines_val = String::new();
                    let mut content_val = String::new();

                    for attr in e.attributes().flatten() {
                        let key = attr.key.as_ref();
                        let local_key = if let Some(pos) = key.iter().position(|&b| b == b':') {
                            &key[pos + 1..]
                        } else {
                            key
                        };
                        if local_key == b"name" && attr.value.as_ref() == b"cover" {
                            is_cover_meta = true;
                        } else if local_key == b"content" {
                            content_val = String::from_utf8_lossy(&attr.value).to_string();
                        } else if local_key == b"property" && attr.value.as_ref() == b"title-type" {
                            is_title_type_meta = true;
                        } else if local_key == b"refines" {
                            let r = String::from_utf8_lossy(&attr.value).to_string();
                            refines_val = r.trim_start_matches('#').to_string();
                        }
                    }

                    if is_cover_meta && !content_val.is_empty() {
                        metadata.cover_image_id = Some(content_val);
                    } else if is_title_type_meta && !refines_val.is_empty() {
                        if !content_val.is_empty() {
                            title_types.insert(refines_val, content_val);
                        } else {
                            current_tag = "meta_title_type".to_string();
                            current_meta_refines = Some(refines_val);
                        }
                    }
                } else {
                    current_tag = local;
                }
            }
            Ok(Event::Text(ref e)) => {
                let decoded = match e.unescape() {
                    Ok(s) => s.to_string(),
                    Err(_) => {
                        let raw = String::from_utf8_lossy(e.as_ref()).to_string();
                        raw.replace("&amp;", "&")
                            .replace("&lt;", "<")
                            .replace("&gt;", ">")
                            .replace("&quot;", "\"")
                            .replace("&apos;", "'")
                            .replace("&mdash;", "—")
                            .replace("&ndash;", "–")
                            .replace("&hellip;", "…")
                            .replace("&lsquo;", "‘")
                            .replace("&rsquo;", "’")
                            .replace("&ldquo;", "“")
                            .replace("&rdquo;", "”")
                            .replace("&nbsp;", " ")
                    }
                };
                current_text.push_str(&decoded);
            }
            Ok(Event::End(_)) => {
                let text = current_text.trim().to_string();
                if !text.is_empty() {
                    match current_tag.as_str() {
                        "title" => {
                            titles.push((current_title_id.take(), current_title_type.take(), text));
                        }
                        "identifier" => {
                            identifiers.push((current_identifier_id.take(), text));
                        }
                        "meta_title_type" => {
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
                current_tag.clear();
                current_text.clear();
                current_title_id = None;
                current_title_type = None;
                current_identifier_id = None;
                current_meta_refines = None;
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
        let cover_prop_item = manifest.values().find(|item| {
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
            let cover_name_item = manifest.values().find(|item| {
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
