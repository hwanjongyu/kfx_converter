#![allow(dead_code)]
//! Theme & Color Tokens for Kindle Scribe Studio GPUI

use gpui::*;

pub struct Theme;

impl Theme {
    #[inline]
    pub fn bg_root() -> Hsla {
        hsla(222.0 / 360.0, 0.47, 0.08, 1.0)
    }

    #[inline]
    pub fn bg_card() -> Hsla {
        hsla(222.0 / 360.0, 0.40, 0.12, 1.0)
    }

    #[inline]
    pub fn bg_card_hover() -> Hsla {
        hsla(222.0 / 360.0, 0.35, 0.16, 1.0)
    }

    #[inline]
    pub fn border() -> Hsla {
        hsla(222.0 / 360.0, 0.30, 0.20, 1.0)
    }

    // Accent colors
    #[inline]
    pub fn accent_amber() -> Hsla {
        hsla(38.0 / 360.0, 0.92, 0.50, 1.0) // KFX Gold
    }

    #[inline]
    pub fn accent_green() -> Hsla {
        hsla(160.0 / 360.0, 0.84, 0.39, 1.0) // Connected / Success
    }

    #[inline]
    pub fn accent_blue() -> Hsla {
        hsla(217.0 / 360.0, 0.91, 0.60, 1.0) // AZW3
    }

    #[inline]
    pub fn accent_purple() -> Hsla {
        hsla(258.0 / 360.0, 0.90, 0.66, 1.0) // MOBI / Word Wise
    }

    #[inline]
    pub fn accent_red() -> Hsla {
        hsla(0.0, 0.84, 0.60, 1.0) // Error / PDF
    }

    // Typography
    #[inline]
    pub fn text_primary() -> Hsla {
        hsla(210.0 / 360.0, 0.40, 0.98, 1.0)
    }

    #[inline]
    pub fn text_muted() -> Hsla {
        hsla(215.0 / 360.0, 0.20, 0.65, 1.0)
    }
}
