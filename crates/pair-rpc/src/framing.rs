//! Pluggable message framing over a byte stream.
//!
//! Three schemes are supported so the confirmed one can be selected without
//! touching call sites:
//! * [`Framing::JsonLines`] — one compact JSON object per line, `\n`-terminated.
//! * [`Framing::LengthPrefixedBE`] — 4-byte big-endian length, then the body.
//! * [`Framing::ContentLength`] — LSP-style `Content-Length: N\r\n\r\n` header.
//!
//! Confirmed: the reference services use [`Framing::JsonLines`] (one compact
//! JSON-RPC object per `\n`-terminated line; integer `id` per connection, matched
//! by numeric id). This is the default. The other schemes remain available for
//! adjacent transports.

use tokio::io::{AsyncRead, AsyncReadExt};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Framing {
    #[default]
    JsonLines,
    LengthPrefixedBE,
    ContentLength,
}

impl Framing {
    /// Frame a JSON body for transmission.
    pub fn encode(self, body: &[u8]) -> Vec<u8> {
        match self {
            Framing::JsonLines => {
                let mut v = Vec::with_capacity(body.len() + 1);
                v.extend_from_slice(body);
                v.push(b'\n');
                v
            }
            Framing::LengthPrefixedBE => {
                let mut v = Vec::with_capacity(body.len() + 4);
                v.extend_from_slice(&(body.len() as u32).to_be_bytes());
                v.extend_from_slice(body);
                v
            }
            Framing::ContentLength => {
                let mut v = Vec::new();
                v.extend_from_slice(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes());
                v.extend_from_slice(body);
                v
            }
        }
    }

    /// Read one framed body, or `None` at clean EOF (before any partial frame).
    pub async fn read_frame<R: AsyncRead + Unpin>(
        self,
        r: &mut R,
    ) -> anyhow::Result<Option<Vec<u8>>> {
        match self {
            Framing::JsonLines => read_line(r).await,
            Framing::LengthPrefixedBE => read_len_prefixed(r).await,
            Framing::ContentLength => read_content_length(r).await,
        }
    }
}

async fn read_line<R: AsyncRead + Unpin>(r: &mut R) -> anyhow::Result<Option<Vec<u8>>> {
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        let n = r.read(&mut byte).await?;
        if n == 0 {
            return Ok(if buf.is_empty() { None } else { Some(buf) });
        }
        if byte[0] == b'\n' {
            // tolerate trailing \r (CRLF)
            if buf.last() == Some(&b'\r') {
                buf.pop();
            }
            if buf.is_empty() {
                continue; // skip blank lines
            }
            return Ok(Some(buf));
        }
        buf.push(byte[0]);
    }
}

async fn read_len_prefixed<R: AsyncRead + Unpin>(r: &mut R) -> anyhow::Result<Option<Vec<u8>>> {
    let mut len_buf = [0u8; 4];
    match r.read_exact(&mut len_buf).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e.into()),
    }
    let len = u32::from_be_bytes(len_buf) as usize;
    let mut body = vec![0u8; len];
    r.read_exact(&mut body).await?;
    Ok(Some(body))
}

async fn read_content_length<R: AsyncRead + Unpin>(r: &mut R) -> anyhow::Result<Option<Vec<u8>>> {
    // Read headers until a blank line.
    let mut header = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        let n = r.read(&mut byte).await?;
        if n == 0 {
            return Ok(if header.is_empty() { None } else { Some(Vec::new()) });
        }
        header.push(byte[0]);
        if header.ends_with(b"\r\n\r\n") {
            break;
        }
        if header.len() > 8192 {
            anyhow::bail!("content-length header too large");
        }
    }
    let text = String::from_utf8_lossy(&header);
    let mut len = None;
    for line in text.split("\r\n") {
        if let Some(v) = line.strip_prefix("Content-Length:") {
            len = v.trim().parse::<usize>().ok();
        }
    }
    let len = len.ok_or_else(|| anyhow::anyhow!("missing Content-Length"))?;
    let mut body = vec![0u8; len];
    r.read_exact(&mut body).await?;
    Ok(Some(body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_shapes() {
        assert_eq!(Framing::JsonLines.encode(b"{}"), b"{}\n");
        assert_eq!(&Framing::LengthPrefixedBE.encode(b"{}")[..4], &[0, 0, 0, 2]);
        assert!(Framing::ContentLength
            .encode(b"{}")
            .starts_with(b"Content-Length: 2\r\n\r\n"));
    }
}
