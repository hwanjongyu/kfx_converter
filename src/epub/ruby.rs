//! East Asian Ruby / Furigana parsing and text span layout.

use crate::ir::TextSpan;

/// State machine for processing HTML `<ruby>`, `<rb>`, `<rt>`, `<rtc>`, and `<rp>` elements.
#[derive(Debug, Default)]
pub struct RubyState {
    pub in_ruby: bool,
    pub in_rt: bool,
    pub in_rp: bool,
    pub ruby_bases: Vec<String>,
    pub ruby_rts: Vec<String>,
    pub current_base: String,
    pub current_rt: String,
}

impl RubyState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Resets state at the start of a `<ruby>` element.
    pub fn start_ruby(&mut self) {
        self.in_ruby = true;
        self.in_rt = false;
        self.in_rp = false;
        self.ruby_bases.clear();
        self.ruby_rts.clear();
        self.current_base.clear();
        self.current_rt.clear();
    }

    /// Handles text content inside `<ruby>` tags.
    pub fn handle_text(&mut self, text: &str) {
        if self.in_rp {
            // Discard fallback parentheses (<rp>(</rp>) so they don't render alongside ruby annotations
        } else if self.in_rt {
            self.current_rt.push_str(text);
        } else {
            self.current_base.push_str(text);
        }
    }

    /// Handles start tag events within `<ruby>` context.
    pub fn on_start_tag(&mut self, tag: &[u8]) {
        match tag {
            b"rb" => {
                if !self.current_base.is_empty() {
                    self.ruby_bases.push(std::mem::take(&mut self.current_base));
                }
            }
            b"rt" | b"rtc" => {
                self.in_rt = true;
                if !self.current_base.is_empty() {
                    self.ruby_bases.push(std::mem::take(&mut self.current_base));
                }
                self.current_rt.clear();
            }
            b"rp" => {
                self.in_rp = true;
            }
            _ => {}
        }
    }

    /// Handles end tag events for ruby sub-elements.
    /// Returns `true` if `</ruby>` was reached and spans should be emitted.
    pub fn on_end_tag(&mut self, tag: &[u8]) -> bool {
        match tag {
            b"rt" | b"rtc" => {
                self.in_rt = false;
                if !self.current_rt.is_empty() {
                    self.ruby_rts.push(std::mem::take(&mut self.current_rt));
                }
                false
            }
            b"rp" => {
                self.in_rp = false;
                false
            }
            b"rb" => {
                if !self.current_base.is_empty() {
                    self.ruby_bases.push(std::mem::take(&mut self.current_base));
                }
                false
            }
            b"ruby" => {
                self.in_ruby = false;
                if !self.current_base.is_empty() {
                    self.ruby_bases.push(std::mem::take(&mut self.current_base));
                }
                if !self.current_rt.is_empty() {
                    self.ruby_rts.push(std::mem::take(&mut self.current_rt));
                }
                true
            }
            _ => false,
        }
    }

    /// Emits structured `TextSpan`s with `ruby` annotations attached to base characters.
    pub fn emit_spans(
        &mut self,
        base_style: Option<String>,
        anchor: &Option<String>,
        href: &Option<String>,
        spans: &mut Vec<TextSpan>,
    ) {
        if self.ruby_bases.len() == self.ruby_rts.len() {
            for (b, r) in self.ruby_bases.drain(..).zip(self.ruby_rts.drain(..)) {
                if !b.is_empty() {
                    spans.push(TextSpan {
                        text: b,
                        style: base_style.clone(),
                        anchor_id: anchor.clone(),
                        href: href.clone(),
                        ruby: if r.is_empty() { None } else { Some(r) },
                    });
                }
            }
        } else if self.ruby_rts.len() == 1 {
            let b = self.ruby_bases.join("");
            let r = self.ruby_rts.pop().unwrap_or_default();
            if !b.is_empty() {
                spans.push(TextSpan {
                    text: b,
                    style: base_style.clone(),
                    anchor_id: anchor.clone(),
                    href: href.clone(),
                    ruby: if r.is_empty() { None } else { Some(r) },
                });
            }
        } else if self.ruby_rts.is_empty() {
            for b in self.ruby_bases.drain(..) {
                if !b.is_empty() {
                    spans.push(TextSpan {
                        text: b,
                        style: base_style.clone(),
                        anchor_id: anchor.clone(),
                        href: href.clone(),
                        ruby: None,
                    });
                }
            }
        } else {
            let b = self.ruby_bases.join("");
            let r = self.ruby_rts.join("");
            if !b.is_empty() {
                spans.push(TextSpan {
                    text: b,
                    style: base_style.clone(),
                    anchor_id: anchor.clone(),
                    href: href.clone(),
                    ruby: if r.is_empty() { None } else { Some(r) },
                });
            }
        }
        self.ruby_bases.clear();
        self.ruby_rts.clear();
        self.current_base.clear();
        self.current_rt.clear();
    }
}
