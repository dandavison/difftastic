//! Unified diff display, for consumption by tools like delta.

use line_numbers::LineNumber;

use crate::display::context::all_matched_lines_filled;
use crate::display::hunks::{matched_lines_indexes_for_hunk, Hunk};
use crate::lines::split_on_newlines;
use crate::options::DisplayOptions;
use crate::parse::syntax::MatchedPos;

pub(crate) fn print(
    lhs_src: &str,
    rhs_src: &str,
    display_options: &DisplayOptions,
    lhs_mps: &[MatchedPos],
    rhs_mps: &[MatchedPos],
    hunks: &[Hunk],
    display_path: &str,
    extra_info: &Option<String>,
) {
    let lhs_lines: Vec<&str> = split_on_newlines(lhs_src).collect();
    let rhs_lines: Vec<&str> = split_on_newlines(rhs_src).collect();

    let matched_lines = all_matched_lines_filled(lhs_mps, rhs_mps, &lhs_lines, &rhs_lines);

    let display_path = display_path.strip_prefix('/').unwrap_or(display_path);

    println!("--- a/{}", display_path);
    println!("+++ b/{}", display_path);
    if let Some(info) = extra_info {
        for line in info.lines() {
            println!("# {}", line);
        }
    }

    for hunk in hunks {
        let (start_i, end_i) = matched_lines_indexes_for_hunk(
            &matched_lines,
            hunk,
            display_options.num_context_lines as usize,
        );
        let aligned_lines = &matched_lines[start_i..end_i];

        let entries = classify_lines(aligned_lines, &lhs_lines, &rhs_lines);
        let grouped = group_entries(entries);

        let (lhs_start, lhs_count, rhs_start, rhs_count) = hunk_header_counts(&grouped);

        println!(
            "@@ -{},{} +{},{} @@",
            lhs_start, lhs_count, rhs_start, rhs_count
        );

        for entry in &grouped {
            match entry {
                UnifiedEntry::Context { lhs_line, .. } => {
                    println!(" {}", line_content(*lhs_line, &lhs_lines));
                }
                UnifiedEntry::Removed { lhs_line } => {
                    println!("-{}", line_content(*lhs_line, &lhs_lines));
                }
                UnifiedEntry::Added { rhs_line } => {
                    println!("+{}", line_content(*rhs_line, &rhs_lines));
                }
            }
        }
    }
}

fn line_content<'a>(line_num: LineNumber, lines: &[&'a str]) -> &'a str {
    let idx = line_num.as_usize();
    if idx < lines.len() {
        lines[idx]
    } else {
        ""
    }
}

#[derive(Debug)]
enum UnifiedEntry {
    Context {
        lhs_line: LineNumber,
        rhs_line: LineNumber,
    },
    Removed {
        lhs_line: LineNumber,
    },
    Added {
        rhs_line: LineNumber,
    },
}

impl UnifiedEntry {
    fn is_context(&self) -> bool {
        matches!(self, UnifiedEntry::Context { .. })
    }
}

/// Convert aligned line pairs into a sequence of unified diff entries.
/// Classification is based on text equality, not novel sets: the AST
/// awareness is in hunk selection (which hunks to emit), not in
/// within-hunk line classification.
fn classify_lines(
    lines: &[(Option<LineNumber>, Option<LineNumber>)],
    lhs_lines: &[&str],
    rhs_lines: &[&str],
) -> Vec<UnifiedEntry> {
    let mut entries = Vec::new();

    for &(lhs_line, rhs_line) in lines {
        match (lhs_line, rhs_line) {
            (Some(l), Some(r))
                if line_content(l, lhs_lines) == line_content(r, rhs_lines) =>
            {
                entries.push(UnifiedEntry::Context {
                    lhs_line: l,
                    rhs_line: r,
                });
            }
            (Some(l), Some(r)) => {
                entries.push(UnifiedEntry::Removed { lhs_line: l });
                entries.push(UnifiedEntry::Added { rhs_line: r });
            }
            (Some(l), None) => {
                entries.push(UnifiedEntry::Removed { lhs_line: l });
            }
            (None, Some(r)) => {
                entries.push(UnifiedEntry::Added { rhs_line: r });
            }
            (None, None) => {}
        }
    }

    entries
}

