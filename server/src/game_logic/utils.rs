use std::cmp::min;
use std::collections::HashMap;

/// Computes the Damerau-Levenshtein distance between two strings,
/// returning `Some(distance)` if it's less than or equal to a given
/// threshold, or `None` otherwise.
///
/// This implementation includes an early exit mechanism.
///
/// # Arguments
///
/// * `s1`: The first string.
/// * `s2`: The second string.
/// * `threshold`: The maximum allowed distance.
///
/// # Returns
///
/// `Some(distance)` if the Damerau-Levenshtein distance is `<= threshold`,
/// `None` otherwise.
fn damerau_levenshtein_threshold(s1: &str, s2: &str, threshold: usize) -> Option<usize> {
    let s1_chars: Vec<char> = s1.chars().collect();
    let s2_chars: Vec<char> = s2.chars().collect();
    let n = s1_chars.len();
    let m = s2_chars.len();

    if (n as isize - m as isize).abs() as usize > threshold {
        return None;
    }

    if n == 0 {
        return if m <= threshold { Some(m) } else { None };
    }
    if m == 0 {
        return if n <= threshold { Some(n) } else { None };
    }

    let mut dp = vec![vec![threshold + 1; m + 1]; n + 1];

    for i in 0..=n {
        dp[i][0] = i; // Cost if s2 is empty
    }
    for j in 0..=m {
        dp[0][j] = j; // Cost if s1 is empty
    }
    // Correcting initialization if threshold is 0.
    // dp[i][0] should be i, and dp[0][j] should be j.
    // If i > threshold (and threshold is 0), then dp[i][0] > threshold, which is correct.
    // The previous loop is fine.

    let mut da: HashMap<char, usize> = HashMap::new();

    for i in 1..=n {
        let mut db = 0;
        let mut min_cost_in_relevant_band_this_row = threshold + 1;

        // Optimization: Define a window for j.
        // The j loop only needs to go from max(1, i - threshold) to min(m, i + threshold).
        // Cells outside this band will have a distance > threshold if dp[i][j] relies on |i-j| > threshold.
        // Let's keep the full j loop for simplicity here, as the `min_cost_in_relevant_band_this_row`
        // already provides substantial early exit. For extreme optimization, banding `j` is an option.

        for j in 1..=m {
            // If |i-j| > threshold, then this cell (i,j) must have cost > threshold
            // due to insertions/deletions alone. We can skip it or mark its cost high.
            // This is effectively handled by `min_cost_in_relevant_band_this_row` and capping `dp[i][j]`.
            if (i as isize - j as isize).abs() as usize > threshold
                && dp[i - 1][j - 1] > threshold
                && dp[i - 1][j] > threshold
                && dp[i][j - 1] > threshold
            {
                // if all paths to here are already > threshold and this cell is outside the main diagonal band.
                // this check makes it slightly more aggressive for cells far from diagonal.
                dp[i][j] = threshold + 1; // Mark as too expensive
                if (i as isize - j as isize).abs() as usize <= threshold {
                    // check if this cell itself is in the band
                    min_cost_in_relevant_band_this_row =
                        min(min_cost_in_relevant_band_this_row, dp[i][j]);
                }
                continue;
            }

            let k = *da.get(&s2_chars[j - 1]).unwrap_or(&0);
            let l = db;

            let cost = if s1_chars[i - 1] == s2_chars[j - 1] {
                0
            } else {
                1
            };
            if cost == 0 {
                db = j;
            }

            let substitution = dp[i - 1][j - 1].saturating_add(cost);
            let insertion = dp[i][j - 1].saturating_add(1);
            let deletion = dp[i - 1][j].saturating_add(1);

            let mut current_dp_val = min(substitution, min(insertion, deletion));

            if k > 0 && l > 0 {
                let prev_cost_trans = dp[k - 1][l - 1];
                if prev_cost_trans < threshold + 1 {
                    let s1_intermediate_cost = i - k - 1;
                    let s2_intermediate_cost = j - l - 1;
                    let transposition_op_cost = 1;

                    let transposition_cost = prev_cost_trans
                        .saturating_add(s1_intermediate_cost)
                        .saturating_add(transposition_op_cost)
                        .saturating_add(s2_intermediate_cost);

                    current_dp_val = min(current_dp_val, transposition_cost);
                }
            }

            dp[i][j] = min(current_dp_val, threshold + 1);

            if (i as isize - j as isize).abs() as usize <= threshold {
                min_cost_in_relevant_band_this_row =
                    min(min_cost_in_relevant_band_this_row, dp[i][j]);
            }
        }
        da.insert(s1_chars[i - 1], i);

        if min_cost_in_relevant_band_this_row > threshold && i > threshold {
            // Added i > threshold to ensure base cases are considered
            // If after processing row i, the minimum cost within the relevant diagonal band
            // is already greater than the threshold, then it's impossible for dp[n][m]
            // to be <= threshold.
            // The `i > threshold` condition prevents premature exit if, for example, threshold is 0
            // and we are at i=1, j=1 with a mismatch. dp[1][0]=1, dp[0][1]=1. min_cost might be 1.
            // Let's refine the early exit:
            // The critical point for early exit is when any path to the end (n,m) must exceed threshold.
            // If min_cost_in_relevant_band_this_row > threshold, it means that dp[i,j] > threshold for all j
            // such that |i-j| <= threshold.
            // From (i,j), we need at least (n-i) deletions and (m-j) insertions.
            // This existing early exit logic based on `min_cost_in_relevant_band_this_row` is generally sound.
            return None;
        }
    }

    let final_dist = dp[n][m];
    if final_dist <= threshold {
        Some(final_dist)
    } else {
        None
    }
}

