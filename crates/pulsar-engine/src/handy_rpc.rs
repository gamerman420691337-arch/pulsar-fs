//! Unqualified, offline-only codecs for the manufacturer public RPC schema.
//! These are RpcMessage bytes, not BLE/GATT, authenticated session or stop proof.
//! No transport or physical playback authority is provided by this module.
//! Schema identity: manufacturer commit 7158570563bdcd6de356fff62e3f8c159189a6d2.

pub const RESPONSE_LIMIT: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopCommand {
    CurrentMode,
    Hdsp,
    Hsp,
}
impl StopCommand {
    fn field(self) -> u64 {
        match self {
            Self::CurrentMode => 715,
            Self::Hdsp => 746,
            Self::Hsp => 864,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RpcError {
    InvalidId,
    Oversized,
    Malformed,
    UnexpectedMessage,
    MismatchedId,
    RemoteError,
}

/// An explicit RPC request family and nonzero correlation ID; no automatic fallback.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StopRequest {
    id: u32,
    command: StopCommand,
    bytes: Vec<u8>,
}
impl StopRequest {
    pub fn new(command: StopCommand, id: u32) -> Result<Self, RpcError> {
        if id == 0 {
            return Err(RpcError::InvalidId);
        }
        let mut request = vec![0x10];
        varint(u64::from(id), &mut request);
        varint((command.field() << 3) | 2, &mut request);
        request.push(0);
        let mut bytes = vec![0x08, 0x01, 0x12];
        varint(request.len() as u64, &mut bytes);
        bytes.extend_from_slice(&request);
        Ok(Self { id, command, bytes })
    }
    pub fn id(&self) -> u32 {
        self.id
    }
    pub fn command(&self) -> StopCommand {
        self.command
    }
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// A correlated decoded RPC reply only. It carries no physical stop or qualification fact.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StopReply {
    request_id: u32,
    command: StopCommand,
}

impl StopReply {
    pub fn request_id(&self) -> u32 {
        self.request_id
    }
    pub fn command(&self) -> StopCommand {
        self.command
    }
}

fn varint(mut value: u64, out: &mut Vec<u8>) {
    while value >= 128 {
        out.push((value as u8 & 127) | 128);
        value >>= 7;
    }
    out.push(value as u8);
}
struct Reader<'a> {
    bytes: &'a [u8],
    cursor: usize,
    fields: usize,
}
impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            cursor: 0,
            fields: 0,
        }
    }
    fn take(&mut self, size: usize) -> Result<&'a [u8], RpcError> {
        let end = self.cursor.checked_add(size).ok_or(RpcError::Malformed)?;
        let data = self
            .bytes
            .get(self.cursor..end)
            .ok_or(RpcError::Malformed)?;
        self.cursor = end;
        Ok(data)
    }
    fn integer(&mut self) -> Result<u64, RpcError> {
        let mut value = 0u64;
        for i in 0..10 {
            let byte = self.take(1)?[0];
            if i == 9 && byte > 1 {
                return Err(RpcError::Malformed);
            }
            value |= u64::from(byte & 127) << (i * 7);
            if byte & 128 == 0 {
                if i > 0 && byte == 0 {
                    return Err(RpcError::Malformed);
                }
                return Ok(value);
            }
        }
        Err(RpcError::Malformed)
    }
    fn key(&mut self) -> Result<Option<(u64, u8)>, RpcError> {
        if self.cursor == self.bytes.len() {
            return Ok(None);
        }
        self.fields += 1;
        if self.fields > 64 {
            return Err(RpcError::Malformed);
        }
        let key = self.integer()?;
        let field = key >> 3;
        if field == 0 || field > 0x1fff_ffff {
            return Err(RpcError::Malformed);
        }
        Ok(Some((field, (key & 7) as u8)))
    }
    fn message(&mut self) -> Result<&'a [u8], RpcError> {
        let size = usize::try_from(self.integer()?).map_err(|_| RpcError::Malformed)?;
        if size > RESPONSE_LIMIT {
            return Err(RpcError::Oversized);
        }
        self.take(size)
    }
}

// Validate the pinned HspState scalar shapes without exporting its values as proof.
fn hsp_state(bytes: &[u8]) -> Result<(), RpcError> {
    let mut reader = Reader::new(bytes);
    let mut seen = 0u32;
    while let Some((field, wire)) = reader.key()? {
        if !(1..=16).contains(&field) || seen & (1 << field) != 0 {
            return Err(RpcError::UnexpectedMessage);
        }
        seen |= 1 << field;
        if field == 7 || field == 15 {
            if wire != 5 {
                return Err(RpcError::Malformed);
            }
            let value = f32::from_le_bytes(
                reader
                    .take(4)?
                    .try_into()
                    .map_err(|_| RpcError::Malformed)?,
            );
            if !value.is_finite() {
                return Err(RpcError::Malformed);
            }
        } else {
            if wire != 0 {
                return Err(RpcError::Malformed);
            }
            let value = reader.integer()?;
            match field {
                1 if value > 4 => return Err(RpcError::UnexpectedMessage),
                6 | 13 | 14 | 16 if value > 1 => return Err(RpcError::Malformed),
                4 | 5 | 11 if value > i32::MAX as u64 && value < 0xffff_ffff_8000_0000 => {
                    return Err(RpcError::Malformed)
                }
                2 | 3 | 8 | 9 | 10 | 12 if value > u32::MAX as u64 => {
                    return Err(RpcError::Malformed)
                }
                _ => {}
            }
        }
    }
    Ok(())
}
fn hsp_result(bytes: &[u8]) -> Result<(), RpcError> {
    let mut reader = Reader::new(bytes);
    let mut seen = false;
    while let Some((field, wire)) = reader.key()? {
        if field != 1 || wire != 2 || seen {
            return Err(RpcError::UnexpectedMessage);
        }
        seen = true;
        hsp_state(reader.message()?)?;
    }
    Ok(())
}

