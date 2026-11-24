//! Plans how a recording is cut for transcription.
//!
//! A chunk is one run of the speech model. It holds one speaker's speech and
//! is never longer than the model's window, so the vocabulary prompt (which a
//! model applies to the start of a run) reaches all of it, and every line of
//! the transcript has one speaker and a time in the original recording.

use std::ops::Range;

/// Samples per second of the audio every duration here is counted in.
pub const RATE: usize = 16_000;

/// Speaker labels are decided on a grid this fine.
const FRAME: usize = RATE / 50; // 20 ms

/// One speaker's turn from diarization, in samples. Turns of different
/// speakers may overlap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Turn {
    pub start: usize,
    pub end: usize,
    /// The speaker's number, from 1.
    pub speaker: u32,
}

/// One run of the speech model: a stretch of audio with one speaker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    pub range: Range<usize>,
    /// 0 when no speaker is known for it.
    pub speaker: u32,
}

impl Chunk {
    pub fn start_ms(&self) -> u64 {
        (self.range.start * 1000 / RATE) as u64
    }

    pub fn end_ms(&self) -> u64 {
        (self.range.end * 1000 / RATE) as u64
    }
}

/// The rules chunks are cut by, in samples.
#[derive(Debug, Clone, Copy)]
pub struct Rules {
    /// No chunk is longer than this.
    pub max_chunk: usize,
    /// One speaker's speech across a pause up to this long stays one chunk.
    pub join_gap: usize,
    /// A change of speaker shorter than this, in the middle of someone
    /// else's speech, is taken as part of that speech.
    pub min_turn: usize,
    /// Audio kept before and after a chunk so its first and last sounds are
    /// whole, where that does not run into a neighbour.
    pub pad: usize,
    /// Speech shorter than this is not transcribed at all.
    pub min_chunk: usize,
}

impl Rules {
    /// Rules for a model whose window is `window_secs` long (Whisper: 30).
    /// Chunks stay a little under the window so padding still fits.
    pub fn for_window(window_secs: f64) -> Self {
        Self {
            max_chunk: ((window_secs - 2.0).max(5.0) * RATE as f64) as usize,
            join_gap: RATE * 12 / 10,
            min_turn: RATE * 7 / 10,
            pad: RATE * 15 / 100,
            min_chunk: RATE / 5,
        }
    }
}

/// A stretch of speech with one speaker label, before grouping.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Run {
    range: Range<usize>,
    speaker: u32,
}

/// Cuts `speech` (the stretches where someone talks, in order) into chunks,
/// labelled from `turns`. `pcm` is only read to find the quietest place to
/// cut speech that runs longer than a chunk may.
pub fn plan(speech: &[Range<usize>], turns: &[Turn], pcm: &[f32], rules: &Rules) -> Vec<Chunk> {
    let mut sorted_turns = turns.to_vec();
    sorted_turns.sort_by_key(|turn| (turn.start, turn.speaker));

    let mut runs: Vec<Run> = Vec::new();
    // Turns before this index ended before the speech being labelled.
    let mut first_live = 0;
    for segment in speech {
        if segment.end <= segment.start {
            continue;
        }
        runs.extend(label_segment(segment, &sorted_turns, &mut first_live));
    }
    fold_blips(&mut runs, rules.min_turn);

    // Group one speaker's runs across short pauses, up to the longest chunk.
    let mut chunks: Vec<Chunk> = Vec::new();
    for run in runs {
        if let Some(last) = chunks.last_mut() {
            let gap = run.range.start.saturating_sub(last.range.end);
            if last.speaker == run.speaker
                && gap <= rules.join_gap
                && run.range.end - last.range.start <= rules.max_chunk
            {
                last.range.end = run.range.end;
                continue;
            }
        }
        chunks.push(Chunk {
            range: run.range,
            speaker: run.speaker,
        });
    }

    let mut cut: Vec<Chunk> = Vec::new();
    for chunk in chunks {
        split_long(chunk, pcm, rules, &mut cut);
    }
    cut.retain(|chunk| chunk.range.len() >= rules.min_chunk);
    pad(&mut cut, rules.pad, pcm.len());
    cut
}

