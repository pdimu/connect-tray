pub const START_MARKER: u8 = 0x3E; // '>'
pub const END_MARKER: u8 = 0x3C;   // '<'
pub const ESCAPE_MARKER: u8 = 0x3D; // '='

pub const MSG_ACK: u8 = 0x01;
pub const MSG_DATA_MDR: u8 = 0x0C;
pub const MSG_DATA_MDR_NO2: u8 = 0x0E;

#[derive(Debug, PartialEq, Eq)]
pub enum ProtocolError {
    EmptyBuffer,
    InvalidDelimiters,
    TrailingEscape,
    TooShort,
    ChecksumMismatch { expected: u8, calculated: u8 },
    LengthMismatch { declared: usize, actual: usize },
}

impl std::fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyBuffer => write!(f, "empty packet buffer"),
            Self::InvalidDelimiters => write!(f, "missing or invalid frame markers (0x3E / 0x3C)"),
            Self::TrailingEscape => write!(f, "incomplete escape sequence at end of buffer"),
            Self::TooShort => write!(f, "packet shorter than 7-byte header+checksum minimum"),
            Self::ChecksumMismatch { expected, calculated } => {
                write!(f, "checksum failed (got 0x{expected:02x}, computed 0x{calculated:02x})")
            }
            Self::LengthMismatch { declared, actual } => {
                write!(f, "payload size mismatch (declared {declared}, parsed {actual})")
            }
        }
    }
}

impl std::error::Error for ProtocolError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packet {
    pub msg_type: u8,
    pub seq: u8,
    pub payload: Vec<u8>,
}

pub fn escape_bytes(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    for &b in input {
        match b {
            START_MARKER | END_MARKER | ESCAPE_MARKER => {
                out.push(ESCAPE_MARKER);
                out.push(b & 0xEF);
            }
            _ => out.push(b),
        }
    }
    out
}

pub fn unescape_bytes(input: &[u8]) -> Result<Vec<u8>, ProtocolError> {
    let mut out = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        let b = input[i];
        if b == ESCAPE_MARKER {
            i += 1;
            if i >= input.len() {
                return Err(ProtocolError::TrailingEscape);
            }
            out.push(input[i] | 0x10);
        } else {
            out.push(b);
        }
        i += 1;
    }
    Ok(out)
}

pub fn calculate_checksum(bytes: &[u8]) -> u8 {
    let mut sum: u8 = 0;
    for &b in bytes {
        sum = sum.wrapping_add(b);
    }
    sum
}

pub fn encode_packet(msg_type: u8, seq: u8, payload: &[u8]) -> Vec<u8> {
    let mut body = Vec::with_capacity(payload.len() + 7);
    body.push(msg_type);
    body.push(seq);
    let len_bytes = (payload.len() as u32).to_be_bytes();
    body.extend_from_slice(&len_bytes);
    body.extend_from_slice(payload);
    let checksum = calculate_checksum(&body);
    body.push(checksum);

    let escaped = escape_bytes(&body);
    let mut frame = Vec::with_capacity(escaped.len() + 2);
    frame.push(START_MARKER);
    frame.extend_from_slice(&escaped);
    frame.push(END_MARKER);
    frame
}

pub fn encode_ack(rx_seq: u8) -> Vec<u8> {
    let ack_seq = 1 - (rx_seq & 1);
    encode_packet(MSG_ACK, ack_seq, &[])
}

