use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post, put, delete},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;
use tower_http::cors::{CorsLayer, Any};

use super::ws::{events_handler, AppState};
use crate::db::models::*;
use crate::provisioning::vm_provider::{VmProvider, VmResources};

/// Maximum reply depth for thread messages (user-to-agent or agent-to-agent in threads).
/// Prevents infinite reply chains in regular thread conversations.
const MAX_THREAD_REPLY_DEPTH: i32 = 5;

/// Safety ceiling for agent-to-agent DM conversations.
/// Default available models for agent selection.
const DEFAULT_MODELS: &[&str] = &[
    "nemotron-3-super:cloud",
    "minimax-m2.5:cloud",
    "minimax-m2:cloud",
    "glm-5:cloud",
    "kimi-k2-thinking:cloud",
    "kimi-k2.5:cloud",
    "qwen3-coder:480b-cloud",
    "devstral-2:123b-cloud",
    "deepseek-v3.2:cloud",
    "minimax-m2.1:cloud",
    "glm-4.7:cloud",
];

use crate::provisioning::cloudinit::{CloudInitArgs, render_cloud_init};

/// Remove a tag from text even when streaming tokenization has inserted spaces
/// within the tag (e.g. "HE ARTBEAT_OK" or "END _CONVERSATION").  Matches the
/// tag characters in order while allowing arbitrary whitespace between them,
/// optionally wrapped in brackets.  Returns the text with all such occurrences removed.
fn strip_fragmented_tag(text: &str, tag: &str) -> String {
    // Build a char-sequence to match (the tag letters in order)
    let tag_chars: Vec<char> = tag.chars().collect();
    let text_chars: Vec<char> = text.chars().collect();
    let mut result = String::with_capacity(text.len());
    let mut i = 0;
    while i < text_chars.len() {
        // Try to match the tag (with optional leading '[') starting at position i
        let has_bracket = text_chars[i] == '[';
        let start = if has_bracket { i + 1 } else { i };
        let mut ti = 0; // index into tag_chars
        let mut j = start;
        while ti < tag_chars.len() && j < text_chars.len() {
            if text_chars[j] == tag_chars[ti] {
                ti += 1;
                j += 1;
            } else if text_chars[j].is_whitespace() {
                j += 1; // skip whitespace between tag chars
            } else {
                break;
            }
        }
        if ti == tag_chars.len() {
            // Matched the full tag — skip optional trailing ']'
            if j < text_chars.len() && text_chars[j] == ']' {
                j += 1;
            }
            // Only accept if the match started at a word boundary (not mid-word)
            let at_boundary = i == 0 || !text_chars[i - 1].is_alphanumeric();
            if at_boundary {
                i = j; // skip over the matched tag
                continue;
            }
        }
        result.push(text_chars[i]);
        i += 1;
    }
    result
}

/// Strip known system tags and model artifacts from agent responses.
/// Returns (cleaned_text, had_end_conversation).
/// Remove spurious single newlines inserted by OpenClaw's streaming token assembly.
/// Preserves structurally significant newlines: paragraph breaks (\n\n), code blocks,
/// list items, headings, blockquotes, tables, and CommonMark hard breaks.
fn clean_spurious_newlines(text: &str) -> String {
    let lines: Vec<&str> = text.split('\n').collect();
    if lines.len() <= 1 {
        return text.to_string();
    }

    let mut result = String::with_capacity(text.len());
    let mut in_code_block = false;

    for i in 0..lines.len() {
        let line = lines[i];

        // Track code fence state
        if line.trim_start().starts_with("```") {
            in_code_block = !in_code_block;
        }

        result.push_str(line);

        // Don't add separator after the last line
        if i == lines.len() - 1 {
            break;
        }

        let next_line = lines[i + 1];
        let next_trimmed = next_line.trim_start();

        // Keep this newline if it's structurally significant; otherwise replace with space
        let keep_newline =
            in_code_block
            || line.is_empty()                              // part of paragraph break
            || next_line.is_empty()                         // part of paragraph break
            || next_trimmed.starts_with("```")              // entering/leaving code block
            || line.trim_start().starts_with("```")         // just entered/left code block
            || next_trimmed.starts_with("# ")
            || next_trimmed.starts_with("## ")
            || next_trimmed.starts_with("### ")
            || next_trimmed.starts_with("#### ")
            || next_trimmed.starts_with("#####")
            || line.trim_start().starts_with('#')            // prev was heading
            || next_trimmed.starts_with("- ")
            || next_trimmed.starts_with("* ")
            || next_trimmed.starts_with("+ ")
            || is_ordered_list_marker(next_trimmed)
            || next_trimmed.starts_with("> ")                // blockquote
            || next_trimmed.starts_with('|')                 // table
            || line.ends_with("  ")                          // hard line break (two trailing spaces)
            || line.ends_with('\\')                          // hard line break (backslash)
            ;

        if keep_newline {
            result.push('\n');
        } else {
            result.push(' ');
        }
    }

    collapse_spaces(&result)
}

/// Check if a line starts with an ordered list marker like "1. ", "12. ", etc.
fn is_ordered_list_marker(s: &str) -> bool {
    let bytes = s.as_bytes();
    let mut i = 0;
    if i >= bytes.len() || !bytes[i].is_ascii_digit() {
        return false;
    }
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    i + 1 < bytes.len() && bytes[i] == b'.' && bytes[i + 1] == b' '
}

/// Collapse runs of multiple spaces into a single space.
fn collapse_spaces(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut prev_space = false;
    for ch in s.chars() {
        if ch == ' ' {
            if !prev_space {
                result.push(ch);
            }
            prev_space = true;
        } else {
            prev_space = false;
            result.push(ch);
        }
    }
    result
}

/// Fix spacing artifacts from streaming token assembly:
/// - Remove space before sentence punctuation (. , ; : ! ?)
/// - Remove space before apostrophes in contractions (I've, don't, it's)
/// Does NOT remove space before opening quotes (e.g., He said 'hello').
fn fix_punctuation_spacing(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let len = chars.len();
    let mut result = String::with_capacity(s.len());
    let mut i = 0;

    while i < len {
        if chars[i] == ' ' {
            // Look ahead past any consecutive spaces
            let mut j = i + 1;
            while j < len && chars[j] == ' ' {
                j += 1;
            }
            if j < len {
                let next = chars[j];
                // Space before sentence/clause punctuation — collapse
                if matches!(next, '.' | ',' | ';' | ':' | '!' | '?') {
                    i = j;
                    continue;
                }
                // Space before apostrophe in a contraction (letter + space + ' + lowercase)
                if next == '\'' && j + 1 < len && chars[j + 1].is_lowercase() {
                    if i > 0 && chars[i - 1].is_alphabetic() {
                        i = j;
                        continue;
                    }
                }
            }
        }
        result.push(chars[i]);
        i += 1;
    }
    result
}

/// Fix Markdown bold markers broken by streaming: "** word" → "**word", "word **" → "word**".
/// The tokenizer sometimes emits "**" and the word as separate tokens with a space between.
fn strip_markdown_bold(s: &str) -> String {
    // Remove ** bold markers entirely, ensuring a space exists where needed.
    // "the**$150 profit**" → "the $150 profit"
    let chars: Vec<char> = s.chars().collect();
    let len = chars.len();
    let mut result = String::with_capacity(s.len());
    let mut i = 0;

    while i < len {
        if i + 1 < len && chars[i] == '*' && chars[i + 1] == '*' {
            // Skip the **
            i += 2;
            // If removal would glue two non-space chars together, insert a space.
            // e.g. "the**$150" → prev='e', next='$' → insert space
            // But "** word" → prev=start/space, next=' '/alpha → no extra space
            if !result.is_empty() {
                let prev = result.chars().last().unwrap();
                let next = if i < len { chars[i] } else { ' ' };
                if !prev.is_whitespace() && !next.is_whitespace() && prev != '\n' && next != '\n' {
                    result.push(' ');
                }
            }
            continue;
        }
        result.push(chars[i]);
        i += 1;
    }
    result
}

/// Fix mid-word spaces from streaming token assembly.
/// The tokenizer sometimes splits words across tokens, producing "Under stood",
/// "H ire", etc.  Two-tier approach:
///  - Tier 0: hardcoded known patterns (fast, exact, zero false-positives).
///  - Tier 1: single uppercase letter (not A/I) + space + lowercase 3+ chars.
///  - Tier 2: short capitalized fragment (2-4 chars, not a common English word)
///            + space + lowercase 3+ chars.
fn fix_broken_words(s: &str) -> String {
    // Tier 0 — exact patterns confirmed in production (longest first).
    const FIXES: &[(&str, &str)] = &[
        ("Under stood", "Understood"),
        ("under stood", "understood"),
        ("H ire", "Hire"),
        ("h ire", "hire"),
        ("Ac knowledg", "Acknowledg"),
        ("ac knowledg", "acknowledg"),
        ("Acknowled ged", "Acknowledged"),
        ("acknowled ged", "acknowledged"),
        ("Con firmed", "Confirmed"),
        ("con firmed", "confirmed"),
        ("Appro ving", "Approving"),
        ("appro ving", "approving"),
        ("Ap proved", "Approved"),
        ("ap proved", "approved"),
        ("App reciate", "Appreciate"),
        ("app reciate", "appreciate"),
        ("Re ceived", "Received"),
        ("re ceived", "received"),
        ("Pro ceeding", "Proceeding"),
        ("pro ceeding", "proceeding"),
        ("Ex cellent", "Excellent"),
        ("ex cellent", "excellent"),
        ("Cer tainly", "Certainly"),
        ("cer tainly", "certainly"),
        ("Im mediately", "Immediately"),
        ("im mediately", "immediately"),
        ("Document ed", "Documented"),
        ("document ed", "documented"),
        ("Not ed", "Noted"),
        ("not ed", "noted"),
        ("E lena", "Elena"),
        ("S andbox", "Sandbox"),
        ("s andbox", "sandbox"),
        ("Proceed ing", "Proceeding"),
        ("proceed ing", "proceeding"),
        ("Sit uation", "Situation"),
        ("sit uation", "situation"),
        ("Histor ical", "Historical"),
        ("histor ical", "historical"),
        ("Escal ated", "Escalated"),
        ("escal ated", "escalated"),
        ("VM s ", "VMs "),
    ];
    let mut result = s.to_string();
    for (broken, fixed) in FIXES {
        result = result.replace(broken, fixed);
    }

    // Tier 1 + 2 — heuristic merging for novel broken words.
    result = fix_broken_words_heuristic(&result);
    result
}

/// Heuristic pass: merge [UpperFragment] [lowercase continuation] when the
/// fragment is clearly a streaming-split artifact, not a real word.
/// Only Tier 1 (single uppercase letter, not A/I) is active.
/// Tier 2 (2-4 char fragments) is disabled — too many false positives
/// with common words (Grow capital, Add these, Once you, Risk framework, etc.).
/// Use Tier 0 hardcoded patterns in `fix_broken_words()` for known multi-char breaks.
fn fix_broken_words_heuristic(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let len = chars.len();
    let mut result = String::with_capacity(s.len());
    let mut i = 0;

    while i < len {
        // Only act at word boundaries
        let at_boundary = i == 0
            || chars[i - 1].is_whitespace()
            || chars[i - 1].is_ascii_punctuation();

        if at_boundary && chars[i].is_ascii_uppercase() {
            // Scan the capitalized fragment: one uppercase + zero or more lowercase
            let frag_start = i;
            let mut j = i + 1;
            while j < len && chars[j].is_ascii_lowercase() {
                j += 1;
            }
            let frag_len = j - frag_start; // 1 = single letter, 2-4 = short word

            // Check: fragment followed by exactly one space, then lowercase 3+ chars
            if j < len && chars[j] == ' '
                && j + 1 < len && chars[j + 1].is_ascii_lowercase()
            {
                let cont_start = j + 1;
                let mut k = cont_start;
                while k < len && chars[k].is_ascii_lowercase() {
                    k += 1;
                }
                let cont_len = k - cont_start;

                if cont_len >= 3 {
                    let should_merge = if frag_len == 1 {
                        // Tier 1: single uppercase letter — merge unless A or I
                        chars[frag_start] != 'A' && chars[frag_start] != 'I'
                    } else {
                        // Tier 2+ disabled: 2+ char fragments left to Tier 0 hardcoded list
                        false
                    };

                    if should_merge {
                        // Emit fragment chars, skip the space
                        for idx in frag_start..j {
                            result.push(chars[idx]);
                        }
                        i = j + 1; // skip the space, continue with lowercase part
                        continue;
                    }
                }
            }
        }

        result.push(chars[i]);
        i += 1;
    }

    result
}

/// Remove lines that are pure narration filler (defense-in-depth for model ignoring instructions).
/// Only strips lines where the ENTIRE content is a narration phrase + optional trailing punctuation.
fn strip_narration_lines(text: &str) -> String {
    const NARRATION_PREFIXES: &[&str] = &[
        "let me check", "let me look", "let me review", "let me send",
        "let me see", "let me find", "let me get", "let me pull", "let me search",
        "let me read", "let me also", "let me start", "let me create",
        "let me set", "let me prepare", "let me reach out",
        "now let me", "now i'll", "now i will",
        "i'll check", "i'll review", "i'll look", "i'll send",
        "i'll search", "i'll find", "i'll get",
        "i'll read", "i'll prepare", "i'll create", "i'll start", "i'll set up",
        "i will check", "i will review", "i will look",
        "i will send", "i will read",
        "sending now", "checking now", "looking now", "reviewing now", "searching now",
    ];

    // Short filler phrases that can precede narration (e.g. "Good. Let me check...")
    const FILLER_PREFIXES: &[&str] = &[
        "good.", "ok.", "okay.", "sure.", "understood.", "alright.",
        "right.", "great.", "perfect.", "absolutely.", "done.", "noted.",
    ];

    /// Check whether `remainder` (text after narration prefix) contains markers
    /// indicating the line is conversational (addressing someone) rather than
    /// pure internal narration.  Lines with these markers are kept.
    fn has_direct_address(remainder: &str) -> bool {
        remainder.starts_with("you ") || remainder.starts_with("your ")
            || remainder.contains(" you ") || remainder.contains(" your ")
            || remainder.contains(" you.") || remainder.contains(" you,")
            || remainder.ends_with(" you")
            || remainder.contains('?')
    }

    /// Returns true if the line (lowercased) matches a narration prefix and
    /// should be stripped.
    fn is_narration(lower: &str, prefixes: &[&str]) -> bool {
        prefixes.iter().any(|prefix| {
            if !lower.starts_with(prefix) {
                return false;
            }
            let remainder = lower[prefix.len()..].trim();
            // Tier 1: pure narration — prefix + optional punctuation
            if remainder.is_empty() || remainder.chars().all(|c| ".,;:!?…".contains(c)) {
                return true;
            }
            // Tier 2: extended narration — prefix + SHORT content that doesn't address anyone.
            // Long remainders (8+ words) are likely real conversational content,
            // not internal narration (e.g. "building my team immediately with two managers...").
            let word_count = remainder.split_whitespace().count();
            word_count < 12 && !has_direct_address(remainder)
        })
    }

    let lines: Vec<&str> = text.split('\n').collect();
    let mut result: Vec<&str> = Vec::with_capacity(lines.len());

    for line in &lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            result.push(line);
            continue;
        }
        let lower = trimmed.to_lowercase();

        // Check the line as-is against narration prefixes
        if is_narration(&lower, NARRATION_PREFIXES) {
            continue;
        }

        // Also check after stripping a leading filler phrase
        // (e.g. "Good. Let me check if the trade credentials...")
        let filler_narration = FILLER_PREFIXES.iter().any(|filler| {
            if let Some(after) = lower.strip_prefix(filler) {
                let after = after.trim_start();
                !after.is_empty() && is_narration(after, NARRATION_PREFIXES)
            } else {
                false
            }
        });

        if !filler_narration {
            result.push(line);
        }
    }

    result.join("\n")
}

/// Detect and collapse duplicate content blocks in model output.
/// glm-5 occasionally emits the same message body twice in a single response.
fn dedup_content_blocks(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.len() < 40 {
        return text.to_string(); // too short to have meaningful duplicates
    }

    // Strategy 1: check if the text is two identical halves split at a \n\n boundary
    // Scan \n\n positions in the middle third of the text
    let len = trimmed.len();
    let scan_start = len / 3;
    let scan_end = (2 * len) / 3;
    let bytes = trimmed.as_bytes();
    let mut pos = scan_start;
    while pos + 1 < scan_end {
        if bytes[pos] == b'\n' && bytes[pos + 1] == b'\n' {
            let first = trimmed[..pos].trim();
            let second = trimmed[pos + 2..].trim();
            if first == second && !first.is_empty() {
                return first.to_string();
            }
        }
        pos += 1;
    }

    // Strategy 2: remove consecutive duplicate paragraphs
    let paragraphs: Vec<&str> = trimmed.split("\n\n").collect();
    if paragraphs.len() <= 1 {
        return text.to_string();
    }
    let mut deduped: Vec<&str> = vec![paragraphs[0]];
    for i in 1..paragraphs.len() {
        if paragraphs[i].trim() != paragraphs[i - 1].trim() || paragraphs[i].trim().is_empty() {
            deduped.push(paragraphs[i]);
        }
    }
    if deduped.len() < paragraphs.len() {
        return deduped.join("\n\n");
    }

    // Strategy 3: fuzzy paragraph dedup — detect revised/paraphrased content.
    // When a model uses tools mid-response, it often drafts text before tools,
    // narrates "Let me check...", then drafts a revised version after tools.
    // The later version is more informed and should be kept.
    if paragraphs.len() >= 3 {
        let mut remove: std::collections::HashSet<usize> = std::collections::HashSet::new();
        for i in 0..paragraphs.len() {
            if remove.contains(&i) || paragraphs[i].trim().len() < 20 { continue; }
            for j in (i + 1)..paragraphs.len() {
                if remove.contains(&j) || paragraphs[j].trim().len() < 20 { continue; }
                let overlap = word_overlap_ratio(paragraphs[i].trim(), paragraphs[j].trim());
                if overlap > 0.6 {
                    // Later paragraph is a revision — drop earlier + narration between
                    remove.insert(i);
                    for k in (i + 1)..j {
                        let lower = paragraphs[k].trim().to_lowercase();
                        let is_narration = lower.starts_with("let me ")
                            || lower.starts_with("i'll ")
                            || lower.starts_with("i will ")
                            || lower.starts_with("now let me")
                            || lower.starts_with("checking ")
                            || lower.starts_with("looking ");
                        if is_narration || paragraphs[k].trim().len() < 60 {
                            remove.insert(k);
                        }
                    }
                    break; // para i handled, move on
                }
            }
        }
        if !remove.is_empty() {
            let kept: Vec<&str> = paragraphs.iter().enumerate()
                .filter(|(idx, _)| !remove.contains(idx))
                .map(|(_, p)| *p)
                .collect();
            return kept.join("\n\n");
        }
    }

    text.to_string()
}

/// Compute word-overlap ratio between two texts.
/// Returns |A ∩ B| / min(|A|, |B|) on sets of words (>2 chars, punctuation-trimmed).
/// Used to detect when a DM sender repeats/rephrases their own earlier message.
pub(crate) fn word_overlap_ratio(a: &str, b: &str) -> f64 {
    let to_words = |s: &str| -> std::collections::HashSet<String> {
        s.split_whitespace()
            .map(|w| w.trim_matches(|c: char| c.is_ascii_punctuation()).to_lowercase())
            .filter(|w| w.len() > 2)
            .collect()
    };
    let words_a = to_words(a);
    let words_b = to_words(b);
    if words_a.is_empty() || words_b.is_empty() {
        return 0.0;
    }
    let intersection = words_a.intersection(&words_b).count();
    let smaller = words_a.len().min(words_b.len());
    intersection as f64 / smaller as f64
}

/// Decode common HTML entities that glm-5 occasionally outputs.
/// Also handles the partial variant where &#NNN; loses its &# prefix.
fn decode_html_entities(s: &str) -> String {
    let mut result = s.to_string();
    // Numeric HTML entities (3-digit padded first, then short forms)
    result = result.replace("&#039;", "'");
    result = result.replace("&#39;", "'");
    result = result.replace("&#034;", "\"");
    result = result.replace("&#34;", "\"");
    result = result.replace("&#038;", "&");
    result = result.replace("&#38;", "&");
    result = result.replace("&#060;", "<");
    result = result.replace("&#60;", "<");
    result = result.replace("&#062;", ">");
    result = result.replace("&#62;", ">");
    // Named HTML entities
    result = result.replace("&amp;", "&");
    result = result.replace("&lt;", "<");
    result = result.replace("&gt;", ">");
    result = result.replace("&quot;", "\"");
    result = result.replace("&apos;", "'");
    // Partial variant: model outputs '039; (the &# prefix was lost)
    result = result.replace("'039;", "'");
    result
}

/// Check if an agent is currently in a DM conversation turn. Returns a 409
/// response if so, preventing heavy side-effect operations during DMs.
async fn check_dm_mode(state: &AppState, agent_id: Uuid) -> Option<(StatusCode, Json<serde_json::Value>)> {
    let in_dm = state.agents_in_dm.read().await.contains(&agent_id);
    if in_dm {
        Some((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "You are currently in a DM conversation. Focus on responding to the message. \
                          Heavy actions (hiring, DMs, provisioning) are blocked during conversations. \
                          You will receive a separate action prompt after this conversation ends \
                          where you can execute these actions."
            })),
        ))
    } else {
        None
    }
}

pub(crate) fn strip_agent_tags(response: &str) -> (String, bool) {
    let end_conv = {
        let normalized: String = response.chars()
            .filter(|c| !c.is_whitespace() && *c != '[' && *c != ']')
            .collect();
        normalized.contains("END_CONVERSATION") || normalized.contains("ENDCONVERSATION")
            || normalized.contains("NO_REPLY") || normalized.contains("NOREPLY")
    };
    let mut text = response.to_string();
    // Decode HTML entities that glm-5 occasionally outputs (e.g. &#039; -> ')
    text = decode_html_entities(&text);
    // Strip CJK characters that leak from multilingual models (qwen, glm)
    text = text.chars().filter(|c| {
        let cp = *c as u32;
        !(0x4E00..=0x9FFF).contains(&cp)   // CJK Unified Ideographs
        && !(0x3400..=0x4DBF).contains(&cp) // CJK Extension A
        && !(0xF900..=0xFAFF).contains(&cp) // CJK Compatibility Ideographs
        && !(0x3040..=0x309F).contains(&cp) // Hiragana
        && !(0x30A0..=0x30FF).contains(&cp) // Katakana
        && !(0xAC00..=0xD7AF).contains(&cp) // Hangul Syllables
        && !(0x3000..=0x303F).contains(&cp) // CJK Symbols and Punctuation
    }).collect();
    // Strip known system tags (including variants where streaming splits brackets/text across lines)
    text = text.replace("[END_CONVERSATION]", "");
    text = text.replace("[HEARTBEAT_OK]", "");
    text = text.replace("HEARTBEAT_OK", "");
    text = text.replace("[NO_ACTION_NEEDED]", "");
    text = text.replace("NO_ACTION_NEEDED", "");
    text = text.replace("[NO_REPLY]", "");
    text = text.replace("NO_REPLY", "");
    text = text.replace("[BRIEFING_COMPLETE]", "");
    text = text.replace("BRIEFING_COMPLETE", "");
    // Strip fragmented variants where streaming split the tag across tokens
    // (e.g. "HE ARTBEAT_OK", "END _CONVERSATION", "NO_ACTION _NEEDED")
    text = strip_fragmented_tag(&text, "HEARTBEAT_OK");
    text = strip_fragmented_tag(&text, "END_CONVERSATION");
    text = strip_fragmented_tag(&text, "NO_ACTION_NEEDED");
    text = strip_fragmented_tag(&text, "NO_REPLY");
    text = strip_fragmented_tag(&text, "BRIEFING_COMPLETE");
    // Strip internal sentinel from send_message() retry failures
    text = text.replace("[Agent produced no text output]", "");
    // Strip model-narrated OpenClaw failures (model itself says this when a tool call fails)
    text = text.replace("No response from OpenClaw.", "");
    text = text.replace("No response from OpenClaw", "");
    // Strip OpenClaw internal timeout messages (600s agent timeout returns this as text)
    text = text.replace("Request timed out before a response was generated. Please try again, or increase `agents.defaults.timeoutSeconds` in your config.", "");
    text = text.replace("Request timed out before a response was generated. Please try again, or increase agents.defaults.timeoutSeconds in your config.", "");
    // Clean up leftover empty brackets from partial stripping (e.g. "[\n" removed the tag but left "[]")
    text = text.replace("[]", "");
    text = text.replace("[ ]", "");
    // Strip known model artifacts (double-bracket and single-bracket variants)
    text = text.replace("[[reply_to_current]]", "");
    text = text.replace("[reply_to_current]", "");
    text = strip_fragmented_tag(&text, "reply_to_current");
    // Strip leaked tool-call XML markup (model outputs raw XML when OpenClaw fails to parse)
    // e.g. "memory_search<arg_key>query</arg_key><arg_value>...</arg_value></tool_call>"
    // Also handles <tool_call>...</tool_call> wrappers.
    {
        // First: strip complete </tool_call>-terminated blocks.
        // Find the start by looking for <arg_key> or <tool_call (the earliest XML tool tag).
        while let Some(end_pos) = text.find("</tool_call>") {
            let end = end_pos + "</tool_call>".len();
            // Walk backwards from the first <arg_key> or <tool_call to find block start
            let search_region = &text[..end_pos];
            let start = search_region.rfind("<tool_call")
                .or_else(|| search_region.rfind("<arg_key>"))
                .unwrap_or(end_pos);
            // Also consume the tool name token before <arg_key> (e.g. "memory_search")
            // by scanning back over word chars
            let chars: Vec<char> = text[..start].chars().collect();
            let mut i = chars.len();
            while i > 0 && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '_') {
                i -= 1;
            }
            let real_start = chars[..i].iter().collect::<String>().len();
            text = format!("{}{}", &text[..real_start], &text[end..]);
        }
        // Second: strip orphan XML tool tags that appear without </tool_call>
        // (e.g. just "<arg_key>...</arg_key>" fragments)
        let tool_xml_tags = ["<arg_key>", "</arg_key>", "<arg_value>", "</arg_value>",
                             "<tool_call>", "<tool_call ", "</tool_call>"];
        for tag in &tool_xml_tags {
            text = text.replace(tag, "");
        }
    }
    // Strip OpenClaw tool-failure feedback lines the model leaks into output
    // e.g. "⚠️ 📝 Edit: `in /workspace/MEMORY.md (315 chars)` failed"
    {
        let lines: Vec<&str> = text.lines().collect();
        text = lines.into_iter()
            .filter(|line| {
                let trimmed = line.trim();
                !(trimmed.contains('\u{26A0}') && trimmed.contains("failed"))
            })
            .collect::<Vec<_>>()
            .join("\n");
    }
    // Collapse duplicate content blocks (model occasionally emits same text twice)
    text = dedup_content_blocks(&text);
    // Clean spurious newlines from streaming token assembly
    text = clean_spurious_newlines(&text);
    // Strip any remaining [[word_word]] artifacts (model-generated tags)
    // This runs AFTER clean_spurious_newlines so that streaming-fragmented tags
    // like "[[reply\n_to_current]]" have their newlines collapsed first.
    while let Some(start) = text.find("[[") {
        if let Some(end) = text[start..].find("]]") {
            let tag = &text[start..start + end + 2];
            // Only strip if it looks like a simple tag (letters, underscores, hyphens, whitespace)
            let inner = &tag[2..tag.len() - 2];
            if inner.chars().all(|c| c.is_whitespace() || c.is_ascii_alphanumeric() || c == '_' || c == '-') {
                text = text.replacen(tag, "", 1);
                continue;
            }
        }
        break;
    }
    // Second pass: clean up any empty brackets left after tag stripping
    text = text.replace("[]", "");
    text = text.replace("[ ]", "");
    // Fix spacing artifacts: space before punctuation/contractions from token boundaries
    text = fix_punctuation_spacing(&text);
    // Strip Markdown bold markers (**) entirely — agents output plain text for chat
    text = strip_markdown_bold(&text);
    // Fix mid-word spaces from streaming token assembly (e.g. "Under stood" → "Understood")
    text = fix_broken_words(&text);
    // Strip pure narration lines (runs AFTER newline/word fixes so streaming fragments
    // like "Let\nme check my memory..." are cleaned to "Let me check my memory..." first)
    text = strip_narration_lines(&text);
    // Final defense: if the entire remaining text (ignoring whitespace and brackets)
    // is just a fragmented system tag (e.g. "HE ARTBEAT_OK"), treat as empty.
    // This catches all streaming-fragmentation variants in one shot.
    let compact: String = text.chars()
        .filter(|c| !c.is_whitespace() && *c != '[' && *c != ']')
        .collect();
    if matches!(compact.as_str(),
        "HEARTBEAT_OK" | "HEARTBEATOK" | "ENDCONVERSATION" | "END_CONVERSATION" |
        "NO_ACTION_NEEDED" | "NOACTIONNEEDED" |
        "NO_REPLY" | "NOREPLY" |
        "BRIEFING_COMPLETE" | "BRIEFINGCOMPLETE") {
        text = String::new();
    }
    (text.trim().to_string(), end_conv)
}

