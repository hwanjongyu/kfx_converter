//! External resource and raw media entity generation for KFX packaging.

use crate::container::KfxEntity;
use crate::ion::{IonValue, IonWriter};
use crate::symbols::symbol_id;
use super::{symbols::LocalSymbols, KfxPackager};

impl<'a> KfxPackager<'a> {
    /// Builds external resource entities ($164) and raw binary payload entities ($417)
    /// for all images, covers, and embedded assets.
    ///
    /// For every resource in `BookIR::resources`, Yellowjacket emits a dual-entity pair:
    /// 1. $164 (`external_resource`) metadata entity containing MIME type, format, and dimensions
    ///    required by Kindle OS layout engine (KPP / Hermes JS) to compute e-ink pagination
    ///    and bounding boxes. Omission results in height=0px rendering collapse.
    /// 2. $417 (`bcRawMedia`) raw binary image payload entity.
    pub(crate) fn build_resources(&mut self, symbols: &mut LocalSymbols) {
        let sym_ext_resource = symbol_id("external_resource").unwrap_or(164);
        let sym_raw_media = symbol_id("bcRawMedia").unwrap_or(417);
        let sym_resource_name = symbol_id("resource_name").unwrap_or(175);
        let sym_location = symbol_id("location").unwrap_or(165);
        let sym_format = symbol_id("format").unwrap_or(161);
        let sym_mime = symbol_id("mime").unwrap_or(162);
        let sym_resource_width = symbol_id("resource_width").unwrap_or(422);
        let sym_resource_height = symbol_id("resource_height").unwrap_or(423);
        let sym_jpg = symbol_id("jpg").unwrap_or(285);
        let sym_png = symbol_id("png").unwrap_or(284);

        let cover_id = self.resolve_cover_resource_id();
        let mut sorted_resources: Vec<_> = self.book.resources.iter().collect();
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

        for (res_id, asset) in sorted_resources {
            let res_sym_id = symbols.intern(res_id);
            let raw_path = format!("resource/{}", res_id);
            let raw_sym_id = symbols.intern(&raw_path);

            let is_cover_res = cover_id.as_deref() == Some(res_id.as_str());

            // For the book cover, always transcode to a Kindle-compliant baseline RGB JPEG (JFIF).
            // Kindle OS lockscreen ("Display Cover") and extractors require standard baseline JPEG.
            // For general content images, downscale oversized images (> 2560x2560) to protect 512MB Kindle OS.
            let (payload_data, format_sym, mime_type) = if is_cover_res {
                let opt_cover = crate::enhancements::CoverOptimizer::optimize_cover_image(
                    &asset.data,
                    crate::enhancements::KINDLE_MAX_CONTENT_IMAGE_DIM,
                )
                .unwrap_or_else(|_| asset.data.clone());
                (opt_cover, sym_jpg, "image/jpeg".to_string())
            } else if asset.mime_type.starts_with("image/") {
                let opt_content = crate::enhancements::CoverOptimizer::optimize_content_image(
                    &asset.data,
                    crate::enhancements::KINDLE_MAX_CONTENT_IMAGE_DIM,
                )
                .unwrap_or_else(|_| asset.data.clone());
                let is_png = asset.mime_type.contains("png") && opt_content.starts_with(b"\x89PNG");
                let f_sym = if is_png { sym_png } else { sym_jpg };
                let m_type = if is_png { "image/png".to_string() } else { "image/jpeg".to_string() };
                (opt_content, f_sym, m_type)
            } else {
                (asset.data.clone(), sym_jpg, asset.mime_type.clone())
            };

            let dimensions = if mime_type.starts_with("image/") || asset.mime_type.starts_with("image/") {
                crate::enhancements::CoverOptimizer::get_image_dimensions(&payload_data)
            } else {
                None
            };

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
