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

    if (n as isize - m as isize).unsigned_abs() > threshold {
        return None;
    }

    if n == 0 {
        return if m <= threshold { Some(m) } else { None };
    }
    if m == 0 {
        return if n <= threshold { Some(n) } else { None };
    }

    let mut dp = vec![vec![threshold + 1; m + 1]; n + 1];

    for (i, row) in dp.iter_mut().enumerate().take(n + 1) {
        row[0] = i; // Cost if s2 is empty
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
            if (i as isize - j as isize).unsigned_abs() > threshold
                && dp[i - 1][j - 1] > threshold
                && dp[i - 1][j] > threshold
                && dp[i][j - 1] > threshold
            {
                // if all paths to here are already > threshold and this cell is outside the main diagonal band.
                // this check makes it slightly more aggressive for cells far from diagonal.
                dp[i][j] = threshold + 1; // Mark as too expensive
                if (i as isize - j as isize).unsigned_abs() <= threshold {
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

            if (i as isize - j as isize).unsigned_abs() <= threshold {
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
        0
    } else if target_word_len <= 9 {
        // e.g., "vitkål", "isflak", "holland", "pajform", "atlanten", "hackspett"
        2
    } else if target_word_len <= 14 {
        // e.g., "akvarium", "trådrulle", "president", "matador", "neandertalare", "karusell"
        2
    } else {
        // For very long words like "Julgransbelysning", "Pernilla Wahlgren"
        // Allow slightly more, but cap it to prevent overly lenient matches.
        // min(4, target_word_len / 4) could be one way. Let's use a fixed cap for now.
        3
    }
}

/// Checks if a guessed word is an acceptable match for a target word,
/// using Damerau-Levenshtein distance with an adaptive threshold.
/// Additionally requires that the first letter of the guess matches the first letter of the target.
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

    // 2. Check first letter requirement
    // Both target and guess must have the same first character
    let target_first_char = processed_target.chars().next();
    let guess_first_char = processed_guess.chars().next();

    if target_first_char != guess_first_char {
        return false;
    }

    // 3. Determine adaptive threshold
    let target_len = processed_target.chars().count(); // Use .chars().count() for correct Unicode length
    let threshold = determine_adaptive_threshold(target_len);

    // 4. Calculate Damerau-Levenshtein distance with the threshold
    // We pass the processed_target and processed_guess to the core algorithm.
    let distance_result =
        damerau_levenshtein_threshold(&processed_target, &processed_guess, threshold);

    distance_result.is_some()
}

#[cfg(test)]
mod tests_damerau_levenshtein {
    use super::*;

    #[test]
    fn test_exact_matches() {
        assert_eq!(
            damerau_levenshtein_threshold("hackspett", "hackspett", 2),
            Some(0)
        );
        assert_eq!(damerau_levenshtein_threshold("boj", "boj", 1), Some(0));
        assert_eq!(
            damerau_levenshtein_threshold("lilla spöket laban", "lilla spöket laban", 4),
            Some(0)
        );
    }

    #[test]
    fn test_very_short_words_threshold_0() {
        // Threshold 0 means only exact matches allowed
        assert_eq!(damerau_levenshtein_threshold("å", "å", 0), Some(0));
        assert_eq!(damerau_levenshtein_threshold("å", "ä", 0), None);
        assert_eq!(damerau_levenshtein_threshold("vi", "vi", 0), Some(0));
        assert_eq!(damerau_levenshtein_threshold("vi", "vo", 0), None);
        assert_eq!(damerau_levenshtein_threshold("ai", "ai", 0), Some(0));
        assert_eq!(damerau_levenshtein_threshold("ai", "bi", 0), None);
    }

    #[test]
    fn test_short_words_threshold_1() {
        // Test transposition
        assert_eq!(damerau_levenshtein_threshold("boj", "bjo", 1), Some(1));
        assert_eq!(damerau_levenshtein_threshold("bmw", "bwm", 1), Some(1));

        // Test deletion
        assert_eq!(damerau_levenshtein_threshold("boj", "bo", 1), Some(1));
        assert_eq!(damerau_levenshtein_threshold("järv", "jrv", 1), Some(1));

        // Test insertion
        assert_eq!(damerau_levenshtein_threshold("boj", "boja", 1), Some(1));

        // Test substitution
        assert_eq!(damerau_levenshtein_threshold("boj", "bok", 1), Some(1));
        assert_eq!(damerau_levenshtein_threshold("järv", "jarv", 1), Some(1));
        assert_eq!(damerau_levenshtein_threshold("järv", "jävr", 1), Some(1));
        assert_eq!(damerau_levenshtein_threshold("sms", "sm s", 1), Some(1));
        assert_eq!(damerau_levenshtein_threshold("sms", "sos", 1), Some(1));
        assert_eq!(damerau_levenshtein_threshold("atlas", "atla", 1), Some(1));

        // Test distance 2 should fail with threshold 1
        assert_eq!(damerau_levenshtein_threshold("boj", "boks", 1), None);
        assert_eq!(damerau_levenshtein_threshold("atlas", "alta", 1), None);
    }

    #[test]
    fn test_medium_words_threshold_2() {
        // Test distance 1
        assert_eq!(damerau_levenshtein_threshold("vitkål", "vitkå", 2), Some(1));
        assert_eq!(
            damerau_levenshtein_threshold("vitkål", "vitåkl", 2),
            Some(1)
        );
        assert_eq!(
            damerau_levenshtein_threshold("hackspett", "hackspet", 2),
            Some(1)
        );
        assert_eq!(
            damerau_levenshtein_threshold("hackspett", "hacskpett", 2),
            Some(1)
        );
        assert_eq!(
            damerau_levenshtein_threshold("hackspett", "hakspett", 2),
            Some(1)
        );

        // Test actual distances (corrected based on algorithm output)
        assert_eq!(
            damerau_levenshtein_threshold("vitkål", "vitkol", 2),
            Some(1)
        ); // Actually distance 1, not 2
        assert_eq!(
            damerau_levenshtein_threshold("hackspett", "hacskpet", 2),
            Some(2)
        );
        assert_eq!(
            damerau_levenshtein_threshold("hackspett", "hakspet", 2),
            Some(2)
        );

        // Test distance 3 should fail with threshold 2
        assert_eq!(damerau_levenshtein_threshold("vitkål", "vitski", 2), None);
        assert_eq!(
            damerau_levenshtein_threshold("hackspett", "haksppet", 2),
            None
        );
    }

    #[test]
    fn test_long_words_threshold_3() {
        // Test distance 1
        assert_eq!(
            damerau_levenshtein_threshold("akvarium", "akvarim", 3),
            Some(1)
        );
        assert_eq!(
            damerau_levenshtein_threshold("akvarium", "akvrium", 3),
            Some(1)
        );
        assert_eq!(
            damerau_levenshtein_threshold("akvarium", "akvairum", 3),
            Some(1)
        );
        assert_eq!(
            damerau_levenshtein_threshold("neandertalare", "neandertalre", 3),
            Some(1)
        );
        assert_eq!(
            damerau_levenshtein_threshold("neandertalare", "neandertaelare", 3),
            Some(1)
        );

        // Test distance 2
        assert_eq!(
            damerau_levenshtein_threshold("akvarium", "akvairm", 3),
            Some(2)
        );
        assert_eq!(
            damerau_levenshtein_threshold("neandertalare", "neandetalre", 3),
            Some(2)
        );

        // Test distance 3
        assert_eq!(
            damerau_levenshtein_threshold("neandertalare", "neandertaelr", 3),
            Some(3)
        );

        // Test distance 4 should fail with threshold 3
        assert_eq!(
            damerau_levenshtein_threshold("neandertalare", "naendertael", 3),
            None
        );
    }

    #[test]
    fn test_very_long_words_threshold_4() {
        let target = "julgransbelysning";

        // Test distance 1
        assert_eq!(
            damerau_levenshtein_threshold(target, "julgransbelysnin", 4),
            Some(1)
        );
        assert_eq!(
            damerau_levenshtein_threshold(target, "julgransbelysnnig", 4),
            Some(1)
        );

        // Test actual distances (corrected based on algorithm output)
        assert_eq!(
            damerau_levenshtein_threshold(target, "julgransbeysningg", 4),
            Some(2)
        );
        assert_eq!(
            damerau_levenshtein_threshold(target, "julgransbelysnn", 4),
            Some(2)
        ); // Actually distance 2
        assert_eq!(
            damerau_levenshtein_threshold(target, "julgransbeysnin", 4),
            Some(2)
        ); // Actually distance 2, not 3
        assert_eq!(
            damerau_levenshtein_threshold(target, "julgransbeysni", 4),
            Some(3)
        ); // Actually distance 3

        // Test distance 5 should fail with threshold 4
        assert_eq!(
            damerau_levenshtein_threshold(target, "julgarnsbeysn", 4),
            None
        );

        let target_pn = "pernilla wahlgren";
        assert_eq!(
            damerau_levenshtein_threshold(target_pn, "pernilla wahlgren", 4),
            Some(0)
        );
        assert_eq!(
            damerau_levenshtein_threshold(target_pn, "pernila wahlgren", 4),
            Some(1)
        );
        assert_eq!(
            damerau_levenshtein_threshold(target_pn, "pernil wahlgrenn", 4),
            Some(3)
        );
        assert_eq!(
            damerau_levenshtein_threshold(target_pn, "pernil wahlgrn", 4),
            Some(3)
        );
        assert_eq!(
            damerau_levenshtein_threshold(target_pn, "pernl wahlgrn", 4),
            Some(4)
        );
        assert_eq!(
            damerau_levenshtein_threshold(target_pn, "pernl wahlgr", 4),
            None
        );
    }

    #[test]
    fn test_multi_word_phrases() {
        assert_eq!(
            damerau_levenshtein_threshold("lilla spöket laban", "lilla spöket labn", 4),
            Some(1)
        );
        assert_eq!(
            damerau_levenshtein_threshold("lilla spöket laban", "lillaspöketlaban", 4),
            Some(2)
        );
        assert_eq!(
            damerau_levenshtein_threshold("coca-cola", "cocacola", 2),
            Some(1)
        );
        assert_eq!(
            damerau_levenshtein_threshold("coca-cola", "coka cola", 2),
            Some(2)
        );
        // Corrected based on actual algorithm output
        assert_eq!(
            damerau_levenshtein_threshold("rom och cola", "rom ochocla", 3),
            Some(2)
        );
    }

    #[test]
    fn test_empty_strings() {
        assert_eq!(damerau_levenshtein_threshold("", "", 0), Some(0));
        assert_eq!(damerau_levenshtein_threshold("", "guess", 5), Some(5));
        assert_eq!(damerau_levenshtein_threshold("target", "", 6), Some(6));
        assert_eq!(damerau_levenshtein_threshold("", "guess", 3), None); // distance 5 > threshold 3
    }

    #[test]
    fn test_threshold_functionality() {
        // Test that function returns None when distance exceeds threshold
        assert_eq!(
            damerau_levenshtein_threshold("test", "completely different", 5),
            None
        );
        assert_eq!(
            damerau_levenshtein_threshold("short", "verylongstring", 3),
            None
        );

        // Test early exit on length difference
        assert_eq!(damerau_levenshtein_threshold("a", "abcdef", 3), None); // length diff 5 > threshold 3
        assert_eq!(damerau_levenshtein_threshold("a", "abcd", 3), Some(3)); // length diff 3 <= threshold 3
    }
}
