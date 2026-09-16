//! Typed, whitelisted wire protocol between `NumFlow` and the `numflow-input` helper.
//!
//! The protocol is intentionally tiny: a fixed little-endian header followed by a bounded payload.
//! Only the message kinds in [`Message`] exist, and decoding rejects anything else, so the helper
//! never interprets attacker-controlled integers as behavior. This module is platform-independent
//! so both the wire format and its rejection rules are unit-testable without Win32.

use std::vec::Vec;

use numflow_core::MouseButton;

/// Wire magic, the ASCII bytes `"NFIP"`.
pub const PROTOCOL_MAGIC: u32 = 0x4E46_4950;
/// Wire protocol version. Peers reject frames with any other version.
pub const PROTOCOL_VERSION: u16 = 1;
/// Fixed header length: magic (4) + version (2) + message id (2) + payload length (2).
pub const HEADER_LEN: usize = 10;
/// Hard upper bound for any payload. The largest message needs 8 bytes.
pub const MAX_PAYLOAD_LEN: usize = 16;

/// Message id reserved for helper-to-application acknowledgements.
const ACK_MESSAGE_ID: u16 = 10;

/// Why a helper refused or could not complete a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RejectReason {
    /// The request was accepted.
    #[error("no rejection")]
    None,
    /// The frame was valid but the message kind is not part of the whitelist.
    #[error("unknown command")]
    UnknownCommand,
    /// The frame carried a different protocol version.
    #[error("protocol version mismatch")]
    ProtocolVersion,
    /// The payload length or payload content did not match the message kind.
    #[error("invalid payload")]
    InvalidPayload,
    /// `SendInput` did not accept the complete pointer sequence.
    #[error("pointer injection failed")]
    InjectionFailed,
    /// The helper is connected but has not accepted an owner yet.
    #[error("helper is not ready")]
    NotReady,
}

impl RejectReason {
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::None => 0,
            Self::UnknownCommand => 1,
            Self::ProtocolVersion => 2,
            Self::InvalidPayload => 3,
            Self::InjectionFailed => 4,
            Self::NotReady => 5,
        }
    }

    #[must_use]
    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Self::None),
            1 => Some(Self::UnknownCommand),
            2 => Some(Self::ProtocolVersion),
            3 => Some(Self::InvalidPayload),
            4 => Some(Self::InjectionFailed),
            5 => Some(Self::NotReady),
            _ => None,
        }
    }
}

/// Mouse button action for a [`Message::PointerButton`] request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonAction {
    /// Press the button and keep it held.
    Down,
    /// Release a button previously pressed with [`ButtonAction::Down`].
    Up,
}

impl ButtonAction {
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Down => 0,
            Self::Up => 1,
        }
    }

    #[must_use]
    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Self::Down),
            1 => Some(Self::Up),
            _ => None,
        }
    }
}

/// One protocol message in either direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Message {
    /// Application → helper: request ownership and identify the calling process.
    Handshake {
        /// Process id of the connecting application, cross-checked against the pipe peer.
        client_pid: u32,
    },
    /// Helper → application: ownership accepted; reports the helper's actual `UIAccess` state.
    HandshakeAccepted {
        /// Process id of the helper, cross-checked against the pipe peer.
        server_pid: u32,
        /// Whether the helper process token actually carries the `UIAccess` flag.
        ui_access: bool,
    },
    /// Application → helper: liveness probe.
    Ping {
        /// Opaque value carried in the frame; acknowledged with a plain acknowledgement.
        nonce: u32,
    },
    /// Application → helper: move the pointer by a relative desktop delta.
    PointerMove {
        /// Horizontal delta in desktop pixels.
        dx: i32,
        /// Vertical delta in desktop pixels.
        dy: i32,
    },
    /// Application → helper: press or release a mouse button.
    PointerButton {
        /// Button to act on.
        button: MouseButton,
        /// Whether to press or release it.
        action: ButtonAction,
    },
    /// Application → helper: emit one complete click.
    Click {
        /// Button to click.
        button: MouseButton,
    },
    /// Application → helper: emit two complete clicks.
    DoubleClick {
        /// Button to double click.
        button: MouseButton,
    },
    /// Application → helper: release every mouse button the helper injected.
    ReleaseAll,
    /// Application → helper: release injected state and exit.
    Shutdown,
    /// Helper → application: acknowledgement of a request.
    Ack {
        /// Whether the request completed successfully.
        accepted: bool,
        /// Rejection reason; meaningful only when `accepted` is false.
        detail: RejectReason,
    },
}