/// Strict bounded decoder for the selected reply family. Peer error content is discarded.
/// Errors require a complete, unambiguous envelope and the exact request ID.
/// Request IDs must also be bound to a fresh authenticated session by a future broker.
/// A response without an HSP stop result is refused for the HSP family.
pub fn decode_stop_reply(request: &StopRequest, bytes: &[u8]) -> Result<StopReply, RpcError> {
    if bytes.len() > RESPONSE_LIMIT {
        return Err(RpcError::Oversized);
    }
    let mut outer = Reader::new(bytes);
    let mut kind = None;
    let mut response = None;
    while let Some((field, wire)) = outer.key()? {
        match (field, wire) {
            (1, 0) if kind.is_none() => kind = Some(outer.integer()?),
            (4, 2) if response.is_none() => response = Some(outer.message()?),
            _ => return Err(RpcError::UnexpectedMessage),
        }
    }
    if kind != Some(3) {
        return Err(RpcError::UnexpectedMessage);
    }
    let mut response = Reader::new(response.ok_or(RpcError::UnexpectedMessage)?);
    let mut id = None;
    let mut result = false;
    let mut remote_error = false;
    while let Some((field, wire)) = response.key()? {
        match (field, wire) {
            (1, 0) if id.is_none() => id = Some(response.integer()?),
            (2, 2) if !remote_error => {
                response.message()?;
                remote_error = true;
            }
            (864, 2) if request.command == StopCommand::Hsp && !result => {
                hsp_result(response.message()?)?;
                result = true;
            }
            _ => return Err(RpcError::UnexpectedMessage),
        }
    }
    if id != Some(u64::from(request.id)) {
        return Err(RpcError::MismatchedId);
    }
    if remote_error {
        if result {
            return Err(RpcError::UnexpectedMessage);
        }
        return Err(RpcError::RemoteError);
    }
    if request.command == StopCommand::Hsp && !result {
        return Err(RpcError::UnexpectedMessage);
    }
    Ok(StopReply {
        request_id: request.id,
        command: request.command,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    include!("../tests/fixtures/handy-rpc-stop-vectors.rs");
    fn hex(s: &str) -> Vec<u8> {
        s.as_bytes()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u8::from_str_radix(std::str::from_utf8(c).unwrap(), 16).unwrap())
            .collect()
    }
    #[test]
    fn pinned_schema_oracle_vectors_match_requests_and_replies() {
        for &(id, command, request_bytes, reply_bytes) in RPC_STOP_VECTORS {
            let request = StopRequest::new(command, id).unwrap();
            assert_eq!(request.bytes(), hex(request_bytes), "{command:?} {id}");
            let reply = decode_stop_reply(&request, &hex(reply_bytes)).unwrap();
            assert_eq!(reply.request_id(), id);
            assert_eq!(reply.command(), command);
        }
    }
    #[test]
    fn explicit_stop_families_have_distinct_envelopes() {
        for (kind, expected) in [
            (
                StopCommand::CurrentMode,
                vec![8, 1, 18, 5, 16, 1, 218, 44, 0],
            ),
            (StopCommand::Hdsp, vec![8, 1, 18, 5, 16, 1, 210, 46, 0]),
            (StopCommand::Hsp, vec![8, 1, 18, 5, 16, 1, 130, 54, 0]),
        ] {
            let request = StopRequest::new(kind, 1).unwrap();
            assert_eq!(request.bytes(), expected);
            assert_eq!(request.id(), 1);
            assert_eq!(request.command(), kind);
        }
        assert_eq!(
            StopRequest::new(StopCommand::Hsp, 0),
            Err(RpcError::InvalidId)
        );
    }
    #[test]
    fn correlated_replies_do_not_require_or_create_physical_evidence() {
        let generic = [8, 3, 34, 2, 8, 1];
        for command in [StopCommand::CurrentMode, StopCommand::Hdsp] {
            assert_eq!(
                decode_stop_reply(&StopRequest::new(command, 1).unwrap(), &generic),
                Ok(StopReply {
                    request_id: 1,
                    command
                })
            );
        }
        let request = StopRequest::new(StopCommand::Hsp, 1).unwrap();
        let reply = [8, 3, 34, 9, 8, 1, 130, 54, 4, 10, 2, 8, 2];
        assert!(decode_stop_reply(&request, &reply).is_ok());
        assert_eq!(
            decode_stop_reply(&request, &generic),
            Err(RpcError::UnexpectedMessage)
        );
        assert_eq!(
            decode_stop_reply(
                &StopRequest::new(StopCommand::CurrentMode, 1).unwrap(),
                &reply
            ),
            Err(RpcError::UnexpectedMessage)
        );
    }
    #[test]
    fn stale_ids_error_envelopes_duplicates_and_wrong_families_refuse() {
        let request = StopRequest::new(StopCommand::CurrentMode, 1).unwrap();
        assert_eq!(
            decode_stop_reply(&request, &[8, 3, 34, 2, 8, 2]),
            Err(RpcError::MismatchedId)
        );
        assert_eq!(
            decode_stop_reply(&request, &[8, 3, 34, 4, 8, 1, 18, 0]),
            Err(RpcError::RemoteError)
        );
        for bytes in [
            vec![8, 3, 8, 3, 34, 2, 8, 1],
            vec![8, 3, 34, 4, 8, 1, 8, 1],
            vec![8, 1, 18, 2, 8, 1],
            vec![8, 3, 34, 2, 8, 1, 34, 0],
            vec![8, 3, 42, 0],
            vec![8, 3, 34, 0],
            vec![8, 3],
        ] {
            assert!(decode_stop_reply(&request, &bytes).is_err(), "{bytes:?}");
        }
    }
    #[test]
    fn truncated_noncanonical_overflow_and_oversized_frames_refuse() {
        let request = StopRequest::new(StopCommand::CurrentMode, 1).unwrap();
        let reply = [8, 3, 34, 2, 8, 1];
        for end in 0..reply.len() {
            assert!(decode_stop_reply(&request, &reply[..end]).is_err());
        }
        for bytes in [
            vec![8, 131, 0, 34, 2, 8, 1],
            vec![0],
            vec![8, 3, 34, 255],
            vec![8, 3, 34, 255, 255, 255, 255, 255, 255, 255, 255, 255, 2],
            vec![8, 3, 34, 4, 8, 129, 128, 0],
        ] {
            assert!(decode_stop_reply(&request, &bytes).is_err());
        }
        assert_eq!(
            decode_stop_reply(&request, &vec![0; RESPONSE_LIMIT + 1]),
            Err(RpcError::Oversized)
        );
    }
    #[test]
    fn hsp_state_is_bounded_typed_and_never_promoted_to_stopping_proof() {
        let request = StopRequest::new(StopCommand::Hsp, 1).unwrap();
        for state in [
            vec![8, 5],
            vec![48, 2],
            vec![58, 0],
            vec![61, 0, 0, 192, 127],
            vec![8, 2, 8, 2],
            vec![136, 1, 0],
        ] {
            let mut result = vec![10, state.len() as u8];
            result.extend(state);
            let mut response = vec![8, 1, 130, 54, result.len() as u8];
            response.extend(result);
            let mut outer = vec![8, 3, 34, response.len() as u8];
            outer.extend(response);
            assert!(decode_stop_reply(&request, &outer).is_err());
        }
    }
}

#[cfg(test)]
mod error_correlation_regression {
    use super::*;
    fn reply(fields: &[u8]) -> Vec<u8> {
        let mut bytes = vec![0x08, 0x03, 0x22];
        varint(fields.len() as u64, &mut bytes);
        bytes.extend_from_slice(fields);
        bytes
    }
    #[test]
    fn remote_errors_require_complete_unambiguous_correlated_envelopes() {
        let request = StopRequest::new(StopCommand::Hsp, 1).unwrap();
        for fields in [
            &[0x08, 0x02, 0x12, 0x00][..],
            &[0x12, 0x00, 0x08, 0x02][..],
            &[0x12, 0x00][..],
        ] {
            assert_eq!(
                decode_stop_reply(&request, &reply(fields)),
                Err(RpcError::MismatchedId)
            );
        }
        for fields in [&[0x08, 0x01, 0x12, 0x00][..], &[0x12, 0x00, 0x08, 0x01][..]] {
            assert_eq!(
                decode_stop_reply(&request, &reply(fields)),
                Err(RpcError::RemoteError)
            );
        }
        for fields in [
            &[0x08, 0x01, 0x12, 0x00, 0x12, 0x00][..],
            &[0x08, 0x01, 0x12, 0x00, 0x08, 0x01][..],
            &[0x08, 0x01, 0x12, 0x00, 0x18, 0x01][..],
            &[0x08, 0x01, 0x12, 0x00, 0x82, 0x36, 0x00][..],
            &[0x82, 0x36, 0x00, 0x12, 0x00, 0x08, 0x01][..],
        ] {
            assert_eq!(
                decode_stop_reply(&request, &reply(fields)),
                Err(RpcError::UnexpectedMessage)
            );
        }
        assert_eq!(
            decode_stop_reply(&request, &reply(&[0x08, 0x01, 0x12, 0x00, 0x80])),
            Err(RpcError::Malformed)
        );
    }
}