pub fn app_router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        // Health & Install
        .route("/v1/health", get(health))
        .route("/v1/install/init", post(handle_init))
        // Companies
        .route("/v1/companies", get(list_companies).post(create_company))
        .route("/v1/companies/:id", get(get_company).patch(update_company))
        .route("/v1/companies/:id/org-tree", get(get_org_tree))
        .route("/v1/companies/:id/hire-ceo", post(hire_ceo))
        .route("/v1/companies/:id/ledger", get(get_ledger).post(create_ledger_entry))
        .route("/v1/companies/:id/balance", get(get_balance))
        // Agents
        .route("/v1/agents", get(list_agents))
        .route("/v1/agents/:id", get(get_agent).patch(patch_agent))
        .route("/v1/agents/:id/hire-manager", post(hire_manager))
        .route("/v1/agents/:id/hire-worker", post(hire_worker))
        .route("/v1/agents/:id/vm/start", post(vm_start))
        .route("/v1/agents/:id/vm/stop", post(vm_stop))
        .route("/v1/agents/:id/vm/rebuild", post(vm_rebuild))
        .route("/v1/agents/:id/vm/provision", post(vm_provision))
        .route("/v1/agents/:id/vm/sandbox/provision", post(vm_sandbox_provision))
        .route("/v1/agents/:id/vm/exec", post(vm_exec))
        .route("/v1/agents/:id/vm/info", get(vm_info))
        .route("/v1/agents/:id/vm/file/push", post(vm_file_push))
        .route("/v1/agents/:id/vm/file/pull", post(vm_file_pull))
        .route("/v1/agents/:id/vm/copy-to-sandbox", post(vm_copy_to_sandbox))
        .route("/v1/agents/:id/panic", post(agent_panic))
        .route("/v1/agents/:id/thread", get(get_agent_thread))
        .route("/v1/agents/:id/dm", post(agent_dm))
        .route("/v1/agents/:id/dm-user", post(agent_dm_user))
        .route("/v1/agents/:id/send-file", post(agent_send_file))
        .route("/v1/agents/:id/file-transfers", get(agent_file_transfers))
        .route("/v1/agents/:id/threads", get(get_agent_threads))
        .route("/v1/agents/:id/memories", get(get_agent_memories).post(create_agent_memory))
        .route("/v1/agents/:id/memories/:mid", delete(delete_agent_memory))
        .route("/v1/agents/:id/openclaw-files", get(get_openclaw_files))
        .route("/v1/agents/:id/secrets", get(list_agent_secrets))
        .route("/v1/agents/:id/secrets/:name", get(get_agent_secret))
        // Secrets
        .route("/v1/secrets", get(list_secrets).post(create_secret))
        .route("/v1/secrets/:id", delete(delete_secret))
        // Threads & Messages
        .route("/v1/threads", get(get_threads).post(create_thread))
        .route("/v1/threads/:id", get(get_thread))
        .route("/v1/threads/:id/messages", get(get_messages).post(send_message))
        .route("/v1/threads/:id/participants", get(get_thread_participants).post(add_thread_participant))
        .route("/v1/threads/:id/participants/:member_id", delete(remove_thread_participant))
        // Requests & Approvals
        .route("/v1/requests", get(list_requests).post(create_request))
        .route("/v1/requests/:id/approve", post(approve_request))
        .route("/v1/requests/:id/reject", post(reject_request))
        .route("/v1/requests/:id/agent-approve", post(agent_approve_request))
        .route("/v1/requests/:id/agent-reject", post(agent_reject_request))
        // Services
        .route("/v1/services", get(list_services).post(create_service))
        .route("/v1/engagements", post(create_engagement))
        .route("/v1/engagements/:id/activate", post(activate_engagement))
        .route("/v1/engagements/:id/complete", post(complete_engagement))
        // Agentd
        .route("/v1/agentd/register", post(agentd_register))
        .route("/v1/agentd/heartbeat", post(agentd_heartbeat))
        // Scripts (served to VMs during cloud-init)
        .route("/v1/scripts/install-openclaw.sh", get(serve_install_script))
        // Models
        .route("/v1/models", get(list_models))
        .route("/v1/models/pull-status", get(model_pull_status))
        .route("/v1/models/pull", post(pull_model))
        // System
        .route("/v1/system/settings", get(get_system_settings))
        .route("/v1/system/settings", put(update_system_settings))
        .route("/v1/system/update-check", get(system_update_check))
        .route("/v1/system/update", post(system_update))
        .route("/v1/system/containers", get(list_containers))
        .route("/v1/system/containers/:id/logs", get(get_container_logs))
        // Rewrite
        .route("/v1/rewrite", post(rewrite_text))
        // World
        .route("/v1/world/snapshot", get(world_snapshot))
        // Events WS
        .route("/v1/events", get(events_handler))
        .layer(cors)
        .with_state(state)
}

// ═══════════════════════════════════════════════════════════════
// Health
// ═══════════════════════════════════════════════════════════════

async fn health(State(state): State<AppState>) -> impl IntoResponse {
    let db_ok = sqlx::query("SELECT 1").fetch_one(&state.db).await.is_ok();
    if db_ok {
        (StatusCode::OK, Json(json!({"status": "ok", "db": "ok"})))
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"status": "degraded", "db": "unreachable"})))
    }
}

// ═══════════════════════════════════════════════════════════════
// Install Init
// ═══════════════════════════════════════════════════════════════

async fn handle_init(
    State(state): State<AppState>,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    tracing::info!("Received init request, body length={}", body.len());

    let payload: InitRequest = match serde_json::from_slice(&body) {
        Ok(p) => p,
        Err(e) => {
            tracing::error!("Failed to parse init request JSON: {}. Body: {:?}", e, String::from_utf8_lossy(&body));
            return (StatusCode::BAD_REQUEST, Json(json!({"error": format!("Invalid JSON: {}", e)})));
        }
    };

    let holding_name = payload.holding_name.unwrap_or_else(|| "Main Holding".into());
    let agent_name = payload.main_agent_name.unwrap_or_else(|| "MainAgent".into());
    let model = payload.default_model.unwrap_or_else(|| "glm-5:cloud".into());

    let holding_id = Uuid::new_v4();
    let owner_id = Uuid::new_v4();

    // Check if already initialized
    let existing: Option<(i64,)> = sqlx::query_as("SELECT COUNT(*) FROM holdings")
        .fetch_optional(&state.db).await.unwrap_or(None);
    if let Some((count,)) = existing {
        if count > 0 {
            return (StatusCode::OK, Json(json!({"status": "already_initialized"})));
        }
    }

    // Create holding
    if let Err(e) = sqlx::query(
        "INSERT INTO holdings (id, owner_user_id, name, main_agent_name) VALUES ($1, $2, $3, $4)"
    ).bind(holding_id).bind(owner_id).bind(&holding_name).bind(&agent_name)
    .execute(&state.db).await {
        tracing::error!("Failed to create holding: {}", e);
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("{}", e)})));
    }

    // Create default tool policies
    let ceo_policy_id = Uuid::new_v4();
    let mgr_policy_id = Uuid::new_v4();
    let wkr_policy_id = Uuid::new_v4();
    let main_policy_id = Uuid::new_v4();

    for (id, name, allow, deny) in [
        (main_policy_id, "main_agent_policy", json!(["*"]), json!([])),
        (ceo_policy_id, "ceo_policy", json!(["browser","files","coding","sessions"]), json!(["vm_provisioning"])),
        (mgr_policy_id, "manager_policy", json!(["browser","files"]), json!(["system.run"])),
        (wkr_policy_id, "worker_policy", json!(["browser","files"]), json!(["system.run","admin"])),
    ] {
        let _ = sqlx::query(
            "INSERT INTO tool_policies (id, name, allowlist, denylist, notes) VALUES ($1, $2, $3, $4, $5)"
        ).bind(id).bind(name).bind(&allow).bind(&deny).bind("Default policy")
        .execute(&state.db).await;
    }

    // Create MainAgent
    let agent_id = Uuid::new_v4();
    let _ = sqlx::query(
        "INSERT INTO agents (id, holding_id, company_id, role, name, specialty, effective_model, system_prompt, tool_policy_id, status) \
         VALUES ($1, $2, NULL, 'MAIN', $3, 'Holding Company Management', $4, $5, $6, 'ACTIVE')"
    )
    .bind(agent_id).bind(holding_id).bind(&agent_name).bind(&model)
    .bind(format!("You are {}, the Main Agent managing this holding company.", agent_name))
    .bind(main_policy_id)
    .execute(&state.db).await;

    // Spawn OpenClaw instance for MainAgent in background
    state.openclaw.register_pending_spawn(agent_id).await;
    let openclaw = state.openclaw.clone();
    let agent_name_clone = agent_name.clone();
    let model_clone = model.clone();
    let holding_name_clone = holding_name.clone();
    let probe_ceiling = state.config.probe_ceiling;
    tokio::spawn(async move {
        // Probe concurrency now that the model has been pulled by the install script.
        // Startup skipped the probe because no agents existed yet.
        openclaw.probe_concurrency(&model_clone, probe_ceiling).await;

        let config = crate::openclaw::AgentConfig {
            agent_id,
            agent_name: agent_name_clone,
            role: "MAIN".to_string(),
            company_name: holding_name_clone.clone(),
            company_type: None,
            holding_name: holding_name_clone,
            specialty: Some("Holding Company Management".to_string()),
            model: model_clone,
            system_prompt: None,
        };
        match openclaw.spawn_instance(&config).await {
            Ok(inst) => tracing::info!("OpenClaw instance spawned for MainAgent on port {}", inst.port),
            Err(e) => tracing::error!("Failed to spawn OpenClaw instance for MainAgent: {}", e),
        }
    });

    // Store default model in system_meta for use by hire endpoints
    let _ = sqlx::query("INSERT INTO system_meta (key, value) VALUES ('default_model', $1) ON CONFLICT (key) DO UPDATE SET value = $1, updated_at = NOW()")
        .bind(&model).execute(&state.db).await;

    // Seed available models list (don't overwrite if user already customized)
    let _ = sqlx::query("INSERT INTO system_meta (key, value) VALUES ('available_models', $1) ON CONFLICT (key) DO NOTHING")
        .bind(serde_json::to_string(&DEFAULT_MODELS).unwrap_or_default())
        .execute(&state.db).await;

    tracing::info!("Initialized holding '{}' with MainAgent '{}'", holding_name, agent_name);
    (StatusCode::OK, Json(json!({"status": "success", "holding_id": holding_id, "main_agent_id": agent_id})))
}

// ═══════════════════════════════════════════════════════════════
// Companies
// ═══════════════════════════════════════════════════════════════

async fn list_companies(State(state): State<AppState>) -> impl IntoResponse {
    match sqlx::query_as::<_, Company>(
        "SELECT id, holding_id, name, type, description, tags, status, created_at FROM companies ORDER BY created_at DESC"
    ).fetch_all(&state.db).await {
        Ok(c) => (StatusCode::OK, Json(json!(c))),
        Err(e) => { tracing::error!("list_companies: {}", e); (StatusCode::OK, Json(json!([]))) }
    }
}

async fn get_company(State(state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    let uid = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };
    match sqlx::query_as::<_, Company>(
        "SELECT id, holding_id, name, type, description, tags, status, created_at FROM companies WHERE id = $1"
    ).bind(uid).fetch_optional(&state.db).await {
        Ok(Some(c)) => (StatusCode::OK, Json(json!(c))),
        Ok(None) => (StatusCode::NOT_FOUND, Json(json!({"error":"Not found"}))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("{}", e)})))
    }
}

async fn create_company(State(state): State<AppState>, Json(payload): Json<CreateCompanyRequest>) -> impl IntoResponse {
    let id = Uuid::new_v4();
    let holding: Option<Holding> = sqlx::query_as("SELECT id, owner_user_id, name, main_agent_name, created_at FROM holdings LIMIT 1")
        .fetch_optional(&state.db).await.unwrap_or(None);
    let holding_id = holding.map(|h| h.id).unwrap_or(Uuid::from_u128(0));

    // Check for duplicate company name
    let existing: Option<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM companies WHERE holding_id = $1 AND LOWER(name) = LOWER($2)"
    ).bind(holding_id).bind(&payload.name).fetch_optional(&state.db).await.unwrap_or(None);

    if let Some((existing_id,)) = existing {
        return (StatusCode::CONFLICT, Json(json!({
            "error": format!("A company named '{}' already exists", payload.name),
            "existing_id": existing_id
        })));
    }

    match sqlx::query_as::<_, Company>(
        "INSERT INTO companies (id, holding_id, name, type, description, status) VALUES ($1,$2,$3,$4,$5,'ACTIVE') \
         RETURNING id, holding_id, name, type, description, tags, status, created_at"
    ).bind(id).bind(holding_id).bind(&payload.name).bind(&payload.r#type).bind(&payload.description)
    .fetch_one(&state.db).await {
        Ok(c) => {
            let _ = state.tx.send(json!({"type":"company_created","company": c}).to_string());
            (StatusCode::CREATED, Json(json!(c)))
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("{}", e)})))
    }
}

// ═══════════════════════════════════════════════════════════════
// Org Tree
// ═══════════════════════════════════════════════════════════════

async fn get_org_tree(State(state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    let uid = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };
    match sqlx::query_as::<_, Agent>(
        "SELECT id, holding_id, company_id, role, name, specialty, parent_agent_id, preferred_model, effective_model, system_prompt, tool_policy_id, vm_id, sandbox_vm_id, handle, status, created_at \
         FROM agents WHERE company_id = $1 ORDER BY role, name"
    ).bind(uid).fetch_all(&state.db).await {
        Ok(agents) => (StatusCode::OK, Json(json!({"company_id": uid, "tree": agents}))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("{}", e)})))
    }
}

// ═══════════════════════════════════════════════════════════════
// Agents
// ═══════════════════════════════════════════════════════════════

async fn list_agents(State(state): State<AppState>) -> impl IntoResponse {
    match sqlx::query_as::<_, Agent>(
        "SELECT id, holding_id, company_id, role, name, specialty, parent_agent_id, preferred_model, effective_model, system_prompt, tool_policy_id, vm_id, sandbox_vm_id, handle, status, created_at \
         FROM agents ORDER BY created_at"
    ).fetch_all(&state.db).await {
        Ok(agents) => {
            let activities_guard = state.agent_activities.read().await;
            let activities = activities_guard.as_ref();
            let result: Vec<Value> = agents.iter().map(|a| {
                let mut obj = json!(a);
                if let Some(act_map) = activities {
                    if let Some(act) = act_map.get(&a.id) {
                        obj["activity"] = json!({
                            "status": act.status,
                            "task": act.task,
                            "since": act.since,
                        });
                    }
                }
                obj
            }).collect();
            (StatusCode::OK, Json(json!(result)))
        },
        Err(e) => { tracing::error!("list_agents: {}", e); (StatusCode::OK, Json(json!([]))) }
    }
}

async fn get_agent(State(state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    let uid = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };
    match sqlx::query_as::<_, Agent>(
        "SELECT id, holding_id, company_id, role, name, specialty, parent_agent_id, preferred_model, effective_model, system_prompt, tool_policy_id, vm_id, sandbox_vm_id, handle, status, created_at \
         FROM agents WHERE id = $1"
    ).bind(uid).fetch_optional(&state.db).await {
        Ok(Some(a)) => {
            let mut obj = json!(a);
            let activities_guard = state.agent_activities.read().await;
            if let Some(act_map) = activities_guard.as_ref() {
                if let Some(act) = act_map.get(&a.id) {
                    obj["activity"] = json!({
                        "status": act.status,
                        "task": act.task,
                        "since": act.since,
                    });
                }
            }
            (StatusCode::OK, Json(obj))
        },
        Ok(None) => (StatusCode::NOT_FOUND, Json(json!({"error":"Agent not found"}))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("{}", e)})))
    }
}

async fn patch_agent(State(state): State<AppState>, Path(id): Path<String>, Json(p): Json<PatchAgentRequest>) -> impl IntoResponse {
    let uid = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };
    let _ = sqlx::query("UPDATE agents SET preferred_model = COALESCE($1, preferred_model), effective_model = COALESCE($1, effective_model), specialty = COALESCE($2, specialty), system_prompt = COALESCE($3, system_prompt) WHERE id = $4")
        .bind(&p.preferred_model).bind(&p.specialty).bind(&p.system_prompt).bind(uid)
        .execute(&state.db).await;
    (StatusCode::OK, Json(json!({"status":"updated"})))
}

// ═══════════════════════════════════════════════════════════════
// Hiring (CEO / Manager / Worker) — wired to policy engine
// ═══════════════════════════════════════════════════════════════

async fn hire_ceo(State(state): State<AppState>, Path(id): Path<String>, Json(payload): Json<HireRequest>) -> impl IntoResponse {
    let company_id = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };
    // Count existing CEOs
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM company_ceos WHERE company_id = $1")
        .bind(company_id).fetch_one(&state.db).await.unwrap_or((0,));

    if count.0 >= 2 {
        return (StatusCode::CONFLICT, Json(json!({"error":"Maximum 2 CEOs per company"})));
    }

    // If adding 2nd CEO, require explicit approval
    if count.0 == 1 {
        let req_id = Uuid::new_v4();
        let _ = sqlx::query("INSERT INTO requests (id, type, company_id, payload, status, current_approver_type) VALUES ($1,'ADD_SECOND_CEO',$2,$3,'PENDING','USER')")
            .bind(req_id).bind(company_id).bind(json!({"name": payload.name, "specialty": payload.specialty}))
            .execute(&state.db).await;
        let _ = state.tx.send(json!({"type":"approval_required","request_id": req_id, "request_type":"ADD_SECOND_CEO"}).to_string());
        return (StatusCode::ACCEPTED, Json(json!({"status":"requires_approval","request_id": req_id})));
    }

    // Create CEO agent directly
    let holding: Option<Holding> = sqlx::query_as("SELECT id, owner_user_id, name, main_agent_name, created_at FROM holdings LIMIT 1")
        .fetch_optional(&state.db).await.unwrap_or(None);
    let holding_id = holding.map(|h| h.id).unwrap_or(Uuid::from_u128(0));

    // Reject duplicate first names within the holding to avoid confusion
    let first_name = payload.name.split_whitespace().next().unwrap_or(&payload.name);
    let name_conflict: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM agents WHERE holding_id = $1 AND status = 'ACTIVE' AND SPLIT_PART(name, ' ', 1) ILIKE $2"
    ).bind(holding_id).bind(first_name).fetch_one(&state.db).await.unwrap_or((0,));
    if name_conflict.0 > 0 {
        return (StatusCode::CONFLICT, Json(json!({
            "error": format!("An agent with first name '{}' already exists. Choose a different first name to avoid confusion.", first_name)
        })));
    }

    let ceo_policy: Option<ToolPolicy> = sqlx::query_as("SELECT id, name, allowlist, denylist, notes FROM tool_policies WHERE name = 'ceo_policy' LIMIT 1")
        .fetch_optional(&state.db).await.unwrap_or(None);
    let policy_id = ceo_policy.map(|p| p.id).unwrap_or(Uuid::new_v4());
    let system_default: String = sqlx::query_scalar("SELECT value FROM system_meta WHERE key = 'default_model'")
        .fetch_optional(&state.db).await.ok().flatten().unwrap_or_else(|| "minimax-m2.5:cloud".into());
    let model = payload.preferred_model.unwrap_or(system_default);
    let agent_id = Uuid::new_v4();
    let handle = format!("@{}", payload.name.to_lowercase().replace(' ', "-"));

    let _ = sqlx::query(
        "INSERT INTO agents (id, holding_id, company_id, role, name, handle, specialty, effective_model, tool_policy_id, status) VALUES ($1,$2,$3,'CEO',$4,$5,$6,$7,$8,'ACTIVE')"
    ).bind(agent_id).bind(holding_id).bind(company_id).bind(&payload.name).bind(&handle).bind(&payload.specialty).bind(&model).bind(policy_id)
    .execute(&state.db).await;

    let _ = sqlx::query("INSERT INTO company_ceos (company_id, agent_id) VALUES ($1, $2)")
        .bind(company_id).bind(agent_id).execute(&state.db).await;

    // Spawn OpenClaw instance in background
    state.openclaw.register_pending_spawn(agent_id).await;
    let openclaw = state.openclaw.clone();
    let name_clone = payload.name.clone();
    let (company_name, company_type): (String, String) = sqlx::query_as("SELECT name, type FROM companies WHERE id = $1").bind(company_id)
        .fetch_optional(&state.db).await.ok().flatten().unwrap_or_else(|| ("Company".into(), "INTERNAL".into()));
    let holding_name: String = sqlx::query_scalar("SELECT name FROM holdings LIMIT 1")
        .fetch_optional(&state.db).await.ok().flatten().unwrap_or_else(|| "Holdings".into());
    let model_clone = model.clone();
    let specialty_clone = payload.specialty.clone();
    tokio::spawn(async move {
        let config = crate::openclaw::AgentConfig {
            agent_id, agent_name: name_clone, role: "CEO".to_string(),
            company_name, company_type: Some(company_type), holding_name, specialty: specialty_clone,
            model: model_clone, system_prompt: None,
        };
        if let Err(e) = openclaw.spawn_instance(&config).await {
            tracing::error!("Failed to spawn OpenClaw for CEO {}: {}", config.agent_name, e);
        }
    });

    // Auto-provision desktop VM in background
    provision_agent_vm(state.clone(), agent_id, &payload.name, &model, "ceo_policy").await;

    let _ = state.tx.send(json!({"type":"ceo_hired","agent_id": agent_id,"company_id": company_id}).to_string());
    (StatusCode::CREATED, Json(json!({"status":"hired","agent_id": agent_id})))
}

async fn hire_manager(State(state): State<AppState>, Path(id): Path<String>, Json(payload): Json<HireRequest>) -> impl IntoResponse {
    let ceo_id = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };
    if let Some(resp) = check_dm_mode(&state, ceo_id).await { return resp; }
    let ceo: Option<Agent> = sqlx::query_as(
        "SELECT id, holding_id, company_id, role, name, specialty, parent_agent_id, preferred_model, effective_model, system_prompt, tool_policy_id, vm_id, sandbox_vm_id, handle, status, created_at FROM agents WHERE id = $1 AND role = 'CEO'"
    ).bind(ceo_id).fetch_optional(&state.db).await.unwrap_or(None);
    let ceo = match ceo { Some(c) => c, None => return (StatusCode::NOT_FOUND, Json(json!({"error":"CEO not found"}))) };
    let company_id = match ceo.company_id { Some(c) => c, None => return (StatusCode::BAD_REQUEST, Json(json!({"error":"CEO has no company"}))) };

    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM agents WHERE company_id = $1 AND role = 'MANAGER'")
        .bind(company_id).fetch_one(&state.db).await.unwrap_or((0,));
    let new_count = (count.0 + 1) as u32;

    use crate::policy::engine::{can_hire_manager, Role, Decision, ApproverType};
    match can_hire_manager(new_count, Role::Ceo) {
        Decision::AllowedImmediate => {},
        Decision::RequiresRequest { request_type, approver_chain } => {
            // Check for an existing APPROVED request that can be consumed to allow this hire
            let approved_req: Option<Uuid> = sqlx::query_scalar(
                "SELECT id FROM requests WHERE type = $1 AND created_by_agent_id = $2 AND status = 'APPROVED' ORDER BY updated_at DESC LIMIT 1"
            ).bind(&request_type).bind(ceo_id).fetch_optional(&state.db).await.ok().flatten();

            if let Some(approved_id) = approved_req {
                // Consume the approval so it can't be reused for future limit increases
                let _ = sqlx::query("UPDATE requests SET status = 'CONSUMED', updated_at = NOW() WHERE id = $1")
                    .bind(approved_id).execute(&state.db).await;
                tracing::info!("Consumed approved request {} for {} hire by CEO {}", approved_id, request_type, ceo_id);
                // Fall through to the hire logic below
            } else {
                // Check for an existing PENDING request (deduplication)
                let existing_pending: Option<Uuid> = sqlx::query_scalar(
                    "SELECT id FROM requests WHERE type = $1 AND created_by_agent_id = $2 AND status = 'PENDING' LIMIT 1"
                ).bind(&request_type).bind(ceo_id).fetch_optional(&state.db).await.ok().flatten();

                if let Some(existing_id) = existing_pending {
                    // Already have a pending request — don't create a duplicate
                    return (StatusCode::ACCEPTED, Json(json!({
                        "status": "requires_approval",
                        "request_id": existing_id,
                        "message": "A request for this has already been submitted and is awaiting approval. Please wait for the approval notification."
                    })));
                }

                let first_approver = approver_chain.first().unwrap_or(&ApproverType::User);
                let (approver_type_str, approver_id) = match first_approver {
                    ApproverType::MainAgent => {
                        let main_id: Option<Uuid> = sqlx::query_scalar(
                            "SELECT id FROM agents WHERE role = 'MAIN' LIMIT 1"
                        ).fetch_optional(&state.db).await.ok().flatten();
                        ("AGENT".to_string(), main_id)
                    },
                    ApproverType::User => ("USER".to_string(), None),
                    ApproverType::Ceo => ("AGENT".to_string(), Some(ceo_id)),
                };
                let chain_json = serde_json::to_value(&approver_chain).unwrap_or(json!([]));
                let req_id = Uuid::new_v4();
                let _ = sqlx::query(
                    "INSERT INTO requests (id, type, created_by_agent_id, company_id, payload, status, current_approver_type, current_approver_id) \
                     VALUES ($1,$2,$3,$4,$5,'PENDING',$6,$7)"
                ).bind(req_id).bind(&request_type).bind(ceo_id).bind(company_id)
                 .bind(json!({"name": payload.name, "count": new_count, "approver_chain": chain_json}))
                 .bind(&approver_type_str).bind(approver_id)
                 .execute(&state.db).await;

                // DM the approver agent about the pending request
                if approver_type_str == "AGENT" {
                    if let Some(aid) = approver_id {
                        let dm_text = format!(
                            "APPROVAL REQUEST from {}: Hire manager #{} (\"{}\")\n\nRequest ID: {}\nType: {}\n\n\
                             To approve: curl -s -X POST $MULTICLAW_API_URL/v1/requests/{}/agent-approve \
                             -H 'Content-Type: application/json' -d '{{\"agent_id\": \"$AGENT_ID\"}}'\n\n\
                             To reject: curl -s -X POST $MULTICLAW_API_URL/v1/requests/{}/agent-reject \
                             -H 'Content-Type: application/json' -d '{{\"agent_id\": \"$AGENT_ID\"}}'",
                            ceo.name, new_count, payload.name, req_id, request_type, req_id, req_id
                        );
                        let dm_thread = find_or_create_agent_dm_thread(&state.db, ceo_id, aid).await;
                        insert_system_message_in_thread(&state, dm_thread, ceo_id, &dm_text).await;
                        let _ = state.enqueue_message(
                            aid, 2, "approval_escalate",
                            json!({"agent_id": aid.to_string(), "message": dm_text, "task_label": "Processing approval request", "thread_id": dm_thread.to_string()}),
                        ).await;
                    }
                } else {
                    // User-targeted: notify UI
                    let _ = state.tx.send(json!({"type":"new_request","request_id": req_id}).to_string());
                }

                return (StatusCode::ACCEPTED, Json(json!({"status":"requires_approval","request_id": req_id,"approver": approver_type_str})));
            }
        },
        Decision::Denied(reason) => return (StatusCode::FORBIDDEN, Json(json!({"error": reason}))),
    }

    // Reject duplicate first names within the holding to avoid confusion
    let first_name = payload.name.split_whitespace().next().unwrap_or(&payload.name);
    let name_conflict: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM agents WHERE holding_id = $1 AND status = 'ACTIVE' AND SPLIT_PART(name, ' ', 1) ILIKE $2"
    ).bind(ceo.holding_id).bind(first_name).fetch_one(&state.db).await.unwrap_or((0,));
    if name_conflict.0 > 0 {
        return (StatusCode::CONFLICT, Json(json!({
            "error": format!("An agent with first name '{}' already exists. Choose a different first name to avoid confusion.", first_name)
        })));
    }

    let mgr_policy: Option<ToolPolicy> = sqlx::query_as("SELECT id, name, allowlist, denylist, notes FROM tool_policies WHERE name = 'manager_policy' LIMIT 1")
        .fetch_optional(&state.db).await.unwrap_or(None);
    let policy_id = mgr_policy.map(|p| p.id).unwrap_or(Uuid::new_v4());
    let model = payload.preferred_model.unwrap_or_else(|| ceo.effective_model.clone());
    let agent_id = Uuid::new_v4();
    let handle = format!("@{}", payload.name.to_lowercase().replace(' ', "-"));

    let _ = sqlx::query(
        "INSERT INTO agents (id, holding_id, company_id, role, name, handle, specialty, parent_agent_id, effective_model, tool_policy_id, status) VALUES ($1,$2,$3,'MANAGER',$4,$5,$6,$7,$8,$9,'ACTIVE')"
    ).bind(agent_id).bind(ceo.holding_id).bind(company_id).bind(&payload.name).bind(&handle).bind(&payload.specialty).bind(ceo_id).bind(&model).bind(policy_id)
    .execute(&state.db).await;

    // Spawn OpenClaw instance in background
    state.openclaw.register_pending_spawn(agent_id).await;
    let openclaw = state.openclaw.clone();
    let name_clone = payload.name.clone();
    let (company_name, company_type): (String, String) = sqlx::query_as("SELECT name, type FROM companies WHERE id = $1").bind(company_id)
        .fetch_optional(&state.db).await.ok().flatten().unwrap_or_else(|| ("Company".into(), "INTERNAL".into()));
    let holding_name: String = sqlx::query_scalar("SELECT name FROM holdings LIMIT 1")
        .fetch_optional(&state.db).await.ok().flatten().unwrap_or_else(|| "Holdings".into());
    let model_clone = model.clone();
    let specialty_clone = payload.specialty.clone();
    tokio::spawn(async move {
        let config = crate::openclaw::AgentConfig {
            agent_id, agent_name: name_clone, role: "MANAGER".to_string(),
            company_name, company_type: Some(company_type), holding_name, specialty: specialty_clone,
            model: model_clone, system_prompt: None,
        };
        if let Err(e) = openclaw.spawn_instance(&config).await {
            tracing::error!("Failed to spawn OpenClaw for Manager {}: {}", config.agent_name, e);
        }
    });

    // Auto-provision desktop VM in background
    provision_agent_vm(state.clone(), agent_id, &payload.name, &model, "manager_policy").await;

    (StatusCode::CREATED, Json(json!({"status":"hired","agent_id": agent_id})))
}