/// A protocol-level rejection. Transport errors are reported separately.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ProtocolError {
    /// The frame magic did not match [`PROTOCOL_MAGIC`].
    #[error("invalid protocol magic")]
    BadMagic,
    /// The frame used a different [`PROTOCOL_VERSION`].
    #[error("unsupported protocol version")]
    BadVersion,
    /// The message id is not part of the whitelist.
    #[error("unknown protocol command")]
    UnknownCommand,
    /// The payload length did not match the message kind or exceeded [`MAX_PAYLOAD_LEN`].
    #[error("invalid protocol payload length")]
    BadPayloadLength,
    /// A payload field carried a value outside the whitelisted range.
    #[error("invalid protocol payload")]
    InvalidPayload,
    /// A header announced more payload than [`MAX_PAYLOAD_LEN`].
    #[error("protocol payload exceeds the maximum size")]
    OversizePayload,
}

impl Message {
    #[must_use]
    pub const fn message_id(self) -> u16 {
        match self {
            Self::Handshake { .. } => 1,
            Self::Ping { .. } => 2,
            Self::PointerMove { .. } => 3,
            Self::PointerButton { .. } => 4,
            Self::Click { .. } => 5,
            Self::DoubleClick { .. } => 6,
            Self::ReleaseAll => 7,
            Self::Shutdown => 8,
            Self::HandshakeAccepted { .. } => 9,
            Self::Ack { .. } => ACK_MESSAGE_ID,
        }
    }

    #[must_use]
    pub const fn payload_len(self) -> usize {
        match self {
            Self::Handshake { .. } | Self::Ping { .. } => 4,
            Self::PointerMove { .. } => 8,
            Self::PointerButton { .. } | Self::Ack { .. } => 2,
            Self::Click { .. } | Self::DoubleClick { .. } => 1,
            Self::HandshakeAccepted { .. } => 6,
            Self::ReleaseAll | Self::Shutdown => 0,
        }
    }

    /// Encodes the message into a complete frame (header + payload).
    #[must_use]
    pub fn encode(self) -> Vec<u8> {
        let mut frame = Vec::with_capacity(HEADER_LEN + self.payload_len());
        frame.extend_from_slice(&PROTOCOL_MAGIC.to_le_bytes());
        frame.extend_from_slice(&PROTOCOL_VERSION.to_le_bytes());
        frame.extend_from_slice(&self.message_id().to_le_bytes());
        frame.extend_from_slice(
            &u16::try_from(self.payload_len())
                .expect("protocol payload length fits in u16")
                .to_le_bytes(),
        );
        self.append_payload(&mut frame);
        frame
    }

    /// Decodes a complete frame into a message.
    ///
    /// # Errors
    ///
    /// Returns [`ProtocolError`] when the header or payload violates the whitelist.
    pub fn decode(header: &[u8], payload: &[u8]) -> Result<Self, ProtocolError> {
        if header.len() != HEADER_LEN {
            return Err(ProtocolError::BadPayloadLength);
        }

        let magic = u32::from_le_bytes([header[0], header[1], header[2], header[3]]);
        if magic != PROTOCOL_MAGIC {
            return Err(ProtocolError::BadMagic);
        }

        let version = u16::from_le_bytes([header[4], header[5]]);
        if version != PROTOCOL_VERSION {
            return Err(ProtocolError::BadVersion);
        }

        let message_id = u16::from_le_bytes([header[6], header[7]]);
        let payload_len = usize::from(u16::from_le_bytes([header[8], header[9]]));
        if payload_len > MAX_PAYLOAD_LEN {
            return Err(ProtocolError::OversizePayload);
        }

        let expected = expected_payload_len(message_id).ok_or(ProtocolError::UnknownCommand)?;
        if payload_len != expected || payload.len() != expected {
            return Err(ProtocolError::BadPayloadLength);
        }

        decode_payload(message_id, payload)
    }

