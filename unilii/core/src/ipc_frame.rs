//! Bounded line-delimited IPC frame readers.
//!
//! DeskHalloumi's Unix-socket protocols use one UTF-8 JSON object terminated
//! by a newline. These helpers enforce the frame limit while reading, rather
//! than allocating an arbitrarily large `String` and checking it afterwards.

use std::io::{BufRead, Read};

use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt};

pub fn read_utf8_line_bounded<R: BufRead>(
    reader: &mut R,
    max_frame_bytes: usize,
) -> Result<String, String> {
    let mut frame = Vec::with_capacity(max_frame_bytes.min(4096));
    let mut limited = reader.take((max_frame_bytes.saturating_add(1)) as u64);
    limited
        .read_until(b'\n', &mut frame)
        .map_err(|error| format!("failed to read IPC frame: {error}"))?;
    decode_frame(frame, max_frame_bytes)
}

pub async fn read_utf8_line_bounded_async<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    max_frame_bytes: usize,
) -> Result<String, String> {
    let mut frame = Vec::with_capacity(max_frame_bytes.min(4096));
    let mut limited = (&mut *reader).take((max_frame_bytes.saturating_add(1)) as u64);
    limited
        .read_until(b'\n', &mut frame)
        .await
        .map_err(|error| format!("failed to read IPC frame: {error}"))?;
    decode_frame(frame, max_frame_bytes)
}

fn decode_frame(mut frame: Vec<u8>, max_frame_bytes: usize) -> Result<String, String> {
    if frame.is_empty() {
        return Err("empty IPC frame".to_string());
    }
    if frame.len() > max_frame_bytes {
        return Err(format!("IPC frame exceeds {max_frame_bytes} bytes"));
    }
    if frame.last() != Some(&b'\n') {
        return Err("IPC frame is missing its newline terminator".to_string());
    }
    frame.pop();
    if frame.last() == Some(&b'\r') {
        frame.pop();
    }
    String::from_utf8(frame).map_err(|error| format!("IPC frame is not valid UTF-8: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn synchronous_reader_rejects_oversize_without_consuming_unbounded_input() {
        let input = vec![b'x'; 128 * 1024];
        let mut reader = Cursor::new(input);
        let error = read_utf8_line_bounded(&mut reader, 64 * 1024).unwrap_err();
        assert!(error.contains("exceeds"));
        assert!(reader.position() <= (64 * 1024 + 1) as u64);
    }

    #[tokio::test]
    async fn asynchronous_reader_accepts_crlf_and_rejects_missing_terminator() {
        let mut accepted = tokio::io::BufReader::new(&b"{\"ok\":true}\r\n"[..]);
        assert_eq!(
            read_utf8_line_bounded_async(&mut accepted, 64)
                .await
                .unwrap(),
            "{\"ok\":true}"
        );

        let mut unterminated = tokio::io::BufReader::new(&b"partial"[..]);
        assert!(
            read_utf8_line_bounded_async(&mut unterminated, 64)
                .await
                .unwrap_err()
                .contains("newline")
        );
    }
}
