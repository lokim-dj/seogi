//! The transcript, indexed for looking quotes up, and read a window at a
//! time for the model.

use super::*;

// ---------------------------------------------------------------------------
// The transcript, indexed for looking quotes up
// ---------------------------------------------------------------------------

/// Letters and digits only, lowercased: how text is compared.
pub(super) fn bare(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// Where in the transcript `quote` was said: the start of the line it is
/// in, and how closely it matches (1.0 for word for word). `None` when
/// nothing like it was said.
pub fn locate(transcript: &Transcript, quote: &str) -> Option<(u64, f32)> {
    let index = Index::new(transcript);
    let (lines, score) = index.find(quote, None)?;
    Some((transcript.segments.get(*lines.start())?.start_ms, score))
}

pub(super) struct Index<'a> {
    pub(super) segments: &'a [Segment],
    /// Every segment's bare text, joined.
    joined: Vec<char>,
    /// For each char of `joined`, the segment it belongs to.
    owner: Vec<usize>,
    /// `joined` as a string, for looking names up.
    pub(super) text: String,
    /// The confidence below which a line is among the least sure tenth.
    pub(super) unsure_below: f32,
}

impl<'a> Index<'a> {
    pub(super) fn new(transcript: &'a Transcript) -> Self {
        let mut joined = Vec::new();
        let mut owner = Vec::new();
        for (index, segment) in transcript.segments.iter().enumerate() {
            for c in bare(&segment.text).chars() {
                joined.push(c);
                owner.push(index);
            }
        }
        let mut confidences: Vec<f32> = transcript
            .segments
            .iter()
            .filter_map(|segment| segment.confidence)
            .collect();
        confidences.sort_by(f32::total_cmp);
        // Only a recording long enough to have a "least sure tenth" gets one,
        // and only lines that are also not sure in absolute terms count.
        let unsure_below = if confidences.len() >= 20 {
            confidences[confidences.len() / 10].min(0.75)
        } else {
            0.0
        };
        Self {
            segments: &transcript.segments,
            text: joined.iter().collect(),
            joined,
            owner,
            unsure_below,
        }
    }

    /// The segments `quote` is in, and how well it matched (1 for exact).
    /// Looked for near `near_ms` first, then anywhere, then loosely.
    pub(super) fn find(
        &self,
        quote: &str,
        near_ms: Option<u64>,
    ) -> Option<(std::ops::RangeInclusive<usize>, f32)> {
        let needle: Vec<char> = bare(quote).chars().collect();
        if needle.len() < 2 {
            return None;
        }
        let mut hits: Vec<usize> = Vec::new();
        if needle.len() <= self.joined.len() {
            for start in 0..=self.joined.len() - needle.len() {
                if self.joined[start..start + needle.len()] == needle[..] {
                    hits.push(start);
                }
            }
        }
        if !hits.is_empty() {
            // The hit nearest the time the model gave; else the first.
            let best = match near_ms {
                Some(near) => hits
                    .iter()
                    .copied()
                    .min_by_key(|&start| self.segments[self.owner[start]].start_ms.abs_diff(near))
                    .unwrap_or(hits[0]),
                None => hits[0],
            };
            let first = self.owner[best];
            let last = self.owner[best + needle.len() - 1];
            return Some((first..=last, 1.0));
        }

        // Loosely: the line (near the given time if any) sharing the most of
        // the quote's four-character pieces.
        if needle.len() < 6 {
            return None;
        }
        let pieces: HashSet<&[char]> = needle.windows(4).collect();
        let mut best: Option<(usize, f32)> = None;
        for (index, segment) in self.segments.iter().enumerate() {
            let text: Vec<char> = bare(&segment.text).chars().collect();
            if text.len() < 4 {
                continue;
            }
            let have: HashSet<&[char]> = text.windows(4).collect();
            let shared = pieces.iter().filter(|piece| have.contains(*piece)).count();
            let mut score = shared as f32 / pieces.len() as f32;
            // Between equals, the line near the stated time.
            if near_ms.is_some_and(|near| segment.start_ms.abs_diff(near) <= NEAR_MS) {
                score += 0.05;
            }
            if best.is_none_or(|(_, top)| score > top) {
                best = Some((index, score));
            }
        }
        best.filter(|(_, score)| *score >= LOOSE_MATCH)
            .map(|(index, score)| (index..=index, score.min(0.99)))
    }