async fn hire_worker(State(state): State<AppState>, Path(id): Path<String>, Json(payload): Json<HireRequest>) -> impl IntoResponse {
    let mgr_id = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };
    if let Some(resp) = check_dm_mode(&state, mgr_id).await { return resp; }
    let mgr: Option<Agent> = sqlx::query_as(
        "SELECT id, holding_id, company_id, role, name, specialty, parent_agent_id, preferred_model, effective_model, system_prompt, tool_policy_id, vm_id, sandbox_vm_id, handle, status, created_at FROM agents WHERE id = $1 AND role = 'MANAGER'"
    ).bind(mgr_id).fetch_optional(&state.db).await.unwrap_or(None);
    let mgr = match mgr { Some(m) => m, None => return (StatusCode::NOT_FOUND, Json(json!({"error":"Manager not found"}))) };
    let company_id = match mgr.company_id { Some(c) => c, None => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Manager has no company"}))) };

    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM agents WHERE parent_agent_id = $1 AND role = 'WORKER'")
        .bind(mgr_id).fetch_one(&state.db).await.unwrap_or((0,));
    let new_count = (count.0 + 1) as u32;

    use crate::policy::engine::{can_hire_worker, Role, Decision, ApproverType};
    match can_hire_worker(new_count, Role::Manager) {
        Decision::AllowedImmediate => {},
        Decision::RequiresRequest { request_type, approver_chain } => {
            // Check for an existing APPROVED request that can be consumed to allow this hire
            let approved_req: Option<Uuid> = sqlx::query_scalar(
                "SELECT id FROM requests WHERE type = $1 AND created_by_agent_id = $2 AND status = 'APPROVED' ORDER BY updated_at DESC LIMIT 1"
            ).bind(&request_type).bind(mgr_id).fetch_optional(&state.db).await.ok().flatten();

            if let Some(approved_id) = approved_req {
                // Consume the approval so it can't be reused for future limit increases
                let _ = sqlx::query("UPDATE requests SET status = 'CONSUMED', updated_at = NOW() WHERE id = $1")
                    .bind(approved_id).execute(&state.db).await;
                tracing::info!("Consumed approved request {} for {} hire by Manager {}", approved_id, request_type, mgr_id);
                // Fall through to the hire logic below
            } else {
                // Check for an existing PENDING request (deduplication)
                let existing_pending: Option<Uuid> = sqlx::query_scalar(
                    "SELECT id FROM requests WHERE type = $1 AND created_by_agent_id = $2 AND status = 'PENDING' LIMIT 1"
                ).bind(&request_type).bind(mgr_id).fetch_optional(&state.db).await.ok().flatten();

                if let Some(existing_id) = existing_pending {
                    // Already have a pending request — don't create a duplicate
                    return (StatusCode::ACCEPTED, Json(json!({
                        "status": "requires_approval",
                        "request_id": existing_id,
                        "message": "A request for this has already been submitted and is awaiting approval. Please wait for the approval notification."
                    })));
                }

                let first_approver = approver_chain.first().unwrap_or(&ApproverType::User);
                let (approver_type_str, approver_id) = match first_approver {
                    ApproverType::Ceo => {
                        // Manager's parent is the CEO
                        ("AGENT".to_string(), mgr.parent_agent_id)
                    },
                    ApproverType::MainAgent => {
                        let main_id: Option<Uuid> = sqlx::query_scalar(
                            "SELECT id FROM agents WHERE role = 'MAIN' LIMIT 1"
                        ).fetch_optional(&state.db).await.ok().flatten();
                        ("AGENT".to_string(), main_id)
                    },
                    ApproverType::User => ("USER".to_string(), None),
                };
                let chain_json = serde_json::to_value(&approver_chain).unwrap_or(json!([]));
                let req_id = Uuid::new_v4();
                let _ = sqlx::query(
                    "INSERT INTO requests (id, type, created_by_agent_id, company_id, payload, status, current_approver_type, current_approver_id) \
                     VALUES ($1,$2,$3,$4,$5,'PENDING',$6,$7)"
                ).bind(req_id).bind(&request_type).bind(mgr_id).bind(company_id)
                 .bind(json!({"name": payload.name, "count": new_count, "approver_chain": chain_json}))
                 .bind(&approver_type_str).bind(approver_id)
                 .execute(&state.db).await;

                // DM the approver agent about the pending request
                if approver_type_str == "AGENT" {
                    if let Some(aid) = approver_id {
                        let dm_text = format!(
                            "APPROVAL REQUEST from {}: Hire worker #{} (\"{}\")\n\nRequest ID: {}\nType: {}\n\n\
                             To approve: curl -s -X POST $MULTICLAW_API_URL/v1/requests/{}/agent-approve \
                             -H 'Content-Type: application/json' -d '{{\"agent_id\": \"$AGENT_ID\"}}'\n\n\
                             To reject: curl -s -X POST $MULTICLAW_API_URL/v1/requests/{}/agent-reject \
                             -H 'Content-Type: application/json' -d '{{\"agent_id\": \"$AGENT_ID\"}}'",
                            mgr.name, new_count, payload.name, req_id, request_type, req_id, req_id
                        );
                        let dm_thread = find_or_create_agent_dm_thread(&state.db, mgr_id, aid).await;
                        insert_system_message_in_thread(&state, dm_thread, mgr_id, &dm_text).await;
                        let _ = state.enqueue_message(
                            aid, 2, "approval_escalate",
                            json!({"agent_id": aid.to_string(), "message": dm_text, "task_label": "Processing approval request", "thread_id": dm_thread.to_string()}),
                        ).await;
                    }
                } else {
                    // User-targeted: notify UI
                    let _ = state.tx.send(json!({"type":"new_request","request_id": req_id}).to_string());
                }

                return (StatusCode::ACCEPTED, Json(json!({"status":"requires_approval","request_id": req_id,"approver": approver_type_str})));
            }
        },
        Decision::Denied(reason) => return (StatusCode::FORBIDDEN, Json(json!({"error": reason}))),
    }

    // Reject duplicate first names within the holding to avoid confusion
    let first_name = payload.name.split_whitespace().next().unwrap_or(&payload.name);
    let name_conflict: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM agents WHERE holding_id = $1 AND status = 'ACTIVE' AND SPLIT_PART(name, ' ', 1) ILIKE $2"
    ).bind(mgr.holding_id).bind(first_name).fetch_one(&state.db).await.unwrap_or((0,));
    if name_conflict.0 > 0 {
        return (StatusCode::CONFLICT, Json(json!({
            "error": format!("An agent with first name '{}' already exists. Choose a different first name to avoid confusion.", first_name)
        })));
    }

    let wkr_policy: Option<ToolPolicy> = sqlx::query_as("SELECT id, name, allowlist, denylist, notes FROM tool_policies WHERE name = 'worker_policy' LIMIT 1")
        .fetch_optional(&state.db).await.unwrap_or(None);
    let policy_id = wkr_policy.map(|p| p.id).unwrap_or(Uuid::new_v4());
    let model = payload.preferred_model.unwrap_or_else(|| mgr.effective_model.clone());
    let agent_id = Uuid::new_v4();
    let handle = format!("@{}", payload.name.to_lowercase().replace(' ', "-"));

    let _ = sqlx::query(
        "INSERT INTO agents (id, holding_id, company_id, role, name, handle, specialty, parent_agent_id, effective_model, tool_policy_id, status) VALUES ($1,$2,$3,'WORKER',$4,$5,$6,$7,$8,$9,'ACTIVE')"
    ).bind(agent_id).bind(mgr.holding_id).bind(company_id).bind(&payload.name).bind(&handle).bind(&payload.specialty).bind(mgr_id).bind(&model).bind(policy_id)
    .execute(&state.db).await;

    // Spawn OpenClaw instance in background
    state.openclaw.register_pending_spawn(agent_id).await;
    let openclaw = state.openclaw.clone();
    let name_clone = payload.name.clone();
    let (company_name, company_type): (String, String) = sqlx::query_as("SELECT name, type FROM companies WHERE id = $1").bind(company_id)
        .fetch_optional(&state.db).await.ok().flatten().unwrap_or_else(|| ("Company".into(), "INTERNAL".into()));
    let holding_name: String = sqlx::query_scalar("SELECT name FROM holdings LIMIT 1")
        .fetch_optional(&state.db).await.ok().flatten().unwrap_or_else(|| "Holdings".into());
    let model_clone = model.clone();
    let specialty_clone = payload.specialty.clone();
    tokio::spawn(async move {
        let config = crate::openclaw::AgentConfig {
            agent_id, agent_name: name_clone, role: "WORKER".to_string(),
            company_name, company_type: Some(company_type), holding_name, specialty: specialty_clone,
            model: model_clone, system_prompt: None,
        };
        if let Err(e) = openclaw.spawn_instance(&config).await {
            tracing::error!("Failed to spawn OpenClaw for Worker {}: {}", config.agent_name, e);
        }
    });

    // Auto-provision desktop VM in background
    provision_agent_vm(state.clone(), agent_id, &payload.name, &model, "worker_policy").await;

    (StatusCode::CREATED, Json(json!({"status":"hired","agent_id": agent_id})))
}

// ═══════════════════════════════════════════════════════════════
// VM Provisioning Helper
// ═══════════════════════════════════════════════════════════════

/// Provisions an Incus VM for the given agent in a background task.
/// Reads cloud-init templates, renders config, launches the VM,
/// and updates the agent's vm_id in the database.
async fn provision_agent_vm(
    state: AppState,
    agent_id: Uuid,
    agent_name: &str,
    model: &str,
    policy_name: &str,
) {
    let provider = match state.vm_provider {
        Some(ref p) => p.clone(),
        None => {
            tracing::warn!("VM provider not available, skipping VM provisioning for agent {}", agent_id);
            return;
        }
    };

    let host_ip = state.config.host_ip.clone();
    let agent_name = agent_name.to_string();
    let model = model.to_string();
    let policy_name = policy_name.to_string();
    let db = state.db.clone();
    let tx = state.tx.clone();

    tokio::spawn(async move {
        tracing::info!("Provisioning VM for agent {} ({})", agent_name, agent_id);

        // Load tool policy
        let (tools_allow, tools_deny) = {
            let policy: Option<(Value, Value)> = sqlx::query_as(
                "SELECT allowlist, denylist FROM tool_policies WHERE name = $1 LIMIT 1"
            ).bind(&policy_name).fetch_optional(&db).await.unwrap_or(None);
            match policy {
                Some((a, d)) => (a.to_string(), d.to_string()),
                None => ("[\"*\"]".to_string(), "[]".to_string()),
            }
        };

        // Generate tokens for this agent
        let gateway_token = Uuid::new_v4().to_string();
        let agent_token = Uuid::new_v4().to_string();
        let ollama_token = Uuid::new_v4().to_string();

        // Load templates (embedded as compile-time constants would be ideal,
        // but for now read from /opt/multiclaw or the infra/vm directory)
        let base_paths = ["/opt/multiclaw/infra/vm", "infra/vm"];
        let mut base = "";
        for p in &base_paths {
            if std::path::Path::new(p).exists() {
                base = p;
                break;
            }
        }

        if base.is_empty() {
            tracing::error!("VM templates not found, skipping VM provisioning for agent {}", agent_id);
            return;
        }

        let tmpl_user_data = match tokio::fs::read_to_string(format!("{}/cloud-init/agent-user-data.yaml.tmpl", base)).await {
            Ok(t) => t,
            Err(e) => { tracing::error!("Failed to read cloud-init template: {}", e); return; }
        };
        let tmpl_openclaw_json = match tokio::fs::read_to_string(format!("{}/openclaw/openclaw.json.tmpl", base)).await {
            Ok(t) => t,
            Err(e) => { tracing::error!("Failed to read openclaw.json template: {}", e); return; }
        };
        let tmpl_openclaw_svc = match tokio::fs::read_to_string(format!("{}/systemd/openclaw-gateway.service.tmpl", base)).await {
            Ok(t) => t,
            Err(e) => { tracing::error!("Failed to read openclaw service template: {}", e); return; }
        };
        let tmpl_agentd_svc = match tokio::fs::read_to_string(format!("{}/systemd/multiclaw-agentd.service.tmpl", base)).await {
            Ok(t) => t,
            Err(e) => { tracing::error!("Failed to read agentd service template: {}", e); return; }
        };

        let vm_name = format!("mc-{}", &agent_id.to_string()[..8]);

        let args = CloudInitArgs {
            hostname: vm_name.clone(),
            host_ip: host_ip.clone(),
            agent_id: agent_id.to_string(),
            agent_name: agent_name.clone(),
            effective_model: model.clone(),
            agent_token,
            openclaw_gateway_token: gateway_token,
            ollama_token,
            tools_allow,
            tools_deny,
            tmpl_user_data,
            tmpl_openclaw_json,
            tmpl_openclaw_svc,
            tmpl_agentd_svc,
        };

        let cloud_init = match render_cloud_init(&args) {
            Ok(ci) => ci,
            Err(e) => { tracing::error!("Failed to render cloud-init: {}", e); return; }
        };

        let resources = VmResources {
            vcpus: 2,
            memory_mb: 2048,
            disk_gb: 20,
        };

        match provider.provision(&vm_name, &resources, &cloud_init).await {
            Ok(details) => {
                tracing::info!("VM '{}' provisioned for agent {}, ip={:?}", vm_name, agent_id, details.ip_address);
                // Insert record into vms table
                let vm_uuid = Uuid::new_v4();
                let resources_json = serde_json::json!({
                    "vcpus": resources.vcpus,
                    "memory_mb": resources.memory_mb,
                    "disk_gb": resources.disk_gb
                });
                let _ = sqlx::query(
                    "INSERT INTO vms (id, provider, provider_ref, hostname, ip_address, resources, state) \
                     VALUES ($1, 'incus', $2, $3, $4, $5, 'RUNNING')"
                )
                .bind(vm_uuid)
                .bind(&vm_name)
                .bind(&vm_name)
                .bind(&details.ip_address)
                .bind(&resources_json)
                .execute(&db).await;

                // Link agent to vm
                let _ = sqlx::query("UPDATE agents SET vm_id = $1 WHERE id = $2")
                    .bind(vm_uuid).bind(agent_id)
                    .execute(&db).await;

                let _ = tx.send(json!({
                    "type": "vm_provisioned",
                    "agent_id": agent_id,
                    "vm_id": vm_name,
                    "ip": details.ip_address
                }).to_string());
            }
            Err(e) => {
                tracing::error!("Failed to provision VM for agent {}: {}", agent_id, e);
                let _ = tx.send(json!({
                    "type": "vm_provision_failed",
                    "agent_id": agent_id,
                    "error": e.to_string()
                }).to_string());
            }
        }
    });
}

// ═══════════════════════════════════════════════════════════════
// VM Target Resolution
// ═══════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize)]
struct VmTargetQuery {
    target: Option<String>, // "desktop" (default) or "sandbox"
}

async fn resolve_vm_ref(db: &sqlx::PgPool, agent_id: Uuid, target: &str) -> Option<String> {
    if target == "sandbox" {
        sqlx::query_scalar(
            "SELECT v.provider_ref FROM vms v JOIN agents a ON a.sandbox_vm_id = v.id WHERE a.id = $1"
        ).bind(agent_id).fetch_optional(db).await.ok().flatten()
    } else {
        sqlx::query_scalar(
            "SELECT v.provider_ref FROM vms v JOIN agents a ON a.vm_id = v.id WHERE a.id = $1"
        ).bind(agent_id).fetch_optional(db).await.ok().flatten()
    }
}

// ═══════════════════════════════════════════════════════════════
// VM Actions
// ═══════════════════════════════════════════════════════════════

async fn vm_start(State(state): State<AppState>, Path(id): Path<String>, Query(q): Query<VmTargetQuery>) -> impl IntoResponse {
    let uid = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };
    if let Some(resp) = check_dm_mode(&state, uid).await { return resp; }
    let target = q.target.as_deref().unwrap_or("desktop");
    let vm_ref = resolve_vm_ref(&state.db, uid, target).await;
    match vm_ref {
        Some(ref name) => {
            if state.vm_provider.is_some() {
                let _ = tokio::process::Command::new("incus").args(&["start", name]).output().await;
                (StatusCode::ACCEPTED, Json(json!({"status":"vm_started","vm_name": name, "target": target})))
            } else {
                (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error":"VM provider not available"})))
            }
        }
        None => (StatusCode::NOT_FOUND, Json(json!({"error":"No VM assigned to this agent"})))
    }
}

async fn vm_stop(State(state): State<AppState>, Path(id): Path<String>, Query(q): Query<VmTargetQuery>) -> impl IntoResponse {
    let uid = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };
    if let Some(resp) = check_dm_mode(&state, uid).await { return resp; }
    let target = q.target.as_deref().unwrap_or("desktop");
    let vm_ref = resolve_vm_ref(&state.db, uid, target).await;
    match vm_ref {
        Some(ref name) => {
            if let Some(ref provider) = state.vm_provider {
                let _ = provider.stop(name).await;
                (StatusCode::ACCEPTED, Json(json!({"status":"vm_stopped","vm_name": name, "target": target})))
            } else {
                (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error":"VM provider not available"})))
            }
        }
        None => (StatusCode::NOT_FOUND, Json(json!({"error":"No VM assigned to this agent"})))
    }
}

async fn vm_rebuild(State(state): State<AppState>, Path(id): Path<String>, Query(q): Query<VmTargetQuery>) -> impl IntoResponse {
    let uid = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };
    if let Some(resp) = check_dm_mode(&state, uid).await { return resp; }
    let target = q.target.as_deref().unwrap_or("desktop");

    // Cannot wipe/rebuild the persistent desktop VM
    if target == "desktop" {
        return (StatusCode::FORBIDDEN, Json(json!({"error":"Cannot wipe the persistent desktop VM. Only sandbox VMs can be rebuilt."})));
    }

    let vm_ref = resolve_vm_ref(&state.db, uid, target).await;
    match vm_ref {
        Some(ref name) => {
            if let Some(ref provider) = state.vm_provider {
                let _ = provider.destroy(name).await;
                // Clean up: remove vms record and unlink sandbox from agent
                let _ = sqlx::query(
                    "DELETE FROM vms WHERE id = (SELECT sandbox_vm_id FROM agents WHERE id = $1)"
                ).bind(uid).execute(&state.db).await;
                let _ = sqlx::query("UPDATE agents SET sandbox_vm_id = NULL WHERE id = $1")
                    .bind(uid).execute(&state.db).await;
                (StatusCode::ACCEPTED, Json(json!({"status":"sandbox_destroyed_for_rebuild","vm_name": name})))
            } else {
                (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error":"VM provider not available"})))
            }
        }
        None => (StatusCode::NOT_FOUND, Json(json!({"error":"No sandbox VM assigned to this agent"})))
    }
}

/// Provision a VM on-demand for an agent (their "workstation")
async fn vm_provision(State(state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    let uid = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };
    if let Some(resp) = check_dm_mode(&state, uid).await { return resp; }

    // Check if agent already has a VM
    let existing_vm: Option<Uuid> = sqlx::query_scalar(
        "SELECT vm_id FROM agents WHERE id = $1"
    ).bind(uid).fetch_optional(&state.db).await.ok().flatten();

    if existing_vm.is_some() {
        return (StatusCode::CONFLICT, Json(json!({"error": "Agent already has a VM assigned", "vm_id": existing_vm})));
    }

    // Get agent info for provisioning
    let agent_info: Option<(String, String, Option<String>)> = sqlx::query_as(
        "SELECT name, effective_model, (SELECT name FROM tool_policies WHERE id = a.tool_policy_id) FROM agents a WHERE id = $1"
    ).bind(uid).fetch_optional(&state.db).await.ok().flatten();

    match agent_info {
        Some((name, model, policy_name)) => {
            let policy = policy_name.unwrap_or_else(|| "worker_policy".into());
            provision_agent_vm(state.clone(), uid, &name, &model, &policy).await;
            (StatusCode::ACCEPTED, Json(json!({"status": "provisioning", "agent_id": uid, "message": format!("VM provisioning started for {}", name)})))
        }
        None => (StatusCode::NOT_FOUND, Json(json!({"error": "Agent not found"})))
    }
}

/// Provision a sandbox VM for an agent (lightweight temp environment)
async fn vm_sandbox_provision(State(state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    let uid = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };
    if let Some(resp) = check_dm_mode(&state, uid).await { return resp; }

    // Check if agent already has a sandbox VM
    let existing: Option<Uuid> = sqlx::query_scalar(
        "SELECT sandbox_vm_id FROM agents WHERE id = $1"
    ).bind(uid).fetch_optional(&state.db).await.ok().flatten();

    if existing.is_some() {
        return (StatusCode::CONFLICT, Json(json!({"error": "Agent already has a sandbox VM assigned"})));
    }

    let provider = match state.vm_provider {
        Some(ref p) => p.clone(),
        None => return (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error":"VM provider not available"}))),
    };

    let db = state.db.clone();
    let tx = state.tx.clone();

    tokio::spawn(async move {
        let vm_name = format!("mc-{}-sb", &uid.to_string()[..8]);
        tracing::info!("Provisioning sandbox VM '{}' for agent {}", vm_name, uid);

        // Minimal cloud-init: just create agent user
        let cloud_init = format!(
            "#cloud-config\nhostname: {}\nusers:\n  - name: agent\n    shell: /bin/bash\n    sudo: ALL=(ALL) NOPASSWD:ALL\n    groups: sudo\npackage_update: true\npackages:\n  - curl\n  - git\n  - build-essential\n",
            vm_name
        );

        let resources = VmResources { vcpus: 1, memory_mb: 1024, disk_gb: 10 };

        match provider.provision(&vm_name, &resources, &cloud_init).await {
            Ok(details) => {
                tracing::info!("Sandbox VM '{}' provisioned for agent {}, ip={:?}", vm_name, uid, details.ip_address);
                let vm_uuid = Uuid::new_v4();
                let resources_json = serde_json::json!({
                    "vcpus": resources.vcpus, "memory_mb": resources.memory_mb, "disk_gb": resources.disk_gb
                });
                let _ = sqlx::query(
                    "INSERT INTO vms (id, provider, provider_ref, hostname, ip_address, resources, state, vm_type) \
                     VALUES ($1, 'incus', $2, $3, $4, $5, 'RUNNING', 'sandbox')"
                )
                .bind(vm_uuid).bind(&vm_name).bind(&vm_name)
                .bind(&details.ip_address).bind(&resources_json)
                .execute(&db).await;

                let _ = sqlx::query("UPDATE agents SET sandbox_vm_id = $1 WHERE id = $2")
                    .bind(vm_uuid).bind(uid).execute(&db).await;

                let _ = tx.send(json!({
                    "type": "sandbox_provisioned", "agent_id": uid,
                    "vm_id": vm_name, "ip": details.ip_address
                }).to_string());
            }
            Err(e) => {
                tracing::error!("Failed to provision sandbox VM for agent {}: {}", uid, e);
                let _ = tx.send(json!({
                    "type": "sandbox_provision_failed", "agent_id": uid, "error": e.to_string()
                }).to_string());
            }
        }
    });

    (StatusCode::ACCEPTED, Json(json!({"status": "provisioning_sandbox", "agent_id": uid})))
}

async fn agent_panic(State(state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    let uid = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };

    // 1. Update DB status to QUARANTINED
    let _ = sqlx::query("UPDATE agents SET status = 'QUARANTINED' WHERE id = $1").bind(uid).execute(&state.db).await;

    // 2. Stop the OpenClaw Docker container so the agent can't execute anything
    match state.openclaw.stop_instance(uid).await {
        Ok(()) => tracing::info!("Stopped OpenClaw instance for quarantined agent {}", uid),
        Err(e) => tracing::warn!("Failed to stop OpenClaw instance for agent {}: {} (may not have one)", uid, e),
    }

    // 3. Broadcast quarantine event to UI
    let _ = state.tx.send(json!({"type":"agent_quarantined","agent_id": uid}).to_string());
    (StatusCode::OK, Json(json!({"status":"quarantined","agent_id": uid})))
}

// ═══════════════════════════════════════════════════════════════
// VM Interaction: Exec, Info, File Push/Pull
// ═══════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize)]
struct VmExecRequest {
    command: String,
    user: Option<String>,
    working_dir: Option<String>,
    timeout_secs: Option<u64>,
}

async fn vm_exec(State(state): State<AppState>, Path(id): Path<String>, Query(q): Query<VmTargetQuery>, Json(body): Json<VmExecRequest>) -> impl IntoResponse {
    let uid = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };
    if let Some(resp) = check_dm_mode(&state, uid).await { return resp; }
    let target = q.target.as_deref().unwrap_or("desktop");
    let vm_ref = resolve_vm_ref(&state.db, uid, target).await;
    match vm_ref {
        Some(ref name) => {
            if let Some(ref provider) = state.vm_provider {
                match provider.exec_command(
                    name,
                    &body.command,
                    body.user.as_deref().or(Some("employee")),
                    body.working_dir.as_deref().or(Some("/home/employee")),
                    body.timeout_secs,
                ).await {
                    Ok(result) => (StatusCode::OK, Json(json!(result))),
                    Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))),
                }
            } else {
                (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error":"VM provider not available"})))
            }
        }
        None => (StatusCode::NOT_FOUND, Json(json!({"error":"No VM assigned to this agent"})))
    }
}

async fn vm_info(State(state): State<AppState>, Path(id): Path<String>, Query(q): Query<VmTargetQuery>) -> impl IntoResponse {
    let uid = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };
    let target = q.target.as_deref().unwrap_or("desktop");
    let vm_ref = resolve_vm_ref(&state.db, uid, target).await;
    match vm_ref {
        Some(ref name) => {
            if let Some(ref provider) = state.vm_provider {
                match provider.get_info(name).await {
                    Ok(info) => (StatusCode::OK, Json(json!(info))),
                    Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))),
                }
            } else {
                (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error":"VM provider not available"})))
            }
        }
        None => (StatusCode::NOT_FOUND, Json(json!({"error":"No VM assigned to this agent"})))
    }
}

#[derive(Debug, Deserialize)]
struct VmFilePushRequest {
    path: String,
    content: String,
    encoding: Option<String>, // "base64" or "text" (default)
}

async fn vm_file_push(State(state): State<AppState>, Path(id): Path<String>, Query(q): Query<VmTargetQuery>, Json(body): Json<VmFilePushRequest>) -> impl IntoResponse {
    let uid = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };
    if let Some(resp) = check_dm_mode(&state, uid).await { return resp; }
    let target = q.target.as_deref().unwrap_or("desktop");
    let vm_ref = resolve_vm_ref(&state.db, uid, target).await;
    match vm_ref {
        Some(ref name) => {
            if let Some(ref provider) = state.vm_provider {
                let bytes = if body.encoding.as_deref() == Some("base64") {
                    use base64::Engine;
                    match base64::engine::general_purpose::STANDARD.decode(&body.content) {
                        Ok(b) => b,
                        Err(e) => return (StatusCode::BAD_REQUEST, Json(json!({"error": format!("Invalid base64: {}", e)}))),
                    }
                } else {
                    body.content.into_bytes()
                };
                match provider.file_push(name, &bytes, &body.path).await {
                    Ok(()) => (StatusCode::OK, Json(json!({"status":"ok","path": body.path}))),
                    Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))),
                }
            } else {
                (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error":"VM provider not available"})))
            }
        }
        None => (StatusCode::NOT_FOUND, Json(json!({"error":"No VM assigned to this agent"})))
    }
}

#[derive(Debug, Deserialize)]
struct VmFilePullRequest {
    path: String,
}

