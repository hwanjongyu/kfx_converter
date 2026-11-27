//! External resource and raw media entity generation for KFX packaging.

use super::{KfxPackager, symbols::LocalSymbols, types::AllocatedResourceSym};
use crate::container::KfxEntity;
use crate::ion::{IonValue, IonWriter};
use crate::symbols::KfxSymbol;

/// Returns `true` iff `id` collides with a reserved internal packager symbol namespace.
pub(crate) fn is_reserved_packager_symbol(id: &str) -> bool {
    id.is_empty()
        || matches!(id, "toc_nav" | "landmarks_nav" | "page_list_nav")
        || id.starts_with("style_")
        || id.starts_with("c_")
        || id.starts_with("story_")
        || id.starts_with("resource/")
        || id
            .strip_prefix("content_")
            .is_some_and(|r| !r.is_empty() && r.bytes().all(|b| b.is_ascii_digit()))
        || id
            .strip_prefix("a_")
            .is_some_and(|r| !r.is_empty() && r.bytes().all(|b| b.is_ascii_digit()))
        || id
            .strip_prefix("note_")
            .is_some_and(|r| !r.is_empty() && r.bytes().all(|b| b.is_ascii_digit()))
}

/// Returns valid `(width, height)` within safe limits for GIF payloads, since the `image`
/// crate is compiled without the `gif` feature and cannot decode GIF pixels directly.
fn is_valid_safe_gif(data: &[u8]) -> Option<(u32, u32)> {
    if !(data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a")) {
        return None;
    }
    let (w, h) = crate::enhancements::CoverOptimizer::get_image_dimensions(data)?;
    if w <= crate::enhancements::cover::MAX_SAFE_IMAGE_DIMENSION
        && h <= crate::enhancements::cover::MAX_SAFE_IMAGE_DIMENSION
        && (w as u64) * (h as u64) <= crate::enhancements::cover::MAX_SAFE_IMAGE_PIXELS
    {
        Some((w, h))
    } else {
        None
    }
}

fn placeholder_payload() -> (Vec<u8>, u32, String, Option<(u32, u32)>) {
    (
        crate::enhancements::FALLBACK_PLACEHOLDER_JPEG.to_vec(),
        KfxSymbol::Jpg as u32,
        "image/jpeg".to_string(),
        Some((1, 1)),
    )
}

/// Optimizes cover/content raster image payloads and enforces well-formed fallback semantics.
/// Valid safe GIFs are preserved verbatim with header dimensions; corrupt or unsafe non-GIF
/// rasters fall back to the 1x1 baseline JPEG placeholder.
fn resolve_raster_payload(
    res_id: &str,
    data: &[u8],
    is_cover_res: bool,
    ext_hint: &str,
) -> (Vec<u8>, u32, String, Option<(u32, u32)>) {
    let (result, success_hint, label) = if is_cover_res {
        (
            crate::enhancements::CoverOptimizer::optimize_cover_image(
                data,
                crate::enhancements::KINDLE_MAX_CONTENT_IMAGE_DIM,
            ),
            "jpg",
            "Cover image resource",
        )
    } else {
        (
            crate::enhancements::CoverOptimizer::optimize_content_image(
                data,
                crate::enhancements::KINDLE_MAX_CONTENT_IMAGE_DIM,
            ),
            ext_hint,
            "Image resource",
        )
    };

    match result {
        Ok(opt_data) => {
            let (_, f_sym, m_type) =
                crate::enhancements::detect_image_format(&opt_data, success_hint);
            (opt_data, f_sym, m_type.to_string(), None)
        }
        Err(crate::enhancements::CoverError::EmptyData) => {
            eprintln!(
                "Warning: {} '{}' is empty; substituting 1x1 placeholder JPEG.",
                label, res_id
            );
            placeholder_payload()
        }
        Err(crate::enhancements::CoverError::ImageTooLarge(w, h)) => {
            eprintln!(
                "Warning: {} '{}' dimensions ({}x{}) exceed safe limits; substituting 1x1 placeholder JPEG.",
                label, res_id, w, h
            );
            placeholder_payload()
        }
        Err(crate::enhancements::CoverError::LimitExceeded(reason)) => {
            eprintln!(
                "Warning: {} '{}' limit exceeded ({}); substituting 1x1 placeholder JPEG.",
                label, res_id, reason
            );
            placeholder_payload()
        }
        Err(
            crate::enhancements::CoverError::Decode(_) | crate::enhancements::CoverError::Io(_),
        ) => {
            if let Some((w, h)) = is_valid_safe_gif(data) {
                (
                    data.to_vec(),
                    KfxSymbol::Gif as u32,
                    "image/gif".to_string(),
                    Some((w, h)),
                )
            } else {
                eprintln!(
                    "Warning: {} '{}' has corrupt or unrecognized bytes; substituting 1x1 placeholder JPEG.",
                    label, res_id
                );
                placeholder_payload()
            }
        }
    }
}

impl<'a> KfxPackager<'a> {
    /// Pre-allocates collision-free local symbols for all supported raster image resources
    /// (`$164 external_resource` and `$417 bcRawMedia`), running immediately after `build_styles`
    /// so style SIDs `852..=890` remain frozen.
    ///
    /// Clean non-colliding OPF resource IDs retain their original symbol names; reserved or
    /// colliding IDs are deterministically remapped to `res_1`, `res_2`, ...
    pub(crate) fn allocate_resource_symbols(&mut self, symbols: &mut LocalSymbols) {
        self.resource_symbols.clear();
        let cover_id = self.resolve_cover_resource_id();
        let mut sorted_keys: Vec<&String> = self
            .book
            .resources
            .iter()
            .filter_map(|(k, v)| v.is_supported_raster_image().then_some(k))
            .collect();
        sorted_keys.sort_by(|a_key, b_key| {
            let a_is_cover = cover_id.as_deref() == Some(a_key.as_str());
            let b_is_cover = cover_id.as_deref() == Some(b_key.as_str());
            match (a_is_cover, b_is_cover) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a_key.cmp(b_key),
            }
        });

