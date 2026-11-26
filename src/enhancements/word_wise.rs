//! Kindle Word Wise Database Builder
//!
//! Generates binary-compliant SQLite files (`LanguageLayer.<lang>.<asin>.kll`) for Amazon Kindle e-readers.

use rusqlite::{Connection, Result as SqlResult, params};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WordWiseGloss {
    pub start: usize,
    pub end: Option<usize>,
    pub difficulty: u8,
    pub sense_id: u32,
    pub low_confidence: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WordWisePacingConfig {
    /// Maximum vocabulary difficulty level to retain: `1` (hardest/fewest hints) to `5` (easiest/most hints).
    pub max_difficulty_level: u8,
    /// Minimum physical character distance between consecutive gloss annotations.
    pub min_distance: usize,
    /// Maximum allowed gloss annotations within a single reading window.
    pub max_glosses_per_window: usize,
    /// Size of the sliding reading window in characters (typically 1 Kindle page ~ 800 chars).
    pub window_size: usize,
    /// Number of initial characters to skip (covers, copyright, TOC frontmatter).
    pub frontmatter_skip_chars: usize,
    /// Maximum total annotations allowed per book.
    pub max_total_glosses: usize,
}

impl Default for WordWisePacingConfig {
    fn default() -> Self {
        Self {
            max_difficulty_level: 3,
            min_distance: 20,
            max_glosses_per_window: 4,
            window_size: 800,
            frontmatter_skip_chars: 0,
            max_total_glosses: 10_000,
        }
    }
}

pub struct WordWiseBuilder {
    pub asin: String,
    pub acr: String,
    pub target_language: String,
    pub source_language: String,
    pub book_revision: String,
    pub pacing: WordWisePacingConfig,
    glosses: BTreeMap<usize, WordWiseGloss>, // BTreeMap keeps glosses sorted by start position
    last_gloss_pos: Option<usize>,
    window_positions: Vec<usize>,
}

impl WordWiseBuilder {
    fn normalize_lang(lang: &str) -> String {
        let trimmed = lang.trim().to_ascii_lowercase();
        let primary = trimmed.split(['-', '_']).next().unwrap_or("en");
        if primary.is_empty() {
            "en".to_string()
        } else {
            primary.to_string()
        }
    }

    pub fn new(
        asin: &str,
        acr: &str,
        target_language: &str,
        source_language: &str,
        book_revision: &str,
    ) -> Self {
        Self {
            asin: asin.to_string(),
            acr: acr.to_string(),
            target_language: Self::normalize_lang(target_language),
            source_language: Self::normalize_lang(source_language),
            book_revision: book_revision.to_string(),
            pacing: WordWisePacingConfig::default(),
            glosses: BTreeMap::new(),
            last_gloss_pos: None,
            window_positions: Vec::new(),
        }
    }

    /// Sets custom pacing and sliding-window limits for Word Wise generation.
    pub fn with_pacing(mut self, pacing: WordWisePacingConfig) -> Self {
        self.pacing = pacing;
        self
    }

    /// Returns canonical Kindle Word Wise filename convention.
    pub fn get_kll_filename(asin: &str, target_language: &str) -> String {
        format!(
            "LanguageLayer.{}.{}.kll",
            Self::normalize_lang(target_language),
            asin
        )
    }

    /// Adds a gloss hint without pacing constraints (e.g. for testing).
    pub fn add_gloss(
        &mut self,
        start: usize,
        end: Option<usize>,
        difficulty: u8,
        sense_id: u32,
        low_confidence: bool,
    ) {
        let diff = difficulty.clamp(1, 5);
        self.glosses.insert(
            start,
            WordWiseGloss {
                start,
                end,
                difficulty: diff,
                sense_id,
                low_confidence,
            },
        );
    }

    /// Adds a gloss hint applying sliding-window and distance pacing guards.
    /// Returns true if accepted, false if throttled/skipped to protect Kindle OS memory limit.
    pub fn add_gloss_paced(
        &mut self,
        start: usize,
        end: Option<usize>,
        difficulty: u8,
        sense_id: u32,
        low_confidence: bool,
    ) -> bool {
        if difficulty < 1 || difficulty > self.pacing.max_difficulty_level {
            return false;
        }
        if start < self.pacing.frontmatter_skip_chars {
            return false;
        }
        if self.glosses.len() >= self.pacing.max_total_glosses {
            return false;
        }
        if let Some(last) = self.last_gloss_pos
            && start.saturating_sub(last) < self.pacing.min_distance
        {
            return false;
        }
        let cutoff = start.saturating_sub(self.pacing.window_size);
        self.window_positions.retain(|&p| p >= cutoff);
        if self.window_positions.len() >= self.pacing.max_glosses_per_window {
            return false;
        }

        self.add_gloss(start, end, difficulty, sense_id, low_confidence);
        self.last_gloss_pos = Some(start);
        self.window_positions.push(start);
        true
    }

    /// Number of gloss hints.
    pub fn gloss_count(&self) -> usize {
        self.glosses.len()
    }

    /// Applies difficulty-first priority pacing across candidate glosses.
    ///
    /// Ensures:
    /// 1. Higher-difficulty words (difficulty 1 > 2 > 3 > 4 > 5) are prioritized over easier words.
    /// 2. If two words are within `min_distance`, the harder word wins (or earlier word if equal).
    /// 3. In any `window_size` (default 800 chars), at most `max_glosses_per_window` (default 4) words are kept.
    /// 4. Respects `max_difficulty_level`, `frontmatter_skip_chars`, and `max_total_glosses`.
    pub fn filter_candidates_priority_paced(
        candidates: &[WordWiseGloss],
        pacing: &WordWisePacingConfig,
    ) -> Vec<WordWiseGloss> {
        if candidates.is_empty() {
            return Vec::new();
        }

        let mut valid: Vec<&WordWiseGloss> = candidates
            .iter()
            .filter(|c| {
                c.difficulty >= 1
                    && c.difficulty <= pacing.max_difficulty_level
                    && c.start >= pacing.frontmatter_skip_chars
            })
            .collect();

        // Sort by priority:
        // Primary: difficulty ASC (1 is highest priority / hardest, 5 is lowest / easiest)
        // Secondary: start ASC (reading order)
        valid.sort_by(|a, b| {
            a.difficulty
                .cmp(&b.difficulty)
                .then_with(|| a.start.cmp(&b.start))
        });

        let mut selected = BTreeMap::new();

        for candidate in valid {
            if selected.len() >= pacing.max_total_glosses {
                break;
            }

            let start = candidate.start;

            // Check min_distance against closest neighbors in selected
            let prev_close = selected
                .range(..=start)
                .next_back()
                .is_some_and(|(&pos, _)| start.saturating_sub(pos) < pacing.min_distance);

            let next_close = selected
                .range(start..)
                .next()
                .is_some_and(|(&pos, _)| pos.saturating_sub(start) < pacing.min_distance);

            if prev_close || next_close {
                continue;
            }

            // Check sliding window density
            let window_start = start.saturating_sub(pacing.window_size);
            let window_end = start.saturating_add(pacing.window_size);

            let in_vicinity: Vec<usize> = selected
                .range(window_start..=window_end)
                .map(|(&pos, _)| pos)
                .collect();

            let mut all_windows = in_vicinity;
            all_windows.push(start);
            all_windows.sort_unstable();

            let mut window_violated = false;
            for &anchor in &all_windows {
                let count = all_windows
                    .iter()
                    .filter(|&&p| p >= anchor && p.saturating_sub(anchor) < pacing.window_size)
                    .count();
                if count > pacing.max_glosses_per_window {
                    window_violated = true;
                    break;
                }
            }

            if !window_violated {
                selected.insert(start, candidate.clone());
            }
        }

        selected.into_values().collect()
    }

    fn scan_text_for_glosses(
        text: &str,
        pid: &mut usize,
        dict: &super::dictionary::LemmaDictionary,
        raw_candidates: &mut Vec<WordWiseGloss>,
    ) {
        let mut word_char_start = None;
        let mut word_str = String::new();
        let mut char_idx = 0usize;

        for ch in text.chars() {
            let is_word_char = ch.is_alphabetic()
                || ((ch == '\'' || ch == '’' || ch == '-') && word_char_start.is_some());

            if is_word_char {
                if word_char_start.is_none() {
                    word_char_start = Some(char_idx);
                }
                word_str.push(ch);
            } else if let Some(start_c) = word_char_start.take() {
                if let Some((difficulty, sense_id)) = dict.lookup(&word_str, 3) {
                    raw_candidates.push(WordWiseGloss {
                        start: *pid + start_c,
                        end: None,
                        difficulty,
                        sense_id,
                        low_confidence: false,
                    });
                }
                word_str.clear();
            }
            char_idx += ch.len_utf16();
        }

        if let Some(start_c) = word_char_start.take()
            && let Some((difficulty, sense_id)) = dict.lookup(&word_str, 3)
        {
            raw_candidates.push(WordWiseGloss {
                start: *pid + start_c,
                end: None,
                difficulty,
                sense_id,
                low_confidence: false,
            });
        }

        let block_len = char_idx.max(1);
        *pid += block_len;
    }

    /// Extracts tokens and populates Word Wise glosses directly from a BookIR document.
    pub fn generate_from_book(&mut self, book: &crate::ir::BookIR) -> usize {
        let dict = super::dictionary::LemmaDictionary::new();
        let initial_count = self.glosses.len();
        let sections = book.sections_with_cover();

        let mut pid = 0usize;
        let mut raw_candidates = Vec::new();

        for (idx, section) in sections.iter().enumerate() {
            // Section root element occupies 1 PID position in Yellowjacket position_id_map ($265)
            pid += 1;

            let is_cover = section.is_cover_section(idx);

            if section.blocks.is_empty() {
                if !is_cover {
                    pid += 1; // Fallback empty text block placeholder
                }
                continue;
            }

            for block in &section.blocks {
                match block {
                    crate::ir::Block::Image { resource_id, .. } => {
                        let has_res = book.resolve_resource_key(resource_id).is_some();
                        pid += block.yellowjacket_pid_len(has_res);
                    }
                    crate::ir::Block::List { .. } if block.contains_image() => {
                        let mut stack = vec![block];
                        let mut text_buf = String::new();
                        let mut need_sep = false;
                        while let Some(current) = stack.pop() {
                            match current {
                                crate::ir::Block::List { items, .. } => {
                                    for item in items.iter().rev() {
                                        for child in item.iter().rev() {
                                            stack.push(child);
                                        }
                                    }
                                }
                                crate::ir::Block::Image { resource_id, .. } => {
                                    if !text_buf.is_empty() {
                                        Self::scan_text_for_glosses(
                                            &text_buf,
                                            &mut pid,
                                            &dict,
                                            &mut raw_candidates,
                                        );
                                        text_buf.clear();
                                    }
                                    need_sep = false;
                                    let has_res = book.resolve_resource_key(resource_id).is_some();
                                    pid += current.yellowjacket_pid_len(has_res);
                                }
                                other => {
                                    let piece = other.plain_text();
                                    if !piece.is_empty() {
                                        if need_sep {
                                            text_buf.push('\n');
                                        }
                                        text_buf.push_str(&piece);
                                        need_sep = true;
                                    }
                                }
                            }
                        }
                        if !text_buf.is_empty() {
                            Self::scan_text_for_glosses(
                                &text_buf,
                                &mut pid,
                                &dict,
                                &mut raw_candidates,
                            );
                        }
                    }
                    _ => {
                        let text = block.plain_text();
                        Self::scan_text_for_glosses(&text, &mut pid, &dict, &mut raw_candidates);
                    }
                }
            }
        }

        let prioritized = Self::filter_candidates_priority_paced(&raw_candidates, &self.pacing);
        for gloss in prioritized {
            self.add_gloss(
                gloss.start,
                gloss.end,
                gloss.difficulty,
                gloss.sense_id,
                gloss.low_confidence,
            );
        }

        self.glosses.len() - initial_count
    }

    /// Writes the Word Wise `.kll` SQLite database atomically to disk.
    /// Uses a temporary file in the target directory and renames to prevent partial/corrupted
    /// writes that trigger Kindle OS `.bad_file` blacklisting.
    pub fn write_to_file<P: AsRef<Path>>(&self, path: P) -> SqlResult<()> {
        let path = path.as_ref();
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));

        let map_io = |e: std::io::Error| rusqlite::Error::ToSqlConversionFailure(Box::new(e));
        crate::storage::reject_symlink_parent(parent).map_err(map_io)?;
        crate::storage::reject_symlink_path(path).map_err(map_io)?;

        let tmp_file = tempfile::Builder::new()
            .prefix(".kfx_tmp.")
            .suffix(".kll.part")
            .tempfile_in(parent)
            .map_err(map_io)?;
        {
            let mut conn = Connection::open(tmp_file.path())?;
            self.populate_connection(&mut conn)?;
        }

        crate::storage::prepare_writable_non_symlink(path).map_err(map_io)?;
        crate::storage::reject_symlink_path(path).map_err(map_io)?;
        tmp_file.persist(path).map_err(|e| map_io(e.error))?;
        Ok(())
    }

    /// Serializes the Word Wise `.kll` SQLite database directly into an in-memory byte vector.
    pub fn to_bytes(&self) -> SqlResult<Vec<u8>> {
        let mut conn = Connection::open_in_memory()?;
        self.populate_connection(&mut conn)?;
        let data = conn.serialize(rusqlite::MAIN_DB)?;
        let mut bytes = data.to_vec();
        Self::normalize_sqlite_header(&mut bytes)?;
        Ok(bytes)
    }

    /// Stamps missing big-endian SQLite 3 header fields (`change_counter`, `version_valid_for`,
    /// `sqlite_version`) that `conn.serialize()` leaves zeroed in in-memory databases.
    fn normalize_sqlite_header(bytes: &mut [u8]) -> SqlResult<()> {
        const SQLITE_MAGIC: &[u8; 16] = b"SQLite format 3\0";
        if bytes.len() < 100 || &bytes[..16] != SQLITE_MAGIC {
            return Err(rusqlite::Error::ToSqlConversionFailure(
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Serialized SQLite buffer missing valid 100-byte SQLite 3 header",
                )
                .into(),
            ));
        }

        let mut change_counter = u32::from_be_bytes([bytes[24], bytes[25], bytes[26], bytes[27]]);
        if change_counter == 0 {
            change_counter = 1;
            bytes[24..28].copy_from_slice(&change_counter.to_be_bytes());
        }

        let version_valid_for = u32::from_be_bytes([bytes[92], bytes[93], bytes[94], bytes[95]]);
        if version_valid_for == 0 {
            bytes[92..96].copy_from_slice(&change_counter.max(1).to_be_bytes());
        }

        let sqlite_version = u32::from_be_bytes([bytes[96], bytes[97], bytes[98], bytes[99]]);
        if sqlite_version == 0 {
            let ver = u32::try_from(rusqlite::version_number())
                .ok()
                .filter(|&v| v > 0)
                .ok_or_else(|| {
                    rusqlite::Error::ToSqlConversionFailure(
                        std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "Invalid rusqlite::version_number() <= 0",
                        )
                        .into(),
                    )
                })?;
            bytes[96..100].copy_from_slice(&ver.to_be_bytes());
        }

        Ok(())
    }

    /// Populates tables and metadata into a SQLite connection.
    pub fn populate_connection(&self, conn: &mut Connection) -> SqlResult<()> {
        // Optimize PRAGMAs for Kindle OS eMMC flash memory, write amplification mitigation,
        // and crash resilience against corrupt .bad_file generation.
        conn.execute_batch(
            "PRAGMA page_size = 4096;
             PRAGMA journal_mode = MEMORY;
             PRAGMA synchronous = NORMAL;
             PRAGMA temp_store = MEMORY;

             CREATE TABLE IF NOT EXISTS metadata (
                 key TEXT,
                 value TEXT
             );

             CREATE TABLE IF NOT EXISTS glosses (
                 start INTEGER PRIMARY KEY,
                 end INTEGER,
                 difficulty INTEGER,
                 sense_id INTEGER,
                 low_confidence BOOLEAN
             );
             CREATE INDEX IF NOT EXISTS idx_glosses_diff_start ON glosses (difficulty, start);",
        )?;

        let tx = conn.transaction()?;

        // 1. Insert metadata rows
        let metadata_rows = [
            ("acr", self.acr.as_str()),
            ("targetLanguages", self.target_language.as_str()),
            ("sidecarRevision", "9"),
            ("bookRevision", self.book_revision.as_str()),
            ("sourceLanguage", self.source_language.as_str()),
            ("enDictionaryVersion", "2016-09-14"),
            ("enDictionaryRevision", "57"),
            (
                "enDictionaryId",
                &format!("kll.{}.{}", self.target_language, self.target_language),
            ),
            ("sidecarFormat", "1.0"),
        ];

        {
            let mut stmt = tx.prepare("INSERT INTO metadata (key, value) VALUES (?1, ?2)")?;
            for (k, v) in &metadata_rows {
                stmt.execute(params![k, v])?;
            }
        }

        // 2. Insert sorted glosses
        {
            let mut stmt = tx.prepare(
                "INSERT INTO glosses (start, end, difficulty, sense_id, low_confidence)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )?;
            for g in self.glosses.values() {
                stmt.execute(params![
                    g.start as i64,
                    g.end.map(|e| e as i64),
                    g.difficulty as i32,
                    g.sense_id as i64,
                    if g.low_confidence { 1 } else { 0 },
                ])?;
            }
        }

        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_word_wise_kll_database() {
        let mut builder = WordWiseBuilder::new(
            "B0EXAMPLEASIN",
            "CR!EXAMPLEACR12345678901234",
            "en",
            "en",
            "1",
        );

        assert_eq!(
            WordWiseBuilder::get_kll_filename("B0EXAMPLEASIN", "en"),
            "LanguageLayer.en.B0EXAMPLEASIN.kll"
        );

        builder.add_gloss(100, Some(108), 3, 4012, false);
        builder.add_gloss(250, None, 4, 8045, true);
        assert_eq!(builder.gloss_count(), 2);

        let mut conn = Connection::open_in_memory().unwrap();
        builder.populate_connection(&mut conn).unwrap();

        // Query metadata
        let acr: String = conn
            .query_row("SELECT value FROM metadata WHERE key = 'acr'", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(acr, "CR!EXAMPLEACR12345678901234");

        // Query glosses
        let count: i64 = conn
            .query_row("SELECT count(*) FROM glosses", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 2);

        let diff: i32 = conn
            .query_row(
                "SELECT difficulty FROM glosses WHERE start = 100",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(diff, 3);

        // Verify index created
        let index_exists: bool = conn
            .query_row(
                "SELECT count(*) > 0 FROM sqlite_master WHERE type = 'index' AND name = 'idx_glosses_diff_start'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(index_exists, "idx_glosses_diff_start must exist");

        // Verify to_bytes serialization
        let bytes = builder.to_bytes().expect("to_bytes must succeed");
        assert!(bytes.len() >= 4096, "SQLite database must be at least 4KB");
        assert!(
            bytes.starts_with(b"SQLite format 3\0"),
            "Must be valid SQLite header"
        );
    }

    #[test]
    fn test_word_wise_pacing() {
        let mut builder = WordWiseBuilder::new("TESTASIN", "CR!TESTACR", "en", "en", "1")
            .with_pacing(WordWisePacingConfig {
                max_difficulty_level: 2,
                min_distance: 25,
                max_glosses_per_window: 2,
                window_size: 100,
                frontmatter_skip_chars: 50,
                max_total_glosses: 10,
            });

        // 1. Frontmatter skip: start < 50 should be rejected
        assert!(!builder.add_gloss_paced(20, None, 1, 101, false));

        // 2. Difficulty check: difficulty 3 > max_difficulty_level 2 rejected (too easy)
        assert!(!builder.add_gloss_paced(60, None, 3, 102, false));

        // 3. Valid first gloss: start = 60, diff = 1 (hardest word retained)
        assert!(builder.add_gloss_paced(60, None, 1, 103, false));

        // 4. Distance pacing: start = 70 (dist = 10 < 25) rejected
        assert!(!builder.add_gloss_paced(70, None, 2, 104, false));

        // 5. Valid second gloss: start = 90 (dist = 30 >= 25), diff = 2 <= 2
        assert!(builder.add_gloss_paced(90, None, 2, 105, false));

        // 6. Window cap: start = 110 (window 110-100=10 contains 60 and 90, so already 2) rejected
        assert!(!builder.add_gloss_paced(110, None, 1, 106, false));

        // 7. Outside window: start = 180 (window 180-100=80 only contains 90, so 1 item) accepted
        assert!(builder.add_gloss_paced(180, None, 2, 107, false));

        assert_eq!(builder.gloss_count(), 3);
    }

    #[test]
    fn test_generate_from_book() {
        use crate::ir::{Block, BookIR, BookMetadata, Section, TextSpan};

        let ir = BookIR {
            metadata: BookMetadata::default(),
            sections: vec![Section {
                id: "sec1".to_string(),
                title: "Chapter 1".to_string(),
                blocks: vec![Block::Paragraph {
                    id: "p1".to_string(),
                    spans: vec![TextSpan {
                        text: "The abstruse theory was compelling and difficult to elucidate."
                            .to_string(),
                        style: None,
                        anchor_id: None,
                        href: None,
                        ruby: None,
                    }],
                    style: None,
                }],
            }],
            resources: std::collections::HashMap::new(),
            toc: Vec::new(),
            page_list: Vec::new(),
            footnotes: std::collections::HashMap::new(),
        };

        let mut builder = WordWiseBuilder::new("B0TEST", "CR!TEST", "en", "en", "1").with_pacing(
            WordWisePacingConfig {
                max_difficulty_level: 5,
                min_distance: 10,
                max_glosses_per_window: 5,
                window_size: 500,
                frontmatter_skip_chars: 0,
                max_total_glosses: 100,
            },
        );

        let count = builder.generate_from_book(&ir);
        assert!(
            count >= 2,
            "Expected at least 2 glosses (abstruse, elucidate), got {}",
            count
        );
        assert_eq!(builder.gloss_count(), count);
    }

    #[test]
    fn test_word_wise_apostrophe_and_hyphen_tokenization() {
        use crate::ir::{Block, BookIR, BookMetadata, Section, TextSpan};

        let ir = BookIR {
            metadata: BookMetadata::default(),
            sections: vec![Section {
                id: "sec1".to_string(),
                title: "Chapter 1".to_string(),
                blocks: vec![Block::Paragraph {
                    id: "p1".to_string(),
                    spans: vec![TextSpan {
                        text: "The author's abstruse's explanation did not elucidate the dilemma."
                            .to_string(),
                        style: None,
                        anchor_id: None,
                        href: None,
                        ruby: None,
                    }],
                    style: None,
                }],
            }],
            resources: std::collections::HashMap::new(),
            toc: Vec::new(),
            page_list: Vec::new(),
            footnotes: std::collections::HashMap::new(),
        };

        let mut builder = WordWiseBuilder::new("B0TEST", "CR!TEST", "en", "en", "1").with_pacing(
            WordWisePacingConfig {
                max_difficulty_level: 5,
                min_distance: 1,
                max_glosses_per_window: 10,
                window_size: 500,
                frontmatter_skip_chars: 0,
                max_total_glosses: 100,
            },
        );

        let count = builder.generate_from_book(&ir);
        // "abstruse's" has 's stripped by dictionary and matches abstruse
        assert!(count >= 2, "Expected at least 2 glosses, got {}", count);
    }

    #[test]
    fn test_word_wise_pid_matches_cover_section_offset() {
        use crate::ir::{Block, BookIR, BookMetadata, Section, TextSpan};

        let mut resources = std::collections::HashMap::new();
        resources.insert(
            "cover.jpg".to_string(),
            crate::ir::ResourceAsset {
                id: "cover.jpg".to_string(),
                mime_type: "image/jpeg".to_string(),
                data: vec![0xFF, 0xD8, 0xFF, 0xE0],
            },
        );

        let ir = BookIR {
            metadata: BookMetadata::default(),
            sections: vec![Section {
                id: "sec1".to_string(),
                title: "Chapter 1".to_string(),
                blocks: vec![Block::Paragraph {
                    id: "p1".to_string(),
                    spans: vec![TextSpan {
                        text: "The abstruse theorem.".to_string(),
                        style: None,
                        anchor_id: None,
                        href: None,
                        ruby: None,
                    }],
                    style: None,
                }],
            }],
            resources,
            toc: Vec::new(),
            page_list: Vec::new(),
            footnotes: std::collections::HashMap::new(),
        };

        let mut builder = WordWiseBuilder::new("B0TEST", "CR!TEST", "en", "en", "1").with_pacing(
            WordWisePacingConfig {
                max_difficulty_level: 5,
                min_distance: 1,
                max_glosses_per_window: 10,
                window_size: 500,
                frontmatter_skip_chars: 0,
                max_total_glosses: 100,
            },
        );

        let count = builder.generate_from_book(&ir);
        assert_eq!(count, 2);
        let gloss = builder.glosses.values().next().unwrap();
        // Cover section: 1 (root) + 1 (resolved $271 cover image) = 2 PIDs
        // Chapter 1: 1 (root) = PID 3
        // "The " = 4 chars, so "abstruse" starts at offset 4 within block
        // Expected start: 2 (cover) + 1 (root) + 4 ("The ") = 7
        assert_eq!(
            gloss.start, 7,
            "Expected PID 7 with cover offset, got {}",
            gloss.start
        );
    }

    #[test]
    fn test_word_wise_atomic_durability_and_pragmas() {
        let temp = tempfile::tempdir().unwrap();
        let db_path = temp.path().join("LanguageLayer.en.B0TEST.kll");

        let mut builder = WordWiseBuilder::new("B0TEST", "CR!TEST", "en", "en", "1");
        builder.add_gloss(10, Some(15), 3, 101, false);

        // Verify PRAGMA configuration on connection
        let mut test_conn = Connection::open_in_memory().unwrap();
        builder.populate_connection(&mut test_conn).unwrap();
        let journal_mode: String = test_conn
            .query_row("PRAGMA journal_mode;", [], |r| r.get(0))
            .unwrap();
        assert_eq!(journal_mode.to_lowercase(), "memory");
        let sync_mode: i64 = test_conn
            .query_row("PRAGMA synchronous;", [], |r| r.get(0))
            .unwrap();
        assert_eq!(sync_mode, 1); // 1 = NORMAL

        // Test atomic write_to_file
        builder.write_to_file(&db_path).unwrap();
        assert!(db_path.exists());

        // Verify database contents (scoped so SQLite closes handle before Windows overwrite)
        {
            let conn = Connection::open(&db_path).unwrap();
            let count: i64 = conn
                .query_row("SELECT count(*) FROM glosses;", [], |r| r.get(0))
                .unwrap();
            assert_eq!(count, 1);
        }

        // Test overwrite of an existing read-only file
        let mut perms = std::fs::metadata(&db_path).unwrap().permissions();
        perms.set_readonly(true);
        std::fs::set_permissions(&db_path, perms).unwrap();

        builder.add_gloss(20, Some(25), 4, 102, false);
        builder.write_to_file(&db_path).unwrap();

        {
            let conn2 = Connection::open(&db_path).unwrap();
            let count2: i64 = conn2
                .query_row("SELECT count(*) FROM glosses;", [], |r| r.get(0))
                .unwrap();
            assert_eq!(count2, 2);
        }
    }

    #[test]
    fn test_word_wise_priority_pacing_retains_hard_words() {
        let pacing = WordWisePacingConfig {
            max_difficulty_level: 5,
            min_distance: 20,
            max_glosses_per_window: 4,
            window_size: 800,
            frontmatter_skip_chars: 0,
            max_total_glosses: 100,
        };

        // 4 easy words (Difficulty 5: grief, coffin, hesitate, download) appear earlier in the window,
        // followed by a harder word (Difficulty 4: jaundiced).
        let candidates = vec![
            WordWiseGloss {
                start: 100,
                end: None,
                difficulty: 5,
                sense_id: 41273,
                low_confidence: false,
            }, // grief
            WordWiseGloss {
                start: 200,
                end: None,
                difficulty: 5,
                sense_id: 13689,
                low_confidence: false,
            }, // coffin
            WordWiseGloss {
                start: 300,
                end: None,
                difficulty: 5,
                sense_id: 5308,
                low_confidence: false,
            }, // hesitate
            WordWiseGloss {
                start: 400,
                end: None,
                difficulty: 5,
                sense_id: 28861,
                low_confidence: false,
            }, // download
            WordWiseGloss {
                start: 500,
                end: None,
                difficulty: 4,
                sense_id: 19346,
                low_confidence: false,
            }, // jaundiced
        ];

        let selected = WordWiseBuilder::filter_candidates_priority_paced(&candidates, &pacing);

        // Window quota is 4, so exactly 4 words must be selected.
        assert_eq!(selected.len(), 4);

        // Verify that the harder word `jaundiced` (start 500, diff 4) IS selected!
        assert!(
            selected.iter().any(|g| g.start == 500 && g.difficulty == 4),
            "Expected harder word (diff 4, jaundiced) to be retained over diff 5 words!"
        );

        // Verify that the lowest-priority word `download` (start 400, diff 5) was dropped
        assert!(
            !selected.iter().any(|g| g.start == 400),
            "Expected lowest-priority word (start 400, download) to be dropped when quota exceeded!"
        );
    }

    #[test]
    fn test_word_wise_priority_proximity_conflict() {
        let pacing = WordWisePacingConfig {
            max_difficulty_level: 5,
            min_distance: 20,
            max_glosses_per_window: 4,
            window_size: 800,
            frontmatter_skip_chars: 0,
            max_total_glosses: 100,
        };

        // Two words within 10 characters (less than min_distance = 20):
        // Word 1: easy word (diff 5, start 100)
        // Word 2: hard word (diff 2, start 110)
        let candidates = vec![
            WordWiseGloss {
                start: 100,
                end: None,
                difficulty: 5,
                sense_id: 41273,
                low_confidence: false,
            },
            WordWiseGloss {
                start: 110,
                end: None,
                difficulty: 2,
                sense_id: 1001,
                low_confidence: false,
            },
        ];

        let selected = WordWiseBuilder::filter_candidates_priority_paced(&candidates, &pacing);

        assert_eq!(selected.len(), 1);
        // The harder word (start 110, diff 2) must win over the easier word (start 100, diff 5)
        assert_eq!(selected[0].start, 110);
        assert_eq!(selected[0].difficulty, 2);
    }

    #[test]
    fn test_word_wise_max_difficulty_level_scaling() {
        assert_eq!(WordWisePacingConfig::default().max_difficulty_level, 3);

        let candidates: Vec<WordWiseGloss> = (1u8..=5)
            .map(|diff| WordWiseGloss {
                start: (diff as usize) * 100,
                end: None,
                difficulty: diff,
                sense_id: diff as u32 * 10,
                low_confidence: false,
            })
            .collect();

        let level1 = WordWiseBuilder::filter_candidates_priority_paced(
            &candidates,
            &WordWisePacingConfig {
                max_difficulty_level: 1,
                min_distance: 1,
                max_glosses_per_window: 10,
                window_size: 1000,
                frontmatter_skip_chars: 0,
                max_total_glosses: 100,
            },
        );
        assert_eq!(
            level1.iter().map(|g| g.difficulty).collect::<Vec<_>>(),
            vec![1]
        );

        let level3 = WordWiseBuilder::filter_candidates_priority_paced(
            &candidates,
            &WordWisePacingConfig {
                max_difficulty_level: 3,
                min_distance: 1,
                max_glosses_per_window: 10,
                window_size: 1000,
                frontmatter_skip_chars: 0,
                max_total_glosses: 100,
            },
        );
        assert_eq!(
            level3.iter().map(|g| g.difficulty).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );

        let level5 = WordWiseBuilder::filter_candidates_priority_paced(
            &candidates,
            &WordWisePacingConfig {
                max_difficulty_level: 5,
                min_distance: 1,
                max_glosses_per_window: 10,
                window_size: 1000,
                frontmatter_skip_chars: 0,
                max_total_glosses: 100,
            },
        );
        assert_eq!(
            level5.iter().map(|g| g.difficulty).collect::<Vec<_>>(),
            vec![1, 2, 3, 4, 5]
        );
    }

    #[test]
    fn test_word_wise_skips_image_alt_glosses_while_advancing_pid() {
        use crate::ir::{Block, BookIR, BookMetadata, ResourceAsset, Section, TextSpan};
        use crate::packager::KfxPackager;
        use std::io::Cursor;

        let alt_text = "An abstruse and chimerical diagram"; // 34 UTF-16 units
        let mut resources = std::collections::HashMap::new();
        resources.insert(
            "resolved.jpg".to_string(),
            ResourceAsset {
                id: "resolved.jpg".to_string(),
                mime_type: "image/jpeg".to_string(),
                data: super::super::FALLBACK_PLACEHOLDER_JPEG.to_vec(),
            },
        );

        let ir = BookIR {
            metadata: BookMetadata::default(),
            sections: vec![
                Section {
                    id: "cover".to_string(),
                    title: "Cover".to_string(),
                    blocks: vec![Block::Image {
                        id: "img_cover".to_string(),
                        resource_id: "resolved.jpg".to_string(),
                        alt: Some("Cover".to_string()),
                        width: None,
                        height: None,
                        href: None,
                    }],
                },
                Section {
                    id: "sec1".to_string(),
                    title: "Chapter 1".to_string(),
                    blocks: vec![
                        Block::Image {
                            id: "img_alt".to_string(),
                            resource_id: "diagram.png".to_string(), // unresolved, alt present -> 34 PIDs
                            alt: Some(alt_text.to_string()),
                            width: None,
                            height: None,
                            href: None,
                        },
                        Block::Image {
                            id: "img_none".to_string(),
                            resource_id: "spacer.png".to_string(), // unresolved, alt None -> "[Image]" = 7 PIDs
                            alt: None,
                            width: None,
                            height: None,
                            href: None,
                        },
                        Block::Image {
                            id: "img_resolved".to_string(),
                            resource_id: "resolved.jpg".to_string(), // resolved, alt None -> 1 PID
                            alt: None,
                            width: None,
                            height: None,
                            href: None,
                        },
                        Block::Image {
                            id: "img_resolved_long_alt".to_string(),
                            resource_id: "resolved.jpg".to_string(), // resolved, 400-char alt -> still 1 PID
                            alt: Some(format!("An abstruse diagram {}", "X".repeat(380))),
                            width: None,
                            height: None,
                            href: None,
                        },
                        Block::Paragraph {
                            id: "p1".to_string(),
                            spans: vec![TextSpan {
                                text: "To elucidate.".to_string(),
                                style: None,
                                anchor_id: None,
                                href: None,
                                ruby: None,
                            }],
                            style: None,
                        },
                    ],
                },
            ],
            resources,
            toc: Vec::new(),
            page_list: Vec::new(),
            footnotes: std::collections::HashMap::new(),
        };

        let mut builder = WordWiseBuilder::new("B0TEST", "CR!TEST", "en", "en", "1").with_pacing(
            WordWisePacingConfig {
                max_difficulty_level: 5,
                min_distance: 1,
                max_glosses_per_window: 10,
                window_size: 500,
                frontmatter_skip_chars: 0,
                max_total_glosses: 100,
            },
        );

        let count = builder.generate_from_book(&ir);
        assert_eq!(
            count, 1,
            "Image alt text must NOT emit glosses; only 'elucidate' should match: {:?}",
            builder.glosses
        );
        let gloss = builder.glosses.values().next().unwrap();
        assert_eq!(gloss.sense_id, 36391, "Expected gloss for 'elucidate'");
        // Cover section: 1 (root) + 1 (resolved cover img) = 2
        // Section 1: 1 (root) + 34 (unresolved alt) + 7 (unresolved "[Image]") + 1 (resolved alt None)
        //          + 1 (resolved 400-char alt) + 3 ("To ") = 49
        let expected_pid = 2 + 1 + crate::ir::str_utf16_len(alt_text) + 7 + 1 + 1 + 3;
        assert_eq!(gloss.start, expected_pid);

        // Cross-check against KfxPackager's emitted $265 position_id_map total PID length
        let mut packager = KfxPackager::new(&ir, "EBOK");
        let mut buf = Cursor::new(Vec::new());
        packager.package(&mut buf).expect("package must succeed");
        let pos_id_ent = packager
            .entities
            .iter()
            .find(|e| e.chunk_type == crate::symbols::KfxSymbol::PositionIdMap as u32)
            .expect("$265 position_id_map");
        let vals = crate::ion::IonReader::new(&pos_id_ent.data)
            .decode_all()
            .unwrap();
        let sym_contains = crate::symbols::KfxSymbol::Contains as u32;
        let sym_pid = crate::symbols::KfxSymbol::Pid as u32;
        let sym_len = crate::symbols::KfxSymbol::Length as u32;
        let mut sec1_pid_and_len = None;
        if let crate::ion::IonValue::Struct(fields) = &vals[0]
            && let Some((_, crate::ion::IonValue::List(entries))) =
                fields.iter().find(|(k, _)| *k == sym_contains)
            && let Some(crate::ion::IonValue::Struct(s1)) = entries.get(1)
        {
            let p = s1
                .iter()
                .find(|(k, _)| *k == sym_pid)
                .and_then(|(_, v)| match v {
                    crate::ion::IonValue::Int(n) => Some(*n as usize),
                    _ => None,
                });
            let l = s1
                .iter()
                .find(|(k, _)| *k == sym_len)
                .and_then(|(_, v)| match v {
                    crate::ion::IonValue::Int(n) => Some(*n as usize),
                    _ => None,
                });
            sec1_pid_and_len = p.zip(l);
        }
        // Section 1 starts at PID 2 and has length: 1 (root) + 34 + 7 + 1 + 1 + 13 ("To elucidate.") = 57
        assert_eq!(sec1_pid_and_len, Some((2, 1 + 34 + 7 + 1 + 1 + 13)));
    }

    #[test]
    fn test_word_wise_non_bmp_utf16_and_list_image_pid_parity() {
        use crate::ir::{Block, BookIR, BookMetadata, ResourceAsset, Section, TextSpan};
        use crate::packager::KfxPackager;
        use std::io::Cursor;

        let mut resources = std::collections::HashMap::new();
        resources.insert(
            "fig.jpg".to_string(),
            ResourceAsset {
                id: "fig.jpg".to_string(),
                mime_type: "image/jpeg".to_string(),
                data: super::super::FALLBACK_PLACEHOLDER_JPEG.to_vec(),
            },
        );

        let ir = BookIR {
            metadata: BookMetadata::default(),
            sections: vec![
                Section {
                    id: "cover".to_string(),
                    title: "Cover".to_string(),
                    blocks: vec![Block::Image {
                        id: "c_img".to_string(),
                        resource_id: "fig.jpg".to_string(),
                        alt: Some("Cover".to_string()),
                        width: None,
                        height: None,
                        href: None,
                    }],
                },
                Section {
                    id: "sec1".to_string(),
                    title: "Chapter 1".to_string(),
                    blocks: vec![
                        Block::Paragraph {
                            id: "p_emoji".to_string(),
                            spans: vec![TextSpan {
                                text: "📖 The abstruse theorem.".to_string(),
                                style: None,
                                anchor_id: None,
                                href: None,
                                ruby: None,
                            }],
                            style: None,
                        },
                        Block::List {
                            id: "l1".to_string(),
                            ordered: false,
                            items: vec![
                                vec![Block::Paragraph {
                                    id: "lp1".to_string(),
                                    spans: vec![TextSpan {
                                        text: "Item 𠮷".to_string(),
                                        style: None,
                                        anchor_id: None,
                                        href: None,
                                        ruby: None,
                                    }],
                                    style: None,
                                }],
                                vec![Block::Image {
                                    id: "limg".to_string(),
                                    resource_id: "fig.jpg".to_string(),
                                    alt: Some("An abstruse figure".to_string()), // 18 UTF-16 units, must NOT be glossed
                                    width: None,
                                    height: None,
                                    href: None,
                                }],
                                vec![Block::Paragraph {
                                    id: "lp2".to_string(),
                                    spans: vec![TextSpan {
                                        text: "To elucidate.".to_string(),
                                        style: None,
                                        anchor_id: None,
                                        href: None,
                                        ruby: None,
                                    }],
                                    style: None,
                                }],
                            ],
                        },
                    ],
                },
            ],
            resources,
            toc: Vec::new(),
            page_list: Vec::new(),
            footnotes: std::collections::HashMap::new(),
        };

        let mut builder = WordWiseBuilder::new("B0TEST", "CR!TEST", "en", "en", "1").with_pacing(
            WordWisePacingConfig {
                max_difficulty_level: 5,
                min_distance: 1,
                max_glosses_per_window: 10,
                window_size: 500,
                frontmatter_skip_chars: 0,
                max_total_glosses: 100,
            },
        );
        let count = builder.generate_from_book(&ir);
        assert!(count >= 2);

        // Cover: 1 (root) + 1 (resolved cover img) = 2.
        // sec1 root: +1 = 3.
        // p_emoji: "📖 The " -> 📖(2) + " "(1) + "The "(4) = 7 UTF-16 units -> "abstruse" at PID 3 + 7 = 10!
        assert!(
            builder.glosses.contains_key(&10),
            "Expected 'abstruse' at UTF-16 PID 10 (📖 = 2 UTF-16 units), got {:?}",
            builder.glosses
        );

        // p_emoji UTF-16 len = 2 + 22 = 24. Next block starts at 3 + 24 = 27.
        // List text seg 1 ("Item 𠮷"): 5 + 2 = 7 UTF-16 units -> ends at 34.
        // List image seg (resolved fig.jpg, alt "An abstruse figure"): 1 PID -> 34..35 (NOT glossed!).
        // List text seg 2 ("To elucidate."): starts at 35 -> "To " = 3 -> "elucidate" at PID 38!
        assert!(
            builder.glosses.contains_key(&38),
            "Expected 'elucidate' at PID 38 after list image, got {:?}",
            builder.glosses
        );
        // Ensure no gloss was emitted on the list image PID 34
        assert!(
            !builder.glosses.contains_key(&34),
            "List-nested image alt text must NOT be glossed: {:?}",
            builder.glosses
        );

        // Verify exact PID parity against KfxPackager's $265 position_id_map
        let mut packager = KfxPackager::new(&ir, "EBOK");
        let mut buf = Cursor::new(Vec::new());
        packager.package(&mut buf).expect("package must succeed");
        let pos_id_ent = packager
            .entities
            .iter()
            .find(|e| e.chunk_type == crate::symbols::KfxSymbol::PositionIdMap as u32)
            .expect("$265");
        let vals = crate::ion::IonReader::new(&pos_id_ent.data)
            .decode_all()
            .unwrap();
        let sym_contains = crate::symbols::KfxSymbol::Contains as u32;
        let sym_len = crate::symbols::KfxSymbol::Length as u32;
        let mut sec1_len = None;
        if let crate::ion::IonValue::Struct(fields) = &vals[0]
            && let Some((_, crate::ion::IonValue::List(entries))) =
                fields.iter().find(|(k, _)| *k == sym_contains)
            && let Some(crate::ion::IonValue::Struct(s1)) = entries.get(1)
        {
            sec1_len = s1
                .iter()
                .find(|(k, _)| *k == sym_len)
                .and_then(|(_, v)| match v {
                    crate::ion::IonValue::Int(n) => Some(*n as usize),
                    _ => None,
                });
        }
        // Expected sec1 length: 1 (root) + 24 (p_emoji) + 7 ("Item 𠮷") + 1 (resolved list img) + 13 ("To elucidate.") = 46
        assert_eq!(sec1_len, Some(46));
    }

    #[test]
    fn test_write_to_file_bare_relative_filename() {
        if std::env::var_os("KFX_WW_BARE_FILE_CHILD").is_some() {
            let mut builder = WordWiseBuilder::new("B0TEST", "CR!TEST", "en", "en", "1");
            builder.add_gloss(10, Some(15), 3, 101, false);
            let out = Path::new("bare_words.kll");
            assert_eq!(out.parent(), Some(Path::new("")));
            builder
                .write_to_file(out)
                .expect("write_to_file must succeed on bare relative filename");
            let conn = Connection::open(out).unwrap();
            let count: i64 = conn
                .query_row("SELECT count(*) FROM glosses;", [], |r| r.get(0))
                .unwrap();
            assert_eq!(count, 1);
            return;
        }

        let temp = tempfile::tempdir().unwrap();
        let exe = std::env::current_exe().unwrap();
        let output = std::process::Command::new(exe)
            .args([
                "--exact",
                "enhancements::word_wise::tests::test_write_to_file_bare_relative_filename",
            ])
            .current_dir(temp.path())
            .env("KFX_WW_BARE_FILE_CHILD", "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "Bare KLL child test failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(temp.path().join("bare_words.kll").is_file());
    }

    #[test]
    fn test_write_to_file_rejects_symlinks_and_missing_parent_without_fallback() {
        let temp = tempfile::tempdir().unwrap();
        let mut builder = WordWiseBuilder::new("B0TEST", "CR!TEST", "en", "en", "1");
        builder.add_gloss(10, Some(15), 3, 101, false);

        // Missing parent directory must fail cleanly without creating files
        let missing_parent_path = temp.path().join("nonexistent_dir").join("out.kll");
        assert!(builder.write_to_file(&missing_parent_path).is_err());
        assert!(!missing_parent_path.exists());

        #[cfg(unix)]
        {
            use std::os::unix::fs::{PermissionsExt, symlink};

            // Live symlink pointing to read-only victim file
            let victim = temp.path().join("victim.kll");
            std::fs::write(&victim, b"UNTOUCHED_VICTIM").unwrap();
            std::fs::set_permissions(&victim, std::fs::Permissions::from_mode(0o444)).unwrap();

            let live_link = temp.path().join("live_link.kll");
            symlink(&victim, &live_link).unwrap();
            assert!(builder.write_to_file(&live_link).is_err());
            assert_eq!(std::fs::read(&victim).unwrap(), b"UNTOUCHED_VICTIM");
            assert_eq!(
                std::fs::metadata(&victim).unwrap().permissions().mode() & 0o777,
                0o444
            );

            // Dangling symlink must be rejected without creating target
            let missing_target = temp.path().join("never_created.kll");
            let dangling_link = temp.path().join("dangling_link.kll");
            symlink(&missing_target, &dangling_link).unwrap();
            assert!(builder.write_to_file(&dangling_link).is_err());
            assert!(!missing_target.exists());
        }
    }

    #[test]
    fn test_word_wise_to_bytes_in_memory_sqlite_serialization() {
        let mut builder = WordWiseBuilder::new("B0MEMTEST", "CR!MEMTEST", "en", "en-US", "42");
        builder.add_gloss(10, Some(18), 1, 8435, false);
        builder.add_gloss(50, Some(59), 2, 36391, true);

        let bytes = builder
            .to_bytes()
            .expect("in-memory SQLite serialize must succeed");
        assert!(
            bytes.starts_with(b"SQLite format 3\0"),
            "Serialized bytes must start with SQLite 3 magic header"
        );

        // Write bytes to a temp file solely to inspect with rusqlite::Connection::open
        let temp = tempfile::tempdir().unwrap();
        let verify_path = temp.path().join("verify.kll");
        std::fs::write(&verify_path, &bytes).unwrap();

        let conn =
            Connection::open(&verify_path).expect("serialized bytes must be valid SQLite DB");
        let acr: String = conn
            .query_row("SELECT value FROM metadata WHERE key = 'acr'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(acr, "CR!MEMTEST");

        let rev: String = conn
            .query_row(
                "SELECT value FROM metadata WHERE key = 'bookRevision'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(rev, "42");

        let mut stmt = conn
            .prepare("SELECT start, end, difficulty, sense_id, low_confidence FROM glosses ORDER BY start ASC")
            .unwrap();
        let rows: Vec<(i64, Option<i64>, i64, i64, bool)> = stmt
            .query_map([], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })
            .unwrap()
            .collect::<SqlResult<Vec<_>>>()
            .unwrap();
        assert_eq!(
            rows,
            vec![
                (10, Some(18), 1, 8435, false),
                (50, Some(59), 2, 36391, true)
            ]
        );

        let idx_count: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type = 'index' AND name = 'idx_glosses_diff_start'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(idx_count, 1);
    }

    #[test]
    fn test_priority_pacing_saturating_add_near_usize_max() {
        let pacing = WordWisePacingConfig {
            max_difficulty_level: 5,
            min_distance: 20,
            max_glosses_per_window: 2,
            window_size: 100,
            frontmatter_skip_chars: 0,
            max_total_glosses: 10,
        };

        let candidates = vec![
            WordWiseGloss {
                start: usize::MAX - 5,
                end: None,
                difficulty: 1,
                sense_id: 1001,
                low_confidence: false,
            },
            WordWiseGloss {
                start: usize::MAX - 40,
                end: None,
                difficulty: 2,
                sense_id: 1002,
                low_confidence: false,
            },
            WordWiseGloss {
                start: usize::MAX - 80,
                end: None,
                difficulty: 3,
                sense_id: 1003,
                low_confidence: false,
            },
        ];

        let selected = WordWiseBuilder::filter_candidates_priority_paced(&candidates, &pacing);
        assert_eq!(selected.len(), 2);
        assert_eq!(selected[0].start, usize::MAX - 40);
        assert_eq!(selected[1].start, usize::MAX - 5);
    }

    #[cfg(unix)]
    #[test]
    fn test_write_to_file_rejects_symlinks_and_overwrites_readonly() {
        use std::os::unix::fs::{PermissionsExt, symlink};

        let temp = tempfile::tempdir().unwrap();
        let real_dir = temp.path().join("real_dir");
        std::fs::create_dir_all(&real_dir).unwrap();
        let link_dir = temp.path().join("link_dir");
        symlink(&real_dir, &link_dir).unwrap();

        let mut builder = WordWiseBuilder::new("B0TEST", "CR!TEST", "en", "en", "1");
        builder.add_gloss(10, Some(15), 2, 201, false);

        // 1. Symlink destination is rejected and victim remains untouched
        let victim = temp.path().join("victim.kll");
        std::fs::write(&victim, b"SAFE_VICTIM").unwrap();
        std::fs::set_permissions(&victim, std::fs::Permissions::from_mode(0o444)).unwrap();
        let dest_link = real_dir.join("dest_link.kll");
        symlink(&victim, &dest_link).unwrap();

        assert!(builder.write_to_file(&dest_link).is_err());
        assert_eq!(std::fs::read(&victim).unwrap(), b"SAFE_VICTIM");
        assert_eq!(
            std::fs::metadata(&victim).unwrap().permissions().mode() & 0o777,
            0o444
        );

        // 2. Symlink parent directory is rejected without creating target file
        let via_link_parent = link_dir.join("via_parent.kll");
        assert!(builder.write_to_file(&via_link_parent).is_err());
        assert!(!real_dir.join("via_parent.kll").exists());

        // 3. Real read-only .kll file is atomically overwritten via O_NOFOLLOW
        let ro_kll = real_dir.join("readonly.kll");
        builder.write_to_file(&ro_kll).unwrap();
        std::fs::set_permissions(&ro_kll, std::fs::Permissions::from_mode(0o444)).unwrap();
        assert!(std::fs::metadata(&ro_kll).unwrap().permissions().readonly());

        builder.add_gloss(40, Some(48), 1, 202, false);
        builder.write_to_file(&ro_kll).unwrap();

        let conn = Connection::open(&ro_kll).unwrap();
        let count: i64 = conn
            .query_row("SELECT count(*) FROM glosses;", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn test_word_wise_to_bytes_sqlite_header_and_write_to_file_parity() {
        let mut builder = WordWiseBuilder::new(
            "034BE348BEAA0CDDBF568FB246A84883",
            "CR!PARITY",
            "en",
            "en-US",
            "7",
        );
        builder.add_gloss(10, Some(18), 1, 8435, false);
        builder.add_gloss(50, Some(59), 2, 36391, true);
        builder.add_gloss(120, None, 3, 99102, false);

        let bytes = builder.to_bytes().expect("to_bytes must succeed");
        assert!(
            bytes.len() >= 4096,
            "SQLite database must be at least one 4096-byte page"
        );
        assert_eq!(&bytes[..16], b"SQLite format 3\0");

        let change_counter = u32::from_be_bytes([bytes[24], bytes[25], bytes[26], bytes[27]]);
        assert!(change_counter >= 1, "change_counter must be >= 1");
        assert_eq!(
            &bytes[92..96],
            &bytes[24..28],
            "version_valid_for must equal change_counter"
        );
        let expected_sqlite_ver = u32::try_from(rusqlite::version_number())
            .expect("rusqlite version_number must fit in u32")
            .to_be_bytes();
        assert_eq!(&bytes[96..100], &expected_sqlite_ver);
        assert!(
            bytes[72..92].iter().all(|&b| b == 0),
            "reserved expansion area 72..92 must remain all zero"
        );

        let temp = tempfile::tempdir().unwrap();
        let disk_path = temp.path().join("disk_parity.kll");
        builder
            .write_to_file(&disk_path)
            .expect("write_to_file must succeed");
        let disk_bytes = std::fs::read(&disk_path).unwrap();
        assert!(disk_bytes.len() >= 100);

        // Structural SQLite header parity between to_bytes() and write_to_file()
        assert_eq!(&bytes[16..18], &disk_bytes[16..18], "page_size parity");
        assert_eq!(bytes[18], disk_bytes[18], "write_ver parity");
        assert_eq!(bytes[19], disk_bytes[19], "read_ver parity");

        let disk_change_counter = u32::from_be_bytes([
            disk_bytes[24],
            disk_bytes[25],
            disk_bytes[26],
            disk_bytes[27],
        ]);
        assert!(change_counter > 0 && disk_change_counter > 0);

        let mem_vvf = u32::from_be_bytes([bytes[92], bytes[93], bytes[94], bytes[95]]);
        let disk_vvf = u32::from_be_bytes([
            disk_bytes[92],
            disk_bytes[93],
            disk_bytes[94],
            disk_bytes[95],
        ]);
        assert!(mem_vvf > 0 && disk_vvf > 0);
        assert_eq!(mem_vvf, change_counter);
        assert_eq!(disk_vvf, disk_change_counter);

        assert_eq!(
            &bytes[96..100],
            &disk_bytes[96..100],
            "sqlite_version parity"
        );

        let mem_schema_cookie = u32::from_be_bytes([bytes[40], bytes[41], bytes[42], bytes[43]]);
        let disk_schema_cookie = u32::from_be_bytes([
            disk_bytes[40],
            disk_bytes[41],
            disk_bytes[42],
            disk_bytes[43],
        ]);
        assert_ne!(mem_schema_cookie, 0, "schema_cookie must be non-zero");
        assert_eq!(
            mem_schema_cookie, disk_schema_cookie,
            "schema_cookie parity"
        );

        assert_eq!(&bytes[44..48], &disk_bytes[44..48], "schema_format parity");
        assert_eq!(&bytes[56..60], &disk_bytes[56..60], "text_encoding parity");

        // Write to_bytes() output to a temp file and verify SQLite integrity & payload
        let verify_path = temp.path().join("verify_bytes.kll");
        std::fs::write(&verify_path, &bytes).unwrap();
        let conn = Connection::open(&verify_path).expect("re-opened .kll must be valid SQLite DB");

        let integrity: String = conn
            .query_row("PRAGMA integrity_check", [], |r| r.get(0))
            .unwrap();
        assert_eq!(integrity, "ok");

        let acr: String = conn
            .query_row("SELECT value FROM metadata WHERE key = 'acr'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(acr, "CR!PARITY");

        let rev: String = conn
            .query_row(
                "SELECT value FROM metadata WHERE key = 'bookRevision'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(rev, "7");

        let dict_id: String = conn
            .query_row(
                "SELECT value FROM metadata WHERE key = 'enDictionaryId'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(dict_id, "kll.en.en");

        let meta_count: i64 = conn
            .query_row("SELECT count(*) FROM metadata", [], |r| r.get(0))
            .unwrap();
        assert_eq!(meta_count, 9);

        let mut stmt = conn
            .prepare("SELECT start, end, difficulty, sense_id, low_confidence FROM glosses ORDER BY start ASC")
            .unwrap();
        let gloss_rows: Vec<(i64, Option<i64>, i64, i64, bool)> = stmt
            .query_map([], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })
            .unwrap()
            .collect::<SqlResult<Vec<_>>>()
            .unwrap();
        assert_eq!(
            gloss_rows,
            vec![
                (10, Some(18), 1, 8435, false),
                (50, Some(59), 2, 36391, true),
                (120, None, 3, 99102, false),
            ]
        );

        let idx_count: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type = 'index' AND name = 'idx_glosses_diff_start'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(idx_count, 1);
    }

    #[test]
    fn test_normalize_sqlite_header_edge_cases() {
        // 1. Reject < 100-byte buffers
        let mut empty = Vec::new();
        assert!(WordWiseBuilder::normalize_sqlite_header(&mut empty).is_err());

        let mut short = vec![0u8; 99];
        short[..16].copy_from_slice(b"SQLite format 3\0");
        assert!(WordWiseBuilder::normalize_sqlite_header(&mut short).is_err());

        // 2. Reject 100+ byte buffer with corrupt magic header
        let mut bad_magic = vec![0u8; 100];
        bad_magic[..16].copy_from_slice(b"SQLite format 2\0");
        assert!(WordWiseBuilder::normalize_sqlite_header(&mut bad_magic).is_err());

        // 3. Non-zero change_counter = 4 with zero version_valid_for & sqlite_version
        let mut partial = vec![0u8; 100];
        partial[..16].copy_from_slice(b"SQLite format 3\0");
        partial[24..28].copy_from_slice(&4u32.to_be_bytes());
        WordWiseBuilder::normalize_sqlite_header(&mut partial)
            .expect("partial header repair must succeed");

        assert_eq!(
            u32::from_be_bytes([partial[24], partial[25], partial[26], partial[27]]),
            4
        );
        assert_eq!(
            u32::from_be_bytes([partial[92], partial[93], partial[94], partial[95]]),
            4
        );
        let expected_ver = u32::try_from(rusqlite::version_number()).unwrap();
        assert_eq!(
            u32::from_be_bytes([partial[96], partial[97], partial[98], partial[99]]),
            expected_ver
        );
        assert!(
            partial[72..92].iter().all(|&b| b == 0),
            "reserved bytes 72..92 must remain untouched"
        );

        // 4. Already-populated non-zero header must be left completely untouched
        let mut populated = vec![0u8; 100];
        populated[..16].copy_from_slice(b"SQLite format 3\0");
        populated[24..28].copy_from_slice(&7u32.to_be_bytes());
        populated[92..96].copy_from_slice(&7u32.to_be_bytes());
        populated[96..100].copy_from_slice(&3_045_000u32.to_be_bytes());
        let expected_untouched = populated.clone();

        WordWiseBuilder::normalize_sqlite_header(&mut populated)
            .expect("already populated header must succeed");
        assert_eq!(populated, expected_untouched);
    }
}
