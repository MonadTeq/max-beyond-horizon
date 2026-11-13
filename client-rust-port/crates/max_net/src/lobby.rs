//! WS-клиент к Horizon: запрос/ответ JSON-событий.
//!
//! Минимальная реализация RFC 6455 (маскированные текстовые кадры) — по образцу
//! C++-клиента (`src/ws_client.cpp`), но асинхронно на tokio.

use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use serde::de::DeserializeOwned;
use serde::Serialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::protocol::ClientEvent;

/// Одно запрос/ответ-взаимодействие с лобби Horizon.
pub async fn query<T: Serialize, R: DeserializeOwned>(
    host: &str,
    port: u16,
    request: &ClientEvent<T>,
    timeout: Duration,
) -> Result<R> {
    let mut stream = TcpStream::connect((host, port)).await?;
    tokio::time::timeout(timeout, async {
        let key = "dGhlIHNhbXBsZSBub25jZQ=="; // фиксированный ключ — как в C++-клиенте
        let request_line = format!(
            "GET / HTTP/1.1\r\nHost: {host}:{port}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
        );
        stream.write_all(request_line.as_bytes()).await?;

        let mut buf = Vec::new();
        let mut byte = [0u8; 1];
        while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
            stream.read_exact(&mut byte).await?;
            buf.push(byte[0]);
        }
        let header = String::from_utf8_lossy(&buf);
        if !header.starts_with("HTTP/1.1 101") {
            bail!("websocket handshake rejected: {}", header.lines().next().unwrap_or(""));
        }

        let payload = serde_json::to_string(request)?;
        send_masked_text(&mut stream, payload.as_bytes()).await?;

        let frame = read_text_frame(&mut stream).await?;
        let reply: R = serde_json::from_str(&frame)?;
        Ok(reply)
    })
    .await
    .map_err(|_| anyhow!("lobby query timed out"))?
}

async fn send_masked_text(stream: &mut TcpStream, payload: &[u8]) -> Result<()> {
    let mask = [0x12u8, 0x34, 0x56, 0x78];
    let mut frame = Vec::with_capacity(payload.len() + 14);
    frame.push(0x81); // FIN + text
    let len = payload.len();
    if len < 126 {
        frame.push(0x80 | len as u8);
    } else if len < 65536 {
        frame.push(0x80 | 126);
        frame.extend_from_slice(&(len as u16).to_be_bytes());
    } else {
        frame.push(0x80 | 127);
        frame.extend_from_slice(&(len as u64).to_be_bytes());
    }
    frame.extend_from_slice(&mask);
    frame.extend(payload.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
    stream.write_all(&frame).await?;
    Ok(())
}

async fn read_text_frame(stream: &mut TcpStream) -> Result<String> {
    let mut header = [0u8; 2];
    stream.read_exact(&mut header).await?;
    let mut len = (header[1] & 0x7F) as usize;
    if len == 126 {
        let mut ext = [0u8; 2];
        stream.read_exact(&mut ext).await?;
        len = u16::from_be_bytes(ext) as usize;
    } else if len == 127 {
        let mut ext = [0u8; 8];
        stream.read_exact(&mut ext).await?;
        len = u64::from_be_bytes(ext) as usize;
    }
    let mut payload = vec![0u8; len];
    stream.read_exact(&mut payload).await?;
    Ok(String::from_utf8(payload)?)
}