/// Determines an adaptive Damerau-Levenshtein threshold based on the target word's length.
///
/// # Arguments
///
/// * `target_word_len`: The length of the target word (after any preprocessing like lowercasing).
///
/// # Returns
///
/// An appropriate threshold `usize`.
fn determine_adaptive_threshold(target_word_len: usize) -> usize {
    if target_word_len <= 2 {
        // e.g., "å", "vi", "io", "AI"
        0
    } else if target_word_len <= 5 {
        // e.g., "boj", "järv", "kyss", "atlas", "blöja"
        1
    } else if target_word_len <= 9 {
        // e.g., "vitkål", "isflak", "holland", "pajform", "atlanten", "hackspett"
        2
    } else if target_word_len <= 14 {
        // e.g., "akvarium", "trådrulle", "president", "matador", "neandertalare", "karusell"
        3
    } else {
        // For very long words like "Julgransbelysning", "Pernilla Wahlgren"
        // Allow slightly more, but cap it to prevent overly lenient matches.
        // min(4, target_word_len / 4) could be one way. Let's use a fixed cap for now.
        4
    }
}

/// Checks if a guessed word is an acceptable match for a target word,
/// using Damerau-Levenshtein distance with an adaptive threshold.
///
/// # Arguments
///
/// * `target_word`: The correct word or phrase.
/// * `guessed_word`: The word or phrase guessed by the player.
///
/// # Returns
///
/// `true` if the guess is considered acceptable, `false` otherwise.
pub fn is_guess_acceptable(target_word: &str, guessed_word: &str) -> bool {
    // 1. Preprocessing
    let processed_target = target_word.to_lowercase();
    let processed_guess = guessed_word.trim().to_lowercase();

    // Handle empty guess: usually not acceptable unless target is also empty.
    if processed_guess.is_empty() {
        return processed_target.is_empty();
    }

    // Direct match optimization
    if processed_target == processed_guess {
        return true;
    }

    // 2. Determine adaptive threshold
    let target_len = processed_target.chars().count(); // Use .chars().count() for correct Unicode length
    let threshold = determine_adaptive_threshold(target_len);

    // 3. Calculate Damerau-Levenshtein distance with the threshold
    // We pass the processed_target and processed_guess to the core algorithm.
    let distance_result =
        damerau_levenshtein_threshold(&processed_target, &processed_guess, threshold);

    distance_result.is_some()
}

#[cfg(test)]
mod tests_acceptable_guess {
    use super::*;

    #[test]
    fn test_exact_matches() {
        assert!(is_guess_acceptable("Hackspett", "Hackspett"));
        assert!(is_guess_acceptable("Hackspett", "hackspett")); // Case insensitivity
        assert!(is_guess_acceptable("boj", " boj ")); // Trimming
        assert!(is_guess_acceptable(
            "Lilla spöket Laban",
            "lilla spöket laban"
        ));
    }

