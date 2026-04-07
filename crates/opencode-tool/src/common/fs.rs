//! Filesystem helpers: binary detection and line-range reading.

use std::io::{self, BufRead, Read};
use std::path::Path;

/// Max line length before truncation.
pub const MAX_LINE_LEN: usize = 2000;
/// Suffix appended to truncated lines.
pub const LINE_SUFFIX: &str = "... (line truncated to 2000 chars)";
/// Max bytes read in a single ReadTool call.
pub const MAX_BYTES: usize = 50 * 1024;

/// Result of reading a range of lines from a file.
#[derive(Debug, Default)]
pub struct Lines {
    /// The raw (possibly truncated) lines collected.
    pub raw: Vec<String>,
    /// Total line count in the file.
    pub count: usize,
    /// `true` if a line was cut at `MAX_LINE_LEN`.
    pub cut: bool,
    /// `true` if there are more lines beyond what was returned.
    pub more: bool,
    /// The requested offset (1-indexed start line).
    pub offset: usize,
}

/// Known binary extension list (fast-path before byte sampling).
static BINARY_EXTS: &[&str] = &[
    ".zip", ".tar", ".gz", ".exe", ".dll", ".so", ".class", ".jar", ".war", ".7z", ".doc", ".docx",
    ".xls", ".xlsx", ".ppt", ".pptx", ".odt", ".ods", ".odp", ".bin", ".dat", ".obj", ".o", ".a",
    ".lib", ".wasm", ".pyc", ".pyo",
];

/// Returns `true` if the file at `path` appears to be binary.
///
/// Uses extension fast-path first, then samples up to 4096 bytes for null bytes
/// and non-printable character density (>30%).
///
/// # Errors
/// Returns `io::Error` on read failure.
pub fn is_binary(path: &Path) -> io::Result<bool> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| format!(".{}", e.to_lowercase()))
        .unwrap_or_default();

    if BINARY_EXTS.contains(&ext.as_str()) {
        return Ok(true);
    }

    let meta = std::fs::metadata(path)?;
    if meta.len() == 0 {
        return Ok(false);
    }

    let mut f = std::fs::File::open(path)?;
    let sample = (meta.len().min(4096)) as usize;
    let mut buf = vec![0u8; sample];
    let n = f.read(&mut buf)?;
    if n == 0 {
        return Ok(false);
    }
    buf.truncate(n);

    let mut non_print = 0usize;
    for &b in &buf {
        if b == 0 {
            return Ok(true);
        }
        if b < 9 || (b > 13 && b < 32) {
            non_print += 1;
        }
    }
    Ok(non_print as f64 / n as f64 > 0.30)
}

/// Read `limit` lines starting at 1-indexed `offset` from `path`.
///
/// - Lines longer than [`MAX_LINE_LEN`] are truncated with [`LINE_SUFFIX`].
/// - Reading stops early when accumulated bytes exceed [`MAX_BYTES`].
///
/// # Errors
/// Returns `io::Error` on file-open or read failures.
pub fn read_lines(path: &Path, offset: usize, limit: usize) -> io::Result<Lines> {
    let f = std::fs::File::open(path)?;
    let reader = io::BufReader::new(f);

    let start = offset.saturating_sub(1);
    let mut raw = Vec::new();
    let mut count = 0usize;
    let mut bytes = 0usize;
    let mut cut = false;
    let mut more = false;

    for line in reader.lines() {
        let text = line?;
        count += 1;
        if count <= start {
            continue;
        }
        if raw.len() >= limit {
            more = true;
            continue;
        }
        let line_str = if text.len() > MAX_LINE_LEN {
            cut = true;
            format!("{}{}", &text[..MAX_LINE_LEN], LINE_SUFFIX)
        } else {
            text
        };
        // +1 for the newline separator (mimics TS Buffer.byteLength logic)
        let size = line_str.len() + if raw.is_empty() { 0 } else { 1 };
        if bytes + size > MAX_BYTES {
            cut = true;
            more = true;
            break;
        }
        bytes += size;
        raw.push(line_str);
    }

    Ok(Lines {
        raw,
        count,
        cut,
        more,
        offset,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn tmp(content: &str) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(content.as_bytes()).unwrap();
        f
    }

    // ── is_binary ──────────────────────────────────────────────────────────

    #[test]
    fn text_file_not_binary() {
        let f = tmp("hello world\nline 2\n");
        assert!(!is_binary(f.path()).unwrap());
    }

    #[test]
    fn binary_by_extension() {
        let f = NamedTempFile::with_suffix(".zip").unwrap();
        assert!(is_binary(f.path()).unwrap());
    }

    #[test]
    fn binary_by_null_byte() {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(b"hello\x00world").unwrap();
        assert!(is_binary(f.path()).unwrap());
    }

    #[test]
    fn binary_by_non_printable_density() {
        let mut f = NamedTempFile::new().unwrap();
        // 40% non-printable bytes (control chars 0x01)
        let mut data: Vec<u8> = b"aaaaaaaaaa".to_vec();
        for _ in 0..7 {
            data.push(0x01); // non-printable
        }
        f.write_all(&data).unwrap();
        assert!(is_binary(f.path()).unwrap());
    }

    #[test]
    fn empty_file_not_binary() {
        let f = NamedTempFile::new().unwrap();
        assert!(!is_binary(f.path()).unwrap());
    }

    // ── read_lines ─────────────────────────────────────────────────────────

    #[test]
    fn read_text_file() {
        let f = tmp("line1\nline2\nline3\n");
        let result = read_lines(f.path(), 1, 10).unwrap();
        assert_eq!(result.raw, vec!["line1", "line2", "line3"]);
        assert_eq!(result.count, 3);
        assert!(!result.more);
        assert!(!result.cut);
        assert_eq!(result.offset, 1);
    }

    #[test]
    fn read_with_offset() {
        let f = tmp("a\nb\nc\nd\n");
        let result = read_lines(f.path(), 3, 10).unwrap();
        assert_eq!(result.raw, vec!["c", "d"]);
        assert_eq!(result.count, 4);
        assert_eq!(result.offset, 3);
    }

    #[test]
    fn read_respects_limit() {
        let f = tmp("a\nb\nc\nd\ne\n");
        let result = read_lines(f.path(), 1, 2).unwrap();
        assert_eq!(result.raw, vec!["a", "b"]);
        assert!(result.more);
    }

    #[test]
    fn long_line_truncated() {
        let long = "x".repeat(MAX_LINE_LEN + 10);
        let f = tmp(&format!("{}\n", long));
        let result = read_lines(f.path(), 1, 10).unwrap();
        assert!(result.cut);
        assert!(result.raw[0].len() <= MAX_LINE_LEN + LINE_SUFFIX.len());
        assert!(result.raw[0].ends_with(LINE_SUFFIX));
    }

    #[test]
    fn offset_beyond_file() {
        let f = tmp("a\nb\n");
        // offset 10 on a 2-line file: raw is empty, count=2
        let result = read_lines(f.path(), 10, 10).unwrap();
        assert_eq!(result.raw, Vec::<String>::new());
        assert_eq!(result.count, 2);
    }

    #[test]
    fn empty_file_reads_ok() {
        let f = NamedTempFile::new().unwrap();
        let result = read_lines(f.path(), 1, 10).unwrap();
        assert_eq!(result.count, 0);
        assert!(result.raw.is_empty());
    }
}
