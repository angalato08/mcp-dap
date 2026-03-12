use std::fmt::Write;
use std::path::Path;

use crate::error::AppError;

/// Read source file and extract a window of lines around `target_line` (1-indexed).
/// Returns formatted source with line numbers and an arrow on the target line.
pub fn extract_source_context(
    file_path: &Path,
    target_line: usize,
    context_lines: usize,
) -> Result<String, AppError> {
    let content = std::fs::read_to_string(file_path)?;
    let lines: Vec<&str> = content.lines().collect();

    if target_line == 0 || target_line > lines.len() {
        return Ok(format!("[line {} out of range, file has {} lines]", target_line, lines.len()));
    }

    let start = target_line.saturating_sub(context_lines + 1);
    let end = (target_line + context_lines).min(lines.len());

    let mut output = String::new();
    for (i, line) in lines[start..end].iter().enumerate() {
        let line_num = start + i + 1;
        let marker = if line_num == target_line { "→" } else { " " };
        let _ = writeln!(output, "{marker} {line_num:>4} │ {line}");
    }

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    fn temp_file(content: &str) -> std::path::PathBuf {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let mut path = std::env::temp_dir();
        path.push(format!("mcp_dap_test_{}_{id}.txt", std::process::id()));
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
        path
    }

    #[test]
    fn middle_of_file() {
        let path = temp_file("line1\nline2\nline3\nline4\nline5\n");
        let result = extract_source_context(&path, 3, 1).unwrap();
        assert!(result.contains("line2"));
        assert!(result.contains("line3"));
        assert!(result.contains("line4"));
        assert!(result.contains("→"));
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn first_line() {
        let path = temp_file("first\nsecond\nthird\n");
        let result = extract_source_context(&path, 1, 1).unwrap();
        assert!(result.contains("first"));
        assert!(result.contains("→"));
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn last_line() {
        let path = temp_file("aaa\nbbb\nccc\n");
        let result = extract_source_context(&path, 3, 1).unwrap();
        assert!(result.contains("ccc"));
        assert!(result.contains("→"));
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn nonexistent_file() {
        let path = Path::new("/tmp/mcp_dap_nonexistent_file_xyz.txt");
        let result = extract_source_context(path, 1, 1);
        assert!(result.is_err());
    }

    #[test]
    fn line_zero_out_of_range() {
        let path = temp_file("hello\n");
        let result = extract_source_context(&path, 0, 1).unwrap();
        assert!(result.contains("out of range"));
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn line_beyond_end_out_of_range() {
        let path = temp_file("hello\n");
        let result = extract_source_context(&path, 999, 1).unwrap();
        assert!(result.contains("out of range"));
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn single_line_file() {
        let path = temp_file("only line");
        let result = extract_source_context(&path, 1, 2).unwrap();
        assert!(result.contains("only line"));
        assert!(result.contains("→"));
        std::fs::remove_file(&path).ok();
    }
}