    #[test]
    fn test_very_short_words() {
        assert!(is_guess_acceptable("å", "å"));
        assert!(!is_guess_acceptable("å", "ä")); // Threshold 0 for len 1
        assert!(is_guess_acceptable("vi", "vi"));
        assert!(is_guess_acceptable("vi", "Vi")); // after lowercase, it's "vi" vs "vi", threshold 0, ok
        assert!(is_guess_acceptable("AI", "ai"));
        assert!(!is_guess_acceptable("AI", "bi")); // "ai" vs "bi", dist 1, len 2 -> thresh 0. Not acceptable.
        assert!(is_guess_acceptable("bmw", "bwm")); // bmw (3) -> thresh 1. "bwm" is 1 transposition. OK.
    }

    #[test]
    fn test_short_words_threshold_1() {
        assert!(is_guess_acceptable("boj", "bjo")); // Transposition, dist 1
        assert!(is_guess_acceptable("boj", "bo")); // Deletion, dist 1
        assert!(is_guess_acceptable("boj", "boja")); // Insertion, dist 1
        assert!(is_guess_acceptable("boj", "bok")); // Substitution, dist 1
        assert!(!is_guess_acceptable("boj", "boks")); // Dist 2
        assert!(is_guess_acceptable("Järv", "jarv")); // Subst ä->a, dist 1
        assert!(is_guess_acceptable("Järv", "jävr")); // Transp, dist 1
        assert!(is_guess_acceptable("järv", "jrv")); // "järv" (len 4, thresh 1), dist("järv", "jrv") is 1 (delete ä). Acceptable.
        assert!(is_guess_acceptable("SMS", "sm s")); // After processing: "sms" vs "sm s". dist 1. Acceptable.
        assert!(is_guess_acceptable("SMS", "sos")); // "sms" vs "sos", dist 1. Acceptable.
        assert!(is_guess_acceptable("Atlas", "atlas"));
        assert!(is_guess_acceptable("Atlas", "atla")); // dist 1. Acceptable.
        assert!(!is_guess_acceptable("Atlas", "alta")); // "atlas" vs "alta" -> "atlaS" vs "alta ", "atlas" vs "alta". t<->l transposition = 1. cost of s = 1. Total 2. Not acceptable.
        // "atlas" (5) vs "alta" (4). dist 2. (transpose t,l; delete s). Threshold for len 5 is 1. Not acceptable.
        // Correct: atlas -> alta (del s, sub l->t), dist 2.
        // atlas -> (transpose tl) -> alsas -> (sub s->t) -> altas -> (del s) -> alta.
        // D("atlas", "alta") is 2. (substitute 'l' for 't', delete 's'). Threshold 1. Not accepted. Correct.
    }

    #[test]
    fn test_medium_words_threshold_2() {
        assert!(is_guess_acceptable("Vitkål", "vitkol")); // ö->o, å->a. Dist 2. ("vitkål" len 6 -> thresh 2)
        assert!(is_guess_acceptable("Vitkål", "vitkå")); // Dist 1
        assert!(is_guess_acceptable("Vitkål", "vitåkl")); // Transpose kå -> åk. Dist 1.
        assert!(!is_guess_acceptable("Vitkål", "vitski")); // Dist 3
        assert!(is_guess_acceptable("Hackspett", "hackspet")); // dist 1
        assert!(is_guess_acceptable("Hackspett", "hacskpett")); // dist 1 (transpose ck)
        assert!(is_guess_acceptable("Hackspett", "hakspett")); // dist 1 (delete c)
        assert!(is_guess_acceptable("Hackspett", "hacskpet")); // dist 2 (transpose ck, delete t)
        assert!(is_guess_acceptable("Hackspett", "hakspet")); // dist 2. Accepted.
        assert!(!is_guess_acceptable("Hackspett", "haksppet")); // dist 3. Not accepted.
        assert!(!is_guess_acceptable("Hackspett", "haksppet")); // dist 3. Not accepted.
    }

