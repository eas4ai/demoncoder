//! Bounded byte framing for language-server standard input and output.

use anyhow::{Context, Result, ensure};
use serde_json::Value;
use tokio::io::{AsyncBufRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub(super) const MAX_MESSAGE_BYTES: usize = 1024 * 1024;
const MAX_HEADER_BYTES: usize = 8192;

pub(super) async fn read_message<R: AsyncBufRead + Unpin>(reader: &mut R) -> Result<Value> {
    let mut header = Vec::new();
    loop {
        ensure!(
            header.len() < MAX_HEADER_BYTES,
            "LSP headers exceed 8192 bytes"
        );
        header.push(reader.read_u8().await.context("incomplete LSP headers")?);
        if header.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    ensure!(header.is_ascii(), "LSP headers must be ASCII");
    let header = std::str::from_utf8(&header).context("invalid LSP headers")?;
    let mut length = None;
    for line in header[..header.len() - 4].split("\r\n") {
        let (name, value) = line.split_once(':').context("invalid LSP header field")?;
        ensure!(
            !name.is_empty()
                && name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte)),
            "invalid LSP header name"
        );
        ensure!(
            value
                .bytes()
                .all(|byte| byte == b'\t' || (32..127).contains(&byte)),
            "invalid LSP header value"
        );
        let value = value.trim_matches([' ', '\t']);
        if name.eq_ignore_ascii_case("Content-Length") {
            ensure!(length.is_none(), "duplicate LSP Content-Length");
            ensure!(
                !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()),
                "invalid LSP Content-Length"
            );
            let parsed: usize = value.parse().context("invalid LSP Content-Length")?;
            ensure!(
                (1..=MAX_MESSAGE_BYTES).contains(&parsed),
                "LSP body length must be between 1 and 1048576 bytes"
            );
            length = Some(parsed);
        } else if name.eq_ignore_ascii_case("Content-Type") {
            for parameter in value.split(';').skip(1) {
                let (key, value) = parameter
                    .trim()
                    .split_once('=')
                    .context("invalid LSP Content-Type parameter")?;
                if key.trim().eq_ignore_ascii_case("charset") {
                    let charset = value.trim();
                    let charset = charset
                        .strip_prefix('"')
                        .and_then(|value| value.strip_suffix('"'))
                        .unwrap_or(charset);
                    ensure!(
                        charset.eq_ignore_ascii_case("utf-8")
                            || charset.eq_ignore_ascii_case("utf8"),
                        "unsupported LSP charset"
                    );
                }
            }
        }
    }
    let length = length.context("missing LSP Content-Length")?;
    let mut body = vec![0; length];
    reader
        .read_exact(&mut body)
        .await
        .context("incomplete LSP body")?;
    let message: Value = serde_json::from_slice(&body).context("invalid LSP JSON body")?;
    ensure!(message.is_object(), "LSP JSON body must be an object");
    Ok(message)
}

pub(super) async fn write_message<W: AsyncWrite + Unpin>(
    writer: &mut W,
    value: &Value,
) -> Result<()> {
    ensure!(value.is_object(), "LSP JSON body must be an object");
    let mut body = BoundedBody(Vec::new());
    serde_json::to_writer(&mut body, value).context("cannot serialize bounded LSP body")?;
    let header = format!("Content-Length: {}\r\n\r\n", body.0.len());
    writer
        .write_all(header.as_bytes())
        .await
        .context("cannot write LSP headers")?;
    writer
        .write_all(&body.0)
        .await
        .context("cannot write LSP body")?;
    writer.flush().await.context("cannot flush LSP message")
}

struct BoundedBody(Vec<u8>);