/// The speaker at each point of one stretch of speech, as runs. Where turns
/// overlap, the one that began first keeps the floor: a "네" said over
/// someone's sentence does not cut the sentence in two.
fn label_segment(segment: &Range<usize>, turns: &[Turn], first_live: &mut usize) -> Vec<Run> {
    let frames = segment.len().div_ceil(FRAME);
    let mut labels = vec![0u32; frames];
    for (index, label) in labels.iter_mut().enumerate() {
        let at = segment.start + index * FRAME + FRAME / 2;
        // Speech is labelled in order, so turns that have ended stay ended.
        while *first_live < turns.len() && turns[*first_live].end <= at {
            *first_live += 1;
        }
        // `turns` is sorted by start: the first one covering `at` began first.
        *label = turns[*first_live..]
            .iter()
            .take_while(|turn| turn.start <= at)
            .find(|turn| at < turn.end)
            .map_or(0, |turn| turn.speaker);
    }

    // Speech no turn covers takes the speaker next to it: forwards first,
    // then backwards for speech before the first labelled frame.
    let mut last = 0;
    for label in labels.iter_mut() {
        if *label == 0 {
            *label = last;
        } else {
            last = *label;
        }
    }
    let mut next = 0;
    for label in labels.iter_mut().rev() {
        if *label == 0 {
            *label = next;
        } else {
            next = *label;
        }
    }
    // A stretch no turn touches: the nearest turn within two seconds.
    if labels.iter().all(|&label| label == 0) {
        let nearest = turns
            .iter()
            .map(|turn| {
                let distance = if turn.end <= segment.start {
                    segment.start - turn.end
                } else {
                    turn.start.saturating_sub(segment.end)
                };
                (distance, turn.speaker)
            })
            .min();
        if let Some((distance, speaker)) = nearest {
            if distance <= 2 * RATE {
                labels.fill(speaker);
            }
        }
    }

    let mut runs: Vec<Run> = Vec::new();
    for (index, &speaker) in labels.iter().enumerate() {
        let start = segment.start + index * FRAME;
        let end = (start + FRAME).min(segment.end);
        match runs.last_mut() {
            Some(run) if run.speaker == speaker => run.range.end = end,
            _ => runs.push(Run {
                range: start..end,
                speaker,
            }),
        }
    }
    runs
}

/// Folds a brief change of speaker inside continuous speech into the speech
/// around it, and joins the runs that then touch.
fn fold_blips(runs: &mut Vec<Run>, min_turn: usize) {
    let mut index = 0;
    while index < runs.len() {
        if runs[index].range.len() >= min_turn {
            index += 1;
            continue;
        }
        let touches_previous = index > 0 && runs[index - 1].range.end == runs[index].range.start;
        let touches_next =
            index + 1 < runs.len() && runs[index + 1].range.start == runs[index].range.end;
        let adopted = match (touches_previous, touches_next) {
            (true, true) => {
                // The longer neighbour is the one being interrupted.
                if runs[index - 1].range.len() >= runs[index + 1].range.len() {
                    Some(runs[index - 1].speaker)
                } else {
                    Some(runs[index + 1].speaker)
                }
            }
            (true, false) => Some(runs[index - 1].speaker),
            (false, true) => Some(runs[index + 1].speaker),
            // On its own between pauses it is a short answer, not a blip.
            (false, false) => None,
        };
        if let Some(speaker) = adopted {
            runs[index].speaker = speaker;
        }
        index += 1;
    }

    let mut joined: Vec<Run> = Vec::with_capacity(runs.len());
    for run in runs.drain(..) {
        match joined.last_mut() {
            Some(last) if last.speaker == run.speaker && last.range.end == run.range.start => {
                last.range.end = run.range.end;
            }
            _ => joined.push(run),
        }
    }
    *runs = joined;
}

/// Cuts a chunk longer than the rules allow at the quietest point in the
/// last part of each window, so a word is not cut in half if there is any
/// softer place nearby.
fn split_long(chunk: Chunk, pcm: &[f32], rules: &Rules, out: &mut Vec<Chunk>) {
    let mut start = chunk.range.start;
    let end = chunk.range.end;
    while end - start > rules.max_chunk {
        let search = (start + rules.max_chunk * 6 / 10)..(start + rules.max_chunk);
        let cut = quietest_point(pcm, &search).unwrap_or(start + rules.max_chunk);
        out.push(Chunk {
            range: start..cut,
            speaker: chunk.speaker,
        });
        start = cut;
    }
    out.push(Chunk {
        range: start..end,
        speaker: chunk.speaker,
    });
}