    fn append_payload(self, frame: &mut Vec<u8>) {
        match self {
            Self::Handshake { client_pid } => frame.extend_from_slice(&client_pid.to_le_bytes()),
            Self::HandshakeAccepted {
                server_pid,
                ui_access,
            } => {
                frame.extend_from_slice(&server_pid.to_le_bytes());
                frame.push(u8::from(ui_access));
                frame.push(0);
            }
            Self::Ping { nonce } => frame.extend_from_slice(&nonce.to_le_bytes()),
            Self::PointerMove { dx, dy } => {
                frame.extend_from_slice(&dx.to_le_bytes());
                frame.extend_from_slice(&dy.to_le_bytes());
            }
            Self::PointerButton { button, action } => {
                frame.push(button_code(button));
                frame.push(action.code());
            }
            Self::Click { button } | Self::DoubleClick { button } => {
                frame.push(button_code(button));
            }
            Self::ReleaseAll | Self::Shutdown => {}
            Self::Ack { accepted, detail } => {
                frame.push(u8::from(accepted));
                frame.push(detail.code());
            }
        }
    }
}

/// Incremental frame decoder for byte-mode pipe streams.
#[derive(Debug, Default)]
pub struct FrameDecoder {
    buffer: Vec<u8>,
}

impl FrameDecoder {
    /// Appends freshly received bytes to the decoder buffer.
    pub fn push(&mut self, chunk: &[u8]) {
        self.buffer.extend_from_slice(chunk);
        if self.buffer.len() > HEADER_LEN + MAX_PAYLOAD_LEN {
            // A peer that violates the size bound can never recover into a valid stream; reset the
            // buffer to a known-invalid header so the connection fails closed instead of growing.
            self.buffer.clear();
            self.buffer.extend_from_slice(&[0xFF; HEADER_LEN]);
        }
    }

    /// Returns the next complete message, or `None` while the frame is still incomplete.
    ///
    /// # Errors
    ///
    /// Returns [`ProtocolError`] when the buffered frame violates the whitelist.
    pub fn next_frame(&mut self) -> Result<Option<Message>, ProtocolError> {
        if self.buffer.len() < HEADER_LEN {
            return Ok(None);
        }

        let payload_len = usize::from(u16::from_le_bytes([self.buffer[8], self.buffer[9]]));
        if payload_len > MAX_PAYLOAD_LEN {
            return Err(ProtocolError::OversizePayload);
        }

        let total = HEADER_LEN + payload_len;
        if self.buffer.len() < total {
            return Ok(None);
        }

        let header = self.buffer[..HEADER_LEN].to_vec();
        let payload = self.buffer[HEADER_LEN..total].to_vec();
        self.buffer.drain(..total);
        Message::decode(&header, &payload).map(Some)
    }
}

const fn expected_payload_len(message_id: u16) -> Option<usize> {
    match message_id {
        1 | 2 => Some(4),
        3 => Some(8),
        4 | 10 => Some(2),
        5 | 6 => Some(1),
        7 | 8 => Some(0),
        9 => Some(6),
        _ => None,
    }
}

fn decode_payload(message_id: u16, payload: &[u8]) -> Result<Message, ProtocolError> {
    match message_id {
        1 => Ok(Message::Handshake {
            client_pid: read_u32(payload)?,
        }),
        2 => Ok(Message::Ping {
            nonce: read_u32(payload)?,
        }),
        3 => Ok(Message::PointerMove {
            dx: read_i32(payload, 0)?,
            dy: read_i32(payload, 4)?,
        }),
        4 => {
            let button = button_from_code(*payload.first().ok_or(ProtocolError::InvalidPayload)?)?;
            let action =
                ButtonAction::from_code(*payload.get(1).ok_or(ProtocolError::InvalidPayload)?)
                    .ok_or(ProtocolError::InvalidPayload)?;
            Ok(Message::PointerButton { button, action })
        }
        5 | 6 => {
            let button = button_from_code(*payload.first().ok_or(ProtocolError::InvalidPayload)?)?;
            if message_id == 5 {
                Ok(Message::Click { button })
            } else {
                Ok(Message::DoubleClick { button })
            }
        }
        7 => Ok(Message::ReleaseAll),
        8 => Ok(Message::Shutdown),
        9 => {
            let reserved = *payload.get(5).ok_or(ProtocolError::InvalidPayload)?;
            if reserved != 0 {
                return Err(ProtocolError::InvalidPayload);
            }
            Ok(Message::HandshakeAccepted {
                server_pid: read_u32(payload)?,
                ui_access: parse_flag(*payload.get(4).ok_or(ProtocolError::InvalidPayload)?)?,
            })
        }
        10 => {
            let accepted = parse_flag(*payload.first().ok_or(ProtocolError::InvalidPayload)?)?;
            let detail =
                RejectReason::from_code(*payload.get(1).ok_or(ProtocolError::InvalidPayload)?)
                    .ok_or(ProtocolError::InvalidPayload)?;
            if accepted != (detail == RejectReason::None) {
                return Err(ProtocolError::InvalidPayload);
            }
            Ok(Message::Ack { accepted, detail })
        }
        _ => Err(ProtocolError::UnknownCommand),
    }
}

