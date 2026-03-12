/// Strip ANSI escape codes and common control characters from untrusted debuggee output.
pub fn sanitize_debuggee_output(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        // Strip ANSI escape sequences: ESC [ ... final_byte
        if ch == '\x1b' && chars.peek() == Some(&'[') {
            chars.next(); // consume '['
            // Consume parameter bytes (0x30-0x3F), intermediate (0x20-0x2F), final (0x40-0x7E)
            while let Some(&c) = chars.peek() {
                if ('@'..='~').contains(&c) {
                    chars.next(); // consume final byte
                    break;
                }
                chars.next();
            }
            continue;
        }
        // Strip other C0/C1 control characters except common whitespace
        if ch.is_control() && !matches!(ch, '\n' | '\r' | '\t') {
            continue;
        }
        result.push(ch);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_ansi_codes() {
        assert_eq!(sanitize_debuggee_output("\x1b[31mred\x1b[0m"), "red");
    }

    #[test]
    fn strips_control_chars() {
        assert_eq!(sanitize_debuggee_output("hello\x00world"), "helloworld");
    }

    #[test]
    fn preserves_normal_text() {
        assert_eq!(
            sanitize_debuggee_output("normal text\n"),
            "normal text\n"
        );
    }

    #[test]
    fn preserves_unicode() {
        assert_eq!(sanitize_debuggee_output("hello 🌍"), "hello 🌍");
    }
}
