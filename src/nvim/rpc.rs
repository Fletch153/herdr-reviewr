//! msgpack-RPC framing for the embedded nvim channel (msgpack-rpc spec: type 0/1/2 arrays).
//! Pure encode/decode over `Read`/`Write` — no process, no threads; unit-tested on byte buffers.

use rmpv::Value;

/// One inbound message, already shape-validated.
#[derive(Debug)]
pub(crate) enum RpcIn {
    /// `[1, msgid, error, result]` — error is Nil on success.
    Response { msgid: u64, error: Value, result: Value },
    /// `[2, method, params]`.
    Notification { method: String, params: Vec<Value> },
    /// `[0, msgid, method, params]` — nvim→host requests are unexpected for a pure UI host,
    /// but MUST be answered or nvim blocks; the reader replies with an error.
    Request { msgid: u64 },
}

/// Why the channel stopped: distinguishes a clean EOF (nvim exited) from protocol corruption.
#[derive(Debug)]
pub(crate) enum ReadError {
    /// stdout closed — nvim exited (or was killed).
    Eof,
    /// Undecodable or non-msgpack-rpc-shaped data; the channel is unrecoverable.
    Corrupt(String),
}

/// The engine's typed failure, shared by every public API method. `Timeout` and `Dead` drive
/// different UX (busy-with-a-prompt vs restart), so callers match on the variant.
#[derive(Debug)]
pub enum RpcFailure {
    /// nvim exited or the channel broke; `Nvim::died()` has the reason.
    Dead,
    /// No response within the deadline — nvim is alive but blocked (e.g. a modal prompt).
    Timeout,
    /// nvim returned an error value.
    Nvim(String),
    /// A pipe write failed mid-send; the reader will mark the channel dead within the tick.
    Io(String),
}

impl std::fmt::Display for RpcFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Dead => write!(f, "nvim is not running"),
            Self::Timeout => write!(f, "nvim did not respond (busy?)"),
            Self::Nvim(msg) => write!(f, "nvim: {msg}"),
            Self::Io(msg) => write!(f, "nvim channel: {msg}"),
        }
    }
}

impl std::error::Error for RpcFailure {}

/// Blocking-read one message. Called only on the reader thread with a buffered reader.
/// Any decode error or shape violation maps to `Corrupt`, except a clean EOF.
pub(crate) fn read_msg(r: &mut impl std::io::Read) -> Result<RpcIn, ReadError> {
    let v = rmpv::decode::read_value(r)
        .map_err(|e| if is_eof(&e) { ReadError::Eof } else { ReadError::Corrupt(e.to_string()) })?;
    let Value::Array(items) = v else {
        return Err(ReadError::Corrupt("top-level message is not an array".into()));
    };
    let kind = items.first().and_then(Value::as_u64);
    match kind {
        Some(0) => {
            let msgid = items
                .get(1)
                .and_then(Value::as_u64)
                .ok_or_else(|| ReadError::Corrupt("request without msgid".into()))?;
            Ok(RpcIn::Request { msgid })
        }
        Some(1) => {
            let msgid = items
                .get(1)
                .and_then(Value::as_u64)
                .ok_or_else(|| ReadError::Corrupt("response without msgid".into()))?;
            let mut it = items.into_iter().skip(2);
            let error = it.next().unwrap_or(Value::Nil);
            let result = it.next().unwrap_or(Value::Nil);
            Ok(RpcIn::Response { msgid, error, result })
        }
        Some(2) => {
            let method = items
                .get(1)
                .and_then(Value::as_str)
                .ok_or_else(|| ReadError::Corrupt("notification without method".into()))?
                .to_owned();
            let params = match items.into_iter().nth(2) {
                Some(Value::Array(p)) => p,
                _ => Vec::new(),
            };
            Ok(RpcIn::Notification { method, params })
        }
        _ => Err(ReadError::Corrupt("unknown message type".into())),
    }
}

fn is_eof(e: &rmpv::decode::Error) -> bool {
    let mut src: Option<&dyn std::error::Error> = Some(e);
    while let Some(err) = src {
        if let Some(io) = err.downcast_ref::<std::io::Error>() {
            return io.kind() == std::io::ErrorKind::UnexpectedEof;
        }
        src = err.source();
    }
    false
}