    /// The text of segments `range`, widened by `around` lines each side.
    pub(super) fn text_around(
        &self,
        range: &std::ops::RangeInclusive<usize>,
        around: usize,
    ) -> String {
        let first = range.start().saturating_sub(around);
        let last = (range.end() + around).min(self.segments.len() - 1);
        self.segments[first..=last]
            .iter()
            .map(|segment| segment.text.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

// ---------------------------------------------------------------------------
// Windows of transcript, as the model reads them
// ---------------------------------------------------------------------------

/// One line of a window as the model reads it.
pub(super) struct Line {
    /// When the line starts; `None` for a "중요 표시" marker, which is not a
    /// line of speech and gets no number.
    ms: Option<u64>,
    pub(super) text: String,
}

/// The line put where the meeting ended, when the recording goes on after.
pub(super) const AFTER_THE_END: &str = "[회의 끝: 여기부터는 회의가 끝난 뒤의 말일 수 있습니다]";

/// The transcript as lines `[hh:mm:ss] name: text`, with the marks in place
/// and, at `end_ms`, the line saying the meeting is over; cut into windows.
pub(super) fn windows(
    transcript: &Transcript,
    marks_ms: &[u64],
    mark_notes: &[String],
    end_ms: Option<u64>,
) -> Vec<Vec<Line>> {
    // A mark as the model reads it, with the note written beside it if any.
    let mut marks: Vec<(u64, String)> = marks_ms
        .iter()
        .enumerate()
        .map(|(index, &ms)| {
            let note = mark_notes.get(index).map_or("", |note| note.trim());
            let text = if note.is_empty() {
                format!("[중요 표시 {}]", clock(ms))
            } else {
                format!("[중요 표시 {} 메모: {note}]", clock(ms))
            };
            (ms, text)
        })
        .collect();
    marks.sort_by_key(|(ms, _)| *ms);
    let mut marks = marks.into_iter().peekable();

    let mut lines: Vec<Line> = Vec::new();
    let mut ended = false;
    for paragraph in transcript.paragraphs() {
        if !ended && end_ms.is_some_and(|end| paragraph.start_ms >= end) {
            ended = true;
            lines.push(Line {
                ms: None,
                text: AFTER_THE_END.to_string(),
            });
        }
        while marks
            .peek()
            .is_some_and(|(mark, _)| *mark <= paragraph.start_ms)
        {
            if let Some((_, text)) = marks.next() {
                lines.push(Line { ms: None, text });
            }
        }
        lines.push(Line {
            ms: Some(paragraph.start_ms),
            text: format!(
                "[{}] {}: {}",
                clock(paragraph.start_ms),
                transcript.speaker_name(paragraph.speaker),
                paragraph.text
            ),
        });
    }
    for (_, text) in marks {
        lines.push(Line { ms: None, text });
    }

    let mut windows: Vec<Vec<Line>> = Vec::new();
    let mut start = 0;
    while start < lines.len() {
        let mut end = start;
        let mut chars = 0;
        while end < lines.len()
            && (end == start || chars + lines[end].text.chars().count() <= WINDOW_CHARS)
        {
            chars += lines[end].text.chars().count() + 1;
            end += 1;
        }
        windows.push(
            lines[start..end]
                .iter()
                .map(|line| Line {
                    ms: line.ms,
                    text: line.text.clone(),
                })
                .collect(),
        );
        if end >= lines.len() {
            break;
        }
        start = end.saturating_sub(WINDOW_OVERLAP_LINES).max(start + 1);
    }
    windows
}

/// A window as text, each line of speech numbered from 1.
pub(super) fn numbered(window: &[Line]) -> (String, Vec<u64>) {
    let mut text = String::new();
    let mut starts: Vec<u64> = Vec::new();
    for line in window {
        match line.ms {
            Some(ms) => {
                starts.push(ms);
                text.push_str(&format!("{}. {}\n", starts.len(), line.text));
            }
            None => text.push_str(&format!("   {}\n", line.text)),
        }
    }
    (text, starts)
}

#[cfg(test)]
mod tests {
    use super::super::test_help::*;
    use super::*;

    #[test]
    fn a_retyped_quote_is_found_loosely_and_marked() {
        let transcript = transcript();
        let index = Index::new(&transcript);
        // Spacing and punctuation never matter.
        let (lines, score) = index
            .find("신발장도면을 다시그려서, 금요일까지", None)
            .unwrap();
        assert_eq!((*lines.start(), score), (2, 1.0));
        // A few changed characters still find the line, as a loose match.
        let (lines, score) = index
            .find(
                "중문은 신발장 도면을 새로 그려서 금요일까지 보내드리겠습니다",
                None,
            )
            .unwrap();
        assert_eq!(*lines.start(), 2);
        assert!(score < 1.0);
        assert!(index
            .find("전혀 다른 말을 하고 있습니다 여기서는", None)
            .is_none());
    }

    #[test]
    fn windows_keep_marks_in_place_and_overlap_by_two_lines() {
        let mut transcript = transcript();
        // Enough text for more than one window.
        for i in 0..40 {
            transcript.segments.push(line(
                30_000 + i * 10_000,
                (i % 2 + 1) as u32,
                &"자재와 일정에 대해서 계속 이야기를 이어 갑니다. ".repeat(6),
            ));
        }
        let windows = windows(&transcript, &[12_000], &[], None);
        assert!(windows.len() > 1);
        let (text, starts) = numbered(&windows[0]);
        // The mark sits between the lines it fell between, and has no number.
        let mark_at = text.find("   [중요 표시 00:00:12]").unwrap();
        assert!(text.find("3. [00:00:11]").unwrap() < mark_at);
        assert!(mark_at < text.find("4. [00:00:16]").unwrap());
        assert_eq!(starts[..4], [1_000, 6_000, 11_000, 16_000]);
        // The second window starts with the last two lines of the first.
        let first = &windows[0];
        assert_eq!(windows[1][0].text, first[first.len() - 2].text);
        assert!(windows.iter().all(|w| {
            w.iter().map(|l| l.text.chars().count()).sum::<usize>() <= WINDOW_CHARS + 400
        }));
    }
}