async fn vm_file_pull(State(state): State<AppState>, Path(id): Path<String>, Query(q): Query<VmTargetQuery>, Json(body): Json<VmFilePullRequest>) -> impl IntoResponse {
    let uid = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };
    if let Some(resp) = check_dm_mode(&state, uid).await { return resp; }
    let target = q.target.as_deref().unwrap_or("desktop");

    // Sandbox is one-way: you can push files TO it, but not pull files FROM it.
    // This prevents experimental/untrusted code from flowing back to the persistent work computer.
    if target == "sandbox" {
        return (StatusCode::FORBIDDEN, Json(json!({
            "error": "Cannot pull files from your testing environment. Files on the testing environment are temporary and cannot be transferred to your workspace or work computer."
        })));
    }

    let vm_ref = resolve_vm_ref(&state.db, uid, target).await;
    match vm_ref {
        Some(ref name) => {
            if let Some(ref provider) = state.vm_provider {
                match provider.file_pull(name, &body.path).await {
                    Ok(content) => {
                        let text = String::from_utf8_lossy(&content).to_string();
                        (StatusCode::OK, Json(json!({"path": body.path, "content": text, "size": content.len()})))
                    }
                    Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))),
                }
            } else {
                (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error":"VM provider not available"})))
            }
        }
        None => (StatusCode::NOT_FOUND, Json(json!({"error":"No VM assigned to this agent"})))
    }
}

// ═══════════════════════════════════════════════════════════════
// Cross-Computer File Copy (Desktop → Sandbox)
// ═══════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize)]
struct VmCopyToSandboxRequest {
    src_path: String,
    dest_path: Option<String>,
}

async fn vm_copy_to_sandbox(State(state): State<AppState>, Path(id): Path<String>, Json(body): Json<VmCopyToSandboxRequest>) -> impl IntoResponse {
    let uid = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };
    if let Some(resp) = check_dm_mode(&state, uid).await { return resp; }

    let desktop_ref = resolve_vm_ref(&state.db, uid, "desktop").await;
    let sandbox_ref = resolve_vm_ref(&state.db, uid, "sandbox").await;

    let (desktop_name, sandbox_name) = match (desktop_ref, sandbox_ref) {
        (Some(d), Some(s)) => (d, s),
        (None, _) => return (StatusCode::NOT_FOUND, Json(json!({"error":"No personal work computer provisioned. Provision one first."}))),
        (_, None) => return (StatusCode::NOT_FOUND, Json(json!({"error":"No testing environment provisioned. Provision one first."}))),
    };

    let provider = match &state.vm_provider {
        Some(p) => p,
        None => return (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error":"VM provider not available"}))),
    };

    if body.src_path.contains("..") || body.src_path.trim().is_empty() {
        return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid src_path"})));
    }

    let dest = body.dest_path.as_deref().unwrap_or(&body.src_path);
    if dest.contains("..") || dest.trim().is_empty() {
        return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid dest_path"})));
    }

    let content = match provider.file_pull(&desktop_name, &body.src_path).await {
        Ok(c) => c,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("Failed to read file from work computer: {}", e)}))),
    };

    if content.len() as u64 > MAX_FILE_TRANSFER_BYTES {
        return (StatusCode::PAYLOAD_TOO_LARGE, Json(json!({"error": format!("File too large: {} bytes (max {} bytes)", content.len(), MAX_FILE_TRANSFER_BYTES)})));
    }

    match provider.file_push(&sandbox_name, &content, dest).await {
        Ok(()) => (StatusCode::OK, Json(json!({"status":"ok","src_path": body.src_path,"dest_path": dest,"size_bytes": content.len()}))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("Failed to write file to testing environment: {}", e)}))),
    }
}

// ═══════════════════════════════════════════════════════════════
// Threads & Messages
// ═══════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize)]
struct ThreadsQuery {
    agent_only: Option<bool>,
}

async fn get_threads(State(state): State<AppState>, Query(q): Query<ThreadsQuery>) -> impl IntoResponse {
    if q.agent_only.unwrap_or(false) {
        // Return only threads with agent members and NO user members (agent-to-agent only)
        match sqlx::query_as::<_, Thread>(
            "SELECT t.id, t.type, t.title, t.created_by_user_id, t.created_at \
             FROM threads t \
             WHERE EXISTS ( \
                 SELECT 1 FROM thread_members tm \
                 WHERE tm.thread_id = t.id AND tm.member_type = 'AGENT' \
             ) \
             AND NOT EXISTS ( \
                 SELECT 1 FROM thread_members tm2 \
                 WHERE tm2.thread_id = t.id AND tm2.member_type = 'USER' \
             ) \
             ORDER BY COALESCE((SELECT MAX(m.created_at) FROM messages m WHERE m.thread_id = t.id), t.created_at) DESC"
        ).fetch_all(&state.db).await {
            Ok(t) => (StatusCode::OK, Json(json!(t))),
            Err(_) => (StatusCode::OK, Json(json!([])))
        }
    } else {
        // Return only threads where the user is a member (excludes agent-only threads)
        match sqlx::query_as::<_, Thread>(
            "SELECT t.id, t.type, t.title, t.created_by_user_id, t.created_at \
             FROM threads t \
             WHERE EXISTS ( \
                 SELECT 1 FROM thread_members tm \
                 WHERE tm.thread_id = t.id AND tm.member_type = 'USER' \
             ) \
             ORDER BY COALESCE((SELECT MAX(m.created_at) FROM messages m WHERE m.thread_id = t.id), t.created_at) DESC"
        ).fetch_all(&state.db).await {
            Ok(t) => (StatusCode::OK, Json(json!(t))),
            Err(_) => (StatusCode::OK, Json(json!([])))
        }
    }
}

async fn get_thread(State(state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    let uid = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };
    match sqlx::query_as::<_, Thread>("SELECT id, type, title, created_by_user_id, created_at FROM threads WHERE id = $1")
        .bind(uid).fetch_optional(&state.db).await {
        Ok(Some(t)) => (StatusCode::OK, Json(json!(t))),
        _ => (StatusCode::NOT_FOUND, Json(json!({"error":"Thread not found"})))
    }
}

async fn create_thread(State(state): State<AppState>, Json(p): Json<CreateThreadRequest>) -> impl IntoResponse {
    // Deduplication for GROUP threads: reuse existing thread if one has the exact same member set
    if p.r#type == "GROUP" {
        if let Some(ref member_ids) = p.member_ids {
            if !member_ids.is_empty() {
                let mut sorted_ids: Vec<Uuid> = member_ids.iter().copied().collect();
                sorted_ids.sort();
                sorted_ids.dedup();
                let member_count = sorted_ids.len() as i64;

                // Find a GROUP thread with EXACTLY this member set (same IDs, same count)
                let existing: Option<(Uuid, Option<String>)> = sqlx::query_as(
                    "SELECT t.id, t.title FROM threads t \
                     WHERE t.type = 'GROUP' \
                       AND (SELECT COUNT(*) FROM thread_members tm WHERE tm.thread_id = t.id AND tm.member_type = 'AGENT') = $1 \
                       AND NOT EXISTS ( \
                           SELECT 1 FROM unnest($2::uuid[]) AS req_id \
                           WHERE req_id NOT IN (SELECT tm2.member_id FROM thread_members tm2 WHERE tm2.thread_id = t.id AND tm2.member_type = 'AGENT') \
                       ) \
                     LIMIT 1"
                ).bind(member_count).bind(&sorted_ids).fetch_optional(&state.db).await.ok().flatten();

                if let Some((existing_id, existing_title)) = existing {
                    return (StatusCode::OK, Json(json!({
                        "id": existing_id, "type": "GROUP", "title": existing_title, "deduplicated": true
                    })));
                }
            }
        }
    }

    // Enforce communication hierarchy for GROUP threads with agent members
    if p.r#type == "GROUP" {
        if let Some(ref member_ids) = p.member_ids {
            // Load role, company_id, parent_agent_id for each agent member
            let mut agents: Vec<(Uuid, String, Option<Uuid>, Option<Uuid>)> = Vec::new();
            for mid in member_ids {
                if let Ok(Some(row)) = sqlx::query_as::<_, (String, Option<Uuid>, Option<Uuid>)>(
                    "SELECT role, company_id, parent_agent_id FROM agents WHERE id = $1"
                ).bind(mid).fetch_optional(&state.db).await {
                    agents.push((*mid, row.0, row.1, row.2));
                }
            }
            // Check every pair: each must be allowed to DM the other
            for i in 0..agents.len() {
                for j in (i+1)..agents.len() {
                    let (id_a, ref role_a, company_a, parent_a) = agents[i];
                    let (id_b, ref role_b, company_b, parent_b) = agents[j];
                    let allowed =
                        role_a == "MAIN" || role_b == "MAIN"
                        // Direct parent-child
                        || parent_a == Some(id_b)
                        || parent_b == Some(id_a)
                        // CEO can reach managers in their company
                        || (role_a == "CEO" && role_b == "MANAGER" && company_a == company_b && company_a.is_some())
                        || (role_b == "CEO" && role_a == "MANAGER" && company_a == company_b && company_a.is_some())
                        // Peers under the same parent in the same company
                        || (company_a == company_b && parent_a == parent_b && company_a.is_some() && parent_a.is_some());
                    if !allowed {
                        let name_a: String = sqlx::query_scalar("SELECT name FROM agents WHERE id = $1")
                            .bind(id_a).fetch_optional(&state.db).await.ok().flatten().unwrap_or_default();
                        let name_b: String = sqlx::query_scalar("SELECT name FROM agents WHERE id = $1")
                            .bind(id_b).fetch_optional(&state.db).await.ok().flatten().unwrap_or_default();
                        return (StatusCode::FORBIDDEN, Json(json!({
                            "error": format!("{} and {} are not in the same chain of command. Group chats can only include agents who are allowed to communicate directly.", name_a, name_b)
                        })));
                    }
                }
            }
        }
    }

    let id = Uuid::new_v4();
    let _ = sqlx::query("INSERT INTO threads (id, type, title) VALUES ($1, $2, $3)")
        .bind(id).bind(&p.r#type).bind(&p.title).execute(&state.db).await;
    // Auto-add members if provided
    if let Some(member_ids) = &p.member_ids {
        for mid in member_ids {
            let _ = sqlx::query(
                "INSERT INTO thread_members (thread_id, member_type, member_id) VALUES ($1, 'AGENT', $2) \
                 ON CONFLICT DO NOTHING"
            ).bind(id).bind(mid).execute(&state.db).await;
        }
    }
    (StatusCode::CREATED, Json(json!({"id": id, "type": p.r#type, "title": p.title})))
}

async fn get_messages(State(state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    let uid = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };
    match sqlx::query_as::<_, Message>("SELECT id, thread_id, sender_type, sender_id, content, reply_depth, created_at FROM messages WHERE thread_id = $1 ORDER BY created_at ASC")
        .bind(uid).fetch_all(&state.db).await {
        Ok(m) => (StatusCode::OK, Json(json!(m))),
        Err(_) => (StatusCode::OK, Json(json!([])))
    }
}

async fn send_message(
    State(state): State<AppState>, Path(id): Path<String>, Json(p): Json<CreateMessageRequest>
) -> impl IntoResponse {
    let thread_id = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };
    let msg_id = Uuid::new_v4();
    let mut sender_id = p.sender_id.unwrap_or_else(Uuid::new_v4);
    // Auto-detect sender_type: if sender_id matches a known agent, force "AGENT".
    // Models sometimes omit sender_type from their curl commands, causing agent
    // messages to be mislabeled as "USER".
    let sender_type = if p.sender_type.as_deref() == Some("AGENT") {
        "AGENT".to_string()
    } else if p.sender_type.as_deref() == Some("USER") || p.sender_type.as_deref() == Some("SYSTEM") {
        // Explicitly set to USER or SYSTEM — respect it
        p.sender_type.unwrap()
    } else {
        // sender_type was omitted — auto-detect from sender_id
        let is_agent: bool = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM agents WHERE id = $1)"
        ).bind(sender_id).fetch_one(&state.db).await.unwrap_or(false);
        if is_agent {
            "AGENT".to_string()
        } else {
            // sender_id didn't match an agent either — check if this thread has agent
            // members. If so, this is likely an agent that omitted both fields.
            // Human operators posting via the UI always have sender_type explicitly set.
            let has_agent_members: bool = sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS(SELECT 1 FROM thread_members WHERE thread_id = $1 AND member_type = 'AGENT')"
            ).bind(thread_id).fetch_one(&state.db).await.unwrap_or(false);
            if has_agent_members && p.sender_id.is_none() {
                // Try to resolve actual agent ID from thread members so the UI
                // can display the agent's name instead of generic "Agent".
                let agent_ids: Vec<Uuid> = sqlx::query_scalar(
                    "SELECT member_id FROM thread_members WHERE thread_id = $1 AND member_type = 'AGENT'"
                ).bind(thread_id).fetch_all(&state.db).await.unwrap_or_default();
                if agent_ids.len() == 1 {
                    sender_id = agent_ids[0];
                } else if !agent_ids.is_empty() {
                    // Multi-agent thread — pick the agent currently marked as working
                    let activities = state.agent_activities.read().await;
                    if let Some(ref map) = *activities {
                        if let Some(&aid) = agent_ids.iter().find(|id| {
                            map.get(id).map(|a| a.status == "WORKING").unwrap_or(false)
                        }) {
                            sender_id = aid;
                        }
                    }
                }
                tracing::warn!("Message on thread {} missing sender_id and sender_type — defaulting to AGENT (resolved: {})", thread_id, sender_id);
                "AGENT".to_string()
            } else {
                "USER".to_string()
            }
        }
    };
    let reply_depth = p.reply_depth.unwrap_or(0);

    // Strip system tags and scrub secrets from agent-sent messages before storing
    let content = if sender_type == "AGENT" {
        if let Some(text) = p.content.get("text").and_then(|v| v.as_str()) {
            let (tag_cleaned, _) = strip_agent_tags(text);
            let scrubbed = if let Some(ref crypto) = state.crypto {
                scrub_secrets(&state.db, crypto, sender_id, &tag_cleaned).await
            } else { tag_cleaned };
            if scrubbed.trim().is_empty() {
                return (StatusCode::OK, Json(json!({"status": "empty_after_cleaning"})));
            }
            json!({"text": scrubbed})
        } else { p.content.clone() }
    } else { p.content.clone() };

    match sqlx::query_as::<_, Message>(
        "INSERT INTO messages (id, thread_id, sender_type, sender_id, content, reply_depth) VALUES ($1,$2,$3,$4,$5,$6) \
         RETURNING id, thread_id, sender_type, sender_id, content, reply_depth, created_at"
    ).bind(msg_id).bind(thread_id).bind(&sender_type).bind(sender_id).bind(&content).bind(reply_depth)
    .fetch_one(&state.db).await {
        Ok(msg) => {
            let _ = state.tx.send(json!({"type":"new_message","message": msg}).to_string());

            // Trigger agent responses for USER or AGENT senders (with depth-based loop prevention)
            if (sender_type == "USER" || sender_type == "AGENT") && reply_depth < MAX_THREAD_REPLY_DEPTH {
                let user_text = p.content.get("text")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                if !user_text.is_empty() {
                    let is_agent_sender = sender_type == "AGENT";

                    // Resolve which agents should respond (agent routing logic)
                    let thread_type: String = sqlx::query_scalar("SELECT type FROM threads WHERE id = $1")
                        .bind(thread_id).fetch_optional(&state.db).await.ok().flatten()
                        .unwrap_or_else(|| "DM".to_string());

                    let agent_ids: Vec<Uuid> = sqlx::query_scalar(
                        "SELECT member_id FROM thread_members WHERE thread_id = $1 AND member_type = 'AGENT'"
                    ).bind(thread_id).fetch_all(&state.db).await.unwrap_or_default();

                    let mut responding_agents: Vec<Uuid> = if thread_type == "GROUP" {
                        let mut mentioned: Vec<Uuid> = Vec::new();
                        for aid in &agent_ids {
                            let agent_info: Option<(String, Option<String>)> = sqlx::query_as(
                                "SELECT name, handle FROM agents WHERE id = $1"
                            ).bind(aid).fetch_optional(&state.db).await.ok().flatten();
                            if let Some((name, handle)) = agent_info {
                                let lower_text = user_text.to_lowercase();
                                if lower_text.contains(&format!("@{}", name.to_lowercase().replace(' ', "-")))
                                    || handle.as_ref().map(|h| lower_text.contains(&h.to_lowercase())).unwrap_or(false)
                                    || lower_text.contains(&name.to_lowercase())
                                {
                                    mentioned.push(*aid);
                                }
                            }
                        }
                        if mentioned.is_empty() {
                            agent_ids.iter().take(3).cloned().collect()
                        } else {
                            mentioned.into_iter().take(3).collect()
                        }
                    } else {
                        if let Some(aid) = agent_ids.first() {
                            vec![*aid]
                        } else {
                            let main_id: Option<Uuid> = sqlx::query_scalar(
                                "SELECT id FROM agents WHERE role = 'MAIN' LIMIT 1"
                            ).fetch_optional(&state.db).await.ok().flatten();
                            main_id.into_iter().collect()
                        }
                    };

                    if is_agent_sender {
                        responding_agents.retain(|id| *id != sender_id);
                    }

                    // Enqueue one thread_reply per responding agent
                    let priority = if sender_type == "USER" { 1i16 } else { 3 };
                    for responding_agent_id in responding_agents {
                        let _ = state.enqueue_message(
                            responding_agent_id,
                            priority,
                            "thread_reply",
                            json!({
                                "thread_id": thread_id.to_string(),
                                "message_text": user_text,
                                "sender_id": sender_id.to_string(),
                                "sender_type": sender_type,
                                "reply_depth": reply_depth,
                                "responding_agent_id": responding_agent_id.to_string(),
                            }),
                        ).await;
                    }
                }
            }

            (StatusCode::CREATED, Json(json!(msg)))
        },
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("{}", e)})))
    }
}

// ═══════════════════════════════════════════════════════════════
// Requests & Approvals
// ═══════════════════════════════════════════════════════════════

#[derive(Deserialize)]
struct RequestQuery { status: Option<String>, approver_type: Option<String> }

async fn list_requests(State(state): State<AppState>, Query(q): Query<RequestQuery>) -> impl IntoResponse {
    let status_filter = q.status.unwrap_or_else(|| "%".into());
    let approver_filter = q.approver_type.unwrap_or_else(|| "%".into());
    match sqlx::query_as::<_, Request>(
        "SELECT id, type, created_by_agent_id, created_by_user_id, company_id, payload, status, current_approver_type, current_approver_id, created_at, updated_at \
         FROM requests WHERE status LIKE $1 AND current_approver_type LIKE $2 ORDER BY created_at DESC"
    ).bind(&status_filter).bind(&approver_filter).fetch_all(&state.db).await {
        Ok(r) => (StatusCode::OK, Json(json!(r))),
        Err(_) => (StatusCode::OK, Json(json!([])))
    }
}

/// Find an agent's direct superior in the chain of command.
/// Worker → Manager, Manager → CEO, CEO → MAIN, MAIN → None (user).
async fn find_superior(db: &sqlx::PgPool, agent_id: Uuid) -> Option<Uuid> {
    // First check parent_agent_id
    let parent: Option<Uuid> = sqlx::query_scalar("SELECT parent_agent_id FROM agents WHERE id = $1")
        .bind(agent_id).fetch_optional(db).await.ok().flatten();
    if parent.is_some() {
        return parent;
    }
    // No parent — check if this is a CEO (route to MAIN) or MAIN (route to user/None)
    let role: Option<String> = sqlx::query_scalar("SELECT role FROM agents WHERE id = $1")
        .bind(agent_id).fetch_optional(db).await.ok().flatten();
    match role.as_deref() {
        Some("CEO") => {
            // Route to MAIN agent
            sqlx::query_scalar("SELECT id FROM agents WHERE role = 'MAIN' LIMIT 1")
                .fetch_optional(db).await.ok().flatten()
        }
        _ => None, // MAIN agent or unknown — route to user
    }
}

async fn create_request(State(state): State<AppState>, Json(p): Json<CreateRequestPayload>) -> impl IntoResponse {
    let id = Uuid::new_v4();
    let requester_id = p.requester_id;
    if let Some(resp) = check_dm_mode(&state, requester_id).await { return resp; }

    // Scrub secrets from request payload
    let payload = if let Some(ref crypto) = state.crypto {
        let payload_str = p.payload.to_string();
        let scrubbed = scrub_secrets(&state.db, crypto, requester_id, &payload_str).await;
        serde_json::from_str(&scrubbed).unwrap_or(p.payload.clone())
    } else { p.payload.clone() };

    // Reject duplicate REQUEST_TOOL submissions — same agent + same tool_name still PENDING/APPROVED
    if p.r#type == "REQUEST_TOOL" {
        if let Some(tool_name) = payload.get("tool_name").and_then(|v| v.as_str()) {
            let existing: Option<Uuid> = sqlx::query_scalar(
                "SELECT id FROM requests \
                 WHERE created_by_agent_id = $1 AND type = 'REQUEST_TOOL' \
                   AND status IN ('PENDING', 'APPROVED') \
                   AND payload->>'tool_name' = $2 \
                 LIMIT 1"
            )
            .bind(requester_id).bind(tool_name)
            .fetch_optional(&state.db).await.ok().flatten();

            if let Some(existing_id) = existing {
                return (StatusCode::CONFLICT, Json(json!({
                    "error": format!("You already have a pending request for '{}' (request {})", tool_name, existing_id),
                    "existing_request_id": existing_id
                })));
            }
        }
    }

    // Route to requester's superior in the chain of command
    let (approver_type, approver_id) = match find_superior(&state.db, requester_id).await {
        Some(superior_id) => ("AGENT".to_string(), Some(superior_id)),
        None => ("USER".to_string(), None), // MAIN agent or fallback → user
    };

    let _ = sqlx::query(
        "INSERT INTO requests (id, type, created_by_agent_id, company_id, payload, status, current_approver_type, current_approver_id) \
         VALUES ($1,$2,$3,$4,$5,'PENDING',$6,$7)"
    ).bind(id).bind(&p.r#type).bind(requester_id).bind(p.company_id).bind(&payload)
     .bind(&approver_type).bind(approver_id)
     .execute(&state.db).await;

    if approver_type == "USER" {
        // Only notify the user UI for user-targeted requests
        let _ = state.tx.send(json!({"type":"new_request","request_id": id}).to_string());
    } else if let Some(superior_id) = approver_id {
        // DM the approver agent about the pending request
        let requester_name: String = sqlx::query_scalar("SELECT name FROM agents WHERE id = $1")
            .bind(requester_id).fetch_optional(&state.db).await.ok().flatten().unwrap_or_else(|| "An agent".into());
        let description = payload.get("description").and_then(|v| v.as_str()).unwrap_or("(no description)");
        let dm_text = format!(
            "APPROVAL REQUEST from {}: \"{}\"\n\nRequest ID: {}\nType: {}\n\n\
             To approve: curl -s -X POST $MULTICLAW_API_URL/v1/requests/{}/agent-approve \
             -H 'Content-Type: application/json' -d '{{\"agent_id\": \"$AGENT_ID\"}}'\n\n\
             To reject: curl -s -X POST $MULTICLAW_API_URL/v1/requests/{}/agent-reject \
             -H 'Content-Type: application/json' -d '{{\"agent_id\": \"$AGENT_ID\"}}'",
            requester_name, description, id, p.r#type, id, id
        );
        let dm_thread = find_or_create_agent_dm_thread(&state.db, requester_id, superior_id).await;
        insert_system_message_in_thread(&state, dm_thread, requester_id, &dm_text).await;
        let _ = state.enqueue_message(
            superior_id, 2, "approval_escalate",
            json!({"agent_id": superior_id.to_string(), "message": dm_text, "task_label": "Processing approval request", "thread_id": dm_thread.to_string()}),
        ).await;
    }

    (StatusCode::CREATED, Json(json!({"id": id, "status":"PENDING", "approver_type": approver_type})))
}

/// Post-approval hook: if this is a REQUEST_TOOL, instruct MAIN to create (or update) the tool.
async fn trigger_tool_creation(state: &AppState, request_id: Uuid) {
    let request_type: Option<String> = sqlx::query_scalar("SELECT type FROM requests WHERE id = $1")
        .bind(request_id).fetch_optional(&state.db).await.ok().flatten();

    if request_type.as_deref() == Some("REQUEST_TOOL") {
        let row: Option<(Uuid, Value)> = sqlx::query_as(
            "SELECT created_by_agent_id, payload FROM requests WHERE id = $1"
        ).bind(request_id).fetch_optional(&state.db).await.ok().flatten();

        if let Some((requester_id, payload)) = row {
            let tool_name = payload.get("tool_name").and_then(|v| v.as_str()).unwrap_or("unknown");
            let description = payload.get("description").and_then(|v| v.as_str()).unwrap_or("");
            let use_case = payload.get("use_case").and_then(|v| v.as_str()).unwrap_or("");
            let issue = payload.get("issue").and_then(|v| v.as_str()).unwrap_or("");
            let requester_name: String = sqlx::query_scalar("SELECT name FROM agents WHERE id = $1")
                .bind(requester_id).fetch_optional(&state.db).await.ok().flatten()
                .unwrap_or_else(|| requester_id.to_string());

            // Sanitize tool_name the same way main_agent does
            let sanitized_name: String = tool_name.chars()
                .filter(|c| c.is_alphanumeric() || *c == '-')
                .collect();

            // Check if the tool already exists on disk — if so, this is an update
            let data_dir = std::env::var("MULTICLAW_OPENCLAW_DATA")
                .unwrap_or_else(|_| "/opt/multiclaw/openclaw-data".into());
            let skill_path = std::path::PathBuf::from(&data_dir)
                .join(requester_id.to_string())
                .join("workspace").join("skills").join(&sanitized_name).join("SKILL.md");

            let msg = if skill_path.exists() {
                let existing_content = tokio::fs::read_to_string(&skill_path).await.unwrap_or_default();
                let issue_line = if issue.is_empty() {
                    String::new()
                } else {
                    format!("Issue reported: {}. ", issue)
                };
                format!(
                    "Tool request {} has been approved. \
                     UPDATE existing tool '{}' for agent {} (ID: {}). \
                     {}Description: {}. Use case: {}.\n\
                     Current SKILL.md content:\n---\n{}\n---\n\
                     Use create_tool_for_agent to write the corrected/improved SKILL.md and deliver it.",
                    request_id, tool_name, requester_name, requester_id,
                    issue_line, description, use_case, existing_content
                )
            } else {
                format!(
                    "Tool request {} has been approved. \
                     Create tool '{}' for agent {} (ID: {}). \
                     Description: {}. Use case: {}. \
                     Use create_tool_for_agent to generate a complete SKILL.md with usage instructions and deliver it.",
                    request_id, tool_name, requester_name, requester_id, description, use_case
                )
            };

            // Find the MAIN agent to enqueue the tool creation prompt
            let main_id: Option<Uuid> = sqlx::query_scalar("SELECT id FROM agents WHERE role = 'MAIN' LIMIT 1")
                .fetch_optional(&state.db).await.ok().flatten();
            if let Some(mid) = main_id {
                let _ = state.enqueue_message(
                    mid, 2, "generic_send",
                    json!({
                        "agent_id": mid.to_string(),
                        "message": msg,
                        "task_label": "Creating tool for agent",
                    }),
                ).await;
            } else {
                tracing::error!("No MAIN agent found for tool creation request {}", request_id);
            }
        }
    }
}

async fn approve_request(State(state): State<AppState>, Path(id): Path<String>, body: Option<Json<ApprovalAction>>) -> impl IntoResponse {
    let uid = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };
    // Only allow user approval on requests targeting the user
    let approver_type: Option<String> = sqlx::query_scalar("SELECT current_approver_type FROM requests WHERE id = $1")
        .bind(uid).fetch_optional(&state.db).await.ok().flatten();
    if approver_type.as_deref() != Some("USER") {
        return (StatusCode::FORBIDDEN, Json(json!({"error":"This request is not awaiting user approval"})));
    }
    let note = body.and_then(|b| b.note.clone());
    let approval_id = Uuid::new_v4();
    let _ = sqlx::query("INSERT INTO approvals (id, request_id, approver_type, approver_id, decision, note) VALUES ($1,$2,'USER',$3,'APPROVE',$4)")
        .bind(approval_id).bind(uid).bind(Uuid::new_v4()).bind(&note).execute(&state.db).await;
    let _ = sqlx::query("UPDATE requests SET status = 'APPROVED', updated_at = NOW() WHERE id = $1").bind(uid).execute(&state.db).await;
    let _ = state.tx.send(json!({"type":"request_approved","request_id": uid}).to_string());
    // Notify the requester agent
    notify_requester(&state, uid, "APPROVED", note.as_deref()).await;

    trigger_tool_creation(&state, uid).await;

    (StatusCode::OK, Json(json!({"status":"approved"})))
}

async fn reject_request(State(state): State<AppState>, Path(id): Path<String>, body: Option<Json<ApprovalAction>>) -> impl IntoResponse {
    let uid = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };
    // Only allow user rejection on requests targeting the user
    let approver_type: Option<String> = sqlx::query_scalar("SELECT current_approver_type FROM requests WHERE id = $1")
        .bind(uid).fetch_optional(&state.db).await.ok().flatten();
    if approver_type.as_deref() != Some("USER") {
        return (StatusCode::FORBIDDEN, Json(json!({"error":"This request is not awaiting user approval"})));
    }
    let note = body.and_then(|b| b.note.clone());
    let approval_id = Uuid::new_v4();
    let _ = sqlx::query("INSERT INTO approvals (id, request_id, approver_type, approver_id, decision, note) VALUES ($1,$2,'USER',$3,'REJECT',$4)")
        .bind(approval_id).bind(uid).bind(Uuid::new_v4()).bind(&note).execute(&state.db).await;
    let _ = sqlx::query("UPDATE requests SET status = 'REJECTED', updated_at = NOW() WHERE id = $1").bind(uid).execute(&state.db).await;
    let _ = state.tx.send(json!({"type":"request_rejected","request_id": uid}).to_string());
    // Notify the requester agent
    notify_requester(&state, uid, "REJECTED", note.as_deref()).await;
    (StatusCode::OK, Json(json!({"status":"rejected"})))
}

