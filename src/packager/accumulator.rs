//! Content text accumulator for Yellowjacket 8KB chunking.

pub const MAX_CONTENT_CHUNK_BYTES: usize = 8192;

/// Book-global text accumulator for Yellowjacket content entities ($145).
///
/// Text is packed into `content_1..content_N` chunks of at most [`MAX_CONTENT_CHUNK_BYTES`]
/// (measured in UTF-8 bytes), spanning section boundaries without exceeding Kindle memory bounds.
#[derive(Default)]
pub struct TextAccumulator {
    finished: Vec<(usize, Vec<String>)>,
    segments: Vec<String>,
    current_bytes: usize,
    current_chunk: usize,
}

impl TextAccumulator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, text: &str) -> (usize, usize) {
        if self.current_chunk == 0 {
            self.current_chunk = 1;
        } else if !self.segments.is_empty()
            && self.current_bytes.saturating_add(text.len()) > MAX_CONTENT_CHUNK_BYTES
        {
            self.finished
                .push((self.current_chunk, std::mem::take(&mut self.segments)));
            self.current_chunk += 1;
            self.current_bytes = 0;
        }
        let index = self.segments.len();
        self.current_bytes = self.current_bytes.saturating_add(text.len());
        self.segments.push(text.to_string());
        (self.current_chunk, index)
    }

    pub fn drain_chunks(&mut self) -> Vec<(usize, Vec<String>)> {
        let mut chunks = std::mem::take(&mut self.finished);
        if !self.segments.is_empty() {
            chunks.push((self.current_chunk, std::mem::take(&mut self.segments)));
        }
        self.current_bytes = 0;
        self.current_chunk = 0;
        chunks
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_accumulator_never_overshoots_multi_segment_chunks() {
        let cases: &[Vec<String>] = &[
            vec!["a".repeat(5000), "b".repeat(6000)],
            vec!["x".repeat(4096), "y".repeat(4096), "z".to_string()],
            vec!["m".repeat(MAX_CONTENT_CHUNK_BYTES), "n".to_string()],
            // Multi-byte UTF-8 ("한" = 3 bytes): 2000 * 3 = 6000 bytes, 1000 * 3 = 3000 bytes
            vec!["한".repeat(2000), "한".repeat(1000), "글".repeat(1000)],
        ];

        for inputs in cases {
            let mut acc = TextAccumulator::new();
            let mut coords = Vec::with_capacity(inputs.len());
            for s in inputs {
                coords.push((acc.push(s), s.clone()));
            }
            let chunks = acc.drain_chunks();

            for (chunk_id, segs) in &chunks {
                let total: usize = segs.iter().map(|s| s.len()).sum();
                assert!(
                    total <= MAX_CONTENT_CHUNK_BYTES,
                    "Chunk {} overshot MAX_CONTENT_CHUNK_BYTES: {}",
                    chunk_id,
                    total
                );
                assert!(!segs.is_empty(), "Must not emit empty chunks");
            }

            for ((chunk_id, seg_idx), expected) in coords {
                let (_, segs) = chunks
                    .iter()
                    .find(|(cid, _)| *cid == chunk_id)
                    .expect("chunk_id must exist");
                assert_eq!(&segs[seg_idx], &expected);
            }
        }
    }

    #[test]
    fn test_accumulator_single_oversized_segment_no_empty_chunk() {
        let mut acc = TextAccumulator::new();
        let huge = "H".repeat(10_000);
        let c1 = acc.push(&huge);
        let c2 = acc.push("after");
        assert_eq!(c1, (1, 0));
        assert_eq!(c2, (2, 0));

        let chunks = acc.drain_chunks();
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0], (1, vec![huge]));
        assert_eq!(chunks[1], (2, vec!["after".to_string()]));
    }
}
