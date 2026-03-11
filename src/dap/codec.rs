use bytes::{Buf, BufMut, BytesMut};
use tokio_util::codec::{Decoder, Encoder};

use crate::error::AppError;

const CONTENT_LENGTH: &str = "Content-Length: ";
const HEADER_SEPARATOR: &[u8] = b"\r\n\r\n";

/// DAP wire protocol codec: `Content-Length: N\r\n\r\n{json}` framing.
#[derive(Debug, Default)]
pub struct DapCodec;

impl Decoder for DapCodec {
    type Item = serde_json::Value;
    type Error = AppError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        // Find the header separator.
        let sep_pos = src
            .windows(HEADER_SEPARATOR.len())
            .position(|w| w == HEADER_SEPARATOR);

        let sep_pos = match sep_pos {
            Some(pos) => pos,
            None => return Ok(None),
        };

        // Parse Content-Length from the header section.
        let header = std::str::from_utf8(&src[..sep_pos])
            .map_err(|e| AppError::Codec(e.to_string()))?;

        let content_length: usize = header
            .lines()
            .find_map(|line| line.strip_prefix(CONTENT_LENGTH))
            .ok_or_else(|| AppError::Codec("missing Content-Length header".into()))?
            .trim()
            .parse()
            .map_err(|e: std::num::ParseIntError| AppError::Codec(e.to_string()))?;

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
