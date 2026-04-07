//! Output truncation helper matching the TypeScript `Truncate` service behaviour.

use std::fs;
use std::path::{Path, PathBuf};

/// Maximum lines before truncation kicks in.
pub const MAX_LINES: usize = 2000;
/// Maximum bytes before truncation kicks in.
pub const MAX_BYTES: usize = 50 * 1024;

/// Direction to keep when truncating.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Direction {
    /// Keep the beginning (default).
    #[default]
    Head,
    /// Keep the end.
    Tail,
}

/// Result of a truncation call.
#[derive(Debug)]
pub struct TruncResult {
    /// The (possibly truncated) content to surface to the caller.
    pub content: String,
    /// Whether the output was truncated.
    pub truncated: bool,
    /// Path to the file containing the full output (only set when truncated).
    pub output_path: Option<PathBuf>,
}

/// Truncate `text` if it exceeds `max_lines` or `max_bytes`.
///
/// When truncated, the full text is written to `<out_dir>/<filename>` and a
/// hint is appended to the returned content.
///
/// # Errors
/// Returns `io::Error` if writing the overflow file fails.
pub fn truncate(
    text: &str,
    max_lines: usize,
    max_bytes: usize,
    direction: Direction,
    out_dir: &Path,
    filename: &str,
) -> std::io::Result<TruncResult> {
    let lines: Vec<&str> = text.split('\n').collect();
    let total_bytes = text.len(); // UTF-8 byte length

    if lines.len() <= max_lines && total_bytes <= max_bytes {
        return Ok(TruncResult {
            content: text.to_string(),
            truncated: false,
            output_path: None,
        });
    }

    let (out, removed, hit_bytes) = collect_lines(&lines, max_lines, max_bytes, direction);
    let unit = if hit_bytes { "bytes" } else { "lines" };
    let preview = out.join("\n");
    let file = out_dir.join(filename);

    fs::create_dir_all(out_dir)?;
    fs::write(&file, text)?;

    let hint = format!(
        "The tool call succeeded but the output was truncated. Full output saved to: {}\nUse Grep to search the full content or Read with offset/limit to view specific sections.",
        file.display()
    );

    let content = match direction {
        Direction::Head => format!("{preview}\n\n...{removed} {unit} truncated...\n\n{hint}"),
        Direction::Tail => format!("...{removed} {unit} truncated...\n\n{hint}\n\n{preview}"),
    };

    Ok(TruncResult {
        content,
        truncated: true,
        output_path: Some(file),
    })
}

fn collect_lines<'a>(
    lines: &[&'a str],
    max_lines: usize,
    max_bytes: usize,
    direction: Direction,
) -> (Vec<&'a str>, usize, bool) {
    let mut out: Vec<&'a str> = Vec::new();
    let mut bytes = 0usize;
    let mut hit = false;

    match direction {
        Direction::Head => {
            for (i, &line) in lines.iter().enumerate() {
                if i >= max_lines {
                    break;
                }
                let size = line.len() + if i > 0 { 1 } else { 0 };
                if bytes + size > max_bytes {
                    hit = true;
                    break;
                }
                out.push(line);
                bytes += size;
            }
        }
        Direction::Tail => {
            for (i, &line) in lines.iter().enumerate().rev() {
                if out.len() >= max_lines {
                    break;
                }
                let size = line.len() + if !out.is_empty() { 1 } else { 0 };
                if bytes + size > max_bytes {
                    hit = true;
                    break;
                }
                out.insert(0, line);
                bytes += size;
                let _ = i; // suppress warning
            }
        }
    }

    let removed = if hit {
        (lines.iter().map(|l| l.len()).sum::<usize>()) - bytes
    } else {
        lines.len() - out.len()
    };
    (out, removed, hit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn dir() -> TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn within_limits_no_op() {
        let d = dir();
        let text = "line1\nline2\nline3";
        let r = truncate(text, 100, 10_000, Direction::Head, d.path(), "out.txt").unwrap();
        assert!(!r.truncated);
        assert_eq!(r.content, text);
        assert!(r.output_path.is_none());
    }

    #[test]
    fn line_overflow_head() {
        let d = dir();
        let text = (0..10)
            .map(|i| format!("line{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let r = truncate(&text, 3, 100_000, Direction::Head, d.path(), "out.txt").unwrap();
        assert!(r.truncated);
        assert!(r.output_path.is_some());
        // content starts with the head lines
        assert!(r.content.starts_with("line0\nline1\nline2"));
        assert!(r.content.contains("truncated"));
    }

    #[test]
    fn byte_overflow_head() {
        let d = dir();
        // 100 lines of 10 bytes each = 1000 bytes; limit to 50 bytes
        let text = (0..100)
            .map(|_| "1234567890")
            .collect::<Vec<_>>()
            .join("\n");
        let r = truncate(&text, 10_000, 50, Direction::Head, d.path(), "out.txt").unwrap();
        assert!(r.truncated);
        assert!(r.content.contains("bytes truncated"));
    }

    #[test]
    fn tail_direction() {
        let d = dir();
        let text = (0..10)
            .map(|i| format!("line{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let r = truncate(&text, 3, 100_000, Direction::Tail, d.path(), "out.txt").unwrap();
        assert!(r.truncated);
        // tail: content ends with the last lines
        assert!(r.content.contains("line9"));
        assert!(r.content.starts_with("..."));
    }

    #[test]
    fn file_written_to_out_dir() {
        let d = dir();
        let text = (0..5_000)
            .map(|i| format!("l{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let r = truncate(
            &text,
            100,
            100_000,
            Direction::Head,
            d.path(),
            "test-out.txt",
        )
        .unwrap();
        assert!(r.truncated);
        let p = r.output_path.unwrap();
        assert!(p.exists());
        let saved = std::fs::read_to_string(&p).unwrap();
        assert_eq!(saved, text);
    }
}