const fn parse_flag(value: u8) -> Result<bool, ProtocolError> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(ProtocolError::InvalidPayload),
    }
}

const fn button_code(button: MouseButton) -> u8 {
    match button {
        MouseButton::Left => 0,
        MouseButton::Right => 1,
        MouseButton::Middle => 2,
    }
}

const fn button_from_code(code: u8) -> Result<MouseButton, ProtocolError> {
    match code {
        0 => Ok(MouseButton::Left),
        1 => Ok(MouseButton::Right),
        2 => Ok(MouseButton::Middle),
        _ => Err(ProtocolError::InvalidPayload),
    }
}

const fn read_u32(payload: &[u8]) -> Result<u32, ProtocolError> {
    if payload.len() < 4 {
        return Err(ProtocolError::InvalidPayload);
    }

    Ok(u32::from_le_bytes([
        payload[0], payload[1], payload[2], payload[3],
    ]))
}

const fn read_i32(payload: &[u8], offset: usize) -> Result<i32, ProtocolError> {
    if payload.len() < offset + 4 {
        return Err(ProtocolError::InvalidPayload);
    }

    Ok(i32::from_le_bytes([
        payload[offset],
        payload[offset + 1],
        payload[offset + 2],
        payload[offset + 3],
    ]))
}

#[cfg(test)]
mod tests {
    use super::{
        ButtonAction, FrameDecoder, HEADER_LEN, MAX_PAYLOAD_LEN, Message, PROTOCOL_MAGIC,
        PROTOCOL_VERSION, ProtocolError, RejectReason,
    };
    use numflow_core::MouseButton;

    const SAMPLE_MESSAGES: [Message; 10] = [
        Message::Handshake { client_pid: 4_242 },
        Message::HandshakeAccepted {
            server_pid: 8_484,
            ui_access: true,
        },
        Message::Ping { nonce: 0x0102_0304 },
        Message::PointerMove { dx: -12, dy: 3_402 },
        Message::PointerButton {
            button: MouseButton::Middle,
            action: ButtonAction::Down,
        },
        Message::Click {
            button: MouseButton::Right,
        },
        Message::DoubleClick {
            button: MouseButton::Left,
        },
        Message::ReleaseAll,
        Message::Shutdown,
        Message::Ack {
            accepted: false,
            detail: RejectReason::InjectionFailed,
        },
    ];

    #[test]
    fn every_whitelisted_message_round_trips() {
        for message in SAMPLE_MESSAGES {
            let frame = message.encode();
            assert_eq!(frame.len(), HEADER_LEN + message.payload_len());
            let decoded = Message::decode(&frame[..HEADER_LEN], &frame[HEADER_LEN..])
                .expect("whitelisted message must decode");
            assert_eq!(decoded, message);
        }
    }

    #[test]
    fn decoder_reassembles_frames_split_across_reads() {
        let mut decoder = FrameDecoder::default();
        let mut delivered = Vec::new();

        for message in SAMPLE_MESSAGES {
            for chunk in message.encode().chunks(3) {
                decoder.push(chunk);
                if let Some(decoded) = decoder.next_frame().expect("split frames stay valid") {
                    delivered.push(decoded);
                }
            }
        }

        assert_eq!(delivered.len(), SAMPLE_MESSAGES.len());
        for (decoded, expected) in delivered.into_iter().zip(SAMPLE_MESSAGES) {
            assert_eq!(decoded, expected);
        }
    }

    #[test]
    fn decoder_rejects_oversize_stream_and_fails_closed() {
        let mut header = [0_u8; HEADER_LEN];
        header[0..4].copy_from_slice(&PROTOCOL_MAGIC.to_le_bytes());
        header[4..6].copy_from_slice(&PROTOCOL_VERSION.to_le_bytes());
        header[6..8].copy_from_slice(&3_u16.to_le_bytes());
        header[8..10].copy_from_slice(
            &u16::try_from(MAX_PAYLOAD_LEN)
                .expect("payload bound fits in u16")
                .wrapping_add(1)
                .to_le_bytes(),
        );

        assert_eq!(
            Message::decode(&header, &[]),
            Err(ProtocolError::OversizePayload)
        );

        let mut decoder = FrameDecoder::default();
        decoder.push(&header);
        assert_eq!(decoder.next_frame(), Err(ProtocolError::OversizePayload));
        assert!(
            decoder.next_frame().is_err(),
            "a failed stream must stay failed instead of resynchronizing silently"
        );
    }

