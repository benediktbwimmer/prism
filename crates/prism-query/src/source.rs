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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditSliceOptions {
    pub before_lines: usize,
    pub after_lines: usize,
    pub max_lines: usize,
    pub max_chars: usize,
}

impl Default for EditSliceOptions {
    fn default() -> Self {
        Self {
            before_lines: 1,
            after_lines: 1,
            max_lines: 12,
            max_chars: 1200,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditSlice {
    pub text: String,
    pub start_line: usize,
    pub end_line: usize,
    pub focus: SourceLocation,
    pub relative_focus: SourceLocation,
    pub truncated: bool,
}

pub fn source_excerpt_for_line_range(
    source: &str,
    start_line: usize,
    end_line: usize,
    max_chars: usize,
) -> SourceExcerpt {
    SourceDocument::new(source).excerpt_for_line_range(start_line, end_line, max_chars)
}

pub fn source_slice_around_line(
    source: &str,
    line: usize,
    before_lines: usize,
    after_lines: usize,
    max_chars: usize,
) -> EditSlice {
    SourceDocument::new(source).slice_around_line(line, before_lines, after_lines, max_chars)
}

pub fn source_location_for_span(source: &str, start: usize, end: usize) -> SourceLocation {
    SourceDocument::new(source).location(Span {
        start: start as u32,
        end: end as u32,
    })
}

pub fn source_excerpt_for_span(
    source: &str,
    start: usize,
    end: usize,
    context_lines: usize,
    max_chars: usize,
) -> SourceExcerpt {
    SourceDocument::new(source).excerpt(
        Span {
            start: start as u32,
            end: end as u32,
        },
        SourceExcerptOptions {
            context_lines,
            max_lines: 0,
            max_chars,
        },
    )
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
        let (start_index, end_index) = self.line_bounds(span);

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

    pub(crate) fn excerpt_for_line_range(
        &self,
        start_line: usize,
        end_line: usize,
        max_chars: usize,
    ) -> SourceExcerpt {
        let start_index = self.line_index_for_number(start_line);
        let end_index = self.line_index_for_number(end_line).max(start_index);
        self.excerpt(
            self.span_for_lines(start_index, end_index),
            SourceExcerptOptions {
                context_lines: 0,
                max_lines: end_index - start_index + 1,
                max_chars,
            },
        )
    }

    pub(crate) fn edit_slice(&self, span: Span, options: EditSliceOptions) -> EditSlice {
        let (focus_start, focus_end) = self.line_bounds(span);
        let focus = self.location(span);
        let mut slice_start = focus_start.saturating_sub(options.before_lines);
        let mut slice_end = usize::min(
            focus_end.saturating_add(options.after_lines),
            self.line_ranges.len() - 1,
        );
        let mut truncated = false;

        if options.max_lines > 0 {
            let allowed_lines = options.max_lines.max(focus_end - focus_start + 1);
            while slice_end - slice_start + 1 > allowed_lines {
                if !self.trim_slice_context(
                    &mut slice_start,
                    &mut slice_end,
                    focus_start,
                    focus_end,
                ) {
                    break;
                }
                truncated = true;
            }
        }

        if options.max_chars > 0 {
            while self.slice_lines(slice_start, slice_end).chars().count() > options.max_chars {
                if !self.trim_slice_context(
                    &mut slice_start,
                    &mut slice_end,
                    focus_start,
                    focus_end,
                ) {
                    break;
                }
                truncated = true;
            }
        }

        EditSlice {
            text: self.slice_lines(slice_start, slice_end).to_owned(),
            start_line: slice_start + 1,
            end_line: slice_end + 1,
            focus,
            relative_focus: SourceLocation {
                start_line: focus.start_line - slice_start,
                start_column: focus.start_column,
                end_line: focus.end_line - slice_start,
                end_column: focus.end_column,
            },
            truncated,
        }
    }

    pub(crate) fn slice_around_line(
        &self,
        line: usize,
        before_lines: usize,
        after_lines: usize,
        max_chars: usize,
    ) -> EditSlice {
        let line_index = self.line_index_for_number(line);
        self.edit_slice(
            self.line_span(line_index),
            EditSliceOptions {
                before_lines,
                after_lines,
                max_lines: 0,
                max_chars,
            },
        )
    }

    fn slice_lines(&self, start_line: usize, end_line: usize) -> &'a str {
        let start = self.line_ranges[start_line].0;
        let end = self.line_ranges[end_line].1;
        self.source.get(start..end).unwrap_or_default()
    }

    fn line_bounds(&self, span: Span) -> (usize, usize) {
        let start = usize::min(span.start as usize, self.source.len());
        let end = usize::min(span.end as usize, self.source.len());
        let start_index = self.line_index_for_offset(start);
        let end_index = if end > start {
            self.line_index_for_offset(end.saturating_sub(1))
        } else {
            start_index
        };
        (start_index, end_index)
    }

    fn line_index_for_number(&self, line: usize) -> usize {
        line.saturating_sub(1)
            .min(self.line_ranges.len().saturating_sub(1))
    }

    fn line_span(&self, line_index: usize) -> Span {
        let (start, end) = self.line_content_bounds(line_index);
        Span {
            start: start as u32,
            end: end as u32,
        }
    }

    fn span_for_lines(&self, start_index: usize, end_index: usize) -> Span {
        let start = self.line_ranges[start_index].0;
        let end = self.line_ranges[end_index].1;
        Span {
            start: start as u32,
            end: end as u32,
        }
    }

    fn line_content_bounds(&self, line_index: usize) -> (usize, usize) {
        let (start, end) = self.line_ranges[line_index];
        let line = &self.source[start..end];
        let trimmed = line.trim_end_matches(['\r', '\n']);
        (start, start + trimmed.len())
    }

    fn trim_slice_context(
        &self,
        slice_start: &mut usize,
        slice_end: &mut usize,
        focus_start: usize,
        focus_end: usize,
    ) -> bool {
        let before = focus_start.saturating_sub(*slice_start);
        let after = slice_end.saturating_sub(focus_end);
        if before == 0 && after == 0 {
            return false;
        }
        if after > before {
            *slice_end = slice_end.saturating_sub(1);
        } else if before > 0 {
            *slice_start = slice_start.saturating_add(1);
        } else {
            *slice_end = slice_end.saturating_sub(1);
        }
        true
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

#[cfg(test)]
mod tests {
    use prism_ir::Span;

    use super::{
        source_excerpt_for_line_range, source_slice_around_line, EditSliceOptions, SourceDocument,
    };

    #[test]
    fn edit_slice_tracks_focus_inside_context_window() {
        let source = "zero\none\nalpha();\nbeta();\nfour\n";
        let start = source.find("alpha").expect("alpha present");
        let end = source.find("beta").expect("beta present") + "beta();".len();
        let document = SourceDocument::new(source);

        let slice = document.edit_slice(
            Span {
                start: start as u32,
                end: end as u32,
            },
            EditSliceOptions {
                before_lines: 1,
                after_lines: 1,
                ..EditSliceOptions::default()
            },
        );

        assert_eq!(slice.start_line, 2);
        assert_eq!(slice.end_line, 5);
        assert_eq!(slice.focus.start_line, 3);
        assert_eq!(slice.focus.end_line, 4);
        assert_eq!(slice.relative_focus.start_line, 2);
        assert_eq!(slice.relative_focus.end_line, 3);
        assert_eq!(slice.truncated, false);
    }

    #[test]
    fn edit_slice_preserves_focus_when_caps_trim_context() {
        let source = "zero\none\ntwo\nalpha();\nbeta();\nthree\nfour\n";
        let start = source.find("alpha").expect("alpha present");
        let end = source.find("beta").expect("beta present") + "beta();".len();
        let document = SourceDocument::new(source);

        let slice = document.edit_slice(
            Span {
                start: start as u32,
                end: end as u32,
            },
            EditSliceOptions {
                before_lines: 2,
                after_lines: 2,
                max_lines: 2,
                max_chars: usize::MAX,
            },
        );

        assert_eq!(slice.start_line, 4);
        assert_eq!(slice.end_line, 5);
        assert!(slice.text.contains("alpha();"));
        assert!(slice.text.contains("beta();"));
        assert_eq!(slice.relative_focus.start_line, 1);
        assert_eq!(slice.relative_focus.end_line, 2);
        assert!(slice.truncated);
    }

    #[test]
    fn line_range_excerpt_returns_exact_requested_lines() {
        let source = "zero\none\ntwo\nthree\n";

        let excerpt = source_excerpt_for_line_range(source, 2, 3, usize::MAX);

        assert_eq!(excerpt.start_line, 2);
        assert_eq!(excerpt.end_line, 3);
        assert_eq!(excerpt.text, "one\ntwo\n");
        assert!(!excerpt.truncated);
    }

    #[test]
    fn around_line_slice_tracks_requested_focus_line() {
        let source = "zero\none\ntwo\nthree\n";

        let slice = source_slice_around_line(source, 3, 1, 1, usize::MAX);

        assert_eq!(slice.start_line, 2);
        assert_eq!(slice.end_line, 4);
        assert_eq!(slice.focus.start_line, 3);
        assert_eq!(slice.focus.end_line, 3);
        assert_eq!(slice.relative_focus.start_line, 2);
        assert_eq!(slice.relative_focus.end_line, 2);
        assert_eq!(slice.text, "one\ntwo\nthree\n");
        assert!(!slice.truncated);
    }
}