pub fn decode_packet(raw: &[u8]) -> Result<Packet, ProtocolError> {
    if raw.is_empty() {
        return Err(ProtocolError::EmptyBuffer);
    }
    if raw.len() < 2 || raw[0] != START_MARKER || raw[raw.len() - 1] != END_MARKER {
        return Err(ProtocolError::InvalidDelimiters);
    }

    let unescaped = unescape_bytes(&raw[1..raw.len() - 1])?;
    if unescaped.len() < 7 {
        return Err(ProtocolError::TooShort);
    }

    let body_len = unescaped.len() - 1;
    let expected_chk = unescaped[body_len];
    let calculated_chk = calculate_checksum(&unescaped[..body_len]);
    if expected_chk != calculated_chk {
        return Err(ProtocolError::ChecksumMismatch {
            expected: expected_chk,
            calculated: calculated_chk,
        });
    }

    let msg_type = unescaped[0];
    let seq = unescaped[1];
    let payload_len = u32::from_be_bytes([
        unescaped[2],
        unescaped[3],
        unescaped[4],
        unescaped[5],
    ]) as usize;

    let actual_payload_len = body_len - 6;
    if payload_len != actual_payload_len {
        return Err(ProtocolError::LengthMismatch {
            declared: payload_len,
            actual: actual_payload_len,
        });
    }

    let payload = unescaped[6..6 + payload_len].to_vec();
    Ok(Packet { msg_type, seq, payload })
}

// --- V2 Command & State Payloads ---

pub const CMD_INIT_REQ: u8 = 0x00;
pub const CMD_INIT_RET: u8 = 0x01;
pub const CMD_BATTERY_GET: u8 = 0x22;
pub const CMD_BATTERY_RET: u8 = 0x23;
pub const CMD_BATTERY_NTFY: u8 = 0x25;
pub const CMD_NCASM_GET: u8 = 0x66;
pub const CMD_NCASM_RET: u8 = 0x67;
pub const CMD_NCASM_SET: u8 = 0x68;
pub const CMD_NCASM_NTFY: u8 = 0x69;
pub const CMD_EQ_GET: u8 = 0x56;
pub const CMD_EQ_RET: u8 = 0x57;
pub const CMD_EQ_SET: u8 = 0x58;
pub const CMD_EQ_NTFY: u8 = 0x59;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AncMode {
    NoiseCanceling,
    AmbientSound { level: u8, voice: bool },
    Off,
}

impl AncMode {
    pub fn serialize_v2(&self) -> Vec<u8> {
        match self {
            Self::NoiseCanceling => vec![CMD_NCASM_SET, 0x17, 0x01, 0x01, 0x00, 0x00, 0x01],
            Self::AmbientSound { level, voice } => {
                let clamped_level = (*level).clamp(1, 20);
                vec![
                    CMD_NCASM_SET,
                    0x17,
                    0x01,
                    0x01,
                    0x01,
                    if *voice { 0x01 } else { 0x00 },
                    clamped_level,
                ]
            }
            Self::Off => vec![CMD_NCASM_SET, 0x17, 0x01, 0x00, 0x00, 0x00, 0x01],
        }
    }
}