    #[test]
    fn headers_outside_the_whitelist_are_rejected() {
        let frame = Message::PointerMove { dx: 1, dy: 2 }.encode();
        let mut header = frame[..HEADER_LEN].to_vec();
        let payload = &frame[HEADER_LEN..];

        header[0] ^= 0xFF;
        assert_eq!(
            Message::decode(&header, payload),
            Err(ProtocolError::BadMagic)
        );
        header[0] = u8::try_from(PROTOCOL_MAGIC & 0xFF).expect("low magic byte fits in u8");

        header[4] = 0x99;
        assert_eq!(
            Message::decode(&header, payload),
            Err(ProtocolError::BadVersion)
        );
        header[4..6].copy_from_slice(&PROTOCOL_VERSION.to_le_bytes());

        header[6] = 0x7F;
        assert_eq!(
            Message::decode(&header, payload),
            Err(ProtocolError::UnknownCommand)
        );

        header[6..8].copy_from_slice(&3_u16.to_le_bytes());
        header[8..10].copy_from_slice(&7_u16.to_le_bytes());
        assert_eq!(
            Message::decode(&header, payload),
            Err(ProtocolError::BadPayloadLength)
        );
    }

    #[test]
    fn payload_values_outside_the_whitelist_are_rejected() {
        let button_frame = Message::PointerButton {
            button: MouseButton::Left,
            action: ButtonAction::Down,
        }
        .encode();
        let (_, button_payload) = button_frame.split_at(HEADER_LEN);
        assert_eq!(
            Message::decode(&button_frame[..HEADER_LEN], &[7, 0]),
            Err(ProtocolError::InvalidPayload)
        );
        assert_eq!(button_payload, &[0, 0]);

        let click_frame = Message::Click {
            button: MouseButton::Left,
        }
        .encode();
        assert_eq!(
            Message::decode(&click_frame[..HEADER_LEN], &[9]),
            Err(ProtocolError::InvalidPayload)
        );
    }

    #[test]
    fn boolean_and_reject_fields_are_strictly_validated() {
        let handshake = Message::HandshakeAccepted {
            server_pid: 54_321,
            ui_access: false,
        }
        .encode();
        let mut invalid_flag = handshake.clone();
        invalid_flag[HEADER_LEN + 4] = 2;
        assert_eq!(
            Message::decode(&invalid_flag[..HEADER_LEN], &invalid_flag[HEADER_LEN..]),
            Err(ProtocolError::InvalidPayload)
        );

        let mut invalid_reserved = handshake;
        invalid_reserved[HEADER_LEN + 5] = 1;
        assert_eq!(
            Message::decode(
                &invalid_reserved[..HEADER_LEN],
                &invalid_reserved[HEADER_LEN..]
            ),
            Err(ProtocolError::InvalidPayload)
        );

        let ack = Message::Ack {
            accepted: false,
            detail: RejectReason::InjectionFailed,
        }
        .encode();
        let mut invalid_reason = ack.clone();
        invalid_reason[HEADER_LEN + 1] = 0xFF;
        assert_eq!(
            Message::decode(&invalid_reason[..HEADER_LEN], &invalid_reason[HEADER_LEN..]),
            Err(ProtocolError::InvalidPayload)
        );

        let mut contradictory_ack = ack;
        contradictory_ack[HEADER_LEN] = 1;
        assert_eq!(
            Message::decode(
                &contradictory_ack[..HEADER_LEN],
                &contradictory_ack[HEADER_LEN..]
            ),
            Err(ProtocolError::InvalidPayload)
        );
    }

    #[test]
    fn handshake_frames_carry_verified_peer_data() {
        let request = Message::Handshake { client_pid: 12_345 }.encode();
        assert_eq!(&request[HEADER_LEN..], &12_345_u32.to_le_bytes());

        let response = Message::HandshakeAccepted {
            server_pid: 54_321,
            ui_access: false,
        }
        .encode();
        assert_eq!(
            &response[HEADER_LEN..HEADER_LEN + 4],
            &54_321_u32.to_le_bytes()
        );
        assert_eq!(response[HEADER_LEN + 4], 0);
        assert_eq!(response[HEADER_LEN + 5], 0);
    }
}