/// Notify the original requester agent about request outcome.
async fn notify_requester(state: &AppState, request_id: Uuid, decision: &str, note: Option<&str>) {
    let requester_id: Option<Uuid> = sqlx::query_scalar("SELECT created_by_agent_id FROM requests WHERE id = $1")
        .bind(request_id).fetch_optional(&state.db).await.ok().flatten();
    if let Some(agent_id) = requester_id {
        let req_type: String = sqlx::query_scalar("SELECT type FROM requests WHERE id = $1")
            .bind(request_id).fetch_optional(&state.db).await.ok().flatten().unwrap_or_default();
        let note_text = note.map(|n| format!(" Note: {}", n)).unwrap_or_default();
        let availability_caveat = if decision == "APPROVED" {
            match req_type.as_str() {
                "INCREASE_MANAGER_LIMIT" => " Your manager hiring limit has been increased. \
                    You can now retry the hire-manager command to complete the hire.",
                "INCREASE_WORKER_LIMIT" => " Your worker hiring limit has been increased. \
                    You can now retry the hire-worker command to complete the hire.",
                "REQUEST_TOOL" => " Your tool has been created/updated and is available \
                    in your /workspace/skills/ directory. Check your skills folder.",
                _ => " IMPORTANT: Approval does NOT mean credentials or resources are available yet. \
                    The operator must still provision them via the Secrets page. Do NOT tell your team \
                    credentials are active until you verify via the secrets API \
                    (GET /v1/agents/{your-id}/secrets).",
            }
        } else { "" };
        let msg = format!(
            "Your request \"{}\" (ID: {}) has been {}.{}{}",
            req_type.replace('_', " "), request_id, decision, note_text, availability_caveat
        );

        // Find the most recent AGENT approver to determine the DM thread
        let last_agent_approver: Option<Uuid> = sqlx::query_scalar(
            "SELECT approver_id FROM approvals \
             WHERE request_id = $1 AND approver_type = 'AGENT' \
             ORDER BY created_at DESC LIMIT 1"
        ).bind(request_id).fetch_optional(&state.db).await.ok().flatten();

        let mut payload = json!({
            "agent_id": agent_id.to_string(),
            "message": msg,
            "task_label": "Processing approval decision"
        });

        if let Some(approver_agent_id) = last_agent_approver {
            let dm_thread = find_or_create_agent_dm_thread(&state.db, agent_id, approver_agent_id).await;
            // SYSTEM message insertion moved to handle_hire_notify so it appears
            // after the approver's response (which is stored by handle_generic_send).
            payload.as_object_mut().unwrap().insert(
                "thread_id".to_string(),
                json!(dm_thread.to_string()),
            );
            payload.as_object_mut().unwrap().insert(
                "approver_id".to_string(),
                json!(approver_agent_id.to_string()),
            );
        }

        let _ = state.enqueue_message(
            agent_id, 2, "hire_notify", payload,
        ).await;
    }
}

/// Agent approves a subordinate's request. If the approving agent is MAIN, the request is
/// fully approved. Otherwise, it escalates to the next superior in the chain.
async fn agent_approve_request(State(state): State<AppState>, Path(id): Path<String>, Json(p): Json<AgentApprovalAction>) -> impl IntoResponse {
    let request_id = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };
    if let Some(resp) = check_dm_mode(&state, p.agent_id).await { return resp; }

    // Verify this agent is the current approver
    let current: Option<(String, Option<Uuid>)> = sqlx::query_as(
        "SELECT current_approver_type, current_approver_id FROM requests WHERE id = $1 AND status = 'PENDING'"
    ).bind(request_id).fetch_optional(&state.db).await.ok().flatten();
    match &current {
        Some((t, Some(aid))) if t == "AGENT" && *aid == p.agent_id => {},
        _ => return (StatusCode::FORBIDDEN, Json(json!({"error":"You are not the current approver for this request"}))),
    }

    // Record this approval step
    let approval_id = Uuid::new_v4();
    let _ = sqlx::query("INSERT INTO approvals (id, request_id, approver_type, approver_id, decision, note) VALUES ($1,$2,'AGENT',$3,'APPROVE',$4)")
        .bind(approval_id).bind(request_id).bind(p.agent_id).bind(&p.note).execute(&state.db).await;

    // Check approver's role to decide: approve or escalate
    let approver_role: Option<String> = sqlx::query_scalar("SELECT role FROM agents WHERE id = $1")
        .bind(p.agent_id).fetch_optional(&state.db).await.ok().flatten();

    if approver_role.as_deref() == Some("MAIN") {
        // Check if User is required in the approver chain (stored in payload)
        let req_payload: Option<Value> = sqlx::query_scalar(
            "SELECT payload FROM requests WHERE id = $1"
        ).bind(request_id).fetch_optional(&state.db).await.ok().flatten();

        let needs_user = req_payload.as_ref()
            .and_then(|p| p.get("approver_chain"))
            .and_then(|c| c.as_array())
            .map(|arr| arr.iter().any(|v| v.as_str() == Some("User")))
            .unwrap_or(false);

        if needs_user {
            // Escalate to user — MAIN approved but User is also required
            let _ = sqlx::query(
                "UPDATE requests SET current_approver_type = 'USER', current_approver_id = NULL, updated_at = NOW() WHERE id = $1"
            ).bind(request_id).execute(&state.db).await;
            let _ = state.tx.send(json!({"type":"new_request","request_id": request_id}).to_string());
            (StatusCode::OK, Json(json!({"status":"escalated_to_user"})))
        } else {
            // MAIN has final authority — approve the request
            let _ = sqlx::query("UPDATE requests SET status = 'APPROVED', updated_at = NOW() WHERE id = $1")
                .bind(request_id).execute(&state.db).await;
            let _ = state.tx.send(json!({"type":"request_approved","request_id": request_id}).to_string());
            notify_requester(&state, request_id, "APPROVED", p.note.as_deref()).await;
            trigger_tool_creation(&state, request_id).await;
            (StatusCode::OK, Json(json!({"status":"approved"})))
        }
    } else {
        // Escalate to this agent's superior
        match find_superior(&state.db, p.agent_id).await {
            Some(next_superior_id) => {
                let _ = sqlx::query("UPDATE requests SET current_approver_id = $1, updated_at = NOW() WHERE id = $2")
                    .bind(next_superior_id).bind(request_id).execute(&state.db).await;

                // DM the next approver
                let approver_name: String = sqlx::query_scalar("SELECT name FROM agents WHERE id = $1")
                    .bind(p.agent_id).fetch_optional(&state.db).await.ok().flatten().unwrap_or_else(|| "Agent".into());
                let req_type: String = sqlx::query_scalar("SELECT type FROM requests WHERE id = $1")
                    .bind(request_id).fetch_optional(&state.db).await.ok().flatten().unwrap_or_default();
                let payload: Option<Value> = sqlx::query_scalar("SELECT payload FROM requests WHERE id = $1")
                    .bind(request_id).fetch_optional(&state.db).await.ok().flatten();
                let description = payload.as_ref().and_then(|p| p.get("description")).and_then(|v| v.as_str()).unwrap_or("(no description)");

                let dm_text = format!(
                    "APPROVAL REQUEST (escalated, approved by {}): \"{}\"\n\nRequest ID: {}\nType: {}\n\n\
                     To approve: curl -s -X POST $MULTICLAW_API_URL/v1/requests/{}/agent-approve \
                     -H 'Content-Type: application/json' -d '{{\"agent_id\": \"$AGENT_ID\"}}'\n\n\
                     To reject: curl -s -X POST $MULTICLAW_API_URL/v1/requests/{}/agent-reject \
                     -H 'Content-Type: application/json' -d '{{\"agent_id\": \"$AGENT_ID\"}}'",
                    approver_name, description, request_id, req_type, request_id, request_id
                );
                let dm_thread = find_or_create_agent_dm_thread(&state.db, p.agent_id, next_superior_id).await;
                insert_system_message_in_thread(&state, dm_thread, p.agent_id, &dm_text).await;
                let _ = state.enqueue_message(
                    next_superior_id, 2, "approval_escalate",
                    json!({"agent_id": next_superior_id.to_string(), "message": dm_text, "task_label": "Processing escalated approval", "thread_id": dm_thread.to_string()}),
                ).await;
                (StatusCode::OK, Json(json!({"status":"escalated","next_approver_id": next_superior_id})))
            }
            None => {
                // No superior found (shouldn't happen for non-MAIN agents, but handle gracefully)
                // Escalate to user
                let _ = sqlx::query("UPDATE requests SET current_approver_type = 'USER', current_approver_id = NULL, updated_at = NOW() WHERE id = $1")
                    .bind(request_id).execute(&state.db).await;
                let _ = state.tx.send(json!({"type":"new_request","request_id": request_id}).to_string());
                (StatusCode::OK, Json(json!({"status":"escalated_to_user"})))
            }
        }
    }
}

/// Agent rejects a subordinate's request. The request is marked as rejected immediately.
async fn agent_reject_request(State(state): State<AppState>, Path(id): Path<String>, Json(p): Json<AgentApprovalAction>) -> impl IntoResponse {
    let request_id = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };
    if let Some(resp) = check_dm_mode(&state, p.agent_id).await { return resp; }

    // Verify this agent is the current approver
    let current: Option<(String, Option<Uuid>)> = sqlx::query_as(
        "SELECT current_approver_type, current_approver_id FROM requests WHERE id = $1 AND status = 'PENDING'"
    ).bind(request_id).fetch_optional(&state.db).await.ok().flatten();
    match &current {
        Some((t, Some(aid))) if t == "AGENT" && *aid == p.agent_id => {},
        _ => return (StatusCode::FORBIDDEN, Json(json!({"error":"You are not the current approver for this request"}))),
    }

    // Record rejection and update status
    let approval_id = Uuid::new_v4();
    let _ = sqlx::query("INSERT INTO approvals (id, request_id, approver_type, approver_id, decision, note) VALUES ($1,$2,'AGENT',$3,'REJECT',$4)")
        .bind(approval_id).bind(request_id).bind(p.agent_id).bind(&p.note).execute(&state.db).await;
    let _ = sqlx::query("UPDATE requests SET status = 'REJECTED', updated_at = NOW() WHERE id = $1")
        .bind(request_id).execute(&state.db).await;

    // Notify requester
    notify_requester(&state, request_id, "REJECTED", p.note.as_deref()).await;
    (StatusCode::OK, Json(json!({"status":"rejected"})))
}

// ═══════════════════════════════════════════════════════════════
// Services & Engagements
// ═══════════════════════════════════════════════════════════════

async fn list_services(State(state): State<AppState>) -> impl IntoResponse {
    match sqlx::query_as::<_, ServiceCatalogItem>(
        "SELECT id, provider_company_id, name, description, pricing_model, rate, tags, active, created_at FROM service_catalog WHERE active = true"
    ).fetch_all(&state.db).await {
        Ok(s) => (StatusCode::OK, Json(json!(s))),
        Err(_) => (StatusCode::OK, Json(json!([])))
    }
}

async fn create_service(State(state): State<AppState>, Json(p): Json<CreateServiceRequest>) -> impl IntoResponse {
    let id = Uuid::new_v4();
    let _ = sqlx::query(
        "INSERT INTO service_catalog (id, provider_company_id, name, description, pricing_model, rate) VALUES ($1,$2,$3,$4,$5,$6)"
    ).bind(id).bind(p.provider_company_id).bind(&p.name).bind(&p.description).bind(&p.pricing_model).bind(&p.rate)
    .execute(&state.db).await;
    (StatusCode::CREATED, Json(json!({"id": id})))
}

async fn create_engagement(State(state): State<AppState>, Json(p): Json<CreateEngagementRequest>) -> impl IntoResponse {
    let thread_id = Uuid::new_v4();
    let _ = sqlx::query("INSERT INTO threads (id, type, title) VALUES ($1, 'ENGAGEMENT', 'Service Engagement')")
        .bind(thread_id).execute(&state.db).await;

    let svc: Option<ServiceCatalogItem> = sqlx::query_as(
        "SELECT id, provider_company_id, name, description, pricing_model, rate, tags, active, created_at FROM service_catalog WHERE id = $1"
    ).bind(p.service_id).fetch_optional(&state.db).await.unwrap_or(None);
    let provider_id = svc.map(|s| s.provider_company_id).unwrap_or(Uuid::new_v4());

    let id = Uuid::new_v4();
    let _ = sqlx::query(
        "INSERT INTO service_engagements (id, service_id, client_company_id, provider_company_id, scope, status, created_by_agent_id, thread_id) VALUES ($1,$2,$3,$4,$5,'PENDING',$6,$7)"
    ).bind(id).bind(p.service_id).bind(p.client_company_id).bind(provider_id).bind(&p.scope).bind(p.created_by_agent_id).bind(thread_id)
    .execute(&state.db).await;
    (StatusCode::CREATED, Json(json!({"id": id, "thread_id": thread_id})))
}

async fn activate_engagement(State(state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    let uid = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };
    let _ = sqlx::query("UPDATE service_engagements SET status = 'ACTIVE' WHERE id = $1").bind(uid).execute(&state.db).await;
    (StatusCode::OK, Json(json!({"status":"activated"})))
}

async fn complete_engagement(State(state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    let uid = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };
    let _ = sqlx::query("UPDATE service_engagements SET status = 'COMPLETED' WHERE id = $1").bind(uid).execute(&state.db).await;

    // Auto-record paired ledger entries for the engagement
    let engagement: Option<(Uuid, Uuid, Uuid)> = sqlx::query_as(
        "SELECT client_company_id, provider_company_id, service_id FROM service_engagements WHERE id = $1"
    ).bind(uid).fetch_optional(&state.db).await.ok().flatten();

    if let Some((client_id, provider_id, service_id)) = engagement {
        let svc: Option<(String, Value)> = sqlx::query_as(
            "SELECT name, rate FROM service_catalog WHERE id = $1"
        ).bind(service_id).fetch_optional(&state.db).await.ok().flatten();

        if let Some((service_name, rate)) = svc {
            let amount = rate["amount"].as_f64().unwrap_or(0.0);
            let currency = rate["currency"].as_str().unwrap_or("USD").to_string();

            if amount > 0.0 {
                let amount_str = format!("{}", amount);
                let expense_memo = format!("Service: {} (engagement completed)", service_name);
                let revenue_memo = format!("Service: {} (engagement completed)", service_name);

                // EXPENSE for client
                let _ = sqlx::query(
                    "INSERT INTO ledger_entries (id, company_id, counterparty_company_id, engagement_id, type, amount, currency, memo, is_virtual) \
                     VALUES ($1, $2, $3, $4, 'EXPENSE', $5::NUMERIC, $6, $7, true)"
                ).bind(Uuid::new_v4()).bind(client_id).bind(Some(provider_id)).bind(Some(uid))
                 .bind(&amount_str).bind(&currency).bind(&expense_memo)
                 .execute(&state.db).await;

                // REVENUE for provider
                let _ = sqlx::query(
                    "INSERT INTO ledger_entries (id, company_id, counterparty_company_id, engagement_id, type, amount, currency, memo, is_virtual) \
                     VALUES ($1, $2, $3, $4, 'REVENUE', $5::NUMERIC, $6, $7, true)"
                ).bind(Uuid::new_v4()).bind(provider_id).bind(Some(client_id)).bind(Some(uid))
                 .bind(&amount_str).bind(&currency).bind(&revenue_memo)
                 .execute(&state.db).await;
            }
        }
    }

    (StatusCode::OK, Json(json!({"status":"completed"})))
}

// ═══════════════════════════════════════════════════════════════
// Ledger
// ═══════════════════════════════════════════════════════════════

async fn get_ledger(State(state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    let uid = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };
    match sqlx::query_as::<_, LedgerEntry>(
        "SELECT id, company_id, counterparty_company_id, engagement_id, type, amount, currency, memo, is_virtual, created_at \
         FROM ledger_entries WHERE company_id = $1 ORDER BY created_at DESC"
    ).bind(uid).fetch_all(&state.db).await {
        Ok(l) => (StatusCode::OK, Json(json!(l))),
        Err(_) => (StatusCode::OK, Json(json!([])))
    }
}

#[derive(Debug, Deserialize)]
struct CreateLedgerEntryRequest {
    r#type: String,
    amount: f64,
    currency: String,
    memo: Option<String>,
    counterparty_company_id: Option<String>,
    engagement_id: Option<String>,
}

async fn create_ledger_entry(State(state): State<AppState>, Path(id): Path<String>, Json(p): Json<CreateLedgerEntryRequest>) -> impl IntoResponse {
    let company_id = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };
    let valid_types = ["EXPENSE", "REVENUE", "INTERNAL_TRANSFER", "CAPITAL_INJECTION"];
    if !valid_types.contains(&p.r#type.as_str()) {
        return (StatusCode::BAD_REQUEST, Json(json!({"error": format!("Invalid type. Must be one of: {}", valid_types.join(", "))})));
    }
    if p.amount <= 0.0 {
        return (StatusCode::BAD_REQUEST, Json(json!({"error":"Amount must be positive"})));
    }

    let counterparty_id = p.counterparty_company_id.as_deref()
        .and_then(|s| Uuid::parse_str(s).ok());
    let engagement_id = p.engagement_id.as_deref()
        .and_then(|s| Uuid::parse_str(s).ok());

    let entry_id = Uuid::new_v4();
    let amount_str = format!("{}", p.amount);

    if let Err(e) = sqlx::query(
        "INSERT INTO ledger_entries (id, company_id, counterparty_company_id, engagement_id, type, amount, currency, memo, is_virtual) \
         VALUES ($1, $2, $3, $4, $5, $6::NUMERIC, $7, $8, true)"
    ).bind(entry_id).bind(company_id).bind(counterparty_id).bind(engagement_id)
     .bind(&p.r#type).bind(&amount_str).bind(&p.currency).bind(&p.memo)
     .execute(&state.db).await {
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("{}", e)})));
    }

    // For INTERNAL_TRANSFER, create the paired entry on the counterparty
    if p.r#type == "INTERNAL_TRANSFER" {
        if let Some(cp_id) = counterparty_id {
            let paired_id = Uuid::new_v4();
            let paired_memo = p.memo.as_deref().map(|m| format!("Transfer from counterparty: {}", m))
                .unwrap_or_else(|| "Transfer from counterparty".to_string());
            let _ = sqlx::query(
                "INSERT INTO ledger_entries (id, company_id, counterparty_company_id, engagement_id, type, amount, currency, memo, is_virtual) \
                 VALUES ($1, $2, $3, $4, 'REVENUE', $5::NUMERIC, $6, $7, true)"
            ).bind(paired_id).bind(cp_id).bind(Some(company_id)).bind(engagement_id)
             .bind(&amount_str).bind(&p.currency).bind(&paired_memo)
             .execute(&state.db).await;
        }
    }

    (StatusCode::CREATED, Json(json!({"id": entry_id})))
}

async fn get_balance(State(state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    let uid = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };

    let rows: Vec<(String, String, rust_decimal::Decimal)> = sqlx::query_as(
        "SELECT currency, type, COALESCE(SUM(amount), 0) as total \
         FROM ledger_entries WHERE company_id = $1 GROUP BY currency, type"
    ).bind(uid).fetch_all(&state.db).await.unwrap_or_default();

    let mut balances: serde_json::Map<String, Value> = serde_json::Map::new();
    for (currency, entry_type, total) in &rows {
        let currency_obj = balances.entry(currency.clone())
            .or_insert_with(|| json!({"revenue": 0.0, "expenses": 0.0, "capital": 0.0, "net": 0.0}));
        let total_f64 = total.to_string().parse::<f64>().unwrap_or(0.0);
        match entry_type.as_str() {
            "REVENUE" => { currency_obj["revenue"] = json!(total_f64); }
            "EXPENSE" => { currency_obj["expenses"] = json!(total_f64); }
            "CAPITAL_INJECTION" => { currency_obj["capital"] = json!(total_f64); }
            "INTERNAL_TRANSFER" => { currency_obj["expenses"] = json!(currency_obj["expenses"].as_f64().unwrap_or(0.0) + total_f64); }
            _ => {}
        }
    }
    // Calculate net for each currency
    for (_, obj) in balances.iter_mut() {
        let revenue = obj["revenue"].as_f64().unwrap_or(0.0);
        let expenses = obj["expenses"].as_f64().unwrap_or(0.0);
        let capital = obj["capital"].as_f64().unwrap_or(0.0);
        obj["net"] = json!(capital + revenue - expenses);
    }

    (StatusCode::OK, Json(json!(balances)))
}

// ═══════════════════════════════════════════════════════════════
// Agentd Registration (called by VMs)
// ═══════════════════════════════════════════════════════════════

async fn agentd_register() -> impl IntoResponse {
    (StatusCode::OK, Json(json!({"status":"registered"})))
}

async fn agentd_heartbeat() -> impl IntoResponse {
    (StatusCode::OK, Json(json!({"status":"ok"})))
}

// ═══════════════════════════════════════════════════════════════
// Update Company
// ═══════════════════════════════════════════════════════════════

#[derive(Deserialize)]
struct UpdateCompanyRequest {
    name: Option<String>,
    r#type: Option<String>,
    description: Option<String>,
    status: Option<String>,
}

async fn update_company(State(state): State<AppState>, Path(id): Path<String>, Json(p): Json<UpdateCompanyRequest>) -> impl IntoResponse {
    let uid = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };
    let _ = sqlx::query(
        "UPDATE companies SET name = COALESCE($1, name), type = COALESCE($2, type), description = COALESCE($3, description), status = COALESCE($4, status) WHERE id = $5"
    )
    .bind(&p.name).bind(&p.r#type).bind(&p.description).bind(&p.status).bind(uid)
    .execute(&state.db).await;
    
    // Re-fetch and return
    match sqlx::query_as::<_, Company>(
        "SELECT id, holding_id, name, type, description, tags, status, created_at FROM companies WHERE id = $1"
    ).bind(uid).fetch_optional(&state.db).await {
        Ok(Some(c)) => {
            let _ = state.tx.send(json!({"type":"company_updated","company": c}).to_string());
            (StatusCode::OK, Json(json!(c)))
        }
        _ => (StatusCode::NOT_FOUND, Json(json!({"error":"Company not found"})))
    }
}

// ═══════════════════════════════════════════════════════════════
// Agent DM Thread (get or create)
// ═══════════════════════════════════════════════════════════════

async fn get_agent_thread(State(state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    let agent_id = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };

    // Check if a user-to-agent DM thread already exists with this agent
    // Must require a USER member to avoid returning agent-to-agent DM threads
    let existing: Option<(Uuid,)> = sqlx::query_as(
        "SELECT tm.thread_id FROM thread_members tm JOIN threads t ON t.id = tm.thread_id \
         WHERE tm.member_type = 'AGENT' AND tm.member_id = $1 AND t.type = 'DM' \
           AND EXISTS ( \
               SELECT 1 FROM thread_members tm2 \
               WHERE tm2.thread_id = t.id AND tm2.member_type = 'USER' \
           ) \
         LIMIT 1"
    ).bind(agent_id).fetch_optional(&state.db).await.unwrap_or(None);

    if let Some((thread_id,)) = existing {
        return (StatusCode::OK, Json(json!({"thread_id": thread_id, "created": false})));
    }

    // Get agent info for thread title
    let agent_name: Option<String> = sqlx::query_scalar("SELECT name FROM agents WHERE id = $1")
        .bind(agent_id).fetch_optional(&state.db).await.ok().flatten();
    let name = agent_name.unwrap_or_else(|| "Agent".into());

    // Create new DM thread
    let thread_id = Uuid::new_v4();
    let _ = sqlx::query("INSERT INTO threads (id, type, title) VALUES ($1, 'DM', $2)")
        .bind(thread_id).bind(format!("DM with {}", name))
        .execute(&state.db).await;

    // Add agent as member
    let _ = sqlx::query("INSERT INTO thread_members (thread_id, member_type, member_id) VALUES ($1, 'AGENT', $2)")
        .bind(thread_id).bind(agent_id).execute(&state.db).await;

    // Add USER as member (placeholder user ID)
    let _ = sqlx::query("INSERT INTO thread_members (thread_id, member_type, member_id) VALUES ($1, 'USER', $2)")
        .bind(thread_id).bind(Uuid::from_u128(0)).execute(&state.db).await;

    (StatusCode::CREATED, Json(json!({"thread_id": thread_id, "created": true})))
}

// ═══════════════════════════════════════════════════════════════
// Agent-to-Agent DM
// ═══════════════════════════════════════════════════════════════

/// Find or create an agent-to-agent DM thread (no USER members).
async fn find_or_create_agent_dm_thread(db: &sqlx::PgPool, agent_a: Uuid, agent_b: Uuid) -> Uuid {
    let existing: Option<(Uuid,)> = sqlx::query_as(
        "SELECT t.id FROM threads t \
         JOIN thread_members tm1 ON t.id = tm1.thread_id \
         JOIN thread_members tm2 ON t.id = tm2.thread_id \
         WHERE t.type = 'DM' \
           AND tm1.member_type = 'AGENT' AND tm1.member_id = $1 \
           AND tm2.member_type = 'AGENT' AND tm2.member_id = $2 \
           AND NOT EXISTS ( \
               SELECT 1 FROM thread_members tm3 \
               WHERE tm3.thread_id = t.id AND tm3.member_type = 'USER' \
           ) \
         LIMIT 1"
    ).bind(agent_a).bind(agent_b).fetch_optional(db).await.unwrap_or(None);

    if let Some((tid,)) = existing {
        tid
    } else {
        let name_a: String = sqlx::query_scalar("SELECT name FROM agents WHERE id = $1")
            .bind(agent_a).fetch_optional(db).await.ok().flatten().unwrap_or_else(|| "Agent".into());
        let name_b: String = sqlx::query_scalar("SELECT name FROM agents WHERE id = $1")
            .bind(agent_b).fetch_optional(db).await.ok().flatten().unwrap_or_else(|| "Agent".into());
        let tid = Uuid::new_v4();
        let _ = sqlx::query("INSERT INTO threads (id, type, title) VALUES ($1, 'DM', $2)")
            .bind(tid).bind(format!("{} <-> {}", name_a, name_b))
            .execute(db).await;
        let _ = sqlx::query("INSERT INTO thread_members (thread_id, member_type, member_id) VALUES ($1, 'AGENT', $2)")
            .bind(tid).bind(agent_a).execute(db).await;
        let _ = sqlx::query("INSERT INTO thread_members (thread_id, member_type, member_id) VALUES ($1, 'AGENT', $2)")
            .bind(tid).bind(agent_b).execute(db).await;
        tid
    }
}

/// Insert a SYSTEM message into a thread and broadcast via WebSocket.
pub(crate) async fn insert_system_message_in_thread(state: &AppState, thread_id: Uuid, sender_id: Uuid, text: &str) {
    let msg_id = Uuid::new_v4();
    let content = json!({"text": text});
    if let Ok(msg) = sqlx::query_as::<_, Message>(
        "INSERT INTO messages (id, thread_id, sender_type, sender_id, content, reply_depth) \
         VALUES ($1,$2,'SYSTEM',$3,$4,0) \
         RETURNING id, thread_id, sender_type, sender_id, content, reply_depth, created_at"
    ).bind(msg_id).bind(thread_id).bind(sender_id).bind(&content)
    .fetch_one(&state.db).await {
        let _ = state.tx.send(json!({"type":"new_message","message": msg}).to_string());
    }
}

#[derive(Debug, Deserialize)]
struct AgentDmRequest {
    target: String,   // agent UUID or handle (e.g. "@ceo-acme")
    message: String,
}