/// Encode `[0, msgid, method, params]` into a fresh buffer and write it in one call.
pub(crate) fn write_request(
    w: &mut impl std::io::Write,
    msgid: u64,
    method: &str,
    params: Vec<Value>,
) -> std::io::Result<()> {
    write_frame(
        w,
        &Value::Array(vec![
            Value::from(0),
            Value::from(msgid),
            Value::from(method),
            Value::Array(params),
        ]),
    )
}

/// Encode `[2, method, params]` — fire-and-forget: nvim sends no reply; errors surface only
/// in nvim's own message area (visible in the grid).
pub(crate) fn write_notification(
    w: &mut impl std::io::Write,
    method: &str,
    params: Vec<Value>,
) -> std::io::Result<()> {
    write_frame(w, &Value::Array(vec![Value::from(2), Value::from(method), Value::Array(params)]))
}

/// Encode `[1, msgid, error, nil]` — the reader's stock reply to unexpected nvim→host requests.
pub(crate) fn write_error_response(
    w: &mut impl std::io::Write,
    msgid: u64,
    message: &str,
) -> std::io::Result<()> {
    write_frame(
        w,
        &Value::Array(vec![Value::from(1), Value::from(msgid), Value::from(message), Value::Nil]),
    )
}

fn write_frame(w: &mut impl std::io::Write, v: &Value) -> std::io::Result<()> {
    let mut buf = Vec::with_capacity(64);
    rmpv::encode::write_value(&mut buf, v).map_err(|e| std::io::Error::other(e.to_string()))?;
    w.write_all(&buf)?;
    w.flush()
}

/// Extract the human message from nvim's response error value (`[error_type, message]`),
/// falling back to the value's display form.
pub(crate) fn error_message(error: &Value) -> String {
    error
        .as_array()
        .and_then(|a| a.get(1))
        .and_then(Value::as_str)
        .map_or_else(|| error.to_string(), str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn request_and_notification_round_trip() {
        let mut buf = Vec::new();
        write_request(&mut buf, 7, "nvim_eval", vec![Value::from("1+1")]).unwrap();
        write_notification(&mut buf, "nvim_input", vec![Value::from("j")]).unwrap();
        let mut cur = Cursor::new(buf);
        // Our own request decodes as an inbound Request (same wire shape both directions).
        match read_msg(&mut cur).unwrap() {
            RpcIn::Request { msgid } => assert_eq!(msgid, 7),
            other => panic!("expected request, got {other:?}"),
        }
        match read_msg(&mut cur).unwrap() {
            RpcIn::Notification { method, params } => {
                assert_eq!(method, "nvim_input");
                assert_eq!(params, vec![Value::from("j")]);
            }
            other => panic!("expected notification, got {other:?}"),
        }
    }

    #[test]
    fn response_parses_error_and_result_arms() {
        let mut buf = Vec::new();
        rmpv::encode::write_value(
            &mut buf,
            &Value::Array(vec![Value::from(1), Value::from(3), Value::Nil, Value::from(2)]),
        )
        .unwrap();
        rmpv::encode::write_value(
            &mut buf,
            &Value::Array(vec![
                Value::from(1),
                Value::from(4),
                Value::Array(vec![Value::from(0), Value::from("E492: no such command")]),
                Value::Nil,
            ]),
        )
        .unwrap();
        let mut cur = Cursor::new(buf);
        match read_msg(&mut cur).unwrap() {
            RpcIn::Response { msgid, error, result } => {
                assert_eq!((msgid, error, result), (3, Value::Nil, Value::from(2)));
            }
            other => panic!("{other:?}"),
        }
        match read_msg(&mut cur).unwrap() {
            RpcIn::Response { msgid, error, .. } => {
                assert_eq!(msgid, 4);
                assert_eq!(error_message(&error), "E492: no such command");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn garbage_is_corrupt_and_empty_is_eof() {
        // 0xc1 is the one permanently-unused msgpack marker byte.
        let mut cur = Cursor::new(vec![0xc1, 0x00]);
        assert!(matches!(read_msg(&mut cur), Err(ReadError::Corrupt(_))));
        let mut empty = Cursor::new(Vec::<u8>::new());
        assert!(matches!(read_msg(&mut empty), Err(ReadError::Eof)));
        // Shape violation: a non-array top level.
        let mut buf = Vec::new();
        rmpv::encode::write_value(&mut buf, &Value::from(42)).unwrap();
        let mut cur = Cursor::new(buf);
        assert!(matches!(read_msg(&mut cur), Err(ReadError::Corrupt(_))));
    }
}