        let mut counter = 1usize;
        for res_id in sorted_keys {
            let raw_cand = format!("resource/{}", res_id);
            let (res_sym_name, raw_path) = if !is_reserved_packager_symbol(res_id)
                && symbols.get_id(res_id).is_none()
                && symbols.get_id(&raw_cand).is_none()
            {
                (res_id.clone(), raw_cand)
            } else {
                loop {
                    let cand = format!("res_{}", counter);
                    let cand_raw = format!("resource/{}", cand);
                    counter += 1;
                    if symbols.get_id(&cand).is_none() && symbols.get_id(&cand_raw).is_none() {
                        break (cand, cand_raw);
                    }
                }
            };
            let res_sym_id = symbols.intern(&res_sym_name);
            let raw_sym_id = symbols.intern(&raw_path);
            self.resource_symbols.insert(
                res_id.clone(),
                AllocatedResourceSym {
                    res_sym_id,
                    res_sym_name,
                    raw_sym_id,
                    raw_path,
                },
            );
        }
    }

    /// Builds external resource entities ($164) and raw binary payload entities ($417)
    /// for all supported raster images and covers.
    ///
    /// For every supported raster resource in `BookIR::resources`, Yellowjacket emits a dual-entity pair:
    /// 1. $164 (`external_resource`) metadata entity containing MIME type, format, and dimensions
    ///    required by Kindle OS layout engine (KPP / Hermes JS) to compute e-ink pagination
    ///    and bounding boxes. Omission results in height=0px rendering collapse.
    /// 2. $417 (`bcRawMedia`) raw binary image payload entity.
    pub(crate) fn build_resources(&mut self, symbols: &mut LocalSymbols) {
        if self.resource_symbols.is_empty()
            && self
                .book
                .resources
                .values()
                .any(crate::ir::ResourceAsset::is_supported_raster_image)
        {
            self.allocate_resource_symbols(symbols);
        }

        let sym_ext_resource = KfxSymbol::ExternalResource as u32;
        let sym_raw_media = KfxSymbol::Bcrawmedia as u32;
        let sym_resource_name = KfxSymbol::ResourceName as u32;
        let sym_location = KfxSymbol::Location as u32;
        let sym_format = KfxSymbol::Format as u32;
        let sym_mime = KfxSymbol::Mime as u32;
        let sym_resource_width = KfxSymbol::ResourceWidth as u32;
        let sym_resource_height = KfxSymbol::ResourceHeight as u32;

        let cover_id = self.resolve_cover_resource_id();
        let mut sorted_resources: Vec<(String, AllocatedResourceSym)> = self
            .resource_symbols
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        // Priority ordering: cover image entity MUST be emitted first so that Kindle lockscreen
        // and Calibre extractors picking the first bcRawMedia ($417) entity reliably receive the cover.
        sorted_resources.sort_by(|(a_key, _), (b_key, _)| {
            let a_is_cover = cover_id.as_deref() == Some(a_key.as_str());
            let b_is_cover = cover_id.as_deref() == Some(b_key.as_str());
            match (a_is_cover, b_is_cover) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a_key.cmp(b_key),
            }
        });

        for (res_id, alloc) in sorted_resources {
            let Some(asset) = self.book.resources.get(&res_id) else {
                continue;
            };
            if !asset.is_supported_raster_image() {
                continue;
            }

            let res_sym_id = alloc.res_sym_id;
            let raw_sym_id = alloc.raw_sym_id;
            let raw_path = alloc.raw_path;

            let is_cover_res = cover_id.as_deref() == Some(res_id.as_str());
            let ext_hint = std::path::Path::new(&res_id)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or(asset.mime_type.as_str());

            // For the book cover, transcode to a Kindle-compliant baseline RGB JPEG (JFIF).
            // For empty, oversized, limit-exceeding, or corrupt/unrecognized non-GIF payloads,
            // substitute a valid 1x1 baseline placeholder JPEG rather than emitting corrupt $417 chunks.
            let (payload_data, format_sym, mime_type, forced_dims) =
                resolve_raster_payload(&res_id, &asset.data, is_cover_res, ext_hint);

            let dimensions = forced_dims.or_else(|| {
                crate::enhancements::CoverOptimizer::get_image_dimensions(&payload_data)
            });

            // 1. External Resource metadata entity ($164)
            let mut ext_field_vec = vec![
                (sym_resource_name, IonValue::Symbol(res_sym_id)),
                (sym_location, IonValue::String(raw_path)),
                (sym_format, IonValue::Symbol(format_sym)),
                (sym_mime, IonValue::String(mime_type)),
            ];
            if let Some((w, h)) = dimensions {
                ext_field_vec.push((sym_resource_width, IonValue::Int(w as i64)));
                ext_field_vec.push((sym_resource_height, IonValue::Int(h as i64)));
            }
            let ext_fields = IonValue::Struct(ext_field_vec);

            let mut ext_writer = IonWriter::new();
            ext_writer.write_bvm();
            ext_writer.write_value(&ext_fields);

            self.entities.push(KfxEntity {
                entity_id: res_sym_id,
                chunk_type: sym_ext_resource,
                data: ext_writer.into_bytes(),
            });

            // 2. Raw Media payload entity ($417)
            self.entities.push(KfxEntity {
                entity_id: raw_sym_id,
                chunk_type: sym_raw_media,
                data: payload_data,
            });
        }
    }
}