async fn agent_dm(
    State(state): State<AppState>, Path(id): Path<String>, Json(p): Json<AgentDmRequest>
) -> impl IntoResponse {
    let sender_id = match Uuid::parse_str(&id) {
        Ok(u) => u,
        Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid sender ID"}))),
    };
    if let Some(resp) = check_dm_mode(&state, sender_id).await { return resp; }

    // Resolve target: UUID or @handle
    let target_id: Uuid = if p.target.starts_with('@') {
        match sqlx::query_scalar::<_, Uuid>("SELECT id FROM agents WHERE handle = $1")
            .bind(&p.target).fetch_optional(&state.db).await {
            Ok(Some(id)) => id,
            _ => return (StatusCode::NOT_FOUND, Json(json!({"error": format!("Agent with handle '{}' not found", p.target)}))),
        }
    } else {
        match Uuid::parse_str(&p.target) {
            Ok(u) => u,
            Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid target — use a UUID or @handle"}))),
        }
    };

    if sender_id == target_id {
        return (StatusCode::BAD_REQUEST, Json(json!({"error":"Cannot DM yourself"})));
    }

    // Block DMs involving quarantined agents
    let sender_status: Option<String> = sqlx::query_scalar("SELECT status FROM agents WHERE id = $1")
        .bind(sender_id).fetch_optional(&state.db).await.ok().flatten();
    let target_status: Option<String> = sqlx::query_scalar("SELECT status FROM agents WHERE id = $1")
        .bind(target_id).fetch_optional(&state.db).await.ok().flatten();
    if sender_status.as_deref() == Some("QUARANTINED") {
        return (StatusCode::FORBIDDEN, Json(json!({"error":"Sender agent is quarantined and cannot send messages"})));
    }
    if target_status.as_deref() == Some("QUARANTINED") {
        return (StatusCode::FORBIDDEN, Json(json!({"error":"Target agent is quarantined and cannot receive messages"})));
    }

    // Enforce communication hierarchy
    {
        let sender_role: Option<String> = sqlx::query_scalar("SELECT role FROM agents WHERE id = $1")
            .bind(sender_id).fetch_optional(&state.db).await.ok().flatten();
        let target_role: Option<String> = sqlx::query_scalar("SELECT role FROM agents WHERE id = $1")
            .bind(target_id).fetch_optional(&state.db).await.ok().flatten();

        // MAIN can DM anyone
        if sender_role.as_deref() != Some("MAIN") {
            let sender_company: Option<Uuid> = sqlx::query_scalar("SELECT company_id FROM agents WHERE id = $1")
                .bind(sender_id).fetch_optional(&state.db).await.ok().flatten();
            let target_company: Option<Uuid> = sqlx::query_scalar("SELECT company_id FROM agents WHERE id = $1")
                .bind(target_id).fetch_optional(&state.db).await.ok().flatten();
            let sender_parent: Option<Uuid> = sqlx::query_scalar("SELECT parent_agent_id FROM agents WHERE id = $1")
                .bind(sender_id).fetch_optional(&state.db).await.ok().flatten();
            let target_parent: Option<Uuid> = sqlx::query_scalar("SELECT parent_agent_id FROM agents WHERE id = $1")
                .bind(target_id).fetch_optional(&state.db).await.ok().flatten();

            let allowed = match (sender_role.as_deref(), target_role.as_deref()) {
                // Only CEOs can DM MAIN
                (Some("CEO"), Some("MAIN")) => true,
                (_, Some("MAIN")) => false,
                // Can DM your direct parent
                _ if sender_parent == Some(target_id) => true,
                // Can DM your direct child
                _ if target_parent == Some(sender_id) => true,
                // CEO can DM managers in their company (not workers — go through the manager)
                (Some("CEO"), Some("MANAGER")) if sender_company == target_company && sender_company.is_some() => true,
                // Peers under the same parent in the same company
                _ if sender_company == target_company && sender_parent == target_parent
                    && sender_company.is_some() => true,
                _ => false,
            };

            if !allowed {
                return (StatusCode::FORBIDDEN, Json(json!({
                    "error": "You can only message agents in your direct chain of command. Escalate through your superior."
                })));
            }
        }
    }

    // Anti-spam: short cooldown to prevent rapid DM re-initiation between same pair
    let pair_key = if sender_id < target_id { (sender_id, target_id) } else { (target_id, sender_id) };
    {
        let cooldowns = state.dm_cooldowns.read().await;
        if let Some(last_completed) = cooldowns.get(&pair_key) {
            let elapsed = last_completed.elapsed().as_secs();
            if elapsed < 10 {
                return (StatusCode::TOO_MANY_REQUESTS, Json(json!({
                    "error": "A DM conversation between these agents just concluded. Please wait a moment.",
                    "cooldown_remaining_secs": 10 - elapsed
                })));
            }
        }
    }

    // Rate limit: max 10 agent messages per minute per sender
    let recent_count: Option<i64> = sqlx::query_scalar(
        "SELECT COUNT(*) FROM messages WHERE sender_id = $1 AND sender_type = 'AGENT' AND created_at > NOW() - INTERVAL '1 minute'"
    ).bind(sender_id).fetch_optional(&state.db).await.ok().flatten();
    if recent_count.unwrap_or(0) >= 10 {
        return (StatusCode::TOO_MANY_REQUESTS, Json(json!({"error":"Rate limit exceeded — max 10 agent messages per minute"})));
    }

    // Prevent concurrent DM conversations between the same pair (atomic check-and-set)
    {
        let mut active = state.active_dm_pairs.write().await;
        if !active.insert(pair_key) {
            return (StatusCode::TOO_MANY_REQUESTS, Json(json!({
                "error": "A DM conversation between these agents is already in progress."
            })));
        }
    }

    let thread_id = find_or_create_agent_dm_thread(&state.db, sender_id, target_id).await;

    // Suppress action-prompt re-narrations: if the sender already completed a
    // dm_outbound to this same target within the last 5 minutes, this new DM is
    // likely a stale re-narration from an action_prompt that re-triggered the
    // same curl DM after the real conversation already happened.
    {
        let recent_completed: Option<i64> = sqlx::query_scalar(
            "SELECT COUNT(*) FROM message_queue \
             WHERE kind = 'dm_outbound' AND status = 'COMPLETED' \
               AND agent_id = $1 AND payload->>'target_id' = $2 \
               AND completed_at > NOW() - INTERVAL '5 minutes'"
        )
        .bind(sender_id).bind(target_id.to_string())
        .fetch_optional(&state.db).await.ok().flatten();

        if recent_completed.unwrap_or(0) > 0 {
            tracing::info!(
                "agent_dm: suppressed re-narration DM from {} to {} (recent dm_outbound already completed)",
                sender_id, target_id
            );
            let mut active = state.active_dm_pairs.write().await;
            active.remove(&pair_key);
            return (StatusCode::OK, Json(json!({
                "status": "suppressed",
                "reason": "A conversation between you and this agent just occurred.",
                "thread_id": thread_id
            })));
        }
    }

    // Insert message (strip tags + scrub secrets before storing)
    let msg_id = Uuid::new_v4();
    let (tag_cleaned, _) = strip_agent_tags(&p.message);
    let scrubbed_msg = if let Some(ref crypto) = state.crypto {
        scrub_secrets(&state.db, crypto, sender_id, &tag_cleaned).await
    } else { tag_cleaned };
    if scrubbed_msg.trim().is_empty() {
        return (StatusCode::OK, Json(json!({"thread_id": thread_id, "message_id": msg_id, "status": "empty_after_cleaning"})));
    }
    // Process dm_outbound inline instead of enqueueing on sender's queue.
    // The sender may have an action_prompt running (which holds the per-agent
    // queue lock for up to 300s). Processing inline avoids that delay — the
    // dm_outbound work is just fast DB operations (INSERT + WS broadcast).
    let content = json!({"text": scrubbed_msg});
    match sqlx::query_as::<_, Message>(
        "INSERT INTO messages (id, thread_id, sender_type, sender_id, content, reply_depth) \
         VALUES ($1,$2,'AGENT',$3,$4,0) \
         RETURNING id, thread_id, sender_type, sender_id, content, reply_depth, created_at"
    ).bind(msg_id).bind(thread_id).bind(sender_id).bind(&content)
    .fetch_one(&state.db).await {
        Ok(msg) => {
            // Broadcast websocket event
            let _ = state.tx.send(json!({"type": "new_message", "message": msg}).to_string());

            // Coalesce check: if a PENDING dm_initiate already exists for the
            // same (sender → target) pair, merge this message into it instead of
            // creating a second conversation.
            let existing_qi: Option<(Uuid, serde_json::Value)> = sqlx::query_as(
                "SELECT id, payload FROM message_queue \
                 WHERE agent_id = $1 AND kind = 'dm_initiate' AND status = 'PENDING' \
                   AND payload->>'sender_id' = $2 AND payload->>'target_id' = $3 \
                 LIMIT 1"
            )
            .bind(target_id).bind(sender_id.to_string()).bind(target_id.to_string())
            .fetch_optional(&state.db).await.ok().flatten();

            if let Some((qi_id, mut qi_payload)) = existing_qi {
                let existing_text = qi_payload["message_text"].as_str().unwrap_or("");
                let combined = format!("{}\n\n{}", existing_text, p.message);
                qi_payload["message_text"] = serde_json::Value::String(combined);
                let _ = sqlx::query("UPDATE message_queue SET payload = $1 WHERE id = $2")
                    .bind(&qi_payload).bind(qi_id)
                    .execute(&state.db).await;
                tracing::info!("agent_dm: coalesced DM from {} to {} into queue item {}", sender_id, target_id, qi_id);
            } else {
                // Enqueue dm_initiate for the TARGET agent
                if let Err(e) = state.enqueue_message(
                    target_id,
                    3, // agent DM priority
                    "dm_initiate",
                    json!({
                        "thread_id": thread_id.to_string(),
                        "sender_id": sender_id.to_string(),
                        "target_id": target_id.to_string(),
                        "message_text": p.message,
                        "pair_key_a": pair_key.0.to_string(),
                        "pair_key_b": pair_key.1.to_string(),
                    }),
                ).await {
                    tracing::error!("agent_dm: failed to enqueue dm_initiate: {}", e);
                }
            }

            (StatusCode::CREATED, Json(json!({"thread_id": thread_id, "message_id": msg_id})))
        }
        Err(e) => {
            // Release active-conversation lock on failure
            {
                let mut active = state.active_dm_pairs.write().await;
                active.remove(&pair_key);
            }
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("{}", e)})))
        }
    }
}

// ═══════════════════════════════════════════════════════════════
// Agent-to-User DM
// ═══════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize)]
struct AgentDmUserRequest {
    message: String,
}

/// Allows an agent to send a DM to the human operator.
/// Creates/finds a DM thread that includes a USER member so it shows
/// up in the operator's thread list.
async fn agent_dm_user(
    State(state): State<AppState>, Path(id): Path<String>, Json(p): Json<AgentDmUserRequest>
) -> impl IntoResponse {
    let agent_id = match Uuid::parse_str(&id) {
        Ok(u) => u,
        Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid agent ID"}))),
    };

    let agent_name: String = sqlx::query_scalar("SELECT name FROM agents WHERE id = $1")
        .bind(agent_id).fetch_optional(&state.db).await.ok().flatten().unwrap_or_else(|| "Unknown".into());
    let preview: String = p.message.chars().take(100).collect();
    tracing::info!("dm-user from {} ({}): '{}'", agent_name, agent_id, preview);

    // Rate limit: max 5 user-directed messages per minute per agent
    let recent_count: Option<i64> = sqlx::query_scalar(
        "SELECT COUNT(*) FROM messages WHERE sender_id = $1 AND sender_type = 'AGENT' AND created_at > NOW() - INTERVAL '1 minute' \
         AND thread_id IN (SELECT thread_id FROM thread_members WHERE member_type = 'USER')"
    ).bind(agent_id).fetch_optional(&state.db).await.ok().flatten();
    if recent_count.unwrap_or(0) >= 5 {
        return (StatusCode::TOO_MANY_REQUESTS, Json(json!({"error":"Rate limit exceeded — max 5 user messages per minute"})));
    }

    // Misrouting guard: detect when an agent mistakenly uses dm-user to message
    // another agent instead of the human operator. Check if the message's first line
    // directly addresses a known agent name in the same holding.
    {
        let holding_id: Option<Uuid> = sqlx::query_scalar("SELECT holding_id FROM agents WHERE id = $1")
            .bind(agent_id).fetch_optional(&state.db).await.ok().flatten();
        if let Some(hid) = holding_id {
            let peer_names: Vec<String> = sqlx::query_scalar(
                "SELECT name FROM agents WHERE holding_id = $1 AND id != $2 AND status = 'ACTIVE'"
            ).bind(hid).bind(agent_id).fetch_all(&state.db).await.unwrap_or_default();

            let msg_lower = p.message.to_lowercase();
            // Check first ~200 chars for an agent name (greeting/address pattern)
            let first_chunk: String = msg_lower.chars().take(200).collect();
            for peer in &peer_names {
                let first_name = peer.split_whitespace().next().unwrap_or(peer).to_lowercase();
                if first_chunk.contains(&first_name) {
                    tracing::warn!(
                        "Misrouted dm-user from agent {}: message addresses '{}' — should use /dm endpoint",
                        agent_id, peer
                    );
                    return (StatusCode::BAD_REQUEST, Json(json!({
                        "error": format!(
                            "This message appears to be addressed to {}, not the human operator. \
                             Use the /dm endpoint (POST /v1/agents/YOUR_ID/dm) with {{\"target\": \"AGENT_ID_OR_HANDLE\", \"message\": \"...\"}} \
                             to message other agents. The /dm-user endpoint is only for contacting the human operator.",
                            peer
                        )
                    })));
                }
            }
        }
    }

    // Find existing user-agent DM thread
    let user_id = Uuid::from_u128(0); // placeholder user ID (matches get_agent_thread)
    let existing: Option<(Uuid,)> = sqlx::query_as(
        "SELECT t.id FROM threads t \
         JOIN thread_members tm1 ON t.id = tm1.thread_id \
         JOIN thread_members tm2 ON t.id = tm2.thread_id \
         WHERE t.type = 'DM' \
           AND tm1.member_type = 'AGENT' AND tm1.member_id = $1 \
           AND tm2.member_type = 'USER' AND tm2.member_id = $2 \
         LIMIT 1"
    ).bind(agent_id).bind(user_id).fetch_optional(&state.db).await.unwrap_or(None);

    let thread_id = if let Some((tid,)) = existing {
        tid
    } else {
        // Create new user-agent DM thread
        let agent_name: String = sqlx::query_scalar("SELECT name FROM agents WHERE id = $1")
            .bind(agent_id).fetch_optional(&state.db).await.ok().flatten().unwrap_or_else(|| "Agent".into());
        let tid = Uuid::new_v4();
        let _ = sqlx::query("INSERT INTO threads (id, type, title) VALUES ($1, 'DM', $2)")
            .bind(tid).bind(format!("DM with {}", agent_name))
            .execute(&state.db).await;
        let _ = sqlx::query("INSERT INTO thread_members (thread_id, member_type, member_id) VALUES ($1, 'AGENT', $2)")
            .bind(tid).bind(agent_id).execute(&state.db).await;
        let _ = sqlx::query("INSERT INTO thread_members (thread_id, member_type, member_id) VALUES ($1, 'USER', $2)")
            .bind(tid).bind(user_id).execute(&state.db).await;
        tid
    };

    // If this agent is already responding to the user in this same thread via
    // the thread-message path, skip — it would create a duplicate with wrong ordering.
    if let Some(&responding_tid) = state.responding_to_user.read().await.get(&agent_id) {
        if responding_tid == thread_id {
            tracing::info!(
                "Skipping dm-user from agent {} — already responding in thread {}",
                agent_id, thread_id
            );
            return (StatusCode::OK, Json(json!({
                "thread_id": thread_id,
                "message_id": Uuid::new_v4(),
                "status": "skipped_already_responding"
            })));
        }
    }

    // Strip tags + scrub secrets from agent message before storing
    let (tag_cleaned, _) = strip_agent_tags(&p.message);
    let scrubbed_message = if let Some(ref crypto) = state.crypto {
        scrub_secrets(&state.db, crypto, agent_id, &tag_cleaned).await
    } else { tag_cleaned };

    // Insert the agent's message
    let msg_id = Uuid::new_v4();
    if scrubbed_message.trim().is_empty() {
        return (StatusCode::OK, Json(json!({"thread_id": thread_id, "message_id": msg_id, "status": "empty_after_cleaning"})));
    }
    let content = json!({"text": scrubbed_message});
    match sqlx::query_as::<_, Message>(
        "INSERT INTO messages (id, thread_id, sender_type, sender_id, content, reply_depth) VALUES ($1,$2,'AGENT',$3,$4,0) \
         RETURNING id, thread_id, sender_type, sender_id, content, reply_depth, created_at"
    ).bind(msg_id).bind(thread_id).bind(agent_id).bind(&content)
    .fetch_one(&state.db).await {
        Ok(msg) => {
            let _ = state.tx.send(json!({"type":"new_message","message": msg}).to_string());
            (StatusCode::CREATED, Json(json!({"thread_id": thread_id, "message_id": msg_id, "status": "delivered"})))
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("{}", e)})))
    }
}

// ═══════════════════════════════════════════════════════════════
// Inter-Agent File Transfer
// ═══════════════════════════════════════════════════════════════

/// Maximum file size for inter-agent transfers: 10 MB
const MAX_FILE_TRANSFER_BYTES: u64 = 10 * 1024 * 1024;

fn parse_role(role_str: &str) -> crate::policy::engine::Role {
    use crate::policy::engine::Role;
    match role_str {
        "MAIN"    => Role::Main,
        "CEO"     => Role::Ceo,
        "MANAGER" => Role::Manager,
        _         => Role::Worker,
    }
}

async fn agent_send_file(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(p): Json<AgentFileSendRequest>,
) -> impl IntoResponse {
    let sender_id = match Uuid::parse_str(&id) {
        Ok(u) => u,
        Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid sender ID"}))),
    };
    if let Some(resp) = check_dm_mode(&state, sender_id).await { return resp; }

    // Resolve target: UUID or @handle
    let receiver_id: Uuid = if p.target.starts_with('@') {
        match sqlx::query_scalar::<_, Uuid>("SELECT id FROM agents WHERE handle = $1")
            .bind(&p.target).fetch_optional(&state.db).await
        {
            Ok(Some(id)) => id,
            _ => return (StatusCode::NOT_FOUND, Json(json!({"error": format!("Agent '{}' not found", p.target)}))),
        }
    } else {
        match Uuid::parse_str(&p.target) {
            Ok(u) => u,
            Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid target — use UUID or @handle"}))),
        }
    };

    if sender_id == receiver_id {
        return (StatusCode::BAD_REQUEST, Json(json!({"error":"Cannot send a file to yourself"})));
    }

    // Fetch both agents
    let sender: Option<Agent> = sqlx::query_as(
        "SELECT id, holding_id, company_id, role, name, specialty, parent_agent_id, \
         preferred_model, effective_model, system_prompt, tool_policy_id, vm_id, \
         sandbox_vm_id, handle, status, created_at FROM agents WHERE id = $1"
    ).bind(sender_id).fetch_optional(&state.db).await.ok().flatten();

    let receiver: Option<Agent> = sqlx::query_as(
        "SELECT id, holding_id, company_id, role, name, specialty, parent_agent_id, \
         preferred_model, effective_model, system_prompt, tool_policy_id, vm_id, \
         sandbox_vm_id, handle, status, created_at FROM agents WHERE id = $1"
    ).bind(receiver_id).fetch_optional(&state.db).await.ok().flatten();

    let (sender, receiver) = match (sender, receiver) {
        (Some(s), Some(r)) => (s, r),
        (None, _) => return (StatusCode::NOT_FOUND, Json(json!({"error":"Sender agent not found"}))),
        (_, None) => return (StatusCode::NOT_FOUND, Json(json!({"error":"Receiver agent not found"}))),
    };

    // Policy check
    let ctx = crate::policy::engine::FileTransferContext {
        sender_role: parse_role(&sender.role),
        receiver_role: parse_role(&receiver.role),
        sender_id,
        receiver_id,
        sender_parent: sender.parent_agent_id,
        receiver_parent: receiver.parent_agent_id,
        sender_company: sender.company_id,
        receiver_company: receiver.company_id,
    };

    match crate::policy::engine::can_send_file(&ctx) {
        crate::policy::engine::Decision::Denied(reason) => {
            return (StatusCode::FORBIDDEN, Json(json!({"error": reason})));
        }
        crate::policy::engine::Decision::AllowedImmediate => {}
        _ => {
            return (StatusCode::FORBIDDEN, Json(json!({"error":"File transfer not permitted"})));
        }
    }

    // Sanitize src_path
    let src_path = p.src_path.trim_start_matches('/');
    if src_path.contains("..") || src_path.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid src_path"})));
    }

    // Build host filesystem paths
    let data_root = std::env::var("MULTICLAW_OPENCLAW_DATA")
        .unwrap_or_else(|_| "/opt/multiclaw/openclaw-data".into());

    let sender_workspace = std::path::PathBuf::from(&data_root)
        .join(sender_id.to_string())
        .join("workspace");
    let src_file = sender_workspace.join(src_path);

    // Verify path stays inside workspace
    let src_canonical = match src_file.canonicalize() {
        Ok(p) => p,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return (StatusCode::NOT_FOUND, Json(json!({
                "error": format!("File not found in sender workspace: {}", src_path)
            })));
        }
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("Path error: {}", e)})));
        }
    };
    if !src_canonical.starts_with(&sender_workspace) {
        return (StatusCode::BAD_REQUEST, Json(json!({"error":"src_path escapes workspace boundary"})));
    }

    // Read and enforce size limit
    let file_bytes = match tokio::fs::read(&src_canonical).await {
        Ok(b) => b,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("Failed to read file: {}", e)}))),
    };

    if file_bytes.len() as u64 > MAX_FILE_TRANSFER_BYTES {
        return (StatusCode::PAYLOAD_TOO_LARGE, Json(json!({
            "error": format!("File too large: {} bytes (max {} bytes)", file_bytes.len(), MAX_FILE_TRANSFER_BYTES)
        })));
    }

    let size_bytes = file_bytes.len() as i64;
    let encoding = p.encoding.as_deref().unwrap_or("text");

    // Determine destination path
    let filename = std::path::Path::new(src_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(src_path);
    let dest_relative = p.dest_path.as_deref().unwrap_or(filename);
    let dest_relative = dest_relative.trim_start_matches('/');
    if dest_relative.contains("..") || dest_relative.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid dest_path"})));
    }

    let receiver_workspace = std::path::PathBuf::from(&data_root)
        .join(receiver_id.to_string())
        .join("workspace");
    let dest_file = receiver_workspace.join(dest_relative);

    // Verify dest stays inside workspace (pre-creation check via join logic)
    if !dest_file.starts_with(&receiver_workspace) {
        return (StatusCode::BAD_REQUEST, Json(json!({"error":"dest_path escapes workspace boundary"})));
    }

    // Create parent directories if needed, then write
    if let Some(parent) = dest_file.parent() {
        if let Err(e) = tokio::fs::create_dir_all(parent).await {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({
                "error": format!("Failed to create destination directory: {}", e)
            })));
        }
    }

    if let Err(e) = tokio::fs::write(&dest_file, &file_bytes).await {
        let transfer_id = Uuid::new_v4();
        let _ = sqlx::query(
            "INSERT INTO file_transfers (id, sender_id, receiver_id, filename, size_bytes, encoding, dest_path, status, error) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,'FAILED',$8)"
        ).bind(transfer_id).bind(sender_id).bind(receiver_id)
         .bind(filename).bind(size_bytes).bind(encoding)
         .bind(dest_relative).bind(e.to_string())
         .execute(&state.db).await;
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("Failed to write file: {}", e)})));
    }

    // Record successful transfer
    let transfer_id = Uuid::new_v4();
    let _ = sqlx::query(
        "INSERT INTO file_transfers (id, sender_id, receiver_id, filename, size_bytes, encoding, dest_path, status) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,'DELIVERED')"
    ).bind(transfer_id).bind(sender_id).bind(receiver_id)
     .bind(filename).bind(size_bytes).bind(encoding).bind(dest_relative)
     .execute(&state.db).await;

    // Insert a message into the sender↔receiver DM thread so file transfers
    // are visible in Agent Comms.
    {
        let ft_thread: Option<(Uuid,)> = sqlx::query_as(
            "SELECT t.id FROM threads t \
             JOIN thread_members tm1 ON t.id = tm1.thread_id \
             JOIN thread_members tm2 ON t.id = tm2.thread_id \
             WHERE t.type = 'DM' \
               AND tm1.member_type = 'AGENT' AND tm1.member_id = $1 \
               AND tm2.member_type = 'AGENT' AND tm2.member_id = $2 \
               AND NOT EXISTS ( \
                   SELECT 1 FROM thread_members tm3 \
                   WHERE tm3.thread_id = t.id AND tm3.member_type = 'USER' \
               ) \
             LIMIT 1"
        ).bind(sender_id).bind(receiver_id).fetch_optional(&state.db).await.unwrap_or(None);

        let ft_thread_id = if let Some((tid,)) = ft_thread { tid } else {
            let tid = Uuid::new_v4();
            let _ = sqlx::query("INSERT INTO threads (id, type, title) VALUES ($1, 'DM', $2)")
                .bind(tid).bind(format!("{} <-> {}", sender.name, receiver.name))
                .execute(&state.db).await;
            let _ = sqlx::query("INSERT INTO thread_members (thread_id, member_type, member_id) VALUES ($1, 'AGENT', $2)")
                .bind(tid).bind(sender_id).execute(&state.db).await;
            let _ = sqlx::query("INSERT INTO thread_members (thread_id, member_type, member_id) VALUES ($1, 'AGENT', $2)")
                .bind(tid).bind(receiver_id).execute(&state.db).await;
            tid
        };

        let ft_msg_id = Uuid::new_v4();
        let ft_content = json!({
            "text": format!("Sent file \"{}\" to {} ({} bytes)", filename, receiver.name, size_bytes),
            "file_transfer": transfer_id.to_string(),
        });
        if let Ok(msg) = sqlx::query_as::<_, Message>(
            "INSERT INTO messages (id, thread_id, sender_type, sender_id, content, reply_depth) \
             VALUES ($1,$2,'AGENT',$3,$4,0) \
             RETURNING id, thread_id, sender_type, sender_id, content, reply_depth, created_at"
        ).bind(ft_msg_id).bind(ft_thread_id).bind(sender_id).bind(&ft_content)
        .fetch_one(&state.db).await {
            let _ = state.tx.send(json!({"type": "new_message", "message": msg}).to_string());
        }
    }

    // Notify receiver via queue
    let notify_msg = format!(
        "FILE RECEIVED from {}: '{}' has been placed in your workspace at '/workspace/{}' ({} bytes).",
        sender.name, filename, dest_relative, file_bytes.len()
    );
    let _ = state.enqueue_message(
        receiver_id, 2, "file_notify",
        json!({"agent_id": receiver_id.to_string(), "message": notify_msg, "task_label": "Processing received file"}),
    ).await;

    // Broadcast WebSocket event
    let _ = state.tx.send(json!({
        "type": "file_transferred",
        "transfer_id": transfer_id,
        "sender_id": sender_id,
        "receiver_id": receiver_id,
        "filename": filename,
        "dest_path": dest_relative,
    }).to_string());

    tracing::info!("File '{}' transferred from {} to {} ({} bytes)", filename, sender.name, receiver.name, size_bytes);

    (StatusCode::CREATED, Json(json!({
        "transfer_id": transfer_id,
        "status": "delivered",
        "filename": filename,
        "dest_path": dest_relative,
        "size_bytes": size_bytes,
    })))
}

async fn agent_file_transfers(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let agent_id = match Uuid::parse_str(&id) {
        Ok(u) => u,
        Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))),
    };
    match sqlx::query_as::<_, FileTransfer>(
        "SELECT id, sender_id, receiver_id, filename, size_bytes, encoding, dest_path, \
         status, error, created_at \
         FROM file_transfers \
         WHERE sender_id = $1 OR receiver_id = $1 \
         ORDER BY created_at DESC LIMIT 100"
    ).bind(agent_id).fetch_all(&state.db).await {
        Ok(transfers) => (StatusCode::OK, Json(json!(transfers))),
        Err(_) => (StatusCode::OK, Json(json!([]))),
    }
}

// ═══════════════════════════════════════════════════════════════
// Agent's Own Threads
// ═══════════════════════════════════════════════════════════════

async fn get_agent_threads(State(state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    let agent_id = match Uuid::parse_str(&id) {
        Ok(u) => u,
        Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))),
    };
    match sqlx::query_as::<_, Thread>(
        "SELECT t.id, t.type, t.title, t.created_by_user_id, t.created_at \
         FROM threads t \
         JOIN thread_members tm ON t.id = tm.thread_id \
         WHERE tm.member_type = 'AGENT' AND tm.member_id = $1 \
         ORDER BY t.created_at DESC \
         LIMIT 50"
    ).bind(agent_id).fetch_all(&state.db).await {
        Ok(t) => (StatusCode::OK, Json(json!(t))),
        Err(_) => (StatusCode::OK, Json(json!([])))
    }
}

// ═══════════════════════════════════════════════════════════════
// Thread Participants
// ═══════════════════════════════════════════════════════════════

#[derive(sqlx::FromRow, serde::Serialize)]
struct ThreadMember {
    thread_id: Uuid,
    member_type: String,
    member_id: Uuid,
}

async fn get_thread_participants(State(state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    let uid = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };
    match sqlx::query_as::<_, ThreadMember>(
        "SELECT thread_id, member_type, member_id FROM thread_members WHERE thread_id = $1"
    ).bind(uid).fetch_all(&state.db).await {
        Ok(members) => (StatusCode::OK, Json(json!(members))),
        Err(_) => (StatusCode::OK, Json(json!([])))
    }
}

// ═══════════════════════════════════════════════════════════════
// System Update Check & Update
// ═══════════════════════════════════════════════════════════════

// ═══════════════════════════════════════════════════════════════
// Scripts (served to agent VMs during cloud-init)
// ═══════════════════════════════════════════════════════════════

