//! Unified diff display, for consumption by external tools

use line_numbers::LineNumber;

use crate::display::context::{
    calculate_after_context, calculate_before_context, opposite_positions,
};
use crate::display::hunks::Hunk;
use crate::lines::{split_on_newlines, MaxLine};
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

    let opposite_to_lhs = opposite_positions(lhs_mps);
    let opposite_to_rhs = opposite_positions(rhs_mps);

    let display_path = display_path.strip_prefix('/').unwrap_or(display_path);

    println!("--- a/{}", display_path);
    println!("+++ b/{}", display_path);
    if let Some(info) = extra_info {
        for line in info.lines() {
            println!("# {}", line);
        }
    }

    for hunk in hunks {
        let hunk_lines = hunk.lines.clone();

        let before_lines = calculate_before_context(
            &hunk_lines,
            &opposite_to_lhs,
            &opposite_to_rhs,
            display_options.num_context_lines as usize,
        );
        let after_lines = calculate_after_context(
            &[&before_lines[..], &hunk_lines[..]].concat(),
            &opposite_to_lhs,
            &opposite_to_rhs,
            lhs_src.max_line(),
            rhs_src.max_line(),
            display_options.num_context_lines as usize,
        );

        let all_lines: Vec<(Option<LineNumber>, Option<LineNumber>)> = before_lines
            .iter()
            .chain(hunk_lines.iter())
            .chain(after_lines.iter())
            .copied()
            .collect();

        let entries = classify_lines(&all_lines, &hunk.novel_lhs, &hunk.novel_rhs);
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
fn classify_lines(
    lines: &[(Option<LineNumber>, Option<LineNumber>)],
    novel_lhs: &crate::hash::DftHashSet<LineNumber>,
    novel_rhs: &crate::hash::DftHashSet<LineNumber>,
) -> Vec<UnifiedEntry> {
    let mut entries = Vec::new();

    for &(lhs_line, rhs_line) in lines {
        let lhs_novel = lhs_line.map_or(false, |l| novel_lhs.contains(&l));
        let rhs_novel = rhs_line.map_or(false, |r| novel_rhs.contains(&r));

        match (lhs_line, rhs_line) {
            (Some(l), Some(r)) if !lhs_novel && !rhs_novel => {
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
    use crate::hash::DftHashSet;
    use std::iter::FromIterator;

    #[test]
    fn test_classify_context_only() {
        let lines = vec![
            (Some(LineNumber::from(0)), Some(LineNumber::from(0))),
            (Some(LineNumber::from(1)), Some(LineNumber::from(1))),
        ];
        let novel_lhs = DftHashSet::default();
        let novel_rhs = DftHashSet::default();

        let entries = classify_lines(&lines, &novel_lhs, &novel_rhs);
        assert_eq!(entries.len(), 2);
        assert!(matches!(entries[0], UnifiedEntry::Context { .. }));
        assert!(matches!(entries[1], UnifiedEntry::Context { .. }));
    }

    #[test]
    fn test_classify_modification() {
        let lines = vec![(Some(LineNumber::from(0)), Some(LineNumber::from(0)))];
        let novel_lhs = DftHashSet::from_iter([LineNumber::from(0)]);
        let novel_rhs = DftHashSet::from_iter([LineNumber::from(0)]);

        let entries = classify_lines(&lines, &novel_lhs, &novel_rhs);
        assert_eq!(entries.len(), 2);
        assert!(matches!(entries[0], UnifiedEntry::Removed { .. }));
        assert!(matches!(entries[1], UnifiedEntry::Added { .. }));
    }

    #[test]
    fn test_classify_pure_addition() {
        let lines = vec![(None, Some(LineNumber::from(0)))];
        let novel_lhs = DftHashSet::default();
        let novel_rhs = DftHashSet::from_iter([LineNumber::from(0)]);

        let entries = classify_lines(&lines, &novel_lhs, &novel_rhs);
        assert_eq!(entries.len(), 1);
        assert!(matches!(entries[0], UnifiedEntry::Added { .. }));
    }

    #[test]
    fn test_classify_pure_removal() {
        let lines = vec![(Some(LineNumber::from(0)), None)];
        let novel_lhs = DftHashSet::from_iter([LineNumber::from(0)]);
        let novel_rhs = DftHashSet::default();

        let entries = classify_lines(&lines, &novel_lhs, &novel_rhs);
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
