//! Streaming a file's text to the GTK thread in pieces of about 4 MiB that the editor inserts
//! one per 16 ms (S1: an idle is starved by GtkTextView's validation; one `set_text` blocks for
//! a second at 100 MB). Nothing here touches GTK.

use stet_infrastructure::fs::text_chunks;

/// The size of the pieces the editor inserts at a time.
pub const CHUNK_BYTES: usize = 4 * 1024 * 1024;

/// The size of the first piece: about a screen's worth, so it shows at once and GtkTextView
/// validates it in milliseconds; until it has, idle work is starved (S1), and a window shown
/// with it could not even register for accessibility.
pub const FIRST_CHUNK_BYTES: usize = 64 * 1024;

/// Streams `text` to the receiver in pieces that end at line breaks, the first of about
/// [`FIRST_CHUNK_BYTES`] and the rest of about [`CHUNK_BYTES`], from a thread of its own, so
/// the GTK thread never holds the whole text. The thread stops when the receiver is dropped.
pub fn stream(text: String) -> async_channel::Receiver<String> {
    let (sender, receiver) = async_channel::unbounded();
    let spawned = std::thread::Builder::new()
        .name("stet-loader".into())
        .spawn(move || {
            let first = text_chunks(&text, FIRST_CHUNK_BYTES)
                .next()
                .unwrap_or_default();
            let rest = text_chunks(&text[first.len()..], CHUNK_BYTES);
            for chunk in std::iter::once(first)
                .filter(|first| !first.is_empty())
                .chain(rest)
            {
                if sender.send_blocking(chunk.to_owned()).is_err() {
                    break;
                }
            }
        });
    if let Err(error) = spawned {
        tracing::error!(%error, "could not start the loader thread");
    }
    receiver
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streams_every_byte_in_line_pieces() {
        let text = "line\n".repeat(CHUNK_BYTES / 4);
        let receiver = stream(text.clone());
        let mut joined = String::new();
        let mut pieces = 0;
        while let Ok(piece) = receiver.recv_blocking() {
            assert!(piece.ends_with('\n'));
            let limit = if pieces == 0 {
                FIRST_CHUNK_BYTES
            } else {
                CHUNK_BYTES
            };
            assert!(
                piece.len() <= limit,
                "piece {pieces}: {} bytes",
                piece.len()
            );
            joined.push_str(&piece);
            pieces += 1;
        }
        assert_eq!(joined, text);
        assert_eq!(pieces, 3);
    }
}