async fn serve_install_script() -> impl IntoResponse {
    let paths = ["/opt/multiclaw/infra/vm/scripts/install-openclaw.sh", "infra/vm/scripts/install-openclaw.sh"];
    for p in &paths {
        if let Ok(content) = tokio::fs::read_to_string(p).await {
            return (
                StatusCode::OK,
                [(axum::http::header::CONTENT_TYPE, "text/plain")],
                content,
            ).into_response();
        }
    }
    (StatusCode::NOT_FOUND, "install script not found").into_response()
}

const CURRENT_VERSION: &str = "0.1.1";

/// Simple semver greater-than comparison (a > b).
fn semver_gt(a: &str, b: &str) -> bool {
    let parse = |s: &str| -> (u32, u32, u32) {
        let mut parts = s.split('.').filter_map(|p| p.parse().ok());
        (parts.next().unwrap_or(0), parts.next().unwrap_or(0), parts.next().unwrap_or(0))
    };
    parse(a) > parse(b)
}

async fn system_update_check(State(state): State<AppState>) -> impl IntoResponse {
    // Read update channel from system_meta (default: stable)
    let channel: String = sqlx::query_scalar("SELECT value FROM system_meta WHERE key = 'update_channel'")
        .fetch_optional(&state.db).await.ok().flatten().unwrap_or_else(|| "stable".to_string());

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    let repo = "8PotatoChip8/MultiClaw";

    match channel.as_str() {
        "beta" | "dev" => {
            // Compare latest commit SHA on the target branch vs deployed commit
            let branch = if channel == "beta" { "beta" } else { "main" };
            let url = format!("https://api.github.com/repos/{}/commits/{}", repo, branch);

            // Get deployed commit from system_meta
            let deployed_commit: String = sqlx::query_scalar("SELECT value FROM system_meta WHERE key = 'deployed_commit'")
                .fetch_optional(&state.db).await.ok().flatten().unwrap_or_else(|| "unknown".to_string());

            match client.get(&url).header("User-Agent", "MultiClaw-Updater").send().await {
                Ok(resp) if resp.status().is_success() => {
                    if let Ok(body) = resp.json::<Value>().await {
                        let latest_sha = body["sha"].as_str().unwrap_or("unknown");
                        let short_sha = &latest_sha[..7.min(latest_sha.len())];
                        let deployed_short = &deployed_commit[..7.min(deployed_commit.len())];
                        let commit_msg = body["commit"]["message"].as_str().unwrap_or("").lines().next().unwrap_or("");
                        let update_available = deployed_commit == "unknown" || latest_sha != deployed_commit;
                        // For dev/beta: always use commit-based format so comparison is consistent
                        let current_display = format!("{}-{}", channel, deployed_short);
                        return (StatusCode::OK, Json(json!({
                            "current_version": current_display,
                            "latest_version": format!("{}-{}", channel, short_sha),
                            "update_available": update_available,
                            "channel": channel,
                            "semver": CURRENT_VERSION,
                            "deployed_commit": deployed_short,
                            "latest_commit": short_sha,
                            "commit_message": commit_msg,
                            "release_url": format!("https://github.com/{}/commit/{}", repo, latest_sha)
                        })));
                    }
                },
                _ => {}
            }
            (StatusCode::OK, Json(json!({
                "current_version": CURRENT_VERSION,
                "latest_version": "unknown",
                "update_available": false,
                "channel": channel,
                "semver": CURRENT_VERSION,
                "error": format!("Could not reach GitHub (branch: {})", branch)
            })))
        },
        _ => {
            // Stable channel: check releases/latest
            let url = format!("https://api.github.com/repos/{}/releases/latest", repo);
            match client.get(&url).header("User-Agent", "MultiClaw-Updater").send().await {
                Ok(resp) if resp.status().is_success() => {
                    if let Ok(body) = resp.json::<Value>().await {
                        let latest = body["tag_name"].as_str().unwrap_or("unknown").trim_start_matches('v');
                        let update_available = latest != "unknown" && semver_gt(latest, CURRENT_VERSION);
                        return (StatusCode::OK, Json(json!({
                            "current_version": CURRENT_VERSION,
                            "latest_version": latest,
                            "update_available": update_available,
                            "channel": "stable",
                            "semver": CURRENT_VERSION,
                            "release_url": body["html_url"].as_str().unwrap_or("")
                        })));
                    }
                },
                _ => {}
            }
            (StatusCode::OK, Json(json!({
                "current_version": CURRENT_VERSION,
                "latest_version": CURRENT_VERSION,
                "update_available": false,
                "channel": "stable",
                "semver": CURRENT_VERSION,
                "release_url": ""
            })))
        }
    }
}

async fn system_update(State(state): State<AppState>) -> impl IntoResponse {
    let state_clone = state.clone();
    tokio::spawn(async move {
        tracing::info!("Starting system update...");
        let _ = state_clone.tx.send(json!({"type":"system_update","status":"started"}).to_string());

        // Determine which branch/tag to pull based on update channel
        let channel: String = sqlx::query_scalar("SELECT value FROM system_meta WHERE key = 'update_channel'")
            .fetch_optional(&state_clone.db).await.ok().flatten().unwrap_or_else(|| "stable".to_string());

        let is_stable = !matches!(channel.as_str(), "beta" | "dev");

        if is_stable {
            // Stable channel: fetch the latest release tag from GitHub and checkout that tag
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new());

            let tag = match client.get("https://api.github.com/repos/8PotatoChip8/MultiClaw/releases/latest")
                .header("User-Agent", "MultiClaw-Updater")
                .send().await
            {
                Ok(resp) if resp.status().is_success() => {
                    resp.json::<Value>().await.ok()
                        .and_then(|body| body["tag_name"].as_str().map(String::from))
                },
                _ => None,
            };

            let tag = match tag {
                Some(t) => t,
                None => {
                    tracing::error!("Failed to fetch latest release tag from GitHub");
                    let _ = state_clone.tx.send(json!({"type":"system_update","status":"failed","error":"Could not fetch latest release tag"}).to_string());
                    return;
                }
            };

            tracing::info!("Stable update: checking out release tag {}", tag);

            // Unshallow the repo if it was cloned with --depth 1 (install-stable.sh).
            // Shallow clones may not be able to fetch tags pointing to commits outside the shallow history.
            let is_shallow = tokio::process::Command::new("git")
                .args(["-C", "/opt/multiclaw", "rev-parse", "--is-shallow-repository"])
                .output()
                .await
                .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "true")
                .unwrap_or(false);

            if is_shallow {
                tracing::info!("Repo is shallow, unshallowing before tag fetch...");
                let _ = tokio::process::Command::new("git")
                    .args(["-C", "/opt/multiclaw", "fetch", "--unshallow", "origin"])
                    .output()
                    .await;
            }

            // Fetch all tags
            let fetch = tokio::process::Command::new("git")
                .args(["-C", "/opt/multiclaw", "fetch", "origin", "--tags", "--force"])
                .output()
                .await;

            match fetch {
                Ok(output) if output.status.success() => {
                    tracing::info!("Git fetch tags successful");
                }
                Ok(output) => {
                    let err = String::from_utf8_lossy(&output.stderr);
                    tracing::error!("Git fetch tags failed: {}", err);
                    let _ = state_clone.tx.send(json!({"type":"system_update","status":"failed","error": err.to_string()}).to_string());
                    return;
                }
                Err(e) => {
                    tracing::error!("Git fetch tags error: {}", e);
                    let _ = state_clone.tx.send(json!({"type":"system_update","status":"failed","error": e.to_string()}).to_string());
                    return;
                }
            }

            // Checkout the release tag
            let checkout = tokio::process::Command::new("git")
                .args(["-C", "/opt/multiclaw", "checkout", &tag])
                .output()
                .await;

            match checkout {
                Ok(output) if output.status.success() => {
                    tracing::info!("Git checkout {} successful, rebuilding containers...", tag);
                }
                Ok(output) => {
                    let err = String::from_utf8_lossy(&output.stderr);
                    tracing::error!("Git checkout {} failed: {}", tag, err);
                    let _ = state_clone.tx.send(json!({"type":"system_update","status":"failed","error": err.to_string()}).to_string());
                    return;
                }
                Err(e) => {
                    tracing::error!("Git checkout error: {}", e);
                    let _ = state_clone.tx.send(json!({"type":"system_update","status":"failed","error": e.to_string()}).to_string());
                    return;
                }
            }
        } else {
            // Dev/beta channels: fetch branch and hard-reset to it.
            // Using fetch+reset instead of pull avoids merge conflicts if tracked
            // files were locally modified. reset --hard only affects tracked files;
            // untracked dirs like openclaw-data/ are untouched.
            let branch = if channel == "beta" { "beta" } else { "main" };

            // Unshallow the repo if it was cloned with --depth 1 (install-stable.sh).
            // Shallow clones don't have remote tracking refs, so `origin/main` doesn't exist.
            let is_shallow = tokio::process::Command::new("git")
                .args(["-C", "/opt/multiclaw", "rev-parse", "--is-shallow-repository"])
                .output()
                .await
                .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "true")
                .unwrap_or(false);

            if is_shallow {
                tracing::info!("Repo is shallow, unshallowing before fetch...");
                let _ = tokio::process::Command::new("git")
                    .args(["-C", "/opt/multiclaw", "fetch", "--unshallow", "origin"])
                    .output()
                    .await;
            }

            let fetch = tokio::process::Command::new("git")
                .args(["-C", "/opt/multiclaw", "fetch", "origin", branch])
                .output()
                .await;

            match fetch {
                Ok(output) if output.status.success() => {
                    tracing::info!("Git fetch successful (branch: {})", branch);
                }
                Ok(output) => {
                    let err = String::from_utf8_lossy(&output.stderr);
                    tracing::error!("Git fetch failed: {}", err);
                    let _ = state_clone.tx.send(json!({"type":"system_update","status":"failed","error": err.to_string()}).to_string());
                    return;
                }
                Err(e) => {
                    tracing::error!("Git fetch error: {}", e);
                    let _ = state_clone.tx.send(json!({"type":"system_update","status":"failed","error": e.to_string()}).to_string());
                    return;
                }
            }

            // Use FETCH_HEAD as the reset target — it always exists after a fetch,
            // even on shallow clones where origin/<branch> refs may not be created.
            let reset = tokio::process::Command::new("git")
                .args(["-C", "/opt/multiclaw", "reset", "--hard", "FETCH_HEAD"])
                .output()
                .await;

            match reset {
                Ok(output) if output.status.success() => {
                    tracing::info!("Git reset to FETCH_HEAD ({}) successful, rebuilding containers...", branch);
                }
                Ok(output) => {
                    let err = String::from_utf8_lossy(&output.stderr);
                    tracing::error!("Git reset to FETCH_HEAD failed: {}", err);
                    let _ = state_clone.tx.send(json!({"type":"system_update","status":"failed","error": err.to_string()}).to_string());
                    return;
                }
                Err(e) => {
                    tracing::error!("Git reset error: {}", e);
                    let _ = state_clone.tx.send(json!({"type":"system_update","status":"failed","error": e.to_string()}).to_string());
                    return;
                }
            }
        }

        // Record the new deployed commit SHA after successful pull.
        // /opt/multiclaw is volume-mounted with .git available.
        let new_sha = tokio::process::Command::new("git")
            .args(["-C", "/opt/multiclaw", "rev-parse", "HEAD"])
            .output()
            .await
            .ok()
            .and_then(|o| if o.status.success() {
                String::from_utf8(o.stdout).ok().map(|s| s.trim().to_string())
            } else { None })
            .unwrap_or_else(|| "unknown".to_string());

        sqlx::query("INSERT INTO system_meta (key, value) VALUES ('deployed_commit', $1) ON CONFLICT (key) DO UPDATE SET value = $1, updated_at = NOW()")
            .bind(&new_sha)
            .execute(&state_clone.db)
            .await
            .ok();
        tracing::info!("Updated deployed_commit to {}", &new_sha[..7.min(new_sha.len())]);

        // Rebuild and restart containers via a DETACHED ephemeral container.
        // We cannot run `docker compose up -d --build` directly because it will
        // replace THIS container (multiclawd) mid-execution, killing the compose
        // process before it can recreate the remaining services (ui, ollama-proxy).
        // By running it from a separate container, the rebuild survives our replacement.

        // Clean up any leftover updater from a previous run
        let _ = tokio::process::Command::new("docker")
            .args(["rm", "-f", "multiclaw-updater"])
            .output()
            .await;

        // The updater container also rebuilds the CLI after compose finishes.
        // cli_build_cmd runs inside the same ephemeral container sequentially.
        let rebuild = tokio::process::Command::new("docker")
            .args([
                "run", "-d", "--rm",
                "--name", "multiclaw-updater",
                "-v", "/var/run/docker.sock:/var/run/docker.sock",
                "-v", "/opt/multiclaw:/opt/multiclaw",
                "docker:cli",
                "sh", "-c",
                "docker compose -f /opt/multiclaw/infra/docker/docker-compose.yml up -d --build \
                 && docker run --rm \
                    -v /opt/multiclaw/packages:/usr/src/app/packages \
                    rust:1-slim-bookworm \
                    bash -c 'apt-get update && apt-get install -y pkg-config libssl-dev > /dev/null 2>&1 && cd /usr/src/app/packages && cargo build --release -p multiclaw-cli' \
                 || true"
            ])
            .output()
            .await;

        match rebuild {
            Ok(output) if output.status.success() => {
                tracing::info!("Updater container launched — rebuild will continue independently");
            }
            Ok(output) => {
                let err = String::from_utf8_lossy(&output.stderr);
                tracing::error!("Failed to launch updater container: {}", err);
                let _ = state_clone.tx.send(json!({"type":"system_update","status":"failed","error": err.to_string()}).to_string());
                return;
            }
            Err(e) => {
                tracing::error!("Failed to launch updater container: {}", e);
                let _ = state_clone.tx.send(json!({"type":"system_update","status":"failed","error": e.to_string()}).to_string());
                return;
            }
        }

        // Note: "complete" message may never reach the client because multiclawd
        // will be replaced by the updater. The frontend already handles this by
        // polling /v1/health and reloading when the new container is up.
        tracing::info!("System update handed off to updater container");
        let _ = state_clone.tx.send(json!({"type":"system_update","status":"complete"}).to_string());
    });

    (StatusCode::ACCEPTED, Json(json!({"status":"update_started"})))
}

// ═══════════════════════════════════════════════════════════════
// Docker Container Status & Logs
// ═══════════════════════════════════════════════════════════════

async fn list_containers() -> impl IntoResponse {
    let output = tokio::process::Command::new("docker")
        .args(["ps", "-a", "--format", "{{json .}}"])
        .output()
        .await;

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let containers: Vec<Value> = stdout
                .lines()
                .filter(|l| !l.trim().is_empty())
                .filter_map(|l| serde_json::from_str(l).ok())
                .collect();
            (StatusCode::OK, Json(json!(containers)))
        }
        Err(e) => {
            tracing::error!("Failed to run docker ps: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("Docker CLI error: {}", e)})))
        }
    }
}

#[derive(Deserialize)]
struct LogQuery {
    tail: Option<u32>,
}

async fn get_container_logs(Path(id): Path<String>, Query(q): Query<LogQuery>) -> impl IntoResponse {
    let tail = q.tail.unwrap_or(200).to_string();
    let output = tokio::process::Command::new("docker")
        .args(["logs", "--tail", &tail, "--timestamps", &id])
        .output()
        .await;

    match output {
        Ok(out) => {
            let mut logs = String::from_utf8_lossy(&out.stdout).to_string();
            // Docker logs stderr for some containers
            let stderr = String::from_utf8_lossy(&out.stderr);
            if !stderr.is_empty() {
                logs.push_str(&stderr);
            }
            (StatusCode::OK, Json(json!({"container_id": id, "logs": logs})))
        }
        Err(e) => {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("Failed to get logs: {}", e)})))
        }
    }
}

// ═══════════════════════════════════════════════════════════════
// Agent Memories
// ═══════════════════════════════════════════════════════════════

#[derive(sqlx::FromRow, serde::Serialize)]
struct AgentMemory {
    id: Uuid,
    agent_id: Uuid,
    category: String,
    key: String,
    content: String,
    importance: i32,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
}

async fn get_agent_memories(State(state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    let uid = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };
    match sqlx::query_as::<_, AgentMemory>(
        "SELECT id, agent_id, category, key, content, importance, created_at, updated_at \
         FROM agent_memories WHERE agent_id = $1 ORDER BY importance DESC, updated_at DESC"
    ).bind(uid).fetch_all(&state.db).await {
        Ok(m) => (StatusCode::OK, Json(json!(m))),
        Err(_) => (StatusCode::OK, Json(json!([])))
    }
}

#[derive(Deserialize)]
struct CreateMemoryRequest {
    category: String,
    key: String,
    content: String,
    importance: Option<i32>,
}

async fn create_agent_memory(State(state): State<AppState>, Path(id): Path<String>, Json(p): Json<CreateMemoryRequest>) -> impl IntoResponse {
    let agent_id = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };
    let mem_id = Uuid::new_v4();
    let importance = p.importance.unwrap_or(5);

    // Scrub secrets from memory content before storing
    let content = if let Some(ref crypto) = state.crypto {
        scrub_secrets(&state.db, crypto, agent_id, &p.content).await
    } else { p.content.clone() };

    // Upsert: if same agent+category+key exists, update it
    match sqlx::query(
        "INSERT INTO agent_memories (id, agent_id, category, key, content, importance) \
         VALUES ($1, $2, $3, $4, $5, $6) \
         ON CONFLICT (agent_id, category, key) DO UPDATE SET content = $5, importance = $6, updated_at = NOW()"
    )
    .bind(mem_id).bind(agent_id).bind(&p.category).bind(&p.key).bind(&content).bind(importance)
    .execute(&state.db).await {
        Ok(_) => (StatusCode::CREATED, Json(json!({"id": mem_id, "status": "saved"}))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("{}", e)})))
    }
}

async fn delete_agent_memory(State(state): State<AppState>, Path((id, mid)): Path<(String, String)>) -> impl IntoResponse {
    let agent_id = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid agent ID"}))) };
    let mem_id = match Uuid::parse_str(&mid) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid memory ID"}))) };
    let _ = sqlx::query("DELETE FROM agent_memories WHERE id = $1 AND agent_id = $2")
        .bind(mem_id).bind(agent_id).execute(&state.db).await;
    (StatusCode::OK, Json(json!({"status":"deleted"})))
}

// ═══════════════════════════════════════════════════════════════
// Secrets Management
// ═══════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize)]
struct SecretField {
    label: String,
    value: String,
}

#[derive(Debug, Deserialize)]
struct CreateSecretRequest {
    scope_type: String,  // "agent", "manager", "company", "holding"
    scope_id: Uuid,
    name: String,        // e.g., "coinex_api_key"
    #[serde(default)]
    fields: Vec<SecretField>,    // multi-value: [{label: "Access ID", value: "..."}, ...]
    #[serde(default)]
    value: Option<String>,       // legacy single-value (backward compat for API callers)
    description: Option<String>, // human-readable description of what the secret is for
}

#[derive(Debug, Deserialize)]
struct SecretsQuery {
    scope_type: Option<String>,
    scope_id: Option<Uuid>,
}

async fn create_secret(
    State(state): State<AppState>, Json(p): Json<CreateSecretRequest>
) -> impl IntoResponse {
    let crypto = match &state.crypto {
        Some(c) => c,
        None => return (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error":"Secrets not available — master key not configured"}))),
    };

    // Validate scope_type
    if !["agent", "company", "holding", "manager"].contains(&p.scope_type.as_str()) {
        return (StatusCode::BAD_REQUEST, Json(json!({"error":"scope_type must be 'agent', 'company', 'holding', or 'manager'"})));
    }

    // For holding-scoped secrets, resolve the actual holding ID from the database.
    // The UI doesn't have access to the holding UUID, so the backend resolves it.
    let final_scope_id = if p.scope_type == "holding" {
        let hid: Option<Uuid> = sqlx::query_scalar("SELECT id FROM holdings LIMIT 1")
            .fetch_optional(&state.db).await.ok().flatten();
        match hid {
            Some(id) => id,
            None => return (StatusCode::BAD_REQUEST, Json(json!({"error":"No holding found"}))),
        }
    } else {
        p.scope_id
    };

    // Serialize fields to JSON before encryption (supports multi-value secrets with labels)
    let plaintext = if !p.fields.is_empty() {
        let fields_json: Vec<Value> = p.fields.iter().map(|f| {
            json!({"label": f.label, "value": f.value})
        }).collect();
        serde_json::to_string(&json!({"fields": fields_json})).unwrap()
    } else if let Some(ref val) = p.value {
        // Legacy single-value (backward compat for API callers)
        serde_json::to_string(&json!({"fields": [{"label": "", "value": val}]})).unwrap()
    } else {
        return (StatusCode::BAD_REQUEST, Json(json!({"error":"Either 'fields' or 'value' is required"})));
    };

    let ciphertext = match crypto.encrypt(plaintext.as_bytes()) {
        Ok(ct) => ct,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("Encryption failed: {}", e)}))),
    };

    let id = Uuid::new_v4();
    let desc = p.description.as_deref().unwrap_or("");
    match sqlx::query(
        "INSERT INTO secrets (id, scope_type, scope_id, kind, ciphertext, description) VALUES ($1,$2,$3,$4,$5,$6)"
    ).bind(id).bind(&p.scope_type).bind(final_scope_id).bind(&p.name).bind(&ciphertext).bind(desc)
    .execute(&state.db).await {
        Ok(_) => (StatusCode::CREATED, Json(json!({"id": id, "name": p.name, "scope_type": p.scope_type, "scope_id": final_scope_id, "description": desc}))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("{}", e)}))),
    }
}

async fn list_secrets(
    State(state): State<AppState>, Query(q): Query<SecretsQuery>
) -> impl IntoResponse {
    // Return metadata only — NEVER return plaintext values
    let secrets: Vec<(Uuid, String, Uuid, String, String, chrono::DateTime<chrono::Utc>)> = if let (Some(st), Some(si)) = (&q.scope_type, &q.scope_id) {
        sqlx::query_as(
            "SELECT id, scope_type, scope_id, kind, description, created_at FROM secrets WHERE scope_type = $1 AND scope_id = $2 ORDER BY created_at DESC"
        ).bind(st).bind(si).fetch_all(&state.db).await.unwrap_or_default()
    } else {
        sqlx::query_as(
            "SELECT id, scope_type, scope_id, kind, description, created_at FROM secrets ORDER BY created_at DESC"
        ).fetch_all(&state.db).await.unwrap_or_default()
    };

    let result: Vec<Value> = secrets.iter().map(|(id, st, si, kind, desc, created)| {
        json!({"id": id, "scope_type": st, "scope_id": si, "name": kind, "description": desc, "created_at": created})
    }).collect();

    (StatusCode::OK, Json(json!(result)))
}

async fn delete_secret(
    State(state): State<AppState>, Path(id): Path<String>
) -> impl IntoResponse {
    let secret_id = match Uuid::parse_str(&id) {
        Ok(u) => u,
        Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))),
    };
    let _ = sqlx::query("DELETE FROM secrets WHERE id = $1").bind(secret_id).execute(&state.db).await;
    (StatusCode::OK, Json(json!({"status":"deleted"})))
}

/// List all secrets accessible to an agent (names and descriptions only, never values).
/// Uses the same hierarchical scope logic as get_agent_secret.
async fn list_agent_secrets(
    State(state): State<AppState>, Path(id): Path<String>
) -> impl IntoResponse {
    let agent_id = match Uuid::parse_str(&id) {
        Ok(u) => u,
        Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid agent ID"}))),
    };

    // Get agent's company_id, holding_id, parent_agent_id, and role for hierarchical lookup
    let agent_info: Option<(Option<Uuid>, Uuid, Option<Uuid>, String)> = sqlx::query_as(
        "SELECT a.company_id, a.holding_id, a.parent_agent_id, a.role FROM agents a WHERE a.id = $1"
    ).bind(agent_id).fetch_optional(&state.db).await.ok().flatten();

    let (company_id, holding_id, parent_agent_id, role) = match agent_info {
        Some((cid, hid, pid, r)) => (cid, hid, pid, r),
        None => return (StatusCode::NOT_FOUND, Json(json!({"error":"Agent not found"}))),
    };

    // Build hierarchical scopes: agent → manager (department) → company → holding
    let mut scopes: Vec<(&str, Uuid)> = vec![("agent", agent_id)];
    if role == "MANAGER" {
        scopes.push(("manager", agent_id));
    }
    if let Some(pid) = parent_agent_id {
        let parent_role: Option<String> = sqlx::query_scalar(
            "SELECT role FROM agents WHERE id = $1"
        ).bind(pid).fetch_optional(&state.db).await.ok().flatten();
        if parent_role.as_deref() == Some("MANAGER") {
            scopes.push(("manager", pid));
        }
    }
    if let Some(cid) = company_id {
        scopes.push(("company", cid));
    }
    scopes.push(("holding", holding_id));

    // Collect all accessible secrets (dedup by name — first scope wins, matching fetch behavior)
    let mut seen_names = std::collections::HashSet::new();
    let mut result: Vec<Value> = Vec::new();

    for (scope_type, scope_id) in &scopes {
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT kind, description FROM secrets WHERE scope_type = $1 AND scope_id = $2 ORDER BY kind"
        ).bind(scope_type).bind(scope_id)
        .fetch_all(&state.db).await.unwrap_or_default();

        for (name, description) in rows {
            if seen_names.insert(name.clone()) {
                result.push(json!({
                    "name": name,
                    "description": description,
                    "scope": scope_type,
                }));
            }
        }
    }

    (StatusCode::OK, Json(json!(result)))
}

/// Agent fetches a secret by name. Performs hierarchical lookup:
/// 1. Agent-scoped secrets (scope_type='agent', scope_id=agent_id)
/// 2. Manager/department-scoped secrets (scope_type='manager', scope_id=manager_id)
/// 3. Company-scoped secrets (scope_type='company', scope_id=company_id)
/// 4. Holding-scoped secrets (scope_type='holding', scope_id=holding_id)
async fn get_agent_secret(
    State(state): State<AppState>, Path((id, name)): Path<(String, String)>
) -> impl IntoResponse {
    let agent_id = match Uuid::parse_str(&id) {
        Ok(u) => u,
        Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid agent ID"}))),
    };
    let crypto = match &state.crypto {
        Some(c) => c,
        None => return (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error":"Secrets not available"}))),
    };

    // Get agent's company_id, holding_id, parent_agent_id, and role for hierarchical lookup
    let agent_info: Option<(Option<Uuid>, Uuid, Option<Uuid>, String)> = sqlx::query_as(
        "SELECT a.company_id, a.holding_id, a.parent_agent_id, a.role FROM agents a WHERE a.id = $1"
    ).bind(agent_id).fetch_optional(&state.db).await.ok().flatten();

    let (company_id, holding_id, parent_agent_id, role) = match agent_info {
        Some((cid, hid, pid, r)) => (cid, hid, pid, r),
        None => return (StatusCode::NOT_FOUND, Json(json!({"error":"Agent not found"}))),
    };

    // Hierarchical lookup: agent → manager (department) → company → holding
    let mut scopes: Vec<(&str, Uuid)> = vec![("agent", agent_id)];
    // Managers can access their own department secrets
    if role == "MANAGER" {
        scopes.push(("manager", agent_id));
    }
    // Workers inherit their manager's department secrets
    if let Some(pid) = parent_agent_id {
        let parent_role: Option<String> = sqlx::query_scalar(
            "SELECT role FROM agents WHERE id = $1"
        ).bind(pid).fetch_optional(&state.db).await.ok().flatten();
        if parent_role.as_deref() == Some("MANAGER") {
            scopes.push(("manager", pid));
        }
    }
    if let Some(cid) = company_id {
        scopes.push(("company", cid));
    }
    scopes.push(("holding", holding_id));

    for (scope_type, scope_id) in &scopes {
        let row: Option<(Vec<u8>,)> = sqlx::query_as(
            "SELECT ciphertext FROM secrets WHERE scope_type = $1 AND scope_id = $2 AND kind = $3 LIMIT 1"
        ).bind(scope_type).bind(scope_id).bind(&name)
        .fetch_optional(&state.db).await.ok().flatten();

        if let Some((ciphertext,)) = row {
            match crypto.decrypt(&ciphertext) {
                Ok(plaintext) => {
                    let text = String::from_utf8_lossy(&plaintext).to_string();
                    // Try JSON multi-value format first
                    if let Ok(parsed) = serde_json::from_str::<Value>(&text) {
                        if parsed.get("fields").is_some() {
                            return (StatusCode::OK, Json(json!({"name": name, "fields": parsed["fields"]})));
                        }
                    }
                    // Legacy: plain string value — wrap in fields format
                    return (StatusCode::OK, Json(json!({"name": name, "fields": [{"label": "", "value": text}]})));
                }
                Err(e) => {
                    tracing::error!("Failed to decrypt secret '{}': {}", name, e);
                    return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error":"Decryption failed"})));
                }
            }
        }
    }

    (StatusCode::NOT_FOUND, Json(json!({"error": format!("Secret '{}' not found", name)})))
}

/// Scrub known secret values from agent message text to prevent leaks.
pub(crate) async fn scrub_secrets(db: &sqlx::PgPool, crypto: &crate::crypto::CryptoMaster, agent_id: Uuid, text: &str) -> String {
    // Get agent's company_id, holding_id, parent_agent_id, and role
    let agent_info: Option<(Option<Uuid>, Uuid, Option<Uuid>, String)> = sqlx::query_as(
        "SELECT company_id, holding_id, parent_agent_id, role FROM agents WHERE id = $1"
    ).bind(agent_id).fetch_optional(db).await.ok().flatten();

    let (company_id, holding_id, parent_agent_id, role) = match agent_info {
        Some(info) => info,
        None => return text.to_string(),
    };

    // Collect all scope IDs to query (agent → manager → company → holding)
    let mut scope_conditions = vec![("agent", agent_id)];
    // Manager's own department secrets
    if role == "MANAGER" {
        scope_conditions.push(("manager", agent_id));
    }
    // Worker's department secrets (parent is a manager)
    if let Some(pid) = parent_agent_id {
        let parent_role: Option<String> = sqlx::query_scalar(
            "SELECT role FROM agents WHERE id = $1"
        ).bind(pid).fetch_optional(db).await.ok().flatten();
        if parent_role.as_deref() == Some("MANAGER") {
            scope_conditions.push(("manager", pid));
        }
    }
    if let Some(cid) = company_id {
        scope_conditions.push(("company", cid));
    }
    scope_conditions.push(("holding", holding_id));

    let mut scrubbed = text.to_string();
    for (scope_type, scope_id) in &scope_conditions {
        let rows: Vec<(Vec<u8>,)> = sqlx::query_as(
            "SELECT ciphertext FROM secrets WHERE scope_type = $1 AND scope_id = $2"
        ).bind(scope_type).bind(scope_id)
        .fetch_all(db).await.unwrap_or_default();

        for (ciphertext,) in rows {
            if let Ok(plaintext) = crypto.decrypt(&ciphertext) {
                if let Ok(secret_str) = String::from_utf8(plaintext) {
                    // Try JSON multi-value format: scrub each field's value individually
                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&secret_str) {
                        if let Some(fields) = parsed["fields"].as_array() {
                            for field in fields {
                                if let Some(val) = field["value"].as_str() {
                                    if val.len() >= 4 && scrubbed.contains(val) {
                                        scrubbed = scrubbed.replace(val, "[REDACTED]");
                                    }
                                }
                            }
                            continue;
                        }
                    }
                    // Legacy: plain string value
                    if secret_str.len() >= 4 && scrubbed.contains(&secret_str) {
                        scrubbed = scrubbed.replace(&secret_str, "[REDACTED]");
                    }
                }
            }
        }
    }

    scrubbed
}

