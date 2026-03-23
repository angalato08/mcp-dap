use bytes::{Buf, BufMut, BytesMut};
use memchr::memmem;
use tokio_util::codec::{Decoder, Encoder};

use crate::error::AppError;

const CONTENT_LENGTH: &str = "Content-Length: ";
const HEADER_SEPARATOR: &[u8] = b"\r\n\r\n";
/// 1 MB — generous for any realistic DAP JSON payload.
const MAX_DAP_MESSAGE_SIZE: usize = 1_048_576;
/// Maximum header size before the \r\n\r\n separator. DAP headers are tiny
/// (just Content-Length), so 8 KB is extremely generous.
const MAX_HEADER_SIZE: usize = 8 * 1024;

/// DAP wire protocol codec: `Content-Length: N\r\n\r\n{json}` framing.
#[derive(Debug, Default)]
pub struct DapCodec;

impl Decoder for DapCodec {
    type Item = serde_json::Value;
    type Error = AppError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        // Guard: reject if buffer exceeds max header size without a separator.
        let search_limit = src.len().min(MAX_HEADER_SIZE + HEADER_SEPARATOR.len());
        let sep_pos = memmem::find(&src[..search_limit], HEADER_SEPARATOR);

        let Some(sep_pos) = sep_pos else {
            if src.len() > MAX_HEADER_SIZE {
                return Err(AppError::Codec(format!(
                    "no header separator found within {MAX_HEADER_SIZE} bytes"
                )));
            }
            return Ok(None);
        };

        // Parse Content-Length from the header section.
        let header =
            std::str::from_utf8(&src[..sep_pos]).map_err(|e| AppError::Codec(e.to_string()))?;

        let content_length: usize = header
            .lines()
            .find_map(|line| line.strip_prefix(CONTENT_LENGTH))
            .ok_or_else(|| AppError::Codec("missing Content-Length header".into()))?
            .trim()
            .parse()
            .map_err(|e: std::num::ParseIntError| AppError::Codec(e.to_string()))?;

        if content_length > MAX_DAP_MESSAGE_SIZE {
            return Err(AppError::Codec(format!(
                "Content-Length {content_length} exceeds maximum {MAX_DAP_MESSAGE_SIZE}"
            )));
        }

        let total = sep_pos + HEADER_SEPARATOR.len() + content_length;
        if src.len() < total {
            return Ok(None);
        }

        // Advance past header.
        src.advance(sep_pos + HEADER_SEPARATOR.len());

        // Read the JSON body.
        let body = src.split_to(content_length);
        let value: serde_json::Value = serde_json::from_slice(&body)?;
        Ok(Some(value))
    }
}

impl Encoder<serde_json::Value> for DapCodec {
    type Error = AppError;

    fn encode(&mut self, item: serde_json::Value, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let body = serde_json::to_vec(&item)?;
        let header = format!("{CONTENT_LENGTH}{}\r\n\r\n", body.len());
        dst.reserve(header.len() + body.len());
        dst.put_slice(header.as_bytes());
        dst.put_slice(&body);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio_util::codec::{Decoder, Encoder};

    fn make_message(json: &str) -> BytesMut {
        let header = format!("Content-Length: {}\r\n\r\n", json.len());
        let mut buf = BytesMut::new();
        buf.extend_from_slice(header.as_bytes());
        buf.extend_from_slice(json.as_bytes());
        buf
    }

    #[test]
    fn decode_valid_message() {
        let mut codec = DapCodec;
        let mut buf = make_message(r#"{"type":"event"}"#);
        let result = codec.decode(&mut buf).unwrap().unwrap();
        assert_eq!(result["type"], "event");
    }

    #[test]
    fn encode_decode_roundtrip() {
        let mut codec = DapCodec;
        let original = serde_json::json!({"seq": 1, "type": "request", "command": "initialize"});
        let mut buf = BytesMut::new();
        codec.encode(original.clone(), &mut buf).unwrap();
        let decoded = codec.decode(&mut buf).unwrap().unwrap();
        assert_eq!(original, decoded);
    }

    #[test]
    fn partial_header_returns_none() {
        let mut codec = DapCodec;
        let mut buf = BytesMut::from("Content-Length: 16\r\n");
        assert!(codec.decode(&mut buf).unwrap().is_none());
    }

    #[test]
    fn partial_body_returns_none() {
        let mut codec = DapCodec;
        let mut buf = BytesMut::from("Content-Length: 100\r\n\r\n{\"short\":true}");
        assert!(codec.decode(&mut buf).unwrap().is_none());
    }

    #[test]
    fn two_messages_in_one_buffer() {
        let mut codec = DapCodec;
        let json1 = r#"{"seq":1}"#;
        let json2 = r#"{"seq":2}"#;
        let mut buf = make_message(json1);
        buf.extend_from_slice(&make_message(json2));

        let first = codec.decode(&mut buf).unwrap().unwrap();
        assert_eq!(first["seq"], 1);
        let second = codec.decode(&mut buf).unwrap().unwrap();
        assert_eq!(second["seq"], 2);
    }

    #[test]
    fn missing_content_length_is_err() {
        let mut codec = DapCodec;
        let mut buf = BytesMut::from("X-Custom: foo\r\n\r\n{}");
        let err = codec.decode(&mut buf).unwrap_err();
        assert!(err.to_string().contains("Content-Length"));
    }

    #[test]
    fn non_numeric_content_length_is_err() {
        let mut codec = DapCodec;
        let mut buf = BytesMut::from("Content-Length: abc\r\n\r\n{}");
        let err = codec.decode(&mut buf).unwrap_err();
        assert!(err.to_string().contains("invalid digit"));
    }

    #[test]
    fn oversized_content_length_is_err() {
        let mut codec = DapCodec;
        let mut buf = BytesMut::from("Content-Length: 999999999999\r\n\r\n");
        let err = codec.decode(&mut buf).unwrap_err();
        assert!(err.to_string().contains("exceeds maximum"));
    }

    #[test]
    fn extra_headers_before_content_length() {
        let mut codec = DapCodec;
        let json = r#"{"ok":true}"#;
        let header = format!("X-Extra: value\r\nContent-Length: {}\r\n\r\n", json.len());
        let mut buf = BytesMut::new();
        buf.extend_from_slice(header.as_bytes());
        buf.extend_from_slice(json.as_bytes());
        let result = codec.decode(&mut buf).unwrap().unwrap();
        assert_eq!(result["ok"], true);
    }

    #[test]
    fn utf8_json_body_with_multibyte_chars() {
        let mut codec = DapCodec;
        let json = r#"{"msg":"hello 🌍"}"#;
        let mut buf = make_message(json);
        let result = codec.decode(&mut buf).unwrap().unwrap();
        assert_eq!(result["msg"], "hello 🌍");
    }

    #[test]
    fn oversized_header_without_separator_is_err() {
        let mut codec = DapCodec;
        let junk = vec![b'A'; 9000]; // > MAX_HEADER_SIZE
        let mut buf = BytesMut::from(&junk[..]);
        let err = codec.decode(&mut buf).unwrap_err();
        assert!(err.to_string().contains("separator"));
    }
}
