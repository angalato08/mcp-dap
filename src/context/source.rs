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
        output.push_str(&format!("{marker} {line_num:>4} │ {line}\n"));
    }

    Ok(output)
}