/// The middle of the quietest 50 ms window inside `range`.
fn quietest_point(pcm: &[f32], range: &Range<usize>) -> Option<usize> {
    const WINDOW: usize = RATE / 20;
    let end = range.end.min(pcm.len());
    if range.start + WINDOW > end {
        return None;
    }
    let mut best: Option<(f32, usize)> = None;
    let mut at = range.start;
    while at + WINDOW <= end {
        let energy: f32 = pcm[at..at + WINDOW].iter().map(|s| s * s).sum();
        if best.is_none_or(|(quietest, _)| energy < quietest) {
            best = Some((energy, at + WINDOW / 2));
        }
        at += WINDOW / 2;
    }
    best.map(|(_, at)| at)
}

/// Widens each chunk into the silence around it, never into a neighbour.
fn pad(chunks: &mut [Chunk], pad: usize, total: usize) {
    let bounds: Vec<Range<usize>> = chunks.iter().map(|chunk| chunk.range.clone()).collect();
    for (index, chunk) in chunks.iter_mut().enumerate() {
        let floor = if index == 0 { 0 } else { bounds[index - 1].end };
        let ceiling = bounds.get(index + 1).map_or(total, |next| next.start);
        // Two chunks share the silence between them.
        let before = (chunk.range.start - floor.min(chunk.range.start)) / 2;
        let after = (ceiling.max(chunk.range.end) - chunk.range.end) / 2;
        chunk.range.start -= pad.min(before);
        chunk.range.end = (chunk.range.end + pad.min(after)).min(total.max(chunk.range.end));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(seconds: f64) -> usize {
        (seconds * RATE as f64) as usize
    }

    fn span(start: f64, end: f64) -> Range<usize> {
        secs(start)..secs(end)
    }

    fn turn(start: f64, end: f64, speaker: u32) -> Turn {
        Turn {
            start: secs(start),
            end: secs(end),
            speaker,
        }
    }

    fn rules() -> Rules {
        Rules {
            pad: 0,
            ..Rules::for_window(30.0)
        }
    }

    fn silence(seconds: f64) -> Vec<f32> {
        vec![0.0; secs(seconds)]
    }

    #[test]
    fn without_speakers_speech_joins_across_short_pauses() {
        let speech = [span(0.0, 5.0), span(5.5, 9.0), span(20.0, 22.0)];
        let chunks = plan(&speech, &[], &silence(25.0), &rules());
        assert_eq!(
            chunks,
            vec![
                Chunk {
                    range: span(0.0, 9.0),
                    speaker: 0
                },
                Chunk {
                    range: span(20.0, 22.0),
                    speaker: 0
                },
            ]
        );
    }

    #[test]
    fn a_change_of_speaker_inside_continuous_speech_cuts_the_chunk() {
        let speech = [span(0.0, 9.0)];
        let turns = [turn(0.0, 5.0, 1), turn(5.0, 9.0, 2)];
        let chunks = plan(&speech, &turns, &silence(10.0), &rules());
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].speaker, 1);
        assert_eq!(chunks[1].speaker, 2);
        assert_eq!(chunks[0].range.end, chunks[1].range.start);
        assert_eq!(chunks[0].range.end, secs(5.0));
    }

    #[test]
    fn a_word_said_over_someone_does_not_cut_their_sentence() {
        let speech = [span(0.0, 10.0)];
        let turns = [turn(0.0, 10.0, 1), turn(4.0, 4.3, 2)];
        let chunks = plan(&speech, &turns, &silence(10.0), &rules());
        assert_eq!(
            chunks,
            vec![Chunk {
                range: span(0.0, 10.0),
                speaker: 1
            }]
        );
    }

    #[test]
    fn a_brief_switch_in_the_middle_of_speech_is_folded() {
        let speech = [span(0.0, 10.0)];
        let turns = [turn(0.0, 4.0, 1), turn(4.0, 4.4, 2), turn(4.4, 10.0, 1)];
        let chunks = plan(&speech, &turns, &silence(10.0), &rules());
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].speaker, 1);
    }

    #[test]
    fn a_short_answer_between_pauses_keeps_its_speaker() {
        let speech = [span(0.0, 4.0), span(4.6, 5.0), span(5.6, 9.0)];
        let turns = [turn(0.0, 4.0, 1), turn(4.6, 5.0, 2), turn(5.6, 9.0, 1)];
        let chunks = plan(&speech, &turns, &silence(10.0), &rules());
        assert_eq!(
            chunks.iter().map(|c| c.speaker).collect::<Vec<_>>(),
            vec![1, 2, 1]
        );
    }

    #[test]
    fn speech_no_turn_covers_takes_the_speaker_beside_it() {
        // The diarizer starts a turn late and ends it early.
        let speech = [span(0.0, 6.0)];
        let turns = [turn(1.0, 5.0, 2)];
        let chunks = plan(&speech, &turns, &silence(6.0), &rules());
        assert_eq!(
            chunks,
            vec![Chunk {
                range: span(0.0, 6.0),
                speaker: 2
            }]
        );
        // And a stretch with no turn near it stays unknown.
        let far = [span(50.0, 52.0)];
        let chunks = plan(&far, &turns, &silence(60.0), &rules());
        assert_eq!(chunks[0].speaker, 0);
    }

    #[test]
    fn long_speech_is_cut_into_chunks_no_longer_than_the_window() {
        let speech = [span(0.0, 70.0)];
        let mut pcm = vec![0.5f32; secs(70.0)];
        // A soft spot the first cut should find.
        for sample in &mut pcm[secs(24.0)..secs(24.2)] {
            *sample = 0.0;
        }
        let rules = rules();
        let chunks = plan(&speech, &[], &pcm, &rules);
        assert!(chunks.len() >= 3);
        assert!(chunks.iter().all(|c| c.range.len() <= rules.max_chunk));
        assert_eq!(chunks.first().unwrap().range.start, 0);
        assert_eq!(chunks.last().unwrap().range.end, secs(70.0));
        for pair in chunks.windows(2) {
            assert_eq!(pair[0].range.end, pair[1].range.start);
        }
        let first_cut = chunks[0].range.end;
        assert!((secs(24.0)..=secs(24.2)).contains(&first_cut));
    }

    #[test]
    fn one_speaker_talking_long_is_cut_at_a_pause() {
        let speech = [span(0.0, 20.0), span(20.8, 45.0)];
        let chunks = plan(&speech, &[turn(0.0, 45.0, 1)], &silence(46.0), &rules());
        assert_eq!(
            chunks,
            vec![
                Chunk {
                    range: span(0.0, 20.0),
                    speaker: 1
                },
                Chunk {
                    range: span(20.8, 45.0),
                    speaker: 1
                },
            ]
        );
    }

    #[test]
    fn padding_shares_the_silence_and_never_overlaps() {
        let speech = [span(1.0, 3.0), span(3.1, 5.0), span(9.0, 10.0)];
        let turns = [turn(1.0, 3.0, 1), turn(3.1, 5.0, 2), turn(9.0, 10.0, 1)];
        let rules = Rules::for_window(30.0);
        let chunks = plan(&speech, &turns, &silence(10.0), &rules);
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].range.start, secs(1.0) - rules.pad);
        // 0.1 s between the first two: each takes half.
        assert_eq!(chunks[0].range.end, secs(3.0) + secs(0.05));
        assert_eq!(chunks[1].range.start, secs(3.1) - secs(0.05));
        assert!(chunks[0].range.end <= chunks[1].range.start);
        // The last chunk ends with the audio: nothing to pad into.
        assert_eq!(chunks[2].range.end, secs(10.0));
    }

    #[test]
    fn blips_of_sound_are_not_transcribed() {
        let speech = [span(1.0, 1.1), span(5.0, 7.0)];
        let chunks = plan(&speech, &[], &silence(8.0), &rules());
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].range, span(5.0, 7.0));
    }
}
