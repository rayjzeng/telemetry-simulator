use crate::message::Message;
use tokio::io::{AsyncWrite, AsyncWriteExt};

pub async fn write_message<W: AsyncWrite + Unpin>(
    writer: &mut W,
    msg: &Message,
) -> Result<(), std::io::Error> {
    let payload = msg.to_msgpack().map_err(|e| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())
    })?;
    // Length-prefix framing: readers on the stream need to know where one
    // variable-length msgpack message ends and the next begins.
    let len = (payload.len() as u32).to_be_bytes();
    writer.write_all(&len).await?;
    writer.write_all(&payload).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_writer_frames_with_length_prefix() {
        let mut buf = Vec::new();
        let msg = Message::Event {
            event_id: "test".to_string(),
            event_name: "click".to_string(),
            timestamp_ns: 1000,
            version: 1,
            process_id: 0,
            sequence_number: 0,
        };
        write_message(&mut buf, &msg).await.unwrap();

        let frame_len = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
        assert_eq!(frame_len, buf.len() - 4);

        let decoded = Message::from_msgpack(&buf[4..]).unwrap();
        assert_eq!(msg, decoded);
    }
}
