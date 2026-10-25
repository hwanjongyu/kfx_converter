//! Kindle Word Wise Database Builder
//!
//! Generates binary-compliant SQLite files (`LanguageLayer.<lang>.<asin>.kll`) for Amazon Kindle e-readers.

use std::collections::BTreeMap;
use std::path::Path;
use rusqlite::{params, Connection, Result as SqlResult};

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
    /// Minimum vocabulary difficulty threshold: 1 (easy) to 5 (advanced).
    pub min_difficulty: u8,
    /// Target preferred difficulty level: typically 5.
    pub target_difficulty: u8,
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
            min_difficulty: 1,
            target_difficulty: 3,
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
        let primary = trimmed
            .split(['-', '_'])
            .next()
            .unwrap_or("en");
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
        format!("LanguageLayer.{}.{}.kll", Self::normalize_lang(target_language), asin)
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
        if difficulty < self.pacing.min_difficulty {
            return false;
        }
        if start < self.pacing.frontmatter_skip_chars {
            return false;
        }
        if self.glosses.len() >= self.pacing.max_total_glosses {
            return false;
        }
        if let Some(last) = self.last_gloss_pos
            && start.saturating_sub(last) < self.pacing.min_distance {
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

    /// Extracts tokens and populates Word Wise glosses directly from a BookIR document.
    pub fn generate_from_book(&mut self, book: &crate::ir::BookIR) -> usize {
        let dict = super::dictionary::LemmaDictionary::new();
        let initial_count = self.glosses.len();

        let mut temp_book = crate::ir::BookIR {
            metadata: book.metadata.clone(),
            resources: book.resources.clone(),
            sections: book.sections.clone(),
            ..Default::default()
        };
        temp_book.fragment_large_sections(
            crate::ir::DEFAULT_MAX_SECTION_CHARS,
            crate::ir::DEFAULT_MAX_SECTION_BLOCKS,
        );
        temp_book.ensure_cover_section();

        let mut pid = 0usize;

        for section in &temp_book.sections {
            // Section root element occupies 1 PID position in Yellowjacket position_id_map ($265)
            pid += 1;

            if section.blocks.is_empty() {
                pid += 1;
                continue;
            }

            for block in &section.blocks {
                let text = block.plain_text();
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
                            self.add_gloss_paced(
                                pid + start_c,
                                None,
                                difficulty,
                                sense_id,
                                false,
                            );
                        }
                        word_str.clear();
                    }
                    char_idx += 1;
                }

                if let Some(start_c) = word_char_start.take()
                    && let Some((difficulty, sense_id)) = dict.lookup(&word_str, 3) {
                        self.add_gloss_paced(
                            pid + start_c,
                            None,
                            difficulty,
                            sense_id,
                            false,
                        );
                    }

                let block_len = char_idx.max(1);
                pid += block_len;
            }
        }

        self.glosses.len() - initial_count
    }

    /// Writes the Word Wise `.kll` SQLite database atomically to disk.
    /// Uses a temporary file in the target directory and renames to prevent partial/corrupted
    /// writes that trigger Kindle OS `.bad_file` blacklisting.
    #[allow(clippy::permissions_set_readonly_false)]
    pub fn write_to_file<P: AsRef<Path>>(&self, path: P) -> SqlResult<()> {
        let path = path.as_ref();
        let parent = path.parent().unwrap_or_else(|| Path::new("."));

        if let Ok(tmp_file) = tempfile::NamedTempFile::new_in(parent) {
            {
                let mut conn = Connection::open(tmp_file.path())?;
                self.populate_connection(&mut conn)?;
            }
            if let Ok(meta) = std::fs::metadata(path)
                && meta.permissions().readonly()
            {
                let mut perms = meta.permissions();
                perms.set_readonly(false);
                let _ = std::fs::set_permissions(path, perms);
            }
            if tmp_file.persist(path).is_ok() {
                return Ok(());
            }
        }

        // Fallback: direct connection open if temporary file creation/persisting is unsupported
        let mut conn = Connection::open(path)?;
        self.populate_connection(&mut conn)
    }

    /// Serializes the Word Wise `.kll` SQLite database directly into an in-memory byte vector.
    pub fn to_bytes(&self) -> SqlResult<Vec<u8>> {
        let tmp_file = tempfile::NamedTempFile::new()
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        {
            let mut conn = Connection::open(tmp_file.path())?;
            self.populate_connection(&mut conn)?;
        }
        std::fs::read(tmp_file.path())
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
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

             CREATE TABLE metadata (
                 key TEXT,
                 value TEXT
             );

             CREATE TABLE glosses (
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
            .query_row(
                "SELECT value FROM metadata WHERE key = 'acr'",
                [],
                |row| row.get(0),
            )
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
        assert!(bytes.starts_with(b"SQLite format 3\0"), "Must be valid SQLite header");
    }

    #[test]
    fn test_word_wise_pacing() {
        let mut builder = WordWiseBuilder::new("TESTASIN", "CR!TESTACR", "en", "en", "1")
            .with_pacing(WordWisePacingConfig {
                min_difficulty: 4,
                target_difficulty: 5,
                min_distance: 25,
                max_glosses_per_window: 2,
                window_size: 100,
                frontmatter_skip_chars: 50,
                max_total_glosses: 10,
            });

        // 1. Frontmatter skip: start < 50 should be rejected
        assert!(!builder.add_gloss_paced(20, None, 4, 101, false));

        // 2. Difficulty check: difficulty 3 < min_difficulty 4 rejected
        assert!(!builder.add_gloss_paced(60, None, 3, 102, false));

        // 3. Valid first gloss: start = 60, diff = 4
        assert!(builder.add_gloss_paced(60, None, 4, 103, false));

        // 4. Distance pacing: start = 70 (dist = 10 < 25) rejected
        assert!(!builder.add_gloss_paced(70, None, 4, 104, false));

        // 5. Valid second gloss: start = 90 (dist = 30 >= 25)
        assert!(builder.add_gloss_paced(90, None, 5, 105, false));

        // 6. Window cap: start = 110 (window 110-100=10 contains 60 and 90, so already 2) rejected
        assert!(!builder.add_gloss_paced(110, None, 4, 106, false));

        // 7. Outside window: start = 180 (window 180-100=80 only contains 90, so 1 item) accepted
        assert!(builder.add_gloss_paced(180, None, 4, 107, false));

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
                blocks: vec![
                    Block::Paragraph {
                        id: "p1".to_string(),
                        spans: vec![TextSpan {
                            text: "The abstruse theory was compelling and difficult to elucidate.".to_string(),
                            style: None,
                            anchor_id: None,
                            href: None,
                            ruby: None,
                        }],
                        style: None,
                    },
                ],
            }],
            resources: std::collections::HashMap::new(),
            toc: Vec::new(),
            page_list: Vec::new(),
            footnotes: std::collections::HashMap::new(),
        };

        let mut builder = WordWiseBuilder::new("B0TEST", "CR!TEST", "en", "en", "1")
            .with_pacing(WordWisePacingConfig {
                min_difficulty: 1,
                target_difficulty: 5,
                min_distance: 10,
                max_glosses_per_window: 5,
                window_size: 500,
                frontmatter_skip_chars: 0,
                max_total_glosses: 100,
            });

        let count = builder.generate_from_book(&ir);
        assert!(count >= 2, "Expected at least 2 glosses (abstruse, elucidate), got {}", count);
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
                blocks: vec![
                    Block::Paragraph {
                        id: "p1".to_string(),
                        spans: vec![TextSpan {
                            text: "The author's abstruse's explanation did not elucidate the dilemma.".to_string(),
                            style: None,
                            anchor_id: None,
                            href: None,
                            ruby: None,
                        }],
                        style: None,
                    },
                ],
            }],
            resources: std::collections::HashMap::new(),
            toc: Vec::new(),
            page_list: Vec::new(),
            footnotes: std::collections::HashMap::new(),
        };

        let mut builder = WordWiseBuilder::new("B0TEST", "CR!TEST", "en", "en", "1")
            .with_pacing(WordWisePacingConfig {
                min_difficulty: 1,
                target_difficulty: 5,
                min_distance: 1,
                max_glosses_per_window: 10,
                window_size: 500,
                frontmatter_skip_chars: 0,
                max_total_glosses: 100,
            });

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
                blocks: vec![
                    Block::Paragraph {
                        id: "p1".to_string(),
                        spans: vec![TextSpan {
                            text: "The abstruse theorem.".to_string(),
                            style: None,
                            anchor_id: None,
                            href: None,
                            ruby: None,
                        }],
                        style: None,
                    },
                ],
            }],
            resources,
            toc: Vec::new(),
            page_list: Vec::new(),
            footnotes: std::collections::HashMap::new(),
        };

        let mut builder = WordWiseBuilder::new("B0TEST", "CR!TEST", "en", "en", "1")
            .with_pacing(WordWisePacingConfig {
                min_difficulty: 1,
                target_difficulty: 5,
                min_distance: 1,
                max_glosses_per_window: 10,
                window_size: 500,
                frontmatter_skip_chars: 0,
                max_total_glosses: 100,
            });

        let count = builder.generate_from_book(&ir);
        assert_eq!(count, 2);
        let gloss = builder.glosses.values().next().unwrap();
        // Cover section: 1 (root) + 5 ("Cover") = 6 PIDs
        // Chapter 1: 1 (root) = PID 7
        // "The " = 4 chars, so "abstruse" starts at offset 4 within block
        // Expected start: 6 (cover) + 1 (root) + 4 ("The ") = 11
        assert_eq!(gloss.start, 11, "Expected PID 11 with cover offset, got {}", gloss.start);
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

        // Verify database contents
        let conn = Connection::open(&db_path).unwrap();
        let count: i64 = conn
            .query_row("SELECT count(*) FROM glosses;", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);

        // Test overwrite of an existing read-only file
        let mut perms = std::fs::metadata(&db_path).unwrap().permissions();
        perms.set_readonly(true);
        std::fs::set_permissions(&db_path, perms).unwrap();

        builder.add_gloss(20, Some(25), 4, 102, false);
        builder.write_to_file(&db_path).unwrap();

        let conn2 = Connection::open(&db_path).unwrap();
        let count2: i64 = conn2
            .query_row("SELECT count(*) FROM glosses;", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count2, 2);
    }
}
