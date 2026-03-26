use prism_ir::Span;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceLocation {
    pub start_line: usize,
    pub start_column: usize,
    pub end_line: usize,
    pub end_column: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceExcerptOptions {
    pub context_lines: usize,
    pub max_lines: usize,
    pub max_chars: usize,
}

impl Default for SourceExcerptOptions {
    fn default() -> Self {
        Self {
            context_lines: 1,
            max_lines: 14,
            max_chars: 1400,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceExcerpt {
    pub text: String,
    pub start_line: usize,
    pub end_line: usize,
    pub truncated: bool,
}

pub(crate) struct SourceDocument<'a> {
    source: &'a str,
    line_ranges: Vec<(usize, usize)>,
}

impl<'a> SourceDocument<'a> {
    pub(crate) fn new(source: &'a str) -> Self {
        let mut line_ranges = Vec::new();
        let mut start = 0usize;
        for line in source.split_inclusive('\n') {
            let end = start + line.len();
            line_ranges.push((start, end));
            start = end;
        }
        if line_ranges.is_empty() {
            line_ranges.push((0, 0));
        } else if start < source.len() {
            line_ranges.push((start, source.len()));
        }
        Self {
            source,
            line_ranges,
        }
    }

    pub(crate) fn span_text(&self, span: Span) -> &'a str {
        let start = usize::min(span.start as usize, self.source.len());
        let end = usize::min(span.end as usize, self.source.len());
        self.source.get(start..end).unwrap_or_default()
    }

    pub(crate) fn location(&self, span: Span) -> SourceLocation {
        let start = usize::min(span.start as usize, self.source.len());
        let end = usize::min(span.end as usize, self.source.len());
        let start_position = self.position_for_offset(start);
        let end_position = if end > start {
            self.position_for_offset(end.saturating_sub(1))
        } else {
            start_position
        };
        SourceLocation {
            start_line: start_position.0,
            start_column: start_position.1,
            end_line: end_position.0,
            end_column: end_position.1,
        }
    }

    pub(crate) fn excerpt(&self, span: Span, options: SourceExcerptOptions) -> SourceExcerpt {
        let start = usize::min(span.start as usize, self.source.len());
        let end = usize::min(span.end as usize, self.source.len());
        let start_index = self.line_index_for_offset(start);
        let end_index = if end > start {
            self.line_index_for_offset(end.saturating_sub(1))
        } else {
            start_index
        };

        let excerpt_start = start_index.saturating_sub(options.context_lines);
        let mut excerpt_end = usize::min(
            end_index.saturating_add(options.context_lines),
            self.line_ranges.len() - 1,
        );
        let mut truncated = excerpt_start > start_index || excerpt_end < end_index;

        if options.max_lines > 0 {
            let allowed_end = excerpt_start.saturating_add(options.max_lines.saturating_sub(1));
            if excerpt_end > allowed_end {
                excerpt_end = allowed_end;
                truncated = true;
            }
        }

        let mut text = self.slice_lines(excerpt_start, excerpt_end).to_owned();
        if options.max_chars > 0 && text.chars().count() > options.max_chars {
            while excerpt_start < excerpt_end && text.chars().count() > options.max_chars {
                excerpt_end = excerpt_end.saturating_sub(1);
                text = self.slice_lines(excerpt_start, excerpt_end).to_owned();
                truncated = true;
            }
            if text.chars().count() > options.max_chars {
                text = text.chars().take(options.max_chars).collect();
                truncated = true;
            }
        }

        SourceExcerpt {
            text,
            start_line: excerpt_start + 1,
            end_line: excerpt_end + 1,
            truncated,
        }
    }

    fn slice_lines(&self, start_line: usize, end_line: usize) -> &'a str {
        let start = self.line_ranges[start_line].0;
        let end = self.line_ranges[end_line].1;
        self.source.get(start..end).unwrap_or_default()
    }

    fn line_index_for_offset(&self, offset: usize) -> usize {
        let offset = usize::min(offset, self.source.len());
        let index = self
            .line_ranges
            .partition_point(|(_, end)| *end <= offset)
            .min(self.line_ranges.len().saturating_sub(1));
        if index >= self.line_ranges.len() {
            self.line_ranges.len().saturating_sub(1)
        } else {
            index
        }
    }

    fn position_for_offset(&self, offset: usize) -> (usize, usize) {
        let line_index = self.line_index_for_offset(offset);
        let line_start = self.line_ranges[line_index].0;
        let offset = usize::min(offset, self.source.len());
        let column = self.source[line_start..offset].chars().count() + 1;
        (line_index + 1, column)
    }
}