/// Read OpenClaw's internal files (sessions, agent config) from the host filesystem.
async fn get_openclaw_files(State(_state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    let agent_id = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };

    let data_root = std::env::var("MULTICLAW_OPENCLAW_DATA").unwrap_or_else(|_| "/opt/multiclaw/openclaw-data".into());
    let agent_dir = std::path::Path::new(&data_root).join(agent_id.to_string());
    let config_dir = agent_dir.join("config");

    let mut files: Vec<serde_json::Value> = Vec::new();

    // Read session files
    let sessions_dir = config_dir.join("agents").join("main").join("sessions");
    if let Ok(mut entries) = tokio::fs::read_dir(&sessions_dir).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                let meta = entry.metadata().await.ok();
                let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
                // Read last 50 lines max for session files
                let content = if size < 100_000 {
                    tokio::fs::read_to_string(&path).await.ok()
                } else {
                    Some(format!("[File too large: {} bytes — showing is disabled]", size))
                };
                files.push(json!({
                    "name": name,
                    "path": format!("sessions/{}", name),
                    "type": "session",
                    "size": size,
                    "content": content,
                }));
            }
        }
    }

    // Read agent state files
    let agents_dir = config_dir.join("agents").join("main");
    for filename in &["state.json", "memory.json", "context.json"] {
        let fpath = agents_dir.join(filename);
        if fpath.exists() {
            let content = tokio::fs::read_to_string(&fpath).await.ok();
            let size = tokio::fs::metadata(&fpath).await.ok().map(|m| m.len()).unwrap_or(0);
            files.push(json!({
                "name": filename,
                "path": format!("agents/main/{}", filename),
                "type": "state",
                "size": size,
                "content": content,
            }));
        }
    }

    // Read the main config
    let config_path = config_dir.join("openclaw.json");
    if config_path.exists() {
        let content = tokio::fs::read_to_string(&config_path).await.ok();
        let size = tokio::fs::metadata(&config_path).await.ok().map(|m| m.len()).unwrap_or(0);
        files.push(json!({
            "name": "openclaw.json",
            "path": "openclaw.json",
            "type": "config",
            "size": size,
            "content": content,
        }));
    }

    // Read workspace brain files (SOUL.md, AGENTS.md, TOOLS.md, etc.)
    let workspace_dir = agent_dir.join("workspace");
    for filename in &["SOUL.md", "AGENTS.md", "TOOLS.md", "BOOTSTRAP.md", "IDENTITY.md", "USER.md"] {
        let fpath = workspace_dir.join(filename);
        if fpath.exists() {
            let content = tokio::fs::read_to_string(&fpath).await.ok();
            let size = tokio::fs::metadata(&fpath).await.ok().map(|m| m.len()).unwrap_or(0);
            files.push(json!({
                "name": filename,
                "path": format!("workspace/{}", filename),
                "type": "brain",
                "size": size,
                "content": content,
            }));
        }
    }
    // Read SKILL.md if present
    let skill_path = workspace_dir.join("skills").join("multiclaw").join("SKILL.md");
    if skill_path.exists() {
        let content = tokio::fs::read_to_string(&skill_path).await.ok();
        let size = tokio::fs::metadata(&skill_path).await.ok().map(|m| m.len()).unwrap_or(0);
        files.push(json!({
            "name": "SKILL.md",
            "path": "workspace/skills/multiclaw/SKILL.md",
            "type": "brain",
            "size": size,
            "content": content,
        }));
    }

    (StatusCode::OK, Json(json!(files)))
}

/// Add a participant to a thread.
async fn add_thread_participant(State(state): State<AppState>, Path(id): Path<String>, Json(p): Json<Value>) -> impl IntoResponse {
    let thread_id = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid thread ID"}))) };
    let member_id = match p.get("member_id").and_then(|v| v.as_str()).and_then(|s| Uuid::parse_str(s).ok()) {
        Some(u) => u, None => return (StatusCode::BAD_REQUEST, Json(json!({"error":"member_id required"})))
    };
    let member_type = p.get("member_type").and_then(|v| v.as_str()).unwrap_or("AGENT");

    match sqlx::query(
        "INSERT INTO thread_members (thread_id, member_type, member_id) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING"
    ).bind(thread_id).bind(member_type).bind(member_id).execute(&state.db).await {
        Ok(_) => {
            let _ = state.tx.send(json!({"type":"participant_added","thread_id": thread_id, "member_id": member_id}).to_string());
            (StatusCode::CREATED, Json(json!({"status":"added", "member_id": member_id})))
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("{}", e)})))
    }
}

/// Remove a participant from a thread.
async fn remove_thread_participant(State(state): State<AppState>, Path((id, member_id)): Path<(String, String)>) -> impl IntoResponse {
    let thread_id = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid thread ID"}))) };
    let mid = match Uuid::parse_str(&member_id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid member ID"}))) };

    let _ = sqlx::query("DELETE FROM thread_members WHERE thread_id = $1 AND member_id = $2")
        .bind(thread_id).bind(mid).execute(&state.db).await;
    let _ = state.tx.send(json!({"type":"participant_removed","thread_id": thread_id, "member_id": mid}).to_string());
    (StatusCode::OK, Json(json!({"status":"removed"})))
}

/// List available models and the current default.
async fn list_models(State(state): State<AppState>) -> impl IntoResponse {
    let raw: String = sqlx::query_scalar("SELECT value FROM system_meta WHERE key = 'available_models'")
        .fetch_optional(&state.db).await.ok().flatten()
        .unwrap_or_else(|| serde_json::to_string(&DEFAULT_MODELS).unwrap_or_default());
    let default_model: String = sqlx::query_scalar("SELECT value FROM system_meta WHERE key = 'default_model'")
        .fetch_optional(&state.db).await.ok().flatten()
        .unwrap_or_else(|| "glm-5:cloud".to_string());
    let models: Vec<String> = serde_json::from_str(&raw)
        .unwrap_or_else(|_| DEFAULT_MODELS.iter().map(|s| s.to_string()).collect());
    (StatusCode::OK, Json(json!({"models": models, "default": default_model})))
}

/// Get the pull status of all known models.
async fn model_pull_status(State(state): State<AppState>) -> impl IntoResponse {
    use crate::openclaw::ModelPullStatus;
    let status = state.openclaw.get_pull_status();
    let mut result = serde_json::Map::new();
    for (model, st) in status {
        let (status_str, error) = match st {
            ModelPullStatus::Pulling => ("pulling", None),
            ModelPullStatus::Ready => ("ready", None),
            ModelPullStatus::Failed(msg) => ("failed", Some(msg)),
        };
        let mut entry = serde_json::Map::new();
        entry.insert("status".to_string(), json!(status_str));
        if let Some(err) = error {
            entry.insert("error".to_string(), json!(err));
        }
        result.insert(model, json!(entry));
    }
    (StatusCode::OK, Json(json!(result)))
}

/// Trigger a pull for a specific model.
async fn pull_model(State(state): State<AppState>, Json(body): Json<Value>) -> impl IntoResponse {
    let model = match body["model"].as_str() {
        Some(m) => m.to_string(),
        None => return (StatusCode::BAD_REQUEST, Json(json!({"error": "missing 'model' field"}))),
    };

    let openclaw = state.openclaw.clone();
    let model_clone = model.clone();
    tokio::spawn(async move {
        openclaw.pull_model(&model_clone).await;
    });

    (StatusCode::ACCEPTED, Json(json!({"status": "pulling", "model": model})))
}

/// Get all system settings from system_meta.
async fn get_system_settings(State(state): State<AppState>) -> impl IntoResponse {
    let rows: Vec<(String, String)> = sqlx::query_as("SELECT key, value FROM system_meta")
        .fetch_all(&state.db).await.unwrap_or_default();
    let mut settings = serde_json::Map::new();
    for (k, v) in rows {
        settings.insert(k, json!(v));
    }
    (StatusCode::OK, Json(json!(settings)))
}

/// Update system settings (upsert key-value pairs in system_meta).
async fn update_system_settings(State(state): State<AppState>, Json(body): Json<Value>) -> impl IntoResponse {
    let mut models_changed = false;

    // Capture old model list BEFORE updating so we can diff for new additions
    let old_models: Vec<String> = if body.as_object().map(|o| o.contains_key("available_models")).unwrap_or(false) {
        sqlx::query_scalar::<_, String>("SELECT value FROM system_meta WHERE key = 'available_models'")
            .fetch_optional(&state.db).await.ok().flatten()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    if let Some(obj) = body.as_object() {
        for (key, val) in obj {
            let v = val.as_str().unwrap_or(&val.to_string()).to_string();
            let _ = sqlx::query(
                "INSERT INTO system_meta (key, value, updated_at) VALUES ($1, $2, NOW()) ON CONFLICT (key) DO UPDATE SET value = $2, updated_at = NOW()"
            ).bind(key).bind(&v).execute(&state.db).await;
            if key == "available_models" { models_changed = true; }
        }
    }
    // Refresh the OpenClaw cached models list so new agent spawns use updated models
    if models_changed {
        state.openclaw.refresh_available_models(&state.db).await;

        // Diff: pull only newly added models in the background
        let new_models: Vec<String> = sqlx::query_scalar::<_, String>(
            "SELECT value FROM system_meta WHERE key = 'available_models'"
        ).fetch_optional(&state.db).await.ok().flatten()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();

        let old_set: std::collections::HashSet<&str> = old_models.iter().map(|s| s.as_str()).collect();
        let added: Vec<String> = new_models.into_iter()
            .filter(|m| !old_set.contains(m.as_str()))
            .collect();

        if !added.is_empty() {
            tracing::info!("New models added via settings: {:?} — triggering pull", added);
            let openclaw = state.openclaw.clone();
            tokio::spawn(async move {
                openclaw.pull_all_models(added).await;
            });
        }
    }
    (StatusCode::OK, Json(json!({"status":"updated"})))
}

// ═══════════════════════════════════════════════════════════════
// World Snapshot
// ═══════════════════════════════════════════════════════════════

// ═══════════════════════════════════════════════════════════════
// Rewrite
// ═══════════════════════════════════════════════════════════════

#[derive(Deserialize)]
struct RewriteRequest {
    text: String,
    model: Option<String>,
}

async fn rewrite_text(
    State(state): State<AppState>,
    Json(body): Json<RewriteRequest>,
) -> impl IntoResponse {
    // Determine model: request body → system setting → default
    let model = if let Some(m) = body.model {
        m
    } else {
        let setting: Option<(String,)> = sqlx::query_as(
            "SELECT value FROM system_meta WHERE key = 'rewrite_model'"
        )
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten();
        setting.map(|s| s.0).unwrap_or_else(|| "glm-5:cloud".to_string())
    };

    let system_prompt = "You are a message rewriting assistant. The user will give you a draft \
        message they want to send to someone. Rewrite it to improve clarity, grammar, and flow \
        while preserving ALL details, specifics, and meaning. Keep the same level of detail as \
        the original — do not summarize or condense. Preserve the user's original word choices \
        and phrasing where they are clear and natural. Output ONLY the rewritten message — no \
        explanations, no preamble, no quotes around it. Do NOT use any markdown formatting — no \
        bold, no italics, no headers, no bullet points, no asterisks. Write in plain text only.";

    let ollama_url = format!("{}/api/chat", state.config.ollama_url);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    let chat_req = json!({
        "model": model,
        "messages": [
            { "role": "system", "content": system_prompt },
            { "role": "user", "content": body.text }
        ],
        "stream": false,
        "tools": []
    });

    let resp = client.post(&ollama_url).json(&chat_req).send().await;
    match resp {
        Ok(r) => {
            let status = r.status();
            if !status.is_success() {
                let err_text = r.text().await.unwrap_or_default();
                return (StatusCode::BAD_GATEWAY, Json(json!({
                    "error": format!("Ollama returned {}: {}", status, err_text)
                })));
            }
            match r.json::<Value>().await {
                Ok(val) => {
                    let rewritten = val["message"]["content"]
                        .as_str()
                        .unwrap_or("")
                        .to_string();
                    (StatusCode::OK, Json(json!({ "rewritten": rewritten })))
                }
                Err(e) => (StatusCode::BAD_GATEWAY, Json(json!({
                    "error": format!("Failed to parse Ollama response: {}", e)
                }))),
            }
        }
        Err(e) => (StatusCode::BAD_GATEWAY, Json(json!({
            "error": format!("Failed to reach Ollama: {}", e)
        }))),
    }
}

/// Aggregated snapshot endpoint for the 3D world view.
/// Returns companies, agents, balances, activities, and VM states in one call.
async fn world_snapshot(State(state): State<AppState>) -> impl IntoResponse {
    // 1. Fetch all companies
    let companies: Vec<Company> = sqlx::query_as(
        "SELECT id, holding_id, name, type, description, tags, status, created_at FROM companies ORDER BY created_at"
    ).fetch_all(&state.db).await.unwrap_or_default();

    // 2. Fetch all agents (excluding MAIN role — they're not in any company)
    let agents: Vec<Agent> = sqlx::query_as(
        "SELECT id, holding_id, company_id, role, name, specialty, parent_agent_id, preferred_model, effective_model, system_prompt, tool_policy_id, vm_id, sandbox_vm_id, handle, status, created_at \
         FROM agents WHERE role != 'MAIN' ORDER BY created_at"
    ).fetch_all(&state.db).await.unwrap_or_default();

    // 3. Fetch balances for all companies in one query
    let balance_rows: Vec<(Uuid, String, String, rust_decimal::Decimal)> = sqlx::query_as(
        "SELECT company_id, currency, type, COALESCE(SUM(amount), 0) as total \
         FROM ledger_entries GROUP BY company_id, currency, type"
    ).fetch_all(&state.db).await.unwrap_or_default();

    let mut balances: serde_json::Map<String, Value> = serde_json::Map::new();
    for (company_id, currency, entry_type, total) in &balance_rows {
        let company_key = company_id.to_string();
        let company_obj = balances.entry(company_key)
            .or_insert_with(|| json!({}));
        let currency_obj = company_obj.as_object_mut().unwrap()
            .entry(currency.clone())
            .or_insert_with(|| json!({"revenue": 0.0, "expenses": 0.0, "capital": 0.0, "net": 0.0}));
        let total_f64 = total.to_string().parse::<f64>().unwrap_or(0.0);
        match entry_type.as_str() {
            "REVENUE" => { currency_obj["revenue"] = json!(total_f64); }
            "EXPENSE" => { currency_obj["expenses"] = json!(total_f64); }
            "CAPITAL_INJECTION" => { currency_obj["capital"] = json!(total_f64); }
            "INTERNAL_TRANSFER" => { currency_obj["expenses"] = json!(currency_obj["expenses"].as_f64().unwrap_or(0.0) + total_f64); }
            _ => {}
        }
    }
    // Calculate net for each company/currency
    for (_, company_obj) in balances.iter_mut() {
        if let Some(currencies) = company_obj.as_object_mut() {
            for (_, obj) in currencies.iter_mut() {
                let revenue = obj["revenue"].as_f64().unwrap_or(0.0);
                let expenses = obj["expenses"].as_f64().unwrap_or(0.0);
                let capital = obj["capital"].as_f64().unwrap_or(0.0);
                obj["net"] = json!(capital + revenue - expenses);
            }
        }
    }

    // 4. Activities — from in-memory tracker (if present), otherwise empty
    let activities: serde_json::Map<String, Value> = if let Some(ref tracker) = *state.agent_activities.read().await {
        let mut map = serde_json::Map::new();
        for (agent_id, activity) in tracker.iter() {
            map.insert(agent_id.to_string(), json!({
                "agent_id": agent_id.to_string(),
                "status": activity.status,
                "task": activity.task,
                "since": activity.since,
            }));
        }
        map
    } else {
        serde_json::Map::new()
    };

    // 5. VM states — check if agents have VMs provisioned, batch-query status
    let mut vm_states: serde_json::Map<String, Value> = serde_json::Map::new();

    // Query which agents have VMs assigned
    let vm_rows: Vec<(Uuid, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT a.id, v_desktop.provider_ref, v_sandbox.provider_ref \
         FROM agents a \
         LEFT JOIN vms v_desktop ON a.vm_id = v_desktop.id \
         LEFT JOIN vms v_sandbox ON a.sandbox_vm_id = v_sandbox.id \
         WHERE a.role != 'MAIN'"
    ).fetch_all(&state.db).await.unwrap_or_default();

    // If we have a VM provider, batch-query running instances
    let running_vms: std::collections::HashSet<String> = if let Some(ref provider) = state.vm_provider {
        // Get all running instances in one call
        match provider.list_running().await {
            Ok(names) => names.into_iter().collect(),
            Err(_) => std::collections::HashSet::new(),
        }
    } else {
        std::collections::HashSet::new()
    };

    for (agent_id, desktop_ref, sandbox_ref) in &vm_rows {
        let desktop_status = match desktop_ref {
            Some(name) if running_vms.contains(name) => "RUNNING",
            Some(_) => "STOPPED",
            None => "UNKNOWN",
        };
        let sandbox_status = match sandbox_ref {
            Some(name) if running_vms.contains(name) => "RUNNING",
            Some(_) => "STOPPED",
            None => "UNKNOWN",
        };
        vm_states.insert(agent_id.to_string(), json!({
            "desktop": desktop_status,
            "sandbox": sandbox_status,
        }));
    }

    (StatusCode::OK, Json(json!({
        "companies": companies,
        "agents": agents,
        "balances": balances,
        "activities": activities,
        "vm_states": vm_states,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── strip_narration_lines ──────────────────────────────────────

    #[test]
    fn narration_pure_prefix() {
        assert_eq!(strip_narration_lines("Let me check..."), "");
        assert_eq!(strip_narration_lines("Sending now."), "");
        assert_eq!(strip_narration_lines("I'll review."), "");
    }

    #[test]
    fn narration_extended_prefix_stripped() {
        // Extended narration: prefix + content that doesn't address anyone
        assert_eq!(
            strip_narration_lines("Let me read the multiclaw skill for the API details."),
            ""
        );
        assert_eq!(
            strip_narration_lines("I'll create a new company for crypto trading."),
            ""
        );
        assert_eq!(
            strip_narration_lines("Let me start by reviewing the credentials."),
            ""
        );
    }

    #[test]
    fn narration_preserves_conversational() {
        // Lines addressing someone should be kept
        let line = "I'll send you the report shortly.";
        assert_eq!(strip_narration_lines(line), line);

        let line2 = "Let me check — did you receive it?";
        assert_eq!(strip_narration_lines(line2), line2);

        let line3 = "I'll review your proposal and get back to you.";
        assert_eq!(strip_narration_lines(line3), line3);
    }

    #[test]
    fn narration_filler_prefix() {
        assert_eq!(
            strip_narration_lines("Good. Let me check if the trade credentials have been provisioned."),
            ""
        );
        assert_eq!(
            strip_narration_lines("OK. I'll review the latest market data."),
            ""
        );
        assert_eq!(
            strip_narration_lines("Understood. Let me prepare the deployment."),
            ""
        );
    }

    #[test]
    fn narration_multiline() {
        let input = "Hello Marcus.\nLet me read the skill documentation.\nHere is the update.";
        let result = strip_narration_lines(input);
        assert_eq!(result, "Hello Marcus.\nHere is the update.");
    }

    #[test]
    fn narration_preserves_normal_text() {
        let input = "The market is up 5% today. We should consider increasing our position.";
        assert_eq!(strip_narration_lines(input), input);
    }

    // ── dedup_content_blocks ───────────────────────────────────────

    #[test]
    fn dedup_identical_halves() {
        let msg = "Hello, this is a status update with details about the project.";
        let input = format!("{}\n\n{}", msg, msg);
        assert_eq!(dedup_content_blocks(&input), msg);
    }

    #[test]
    fn dedup_consecutive_paragraphs() {
        let input = "First paragraph here.\n\nSecond paragraph here.\n\nSecond paragraph here.\n\nThird paragraph.";
        let expected = "First paragraph here.\n\nSecond paragraph here.\n\nThird paragraph.";
        assert_eq!(dedup_content_blocks(input), expected);
    }

    #[test]
    fn dedup_no_false_positive() {
        let input = "This is one paragraph.\n\nThis is a different paragraph.";
        assert_eq!(dedup_content_blocks(input), input);
    }

    #[test]
    fn dedup_short_text_unchanged() {
        let input = "Short text.";
        assert_eq!(dedup_content_blocks(input), input);
    }

    // ── strip_agent_tags ───────────────────────────────────────────

    #[test]
    fn tags_end_conversation() {
        let (text, end) = strip_agent_tags("Thanks for the update.\n[END_CONVERSATION]");
        assert_eq!(text, "Thanks for the update.");
        assert!(end);
    }

    #[test]
    fn tags_heartbeat() {
        let (text, _) = strip_agent_tags("[HEARTBEAT_OK]");
        assert!(text.is_empty());
    }

    #[test]
    fn tags_fragmented() {
        // Streaming can split tags across tokens
        let (text, end) = strip_agent_tags("END _CONVERSATION");
        assert!(text.is_empty());
        assert!(end);
    }

    #[test]
    fn tags_timeout_message() {
        let (text, _) = strip_agent_tags(
            "Request timed out before a response was generated. Please try again, or increase `agents.defaults.timeoutSeconds` in your config."
        );
        assert!(text.is_empty());
    }

    #[test]
    fn tags_timeout_message_no_backticks() {
        let (text, _) = strip_agent_tags(
            "Request timed out before a response was generated. Please try again, or increase agents.defaults.timeoutSeconds in your config."
        );
        assert!(text.is_empty());
    }

    #[test]
    fn tags_reply_to_current() {
        let (text, _) = strip_agent_tags("[[reply_to_current]] Hello there.");
        assert_eq!(text, "Hello there.");
    }

    #[test]
    fn tags_preserves_content() {
        let (text, end) = strip_agent_tags("The quarterly results look promising. Revenue is up 15%.");
        assert_eq!(text, "The quarterly results look promising. Revenue is up 15%.");
        assert!(!end);
    }

    #[test]
    fn tags_strips_markdown_bold() {
        let (text, _) = strip_agent_tags("the**$150 profit target within 7 days**");
        assert_eq!(text, "the $150 profit target within 7 days");
    }

    // ── word_overlap_ratio ─────────────────────────────────────────

    #[test]
    fn overlap_high() {
        let a = "Update: We have configured the COINEX API credentials for trading operations.";
        let b = "Good. The COINEX API credentials have been configured. Update sent via DM.";
        let ratio = word_overlap_ratio(a, b);
        assert!(ratio > 0.6, "Expected >60% overlap, got {:.0}%", ratio * 100.0);
    }

    #[test]
    fn overlap_low() {
        let a = "The quarterly revenue report is ready for review.";
        let b = "Please hire a new manager for the engineering department.";
        let ratio = word_overlap_ratio(a, b);
        assert!(ratio < 0.3, "Expected <30% overlap, got {:.0}%", ratio * 100.0);
    }

    // ── fix_broken_words (heuristic) ─────────────────────────────

    #[test]
    fn broken_words_tier0_known_patterns() {
        assert_eq!(fix_broken_words("Under stood"), "Understood");
        assert_eq!(fix_broken_words("H ire a manager"), "Hire a manager");
        assert_eq!(fix_broken_words("Escal ated to CEO"), "Escalated to CEO");
    }

    #[test]
    fn broken_words_tier1_single_letter() {
        assert_eq!(fix_broken_words("D erek said hello"), "Derek said hello");
        assert_eq!(fix_broken_words("E lena reported"), "Elena reported");
        assert_eq!(fix_broken_words("S andbox ready"), "Sandbox ready");
        assert_eq!(fix_broken_words("R eview the plan"), "Review the plan");
        assert_eq!(fix_broken_words("M arcus approved"), "Marcus approved");
    }

    #[test]
    fn broken_words_tier1_preserves_a_and_i() {
        assert_eq!(fix_broken_words("A review is needed"), "A review is needed");
        assert_eq!(fix_broken_words("I mentioned earlier"), "I mentioned earlier");
    }

    #[test]
    fn broken_words_tier2_short_fragment() {
        assert_eq!(fix_broken_words("Ap proved the request"), "Approved the request");
        assert_eq!(fix_broken_words("Ex cellent work today"), "Excellent work today");
        assert_eq!(fix_broken_words("Im mediately after"), "Immediately after");
        assert_eq!(fix_broken_words("Cer tainly we can"), "Certainly we can");
    }

    #[test]
    fn broken_words_tier2_preserves_common_words() {
        assert_eq!(fix_broken_words("The meeting starts"), "The meeting starts");
        assert_eq!(fix_broken_words("His report was good"), "His report was good");
        assert_eq!(fix_broken_words("New orders arrived"), "New orders arrived");
        assert_eq!(fix_broken_words("Our team delivered"), "Our team delivered");
        assert_eq!(fix_broken_words("For example here"), "For example here");
        assert_eq!(fix_broken_words("She mentioned that"), "She mentioned that");
    }

    #[test]
    fn broken_words_multiple_in_one_string() {
        assert_eq!(
            fix_broken_words("D erek Ap proved the H ire request"),
            "Derek Approved the Hire request"
        );
    }

    #[test]
    fn broken_words_no_false_merges() {
        assert_eq!(fix_broken_words("Go ahead and start"), "Go ahead and start");
        assert_eq!(fix_broken_words("Set everything up"), "Set everything up");
        assert_eq!(fix_broken_words("All systems ready"), "All systems ready");
    }

    // ── strip_markdown_bold ────────────────────────────────────────

    #[test]
    fn strip_bold_basic() {
        assert_eq!(strip_markdown_bold("**Status Update:**"), "Status Update:");
        assert_eq!(strip_markdown_bold("** Hello** world"), " Hello world");
    }

    #[test]
    fn strip_bold_glued_to_word() {
        // The reported bug: "the**$150 profit target within 7 days**"
        assert_eq!(strip_markdown_bold("the**$150 profit target within 7 days**"), "the $150 profit target within 7 days");
    }

    #[test]
    fn strip_bold_preserves_plain() {
        assert_eq!(strip_markdown_bold("no bold here"), "no bold here");
    }

    #[test]
    fn strip_bold_multiple() {
        assert_eq!(strip_markdown_bold("**a** and **b**"), "a and b");
    }

    #[test]
    fn strip_bold_with_spaces() {
        // Space already present — don't double up
        assert_eq!(strip_markdown_bold("word **bold** rest"), "word bold rest");
    }

    // ── word_overlap_ratio ─────────────────────────────────────────

    #[test]
    fn overlap_empty() {
        assert_eq!(word_overlap_ratio("", "hello"), 0.0);
        assert_eq!(word_overlap_ratio("hello", ""), 0.0);
        assert_eq!(word_overlap_ratio("", ""), 0.0);
    }

    #[test]
    fn overlap_identical() {
        let text = "This is a test message with several words in it.";
        let ratio = word_overlap_ratio(text, text);
        assert!((ratio - 1.0).abs() < f64::EPSILON);
    }
}