pub fn parse_anc_response(payload: &[u8]) -> Option<AncMode> {
    if payload.len() >= 7 && (payload[0] == CMD_NCASM_RET || payload[0] == CMD_NCASM_NTFY) {
        let effect = payload[3];
        let setting = payload[4];
        let voice = payload[5] != 0;
        let level = payload[6];

        if effect == 0 {
            Some(AncMode::Off)
        } else if setting == 1 {
            Some(AncMode::AmbientSound { level, voice })
        } else {
            Some(AncMode::NoiseCanceling)
        }
    } else {
        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SonyProtocolVersion {
    V1,
    V2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BatteryInfo {
    pub level: u8,
    pub charging: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BatteryTopology {
    Single(BatteryInfo),
    Tws {
        left: BatteryInfo,
        right: BatteryInfo,
        case: Option<BatteryInfo>,
    },
}

pub fn parse_battery_response(payload: &[u8]) -> Option<BatteryInfo> {
    if payload.len() >= 4 && (payload[0] == CMD_BATTERY_RET || payload[0] == CMD_BATTERY_NTFY) {
        Some(BatteryInfo {
            level: payload[2],
            charging: payload[3] != 0,
        })
    } else {
        None
    }
}

pub fn parse_dual_battery_response(payload: &[u8]) -> Option<(BatteryInfo, BatteryInfo)> {
    if payload.len() >= 6
        && (payload[0] == CMD_BATTERY_RET || payload[0] == CMD_BATTERY_NTFY)
        && payload[1] == 0x09
    {
        Some((
            BatteryInfo {
                level: payload[2],
                charging: payload[3] != 0,
            },
            BatteryInfo {
                level: payload[4],
                charging: payload[5] != 0,
            },
        ))
    } else {
        None
    }
}

pub fn parse_case_battery_response(payload: &[u8]) -> Option<BatteryInfo> {
    if payload.len() >= 4
        && (payload[0] == CMD_BATTERY_RET || payload[0] == CMD_BATTERY_NTFY)
        && payload[1] == 0x0A
    {
        Some(BatteryInfo {
            level: payload[2],
            charging: payload[3] != 0,
        })
    } else {
        None
    }
}

// --- Extended Hardware Capability Opcodes ---

pub const CMD_FW_GET: u8 = 0x04;
pub const CMD_FW_RET: u8 = 0x05;
pub const CMD_CONNMODE_GET: u8 = 0x16;
pub const CMD_CONNMODE_RET: u8 = 0x17;
pub const CMD_CONNMODE_SET: u8 = 0x18;
pub const CMD_CODEC_GET: u8 = 0x12;
pub const CMD_CODEC_RET: u8 = 0x13;
pub const CMD_APO_GET: u8 = 0x26;
pub const CMD_APO_RET: u8 = 0x27;
pub const CMD_APO_SET: u8 = 0x28;
pub const CMD_VOICE_GUIDANCE_GET: u8 = 0x46;
pub const CMD_VOICE_GUIDANCE_RET: u8 = 0x47;
pub const CMD_VOICE_GUIDANCE_SET: u8 = 0x48;
pub const CMD_DSEE_GET: u8 = 0xE6;
pub const CMD_DSEE_RET: u8 = 0xE7;
pub const CMD_DSEE_SET: u8 = 0xE8;
pub const CMD_BTNMODE_GET: u8 = 0xF6;
pub const CMD_BTNMODE_RET: u8 = 0xF7;
pub const CMD_BTNMODE_SET: u8 = 0xF8;

pub const SUB_WEARING_SENSOR: u8 = 0x08;
pub const SUB_ADAPTIVE_VOLUME: u8 = 0x0A;
pub const SUB_TOUCH_SENSOR: u8 = 0x0B;
pub const SUB_SPEAK_TO_CHAT: u8 = 0x0C;
pub const SUB_MULTIPOINT: u8 = 0x0E;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ConnectionMode {
    PriorityOnSoundQuality = 0x00,
    PriorityOnStableConnection = 0x01,
}

impl ConnectionMode {
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::PriorityOnSoundQuality => "priority on sound quality",
            Self::PriorityOnStableConnection => "priority on stable connection",
        }
    }
}

pub fn parse_conn_mode_response(payload: &[u8]) -> Option<ConnectionMode> {
    if payload.len() >= 3 && (payload[0] == CMD_CONNMODE_RET || payload[0] == 0x19) && payload[1] == 0x01 {
        match payload[2] {
            0x00 => Some(ConnectionMode::PriorityOnSoundQuality),
            0x01 => Some(ConnectionMode::PriorityOnStableConnection),
            _ => None,
        }
    } else {
        None
    }
}

pub fn encode_conn_mode_set(mode: ConnectionMode) -> Vec<u8> {
    vec![CMD_CONNMODE_SET, 0x01, mode as u8]
}

pub fn parse_voice_guidance_response(payload: &[u8]) -> Option<bool> {
    if payload.len() >= 3
        && (payload[0] == CMD_VOICE_GUIDANCE_RET
            || payload[0] == 0x41
            || payload[0] == 0x49
            || payload[0] == 0x43)
        && payload[1] == 0x01
    {
        Some(payload[2] != 0)
    } else {
        None
    }
}

pub fn encode_voice_guidance_set(enabled: bool) -> Vec<u8> {
    vec![CMD_VOICE_GUIDANCE_SET, 0x01, if enabled { 0x01 } else { 0x00 }]
}

pub fn parse_touch_sensor_response(payload: &[u8]) -> Option<bool> {
    if payload.len() >= 3 && (payload[0] == CMD_BTNMODE_RET || payload[0] == 0xF9) && payload[1] == SUB_TOUCH_SENSOR {
        Some(payload[2] == 0) // 0 = enabled, 1 = disabled
    } else {
        None
    }
}

pub fn encode_touch_sensor_set(enabled: bool) -> Vec<u8> {
    vec![CMD_BTNMODE_SET, SUB_TOUCH_SENSOR, if enabled { 0x00 } else { 0x01 }]
}

pub fn parse_multipoint_response(payload: &[u8]) -> Option<bool> {
    if payload.len() >= 3 && (payload[0] == CMD_BTNMODE_RET || payload[0] == 0xF9) && payload[1] == SUB_MULTIPOINT {
        Some(payload[2] == 0) // 0 = enabled, 1 = disabled
    } else {
        None
    }
}

pub fn encode_multipoint_set(enabled: bool) -> Vec<u8> {
    vec![CMD_BTNMODE_SET, SUB_MULTIPOINT, if enabled { 0x00 } else { 0x01 }]
}

// Table 2 Peripheral / Multipoint Opcodes
pub const PERI_GET_PARAM: u8 = 0x36;
pub const PERI_RET_PARAM: u8 = 0x37;
pub const PERI_SET_PARAM: u8 = 0x38;
pub const PERI_NTFY_PARAM: u8 = 0x39;
pub const PERI_SET_EXT_PARAM: u8 = 0x3C;
pub const PERI_NTFY_EXT_PARAM: u8 = 0x3D;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairedDevice {
    pub mac: String,
    pub name: String,
    pub connected: bool,
    pub slot: u8,
    pub is_playing: bool,
}

pub fn parse_device_list(payload: &[u8]) -> Option<Vec<PairedDevice>> {
    if payload.len() < 3 || (payload[0] != PERI_RET_PARAM && payload[0] != PERI_NTFY_PARAM) {
        return None;
    }
    let t = payload[1];
    let n = payload[2] as usize;
    let mut i = 3;
    let mut devs = Vec::with_capacity(n);

    for _ in 0..n {
        if i + 18 > payload.len() {
            return None;
        }
        let mac = String::from_utf8_lossy(&payload[i..i + 17]).to_string();
        let conn = payload[i + 17];
        i += 18;

        if t == 0x02 {
            if i + 3 > payload.len() {
                return None;
            }
            i += 3; // Class of Device (COD)
        }

        if i >= payload.len() {
            return None;
        }
        let ln = payload[i] as usize;
        i += 1;
        if i + ln > payload.len() {
            return None;
        }
        let name = String::from_utf8_lossy(&payload[i..i + ln]).to_string();
        i += ln;

        devs.push(PairedDevice {
            mac,
            name,
            connected: conn > 0,
            slot: conn,
            is_playing: false,
        });
    }

    let playing_slot = if i < payload.len() { payload[i] } else { 0 };
    for d in &mut devs {
        d.is_playing = d.connected && d.slot == playing_slot && playing_slot > 0;
    }

    Some(devs)
}

pub fn encode_switch_source(mac: &str) -> Vec<u8> {
    let mut p = Vec::with_capacity(2 + mac.len());
    p.push(PERI_SET_EXT_PARAM);
    p.push(0x01); // 0x01 = switch source
    p.extend_from_slice(mac.as_bytes());
    p
}

pub fn parse_firmware_response(payload: &[u8]) -> Option<String> {
    if payload.len() > 2 && payload[0] == CMD_FW_RET {
        let text = String::from_utf8_lossy(&payload[2..]).trim().to_string();
        if !text.is_empty() {
            return Some(text);
        }
    }
    None
}

pub fn parse_codec_response(payload: &[u8]) -> Option<&'static str> {
    if payload.len() >= 3 && payload[0] == CMD_CODEC_RET {
        match payload[2] {
            0x01 => Some("SBC"),
            0x02 => Some("AAC"),
            0x10 => Some("LDAC"),
            0x20 => Some("aptX"),
            0x21 => Some("aptX HD"),
            _ => None,
        }
    } else {
        None
    }
}

pub fn parse_dsee_response(payload: &[u8]) -> Option<bool> {
    if payload.len() >= 3 && payload[0] == CMD_DSEE_RET {
        Some(payload[2] != 0)
    } else {
        None
    }
}

pub fn encode_dsee_set(enabled: bool) -> Vec<u8> {
    vec![CMD_DSEE_SET, 0x01, if enabled { 0x01 } else { 0x00 }]
}

// Auto Power Off timing table (indices 0..=5)
pub const APO_CODES: [(u8, u8); 6] = [
    (0x11, 0x00), // 0: Off
    (0x00, 0x00), // 1: 5 minutes
    (0x01, 0x01), // 2: 30 minutes
    (0x02, 0x02), // 3: 1 hour
    (0x03, 0x03), // 4: 3 hours
    (0x10, 0x00), // 5: When taken off
];

pub const APO_NAMES: [&'static str; 6] = [
    "Off",
    "5 minutes",
    "30 minutes",
    "1 hour",
    "3 hours",
    "When taken off",
];

pub fn parse_apo_response(payload: &[u8]) -> Option<usize> {
    if payload.len() >= 4 && payload[0] == CMD_APO_RET {
        let c0 = payload[2];
        let c1 = payload[3];
        for (i, &(m0, m1)) in APO_CODES.iter().enumerate() {
            if m0 == c0 && m1 == c1 {
                return Some(i);
            }
        }
    }
    None
}

pub fn encode_apo_set(index: usize) -> Option<Vec<u8>> {
    AP_CODES.get(index).map(|&(c0, c1)| vec![CMD_APO_SET, 0x05, c0, c1])
}
const AP_CODES: [(u8, u8); 6] = APO_CODES;

pub fn parse_speak_to_chat_response(payload: &[u8]) -> Option<bool> {
    if payload.len() >= 3 && payload[0] == CMD_BTNMODE_RET && payload[1] == SUB_SPEAK_TO_CHAT {
        // Sony V2 Speak-to-chat enable byte is inverted: 0 = enabled, 1 = disabled
        Some(payload[2] == 0)
    } else {
        None
    }
}

pub fn encode_speak_to_chat_set(enabled: bool) -> Vec<u8> {
    vec![
        CMD_BTNMODE_SET,
        SUB_SPEAK_TO_CHAT,
        if enabled { 0x00 } else { 0x01 },
        0x01,
    ]
}

pub fn parse_wearing_sensor_response(payload: &[u8]) -> Option<bool> {
    if payload.len() >= 3 && payload[0] == CMD_BTNMODE_RET && payload[1] == SUB_WEARING_SENSOR {
        Some(payload[2] == 0)
    } else {
        None
    }
}

pub fn encode_wearing_sensor_set(enabled: bool) -> Vec<u8> {
    vec![
        CMD_BTNMODE_SET,
        SUB_WEARING_SENSOR,
        if enabled { 0x00 } else { 0x01 },
    ]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum EqPreset {
    Off = 0x00,
    Bright = 0x10,
    Excited = 0x11,
    Mellow = 0x12,
    Relaxed = 0x13,
    Vocal = 0x14,
    TrebleBoost = 0x15,
    BassBoost = 0x16,
    Speech = 0x17,
    Manual = 0xA0,
}

impl EqPreset {
    pub fn from_u8(val: u8) -> Option<Self> {
        match val {
            0x00 => Some(Self::Off),
            0x10 => Some(Self::Bright),
            0x11 => Some(Self::Excited),
            0x12 => Some(Self::Mellow),
            0x13 => Some(Self::Relaxed),
            0x14 => Some(Self::Vocal),
            0x15 => Some(Self::TrebleBoost),
            0x16 => Some(Self::BassBoost),
            0x17 => Some(Self::Speech),
            0xA0 => Some(Self::Manual),
            _ => None,
        }
    }

    pub fn serialize_v2(&self) -> Vec<u8> {
        vec![CMD_EQ_SET, 0x00, *self as u8, 0x00]
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Bright => "Bright",
            Self::Excited => "Excited",
            Self::Mellow => "Mellow",
            Self::Relaxed => "Relaxed",
            Self::Vocal => "Vocal",
            Self::TrebleBoost => "Treble Boost",
            Self::BassBoost => "Bass Boost",
            Self::Speech => "Speech",
            Self::Manual => "Custom",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EqInfo {
    pub preset: EqPreset,
    pub clear_bass: i8,
    pub bands: [i8; 5],
}

pub fn parse_eq_response(payload: &[u8]) -> Option<EqInfo> {
    if payload.len() >= 3 && (payload[0] == CMD_EQ_RET || payload[0] == CMD_EQ_NTFY) {
        let preset = EqPreset::from_u8(payload[2]).unwrap_or(EqPreset::Off);
        let mut clear_bass = 0;
        let mut bands = [0i8; 5];

        if payload.len() >= 10 {
            clear_bass = (payload[4] as i16 - 10) as i8;
            for i in 0..5 {
                bands[i] = (payload[5 + i] as i16 - 10) as i8;
            }
        }

        Some(EqInfo { preset, clear_bass, bands })
    } else {
        None
    }
}

pub fn encode_clear_bass_set(clear_bass: i8, preset: EqPreset, bands: [i8; 5]) -> Vec<u8> {
    let clamped_bass = clear_bass.clamp(-10, 10);
    let preset_byte = match preset {
        EqPreset::Manual => 0xA0,
        _ => 0xA0,
    };
    vec![
        CMD_EQ_SET,
        0x00,
        preset_byte,
        0x06,
        (clamped_bass + 10) as u8,
        (bands[0].clamp(-10, 10) + 10) as u8,
        (bands[1].clamp(-10, 10) + 10) as u8,
        (bands[2].clamp(-10, 10) + 10) as u8,
        (bands[3].clamp(-10, 10) + 10) as u8,
        (bands[4].clamp(-10, 10) + 10) as u8,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escape_roundtrip() {
        let raw = vec![0x00, 0x3C, 0x3D, 0x3E, 0x2A, 0x3C, 0xFF];
        let escaped = escape_bytes(&raw);
        assert_eq!(escaped, vec![0x00, 0x3D, 0x2C, 0x3D, 0x2D, 0x3D, 0x2E, 0x2A, 0x3D, 0x2C, 0xFF]);
        let unescaped = unescape_bytes(&escaped).unwrap();
        assert_eq!(raw, unescaped);
    }

    #[test]
    fn test_packet_encode_decode() {
        let payload = vec![0x22, 0x00];
        let encoded = encode_packet(MSG_DATA_MDR, 1, &payload);
        assert_eq!(encoded[0], START_MARKER);
        assert_eq!(*encoded.last().unwrap(), END_MARKER);

        let decoded = decode_packet(&encoded).unwrap();
        assert_eq!(decoded.msg_type, MSG_DATA_MDR);
        assert_eq!(decoded.seq, 1);
        assert_eq!(decoded.payload, payload);
    }

    #[test]
    fn test_ack_encode() {
        let ack = encode_ack(0);
        let decoded = decode_packet(&ack).unwrap();
        assert_eq!(decoded.msg_type, MSG_ACK);
        assert_eq!(decoded.seq, 1);
        assert!(decoded.payload.is_empty());

        let ack2 = encode_ack(1);
        let decoded2 = decode_packet(&ack2).unwrap();
        assert_eq!(decoded2.msg_type, MSG_ACK);
        assert_eq!(decoded2.seq, 0);
    }

    #[test]
    fn test_ground_truth_battery_decode() {
        // Real payload captured from WH-CH720N: 23 00 3D 00 (level 61%, charging false)
        let payload = [0x23, 0x00, 0x3D, 0x00];
        let bat = parse_battery_response(&payload).unwrap();
        assert_eq!(bat.level, 61);
        assert!(!bat.charging);
    }

    #[test]
    fn test_ground_truth_anc_decode() {
        // Real payload captured from WH-CH720N in noise canceling mode
        let payload = [0x67, 0x17, 0x01, 0x01, 0x00, 0x00, 0x01];
        let anc = parse_anc_response(&payload).unwrap();
        assert_eq!(anc, AncMode::NoiseCanceling);
    }

    #[test]
    fn test_ground_truth_eq_decode() {
        // Real payload captured from WH-CH720N in manual mode (clear bass +10, bands [-1, 1, 0, 3, 5])
        // values shifted by +10: [10+10, -1+10, 1+10, 0+10, 3+10, 5+10] = [20, 9, 11, 10, 13, 15] = [0x14, 0x09, 0x0B, 0x0A, 0x0D, 0x0F]
        let payload = [0x57, 0x00, 0xA0, 0x06, 0x14, 0x09, 0x0B, 0x0A, 0x0D, 0x0F];
        let eq = parse_eq_response(&payload).unwrap();
        assert_eq!(eq.preset, EqPreset::Manual);
        assert_eq!(eq.clear_bass, 10);
        assert_eq!(eq.bands, [-1, 1, 0, 3, 5]);
    }

    #[test]
    fn test_clear_bass_encode() {
        let payload = encode_clear_bass_set(10, EqPreset::Manual, [-1, 1, 0, 3, 5]);
        assert_eq!(payload, vec![0x58, 0x00, 0xA0, 0x06, 0x14, 0x09, 0x0B, 0x0A, 0x0D, 0x0F]);
    }

    #[test]
    fn test_tws_battery_decode() {
        // Dual battery response: 23 09 <L_lvl: 85%> <L_chg: 1> <R_lvl: 80%> <R_chg: 0>
        let dual_payload = [0x23, 0x09, 0x55, 0x01, 0x50, 0x00];
        let (left, right) = parse_dual_battery_response(&dual_payload).unwrap();
        assert_eq!(left.level, 85);
        assert!(left.charging);
        assert_eq!(right.level, 80);
        assert!(!right.charging);

        // Case battery response: 23 0A <Case_lvl: 90%> <Case_chg: 1>
        let case_payload = [0x23, 0x0A, 0x5A, 0x01];
        let case = parse_case_battery_response(&case_payload).unwrap();
        assert_eq!(case.level, 90);
        assert!(case.charging);
    }

    #[test]
    fn test_capabilities_decode() {
        // Firmware string: 05 02 '1' '.' '0' '.' '8'
        let fw_payload = [0x05, 0x02, b'1', b'.', b'0', b'.', b'8'];
        assert_eq!(parse_firmware_response(&fw_payload).unwrap(), "1.0.8");

        // Codec: 13 02 02 (AAC), 13 02 10 (LDAC)
        assert_eq!(parse_codec_response(&[0x13, 0x02, 0x02]).unwrap(), "AAC");
        assert_eq!(parse_codec_response(&[0x13, 0x02, 0x10]).unwrap(), "LDAC");

        // DSEE: E7 01 01 (true), E7 01 00 (false)
        assert!(parse_dsee_response(&[0xE7, 0x01, 0x01]).unwrap());
        assert!(!parse_dsee_response(&[0xE7, 0x01, 0x00]).unwrap());

        // APO: 27 05 01 01 (30 minutes -> index 2)
        assert_eq!(parse_apo_response(&[0x27, 0x05, 0x01, 0x01]).unwrap(), 2);
        assert_eq!(parse_apo_response(&[0x27, 0x05, 0x10, 0x00]).unwrap(), 5);

        // Speak-to-chat: F7 0C 00 (enabled = true), F7 0C 01 (disabled = false)
        assert!(parse_speak_to_chat_response(&[0xF7, 0x0C, 0x00]).unwrap());
        assert!(!parse_speak_to_chat_response(&[0xF7, 0x0C, 0x01]).unwrap());

        // Connection mode: 17 01 00 (Sound Quality), 17 01 01 (Stable Connection)
        assert_eq!(parse_conn_mode_response(&[0x17, 0x01, 0x00]).unwrap(), ConnectionMode::PriorityOnSoundQuality);
        assert_eq!(parse_conn_mode_response(&[0x17, 0x01, 0x01]).unwrap(), ConnectionMode::PriorityOnStableConnection);
        assert_eq!(encode_conn_mode_set(ConnectionMode::PriorityOnSoundQuality), vec![0x18, 0x01, 0x00]);

        // Voice guidance: 47 01 01 / 41 01 01 (true), 47 01 00 (false)
        assert!(parse_voice_guidance_response(&[0x47, 0x01, 0x01]).unwrap());
        assert!(parse_voice_guidance_response(&[0x41, 0x01, 0x01]).unwrap());
        assert!(!parse_voice_guidance_response(&[0x47, 0x01, 0x00]).unwrap());
        assert_eq!(encode_voice_guidance_set(true), vec![0x48, 0x01, 0x01]);

        // Touch sensor: F7 0B 00 (enabled), F7 0B 01 (disabled)
        assert!(parse_touch_sensor_response(&[0xF7, 0x0B, 0x00]).unwrap());
        assert!(!parse_touch_sensor_response(&[0xF7, 0x0B, 0x01]).unwrap());
        assert_eq!(encode_touch_sensor_set(true), vec![0xF8, 0x0B, 0x00]);

        // Multipoint: F7 0E 00 (enabled), F7 0E 01 (disabled)
        assert!(parse_multipoint_response(&[0xF7, 0x0E, 0x00]).unwrap());
        assert!(!parse_multipoint_response(&[0xF7, 0x0E, 0x01]).unwrap());
        assert_eq!(encode_multipoint_set(false), vec![0xF8, 0x0E, 0x01]);

        // Device list decode (Table 2 type 0x02)
        // 0x37 0x02 <n=2>
        // Dev 1: "C8:E2:65:D2:0B:E4" (17 bytes), slot 1, COD 0x240404, len 12, "NITRO-5-DDP"
        // Dev 2: "AA:BB:CC:DD:EE:FF" (17 bytes), slot 2, COD 0x5A020C, len 9, "Phone G34"
        // playing slot = 2
        let mut dev_payload = vec![0x37, 0x02, 0x02];
        dev_payload.extend_from_slice(b"C8:E2:65:D2:0B:E4");
        dev_payload.push(0x01); // conn / slot 1
        dev_payload.extend_from_slice(&[0x24, 0x04, 0x04]); // COD
        dev_payload.push(11);
        dev_payload.extend_from_slice(b"NITRO-5-DDP");

        dev_payload.extend_from_slice(b"AA:BB:CC:DD:EE:FF");
        dev_payload.push(0x02); // conn / slot 2
        dev_payload.extend_from_slice(&[0x5A, 0x02, 0x0C]); // COD
        dev_payload.push(9);
        dev_payload.extend_from_slice(b"Phone G34");

        dev_payload.push(0x02); // playing slot = 2

        let parsed_devs = parse_device_list(&dev_payload).unwrap();
        assert_eq!(parsed_devs.len(), 2);
        assert_eq!(parsed_devs[0].name, "NITRO-5-DDP");
        assert_eq!(parsed_devs[0].slot, 1);
        assert!(parsed_devs[0].connected);
        assert!(!parsed_devs[0].is_playing);

        assert_eq!(parsed_devs[1].name, "Phone G34");
        assert_eq!(parsed_devs[1].slot, 2);
        assert!(parsed_devs[1].connected);
        assert!(parsed_devs[1].is_playing);

        // Switch source encode
        let switch_cmd = encode_switch_source("C8:E2:65:D2:0B:E4");
        assert_eq!(switch_cmd[0], 0x3C);
        assert_eq!(switch_cmd[1], 0x01);
        assert_eq!(&switch_cmd[2..], b"C8:E2:65:D2:0B:E4");
    }
}