/// Reorder entries so that within each contiguous block of
/// non-context lines, all removals precede all additions.
/// This matches the standard unified diff convention.
fn group_entries(entries: Vec<UnifiedEntry>) -> Vec<UnifiedEntry> {
    let mut result = Vec::with_capacity(entries.len());
    let mut removals: Vec<UnifiedEntry> = Vec::new();
    let mut additions: Vec<UnifiedEntry> = Vec::new();

    for entry in entries {
        if entry.is_context() {
            result.extend(removals.drain(..));
            result.extend(additions.drain(..));
            result.push(entry);
        } else {
            match &entry {
                UnifiedEntry::Removed { .. } => removals.push(entry),
                UnifiedEntry::Added { .. } => additions.push(entry),
                _ => unreachable!(),
            }
        }
    }

    result.extend(removals);
    result.extend(additions);
    result
}

/// Compute the lhs_start, lhs_count, rhs_start, rhs_count for the
/// @@ header.
fn hunk_header_counts(entries: &[UnifiedEntry]) -> (u32, u32, u32, u32) {
    let mut lhs_start: Option<u32> = None;
    let mut lhs_count: u32 = 0;
    let mut rhs_start: Option<u32> = None;
    let mut rhs_count: u32 = 0;

    for entry in entries {
        match entry {
            UnifiedEntry::Context { lhs_line, rhs_line } => {
                if lhs_start.is_none() {
                    lhs_start = Some(lhs_line.0 + 1);
                }
                if rhs_start.is_none() {
                    rhs_start = Some(rhs_line.0 + 1);
                }
                lhs_count += 1;
                rhs_count += 1;
            }
            UnifiedEntry::Removed { lhs_line } => {
                if lhs_start.is_none() {
                    lhs_start = Some(lhs_line.0 + 1);
                }
                lhs_count += 1;
            }
            UnifiedEntry::Added { rhs_line } => {
                if rhs_start.is_none() {
                    rhs_start = Some(rhs_line.0 + 1);
                }
                rhs_count += 1;
            }
        }
    }

    (
        lhs_start.unwrap_or(1),
        lhs_count,
        rhs_start.unwrap_or(1),
        rhs_count,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_context_only() {
        let lhs: Vec<&str> = vec!["aaa", "bbb"];
        let rhs: Vec<&str> = vec!["aaa", "bbb"];
        let lines = vec![
            (Some(LineNumber::from(0)), Some(LineNumber::from(0))),
            (Some(LineNumber::from(1)), Some(LineNumber::from(1))),
        ];

        let entries = classify_lines(&lines, &lhs, &rhs);
        assert_eq!(entries.len(), 2);
        assert!(matches!(entries[0], UnifiedEntry::Context { .. }));
        assert!(matches!(entries[1], UnifiedEntry::Context { .. }));
    }

    #[test]
    fn test_classify_modification() {
        let lhs: Vec<&str> = vec!["old"];
        let rhs: Vec<&str> = vec!["new"];
        let lines = vec![(Some(LineNumber::from(0)), Some(LineNumber::from(0)))];

        let entries = classify_lines(&lines, &lhs, &rhs);
        assert_eq!(entries.len(), 2);
        assert!(matches!(entries[0], UnifiedEntry::Removed { .. }));
        assert!(matches!(entries[1], UnifiedEntry::Added { .. }));
    }

    #[test]
    fn test_classify_pure_addition() {
        let lhs: Vec<&str> = vec![];
        let rhs: Vec<&str> = vec!["new"];
        let lines = vec![(None, Some(LineNumber::from(0)))];

        let entries = classify_lines(&lines, &lhs, &rhs);
        assert_eq!(entries.len(), 1);
        assert!(matches!(entries[0], UnifiedEntry::Added { .. }));
    }

    #[test]
    fn test_classify_pure_removal() {
        let lhs: Vec<&str> = vec!["old"];
        let rhs: Vec<&str> = vec![];
        let lines = vec![(Some(LineNumber::from(0)), None)];

        let entries = classify_lines(&lines, &lhs, &rhs);
        assert_eq!(entries.len(), 1);
        assert!(matches!(entries[0], UnifiedEntry::Removed { .. }));
    }

    #[test]
    fn test_group_entries_reorders_modifications() {
        let entries = vec![
            UnifiedEntry::Removed {
                lhs_line: LineNumber::from(0),
            },
            UnifiedEntry::Added {
                rhs_line: LineNumber::from(0),
            },
            UnifiedEntry::Removed {
                lhs_line: LineNumber::from(1),
            },
            UnifiedEntry::Added {
                rhs_line: LineNumber::from(1),
            },
        ];

        let grouped = group_entries(entries);
        assert_eq!(grouped.len(), 4);
        assert!(matches!(grouped[0], UnifiedEntry::Removed { .. }));
        assert!(matches!(grouped[1], UnifiedEntry::Removed { .. }));
        assert!(matches!(grouped[2], UnifiedEntry::Added { .. }));
        assert!(matches!(grouped[3], UnifiedEntry::Added { .. }));
    }

    #[test]
    fn test_group_entries_preserves_context_boundaries() {
        let entries = vec![
            UnifiedEntry::Removed {
                lhs_line: LineNumber::from(0),
            },
            UnifiedEntry::Added {
                rhs_line: LineNumber::from(0),
            },
            UnifiedEntry::Context {
                lhs_line: LineNumber::from(1),
                rhs_line: LineNumber::from(1),
            },
            UnifiedEntry::Removed {
                lhs_line: LineNumber::from(2),
            },
            UnifiedEntry::Added {
                rhs_line: LineNumber::from(2),
            },
        ];

        let grouped = group_entries(entries);
        assert_eq!(grouped.len(), 5);
        assert!(matches!(grouped[0], UnifiedEntry::Removed { .. }));
        assert!(matches!(grouped[1], UnifiedEntry::Added { .. }));
        assert!(matches!(grouped[2], UnifiedEntry::Context { .. }));
        assert!(matches!(grouped[3], UnifiedEntry::Removed { .. }));
        assert!(matches!(grouped[4], UnifiedEntry::Added { .. }));
    }

    #[test]
    fn test_hunk_header_counts_simple() {
        let entries = vec![
            UnifiedEntry::Context {
                lhs_line: LineNumber::from(0),
                rhs_line: LineNumber::from(0),
            },
            UnifiedEntry::Removed {
                lhs_line: LineNumber::from(1),
            },
            UnifiedEntry::Added {
                rhs_line: LineNumber::from(1),
            },
            UnifiedEntry::Context {
                lhs_line: LineNumber::from(2),
                rhs_line: LineNumber::from(2),
            },
        ];

        let (ls, lc, rs, rc) = hunk_header_counts(&entries);
        assert_eq!((ls, lc, rs, rc), (1, 3, 1, 3));
    }

    #[test]
    fn test_hunk_header_counts_addition() {
        let entries = vec![
            UnifiedEntry::Context {
                lhs_line: LineNumber::from(4),
                rhs_line: LineNumber::from(4),
            },
            UnifiedEntry::Added {
                rhs_line: LineNumber::from(5),
            },
            UnifiedEntry::Context {
                lhs_line: LineNumber::from(5),
                rhs_line: LineNumber::from(6),
            },
        ];

        let (ls, lc, rs, rc) = hunk_header_counts(&entries);
        assert_eq!((ls, lc, rs, rc), (5, 2, 5, 3));
    }
}
