/// Turns a GPUI action name such as `yard::ToggleZoom` into its readable
/// palette label, such as `yard: toggle zoom`.
pub fn humanize_action_name(name: &str) -> String {
    let mut parts = Vec::new();
    for segment in name.split("::") {
        let mut words = Vec::new();
        for piece in segment.split('_').filter(|piece| !piece.is_empty()) {
            words.push(split_camel_words(piece));
        }
        parts.push(words.join(" "));
    }
    parts.join(": ")
}

fn split_camel_words(piece: &str) -> String {
    let chars: Vec<char> = piece.chars().collect();
    let mut out = String::with_capacity(piece.len() + 4);
    for (index, &ch) in chars.iter().enumerate() {
        if ch.is_uppercase() {
            let prev_upper = index > 0 && chars[index - 1].is_uppercase();
            let next_lower = chars.get(index + 1).is_some_and(|next| next.is_lowercase());
            let next_upper = chars.get(index + 1).is_some_and(|next| next.is_uppercase());
            if index > 0 && (!prev_upper || next_lower) {
                out.push(' ');
            }
            if next_upper || (prev_upper && !next_lower) {
                out.push(ch);
            } else {
                out.extend(ch.to_lowercase());
            }
        } else {
            out.push(ch);
        }
    }
    out
}

/// Rank of a subsequence match: smaller is better.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct MatchRank {
    /// How many matched query characters did NOT land at a word start.
    non_word_starts: usize,
    /// `false` when the first matched character is at a word start, so
    /// leading word-start matches beat interior ones with equal counts.
    first_inside_word: bool,
    /// Number of skipped candidate characters between matched positions.
    gaps: usize,
}

struct LoweredCandidate {
    chars: Vec<char>,
    word_starts: Vec<bool>,
}

impl LoweredCandidate {
    fn new(text: &str) -> Self {
        let chars: Vec<char> = text.chars().flat_map(char::to_lowercase).collect();
        let word_starts = chars
            .iter()
            .enumerate()
            .map(|(index, ch)| {
                ch.is_alphanumeric() && (index == 0 || !chars[index - 1].is_alphanumeric())
            })
            .collect();
        Self { chars, word_starts }
    }
}

/// Matches `query` as a case-insensitive subsequence of `candidate` and
/// returns its best [`MatchRank`], or `None` when `query` is not a subsequence.
pub(super) fn match_query(query: &str, candidate: &str) -> Option<MatchRank> {
    let query_chars: Vec<char> = query.chars().flat_map(char::to_lowercase).collect();
    if query_chars.is_empty() {
        return Some(MatchRank {
            non_word_starts: 0,
            first_inside_word: false,
            gaps: 0,
        });
    }
    let lowered = LoweredCandidate::new(candidate);
    let m = query_chars.len();
    let n = lowered.chars.len();
    if m > n {
        return None;
    }

    let mut prev: Vec<Option<MatchRank>> = lowered
        .chars
        .iter()
        .zip(&lowered.word_starts)
        .map(|(&ch, &word_start)| {
            (ch == query_chars[0]).then_some(MatchRank {
                non_word_starts: usize::from(!word_start),
                first_inside_word: !word_start,
                gaps: 0,
            })
        })
        .collect();

    for (qi, &qch) in query_chars.iter().enumerate().skip(1) {
        let mut curr = vec![None; n];
        for (j, slot) in curr.iter_mut().enumerate().skip(qi) {
            if lowered.chars[j] != qch {
                continue;
            }
            let penalty = usize::from(!lowered.word_starts[j]);
            let mut best: Option<MatchRank> = None;
            for (prev_j, prev_rank) in prev[..j].iter().enumerate() {
                let Some(rank) = prev_rank else {
                    continue;
                };
                let candidate_rank = MatchRank {
                    non_word_starts: rank.non_word_starts + penalty,
                    first_inside_word: rank.first_inside_word,
                    gaps: rank.gaps + (j - prev_j - 1),
                };
                if best.is_none_or(|current| candidate_rank < current) {
                    best = Some(candidate_rank);
                }
            }
            *slot = best;
        }
        prev = curr;
    }

    prev.into_iter().flatten().min()
}

/// Returns the indices of `names` that match `query`, ordered by rank and then
/// by name.
pub(super) fn rank_candidates<S: AsRef<str>>(query: &str, names: &[S]) -> Vec<usize> {
    if query.is_empty() {
        let mut indices: Vec<usize> = (0..names.len()).collect();
        indices.sort_by(|&a, &b| names[a].as_ref().cmp(names[b].as_ref()));
        return indices;
    }
    let mut matched: Vec<(MatchRank, usize)> = names
        .iter()
        .enumerate()
        .filter_map(|(index, name)| match_query(query, name.as_ref()).map(|rank| (rank, index)))
        .collect();
    matched.sort_by(|(rank_a, index_a), (rank_b, index_b)| {
        rank_a
            .cmp(rank_b)
            .then_with(|| names[*index_a].as_ref().cmp(names[*index_b].as_ref()))
    });
    matched.into_iter().map(|(_, index)| index).collect()
}