impl std::io::Write for BoundedBody {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > MAX_MESSAGE_BYTES - self.0.len() {
            return Err(std::io::Error::other("LSP body exceeds 1048576 bytes"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tokio::io::{BufReader, duplex};

    fn frame(body: &[u8]) -> Vec<u8> {
        let mut bytes = format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
        bytes.extend_from_slice(body);
        bytes
    }

    #[tokio::test]
    async fn every_split_of_unicode_frame() {
        let expected = json!({"jsonrpc":"2.0", "result":"héllo 🌍"});
        let bytes = frame(&serde_json::to_vec(&expected).unwrap());
        for split in 0..=bytes.len() {
            let (mut sender, receiver) = duplex(1);
            let bytes = bytes.clone();
            let send = tokio::spawn(async move {
                sender.write_all(&bytes[..split]).await.unwrap();
                tokio::task::yield_now().await;
                sender.write_all(&bytes[split..]).await.unwrap();
            });
            assert_eq!(
                read_message(&mut BufReader::new(receiver)).await.unwrap(),
                expected
            );
            send.await.unwrap();
        }
    }

    #[tokio::test]
    async fn consecutive_frames_and_write_roundtrip() {
        let (mut sender, receiver) = duplex(17);
        let expected = [json!({"id":1,"result":"🌍"}), json!({"id":2,"result":null})];
        let outgoing = expected.clone();
        let send = tokio::spawn(async move {
            for message in outgoing {
                write_message(&mut sender, &message).await.unwrap();
            }
        });
        let mut reader = BufReader::new(receiver);
        for message in expected {
            assert_eq!(read_message(&mut reader).await.unwrap(), message);
        }
        send.await.unwrap();
    }

    #[tokio::test]
    async fn rejects_bad_lengths_and_incomplete_frames() {
        for bytes in [
            b"\r\n\r\n".as_slice(),
            b"Other: value\r\n\r\n{}",
            b"Content-Length: 2\r\nContent-Length: 2\r\n\r\n{}",
            b"Content-Length: 2\r\ncontent-length: 2\r\n\r\n{}",
            b"Content-Length: -2\r\n\r\n{}",
            b"Content-Length: +2\r\n\r\n{}",
            b"Content-Length: 2.0\r\n\r\n{}",
            b"Content-Length: \r\n\r\n{}",
            b"Content-Length: 0\r\n\r\n",
            b"Content-Length: 1048577\r\n\r\n",
            b"Content-Length: 999999999999999999999999999999\r\n\r\n",
            b"Content-Length: 2\r\n\r\n{",
            b"Content-Length: 2\r\n",
            b"Content-Length: 2\n\n{}",
        ] {
            assert!(
                read_message(&mut &bytes[..]).await.is_err(),
                "accepted {bytes:?}"
            );
        }
    }

    #[tokio::test]
    async fn validates_json_charset_and_header_fields() {
        for body in [
            b"[]".as_slice(),
            b"null",
            b"{",
            b"{}{}",
            b"{\"x\":\"\xff\"}",
        ] {
            assert!(read_message(&mut &frame(body)[..]).await.is_err());
        }
        for header in [
            "Content-Type: application/vscode-jsonrpc; charset=latin1",
            "Bad Name: value",
            "Bad: value\nInjected: true",
        ] {
            let bytes = format!("{header}\r\nContent-Length: 2\r\n\r\n{{}}");
            assert!(read_message(&mut bytes.as_bytes()).await.is_err());
        }
        let bytes = b"X-Unknown: fine\r\nContent-Type: application/vscode-jsonrpc; charset=utf8\r\ncontent-length: 2\r\n\r\n{}";
        assert_eq!(read_message(&mut &bytes[..]).await.unwrap(), json!({}));
        let nested = format!("{{\"x\":{}0{}}}", "[".repeat(128), "]".repeat(128));
        assert!(
            read_message(&mut &frame(nested.as_bytes())[..])
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn enforces_header_limit_including_delimiter() {
        let prefix = "Content-Length: 2\r\nX-Padding: ";
        let bytes = format!(
            "{prefix}{}\r\n\r\n{{}}",
            "a".repeat(MAX_HEADER_BYTES - prefix.len() - 4)
        );
        assert_eq!(
            read_message(&mut bytes.as_bytes()).await.unwrap(),
            json!({})
        );
        let bytes = format!(
            "{prefix}{}\r\n\r\n{{}}",
            "a".repeat(MAX_HEADER_BYTES - prefix.len() - 3)
        );
        assert!(read_message(&mut bytes.as_bytes()).await.is_err());
        assert!(
            read_message(&mut &vec![b'a'; MAX_HEADER_BYTES + 1][..])
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn rejects_invalid_outbound_before_writing() {
        let mut output = Vec::new();
        assert!(write_message(&mut output, &json!([])).await.is_err());
        assert!(
            write_message(&mut output, &json!({"text":"a".repeat(MAX_MESSAGE_BYTES)}))
                .await
                .is_err()
        );
        assert!(output.is_empty());
    }

    #[tokio::test]
    async fn accepts_exact_body_limit_and_quoted_utf8_charset() {
        let message = json!({"x": "a".repeat(MAX_MESSAGE_BYTES - 8)});
        let mut output = Vec::new();
        write_message(&mut output, &message).await.unwrap();
        assert!(output.starts_with(b"Content-Length: 1048576\r\n\r\n"));
        assert_eq!(read_message(&mut &output[..]).await.unwrap(), message);
        let bytes = b"Content-Type: application/vscode-jsonrpc; charset=\"UTF-8\"\r\nContent-Length: 2\r\n\r\n{}";
        assert_eq!(read_message(&mut &bytes[..]).await.unwrap(), json!({}));
    }

    #[tokio::test]
    async fn oversized_length_is_rejected_without_waiting_for_body_or_eof() {
        let (mut sender, receiver) = duplex(128);
        sender
            .write_all(b"Content-Length: 1048577\r\n\r\n")
            .await
            .unwrap();
        let mut reader = BufReader::new(receiver);
        let error =
            tokio::time::timeout(std::time::Duration::from_secs(1), read_message(&mut reader))
                .await
                .expect("oversized frame waited for body bytes")
                .unwrap_err();
        assert!(error.to_string().contains("body length"));
        drop(sender);
    }
}
