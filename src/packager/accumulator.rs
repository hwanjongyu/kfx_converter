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
        } else if self.current_bytes >= MAX_CONTENT_CHUNK_BYTES {
            self.finished
                .push((self.current_chunk, std::mem::take(&mut self.segments)));
            self.current_chunk += 1;
            self.current_bytes = 0;
        }
        let index = self.segments.len();
        self.current_bytes += text.len();
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