    #[test]
    fn test_long_words_threshold_3() {
        assert!(is_guess_acceptable("Akvarium", "akvarim")); // dist 1 (u->i). ("akvarium" len 8 -> moved to medium, thresh 2)
        // Oh, `Akvarium` is 8 chars, so falls into 6-9 range, threshold 2.
        assert!(is_guess_acceptable("Akvarium", "akvarim")); // dist("akvarium", "akvarim") = 1. Accepted.
        assert!(is_guess_acceptable("Akvarium", "akvrium")); // dist 1 (del a). Accepted.
        assert!(is_guess_acceptable("Akvarium", "akvairum")); // dist 1 (transpose ri). Accepted.
        assert!(is_guess_acceptable("Akvarium", "akvairm")); // dist 2 (transpose ri, sub u->m). Accepted.
        assert!(!is_guess_acceptable("Akvarium", "akvirm")); // dist 3. ("akvarium" len 8 -> thresh 2). Not accepted.

        // Let's test "Neandertalare" (13 chars, threshold 3)
        assert!(is_guess_acceptable("Neandertalare", "neandertalre")); // dist 1
        assert!(is_guess_acceptable("Neandertalare", "neandertaelare")); // dist 1 (trans)
        assert!(is_guess_acceptable("Neandertalare", "neandetalre")); // dist 2 (del r, del a)
        assert!(is_guess_acceptable("Neandertalare", "neandertaelr")); // dist 3 (trans al, del a, sub e->r)
        assert!(!is_guess_acceptable("Neandertalare", "naendertael")); // dist 4. Not accepted.
    }

    #[test]
    fn test_very_long_words_threshold_4() {
        let target = "Julgransbelysning"; // 18 chars, threshold 4
        assert!(is_guess_acceptable(target, "julgransbelysnin")); // dist 1
        assert!(is_guess_acceptable(target, "julgransbelysnnig")); // dist 1 (trans)
        assert!(is_guess_acceptable(target, "julgransbelysnn")); // dist 3
        assert!(is_guess_acceptable(target, "julgransbeysningg")); // dist 2 (l->y, extra g)
        // "julgransbelysning" vs "julgransbeysningg"
        // ...belysning vs ...beysningg
        // l->y (1), g insert (1) = 2. Accepted.
        assert!(is_guess_acceptable(target, "julgransbeysnin")); // dist 3 (l->y, del s, del g). Accepted.
        assert!(is_guess_acceptable(target, "julgransbeysni")); // dist 4 (l->y, del s, del n, del g). Accepted.
        assert!(!is_guess_acceptable(target, "julgarnsbeysn")); // dist 5. Not accepted.

        let target_pn = "Pernilla Wahlgren"; // 17 chars (incl space), threshold 4
        assert!(is_guess_acceptable(target_pn, "pernilla wahlgren"));
        assert!(is_guess_acceptable(target_pn, "pernila wahlgren")); // dist 1
        assert!(is_guess_acceptable(target_pn, "pernil wahlgrenn")); // dist 2
        assert!(is_guess_acceptable(target_pn, "pernil wahlgrn")); // dist 3
        assert!(is_guess_acceptable(target_pn, "pernl wahlgrn")); // dist 4
        assert!(!is_guess_acceptable(target_pn, "pernl wahlgr")); // dist 5
    }

    #[test]
    fn test_multi_word_phrases() {
        assert!(is_guess_acceptable(
            "Lilla spöket Laban",
            "lilla spöket labn"
        )); // dist 1
        assert!(is_guess_acceptable(
            "Lilla spöket Laban",
            "lillaspöketlaban"
        )); // dist 2 (missing spaces) len 19 (incl spaces) -> thresh 4. Accepted.
        assert!(is_guess_acceptable("Coca-Cola", "cocacola")); // dist 1 (missing hyphen). "coca-cola" len 9 -> thresh 2. Accepted.
        assert!(is_guess_acceptable("Coca-Cola", "coka cola")); // dist 2 (c->k, hyphen->space). Accepted.
        assert!(is_guess_acceptable("Rom och cola", "rom ochocla")); // dist 1. "rom och cola" len 12 -> thresh 3. Accepted.
    }

    #[test]
    fn test_empty_guess() {
        assert!(!is_guess_acceptable("target", ""));
        assert!(is_guess_acceptable("", ""));
        assert!(!is_guess_acceptable("", "guess"));
    }

    #[test]
    fn test_atlas_alta() {
        // target: atlas (len 5 -> threshold 1)
        // guess: alta
        // damerau_levenshtein_threshold("atlas", "alta", 1) -> None because distance is 2
        assert!(!is_guess_acceptable("Atlas", "alta"));
    }
}
