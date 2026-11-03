use std::mem::size_of;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicU64, Ordering};

pub static PACKETS_SENT: AtomicU64 = AtomicU64::new(0);
pub static PACKETS_RECV: AtomicU64 = AtomicU64::new(0);

use windows_sys::Win32::Foundation::{BOOL, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Networking::WinSock::{
    closesocket, connect, recv, select, send, setsockopt, socket, WSACleanup, WSAStartup,
    FD_SET, INVALID_SOCKET, SOCKET, SOCKET_ERROR, SOCK_STREAM, SOL_SOCKET, SO_RCVTIMEO,
    TIMEVAL, WSADATA,
};
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryA};

use crate::protocol::{
    decode_packet, encode_ack, encode_apo_set, encode_clear_bass_set, encode_conn_mode_set,
    encode_dsee_set, encode_multipoint_set, encode_packet, encode_speak_to_chat_set,
    encode_switch_source, encode_touch_sensor_set, encode_voice_guidance_set, encode_wearing_sensor_set,
    parse_anc_response, parse_apo_response, parse_battery_response,
    parse_case_battery_response, parse_codec_response, parse_conn_mode_response,
    parse_device_list, parse_dsee_response, parse_dual_battery_response, parse_eq_response,
    parse_firmware_response, parse_multipoint_response, parse_speak_to_chat_response,
    parse_touch_sensor_response, parse_voice_guidance_response,
    parse_wearing_sensor_response, AncMode, BatteryInfo, ConnectionMode, EqInfo,
    EqPreset, Packet, PairedDevice, SonyProtocolVersion, CMD_APO_GET, CMD_APO_RET, CMD_BATTERY_GET,
    CMD_BATTERY_NTFY, CMD_BATTERY_RET, CMD_BTNMODE_GET, CMD_BTNMODE_RET,
    CMD_CODEC_GET, CMD_CODEC_RET, CMD_CONNMODE_GET, CMD_CONNMODE_RET, CMD_CONNMODE_SET, CMD_DSEE_GET,
    CMD_DSEE_RET, CMD_EQ_GET, CMD_FW_GET, CMD_FW_RET, CMD_INIT_REQ, CMD_NCASM_GET,
    CMD_VOICE_GUIDANCE_GET, CMD_VOICE_GUIDANCE_RET, END_MARKER, MSG_ACK,
    MSG_DATA_MDR, MSG_DATA_MDR_NO2, PERI_GET_PARAM, PERI_NTFY_EXT_PARAM, PERI_NTFY_PARAM,
    PERI_RET_PARAM, PERI_SET_EXT_PARAM, START_MARKER, SUB_MULTIPOINT, SUB_SPEAK_TO_CHAT, SUB_TOUCH_SENSOR,
    SUB_WEARING_SENSOR, CMD_BTNMODE_SET,
};

pub const AF_BTH: u16 = 32;
pub const BTHPROTO_RFCOMM: i32 = 3;

pub const SONY_MDR_V2_UUID: windows_sys::core::GUID = windows_sys::core::GUID {
    data1: 0x956C7B26,
    data2: 0xD49A,
    data3: 0x4BA8,
    data4: [0xB0, 0x3F, 0xB1, 0x7D, 0x39, 0x3C, 0xB6, 0xE2],
};

pub const SONY_MDR_V1_UUID: windows_sys::core::GUID = windows_sys::core::GUID {
    data1: 0x96CC203E,
    data2: 0x5068,
    data3: 0x46AD,
    data4: [0xB3, 0x2D, 0xE3, 0x16, 0xF5, 0xE0, 0x69, 0xBA],
};

pub const SOL_RFCOMM: i32 = 0x03;
pub const SO_BTH_AUTHENTICATE: i32 = 0x80000001u32 as i32;
pub const SO_BTH_ENCRYPT: i32 = 0x00000002;

#[repr(C, packed)]
pub struct SOCKADDR_BTH {
    pub address_family: u16,
    pub bt_addr: u64,
    pub service_class_id: windows_sys::core::GUID,
    pub port: u32,
}

const _: () = assert!(size_of::<SOCKADDR_BTH>() == 30);

// --- Dynamic bthprops.cpl function pointers ---

type FnBluetoothFindFirstDevice = unsafe extern "system" fn(
    *const BLUETOOTH_DEVICE_SEARCH_PARAMS,
    *mut BLUETOOTH_DEVICE_INFO,
) -> HANDLE;

type FnBluetoothFindNextDevice = unsafe extern "system" fn(
    HANDLE,
    *mut BLUETOOTH_DEVICE_INFO,
) -> BOOL;

type FnBluetoothFindDeviceClose = unsafe extern "system" fn(HANDLE) -> BOOL;

#[repr(C)]
#[derive(Copy, Clone, Default)]
pub struct SYSTEMTIME {
    pub w_year: u16,
    pub w_month: u16,
    pub w_day_of_week: u16,
    pub w_day: u16,
    pub w_hour: u16,
    pub w_minute: u16,
    pub w_second: u16,
    pub w_milliseconds: u16,
}

#[repr(C)]
pub struct BLUETOOTH_DEVICE_SEARCH_PARAMS {
    pub dw_size: u32,
    pub f_return_authenticated: BOOL,
    pub f_return_remembered: BOOL,
    pub f_return_unknown: BOOL,
    pub f_return_connected: BOOL,
    pub f_issue_inquiry: BOOL,
    pub c_timeout_multiplier: u8,
    pub _padding: [u8; 7],
    pub h_radio: HANDLE,
}

#[repr(C)]
pub struct BLUETOOTH_DEVICE_INFO {
    pub dw_size: u32,
    pub address: u64,
    pub ul_class_of_device: u32,
    pub f_connected: BOOL,
    pub f_remembered: BOOL,
    pub f_authenticated: BOOL,
    pub st_last_seen: SYSTEMTIME,
    pub st_last_used: SYSTEMTIME,
    pub sz_name: [u16; 248],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredDevice {
    pub name: String,
    pub address: u64,
    pub connected: bool,
}

pub fn find_paired_devices() -> Vec<DiscoveredDevice> {
    let mut out = Vec::new();

    unsafe {
        let bth = LoadLibraryA(b"bthprops.cpl\0".as_ptr());
        if bth.is_null() {
            return out;
        }

        let p_first = GetProcAddress(bth, b"BluetoothFindFirstDevice\0".as_ptr());
        let p_next = GetProcAddress(bth, b"BluetoothFindNextDevice\0".as_ptr());
        let p_close = GetProcAddress(bth, b"BluetoothFindDeviceClose\0".as_ptr());

        if let (Some(first), Some(next), Some(close)) = (p_first, p_next, p_close) {
            let fn_first: FnBluetoothFindFirstDevice = std::mem::transmute(first);
            let fn_next: FnBluetoothFindNextDevice = std::mem::transmute(next);
            let fn_close: FnBluetoothFindDeviceClose = std::mem::transmute(close);

            let mut params = BLUETOOTH_DEVICE_SEARCH_PARAMS {
                dw_size: size_of::<BLUETOOTH_DEVICE_SEARCH_PARAMS>() as u32,
                f_return_authenticated: 1,
                f_return_remembered: 1,
                f_return_unknown: 0,
                f_return_connected: 1,
                f_issue_inquiry: 0,
                c_timeout_multiplier: 2,
                _padding: [0; 7],
                h_radio: null_mut(),
            };

            let mut info = BLUETOOTH_DEVICE_INFO {
                dw_size: size_of::<BLUETOOTH_DEVICE_INFO>() as u32,
                address: 0,
                ul_class_of_device: 0,
                f_connected: 0,
                f_remembered: 0,
                f_authenticated: 0,
                st_last_seen: SYSTEMTIME::default(),
                st_last_used: SYSTEMTIME::default(),
                sz_name: [0; 248],
            };

            let h_find = fn_first(&mut params, &mut info);
            if !h_find.is_null() && h_find != INVALID_HANDLE_VALUE {
                loop {
                    let end = info.sz_name.iter().position(|&c| c == 0).unwrap_or(info.sz_name.len());
                    let name = String::from_utf16_lossy(&info.sz_name[..end]);
                    out.push(DiscoveredDevice {
                        name,
                        address: info.address,
                        connected: info.f_connected != 0,
                    });

                    if fn_next(h_find, &mut info) == 0 {
                        break;
                    }
                }
                fn_close(h_find);
            }
        }
    }
    out
}

pub fn format_mac_addr(addr: u64) -> String {
    format!(
        "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
        (addr >> 40) & 0xFF,
        (addr >> 32) & 0xFF,
        (addr >> 24) & 0xFF,
        (addr >> 16) & 0xFF,
        (addr >> 8) & 0xFF,
        addr & 0xFF,
    )
}

pub fn is_this_pc(name: &str) -> bool {
    let name_lower = name.to_lowercase();
    let computer_name = std::env::var("COMPUTERNAME").unwrap_or_default().to_lowercase();
    if !computer_name.is_empty() {
        if name_lower == computer_name
            || name_lower.contains(&computer_name)
            || computer_name.contains(&name_lower)
        {
            return true;
        }
    }
    name_lower.starts_with("desktop-") || name_lower.starts_with("laptop-")
}

pub fn is_sony_device_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.contains("wh-")
        || lower.contains("wf-")
        || lower.contains("linkbuds")
        || lower.contains("mdr-")
        || lower.contains("wi-")
        || lower.contains("sony")
        || lower.contains("ult")
}

pub fn discover_sony_device() -> Option<(u64, String)> {
    let devices = find_paired_devices();
    // 1. Prioritize connected devices
    for dev in &devices {
        if dev.connected && is_sony_device_name(&dev.name) {
            return Some((dev.address, dev.name.clone()));
        }
    }
    // 2. Otherwise return first paired Sony device
    for dev in &devices {
        if is_sony_device_name(&dev.name) {
            return Some((dev.address, dev.name.clone()));
        }
    }
    None
}

pub fn discover_sony_address() -> Option<u64> {
    discover_sony_device().map(|(addr, _)| addr)
}

pub fn find_all_sony_devices() -> Vec<DiscoveredDevice> {
    let devices = find_paired_devices();
    devices.into_iter().filter(|d| is_sony_device_name(&d.name)).collect()
}

// --- Transport Implementation ---

pub struct SonyRfcomm {
    socket: SOCKET,
    seq: u8,
    rx_buf: Vec<u8>,
    pub protocol_version: SonyProtocolVersion,
}

impl SonyRfcomm {
    unsafe fn create_configured_socket() -> Result<SOCKET, String> {
        unsafe {
            let sock = socket(AF_BTH as i32, SOCK_STREAM as i32, BTHPROTO_RFCOMM);
            if sock == INVALID_SOCKET {
                return Err("failed to create RFCOMM socket".to_string());
            }

            let enable: u32 = 1;
            setsockopt(
                sock,
                SOL_RFCOMM,
                SO_BTH_AUTHENTICATE,
                &enable as *const u32 as *const u8,
                size_of::<u32>() as i32,
            );
            setsockopt(
                sock,
                SOL_RFCOMM,
                SO_BTH_ENCRYPT,
                &enable as *const u32 as *const u8,
                size_of::<u32>() as i32,
            );

            // 800ms receive timeout (fast failover instead of multi-second UI lag)
            let timeout_ms: u32 = 800;
            setsockopt(
                sock,
                SOL_SOCKET,
                SO_RCVTIMEO as i32,
                &timeout_ms as *const u32 as *const u8,
                size_of::<u32>() as i32,
            );

            Ok(sock)
        }
    }

    pub fn connect(bt_addr: u64) -> Result<Self, String> {
        unsafe {
            let mut wsa_data = std::mem::zeroed::<WSADATA>();
            if WSAStartup(0x0202, &mut wsa_data) != 0 {
                return Err("WSAStartup failed".to_string());
            }

            // Attempt 1: Modern Sony V2 UUID
            let sock_v2 = match Self::create_configured_socket() {
                Ok(s) => s,
                Err(e) => {
                    WSACleanup();
                    return Err(e);
                }
            };
            let addr_v2 = SOCKADDR_BTH {
                address_family: AF_BTH,
                bt_addr,
                service_class_id: SONY_MDR_V2_UUID,
                port: 0,
            };

            let res_v2 = connect(
                sock_v2,
                &addr_v2 as *const SOCKADDR_BTH as *const _,
                size_of::<SOCKADDR_BTH>() as i32,
            );

            if res_v2 == 0 {
                let mut client = Self {
                    socket: sock_v2,
                    seq: 0,
                    rx_buf: Vec::with_capacity(2048),
                    protocol_version: SonyProtocolVersion::V2,
                };
                if client.init_handshake().unwrap_or(false) {
                    return Ok(client);
                }
                // If handshake failed, close and try V1 fallback
                closesocket(sock_v2);
            } else {
                closesocket(sock_v2);
            }

            // Attempt 2: Legacy Sony V1 UUID fallback (WH-1000XM4, WH-1000XM3, WF-1000XM3)
            let sock_v1 = match Self::create_configured_socket() {
                Ok(s) => s,
                Err(e) => {
                    WSACleanup();
                    return Err(e);
                }
            };
            let addr_v1 = SOCKADDR_BTH {
                address_family: AF_BTH,
                bt_addr,
                service_class_id: SONY_MDR_V1_UUID,
                port: 0,
            };

            let res_v1 = connect(
                sock_v1,
                &addr_v1 as *const SOCKADDR_BTH as *const _,
                size_of::<SOCKADDR_BTH>() as i32,
            );

            if res_v1 == 0 {
                return Ok(Self {
                    socket: sock_v1,
                    seq: 0,
                    rx_buf: Vec::with_capacity(2048),
                    protocol_version: SonyProtocolVersion::V1,
                });
            }

            closesocket(sock_v1);
            let wsa_err = windows_sys::Win32::Networking::WinSock::WSAGetLastError();
            WSACleanup();
            Err(format!(
                "RFCOMM connect failed on both V2 and V1 UUIDs (WSA error {})",
                wsa_err
            ))
        }
    }

    pub fn is_alive(&mut self) -> bool {
        unsafe {
            let mut read_fds = std::mem::zeroed::<FD_SET>();
            read_fds.fd_count = 1;
            read_fds.fd_array[0] = self.socket;

            let mut except_fds = std::mem::zeroed::<FD_SET>();
            except_fds.fd_count = 1;
            except_fds.fd_array[0] = self.socket;

            let mut tv = TIMEVAL {
                tv_sec: 0,
                tv_usec: 0,
            };
            let ret = select(0, &mut read_fds, null_mut(), &mut except_fds, &mut tv);

            if ret == SOCKET_ERROR {
                return false;
            }

            if ret > 0 {
                if except_fds.fd_count > 0 {
                    return false;
                }
                if read_fds.fd_count > 0 {
                    let mut chunk = [0u8; 1024];
                    let n = recv(self.socket, chunk.as_mut_ptr(), chunk.len() as i32, 0);
                    if n <= 0 {
                        // 0 = graceful disconnect, <0 = socket error / connection reset
                        return false;
                    }
                    self.rx_buf.extend_from_slice(&chunk[..n as usize]);
                }
            }

            true
        }
    }

    pub fn try_recv_async_packet(&mut self) -> Option<Packet> {
        if let Some(start_idx) = self.rx_buf.iter().position(|&b| b == START_MARKER) {
            if start_idx > 0 {
                self.rx_buf.drain(0..start_idx);
            }
            let sub = &self.rx_buf[1..];
            if let Some(end_rel) = sub.iter().position(|&b| b == END_MARKER) {
                let next_start_rel = sub.iter().position(|&b| b == START_MARKER);
                if let Some(next_start) = next_start_rel {
                    if next_start < end_rel {
                        self.rx_buf.drain(0..=next_start);
                        return None;
                    }
                }
                let end_idx = 1 + end_rel;
                let frame: Vec<u8> = self.rx_buf.drain(0..=end_idx).collect();
                if let Ok(pkt) = decode_packet(&frame) {
                    PACKETS_RECV.fetch_add(1, Ordering::Relaxed);
                    if pkt.msg_type == MSG_DATA_MDR || pkt.msg_type == MSG_DATA_MDR_NO2 {
                        let ack_frame = encode_ack(pkt.seq);
                        unsafe {
                            send(
                                self.socket,
                                ack_frame.as_ptr(),
                                ack_frame.len() as i32,
                                0,
                            );
                        }
                    }
                    return Some(pkt);
                }
            }
        }
        None
    }

    fn recv_raw_packet(&mut self) -> Result<Packet, String> {
        loop {
            // Check if rx_buf contains a complete packet
            if let Some(start_idx) = self.rx_buf.iter().position(|&b| b == START_MARKER) {
                if start_idx > 0 {
                    self.rx_buf.drain(0..start_idx);
                }
                let sub = &self.rx_buf[1..];
                if let Some(end_rel) = sub.iter().position(|&b| b == END_MARKER) {
                    let next_start_rel = sub.iter().position(|&b| b == START_MARKER);
                    if let Some(next_start) = next_start_rel {
                        if next_start < end_rel {
                            // Discard broken frame and jump forward to the next start marker
                            self.rx_buf.drain(0..=next_start);
                            continue;
                        }
                    }
                    let end_idx = 1 + end_rel;
                    let frame: Vec<u8> = self.rx_buf.drain(0..=end_idx).collect();
                    let decoded = decode_packet(&frame).map_err(|e| format!("decode error: {e}"))?;
                    PACKETS_RECV.fetch_add(1, Ordering::Relaxed);
                    return Ok(decoded);
                }
            } else {
                self.rx_buf.clear();
            }

            // Need more data from socket
            let mut chunk = [0u8; 1024];
            unsafe {
                let n = recv(
                    self.socket,
                    chunk.as_mut_ptr(),
                    chunk.len() as i32,
                    0,
                );
                if n <= 0 {
                    return Err("socket recv timeout or disconnected".to_string());
                }
                self.rx_buf.extend_from_slice(&chunk[..n as usize]);
            }
        }
    }

    pub fn send_query_command(&mut self, payload: &[u8], expected_opcodes: &[u8]) -> Result<Vec<u8>, String> {
        self.send_query_command_table(payload, expected_opcodes, MSG_DATA_MDR)
    }

    pub fn send_query_command_table(&mut self, payload: &[u8], expected_opcodes: &[u8], table_msg_type: u8) -> Result<Vec<u8>, String> {
        let pkt = encode_packet(table_msg_type, self.seq, payload);
        unsafe {
            let sent = send(
                self.socket,
                pkt.as_ptr(),
                pkt.len() as i32,
                0,
            );
            if sent == SOCKET_ERROR {
                return Err("send failed".to_string());
            }
            PACKETS_SENT.fetch_add(1, Ordering::Relaxed);
        }

        // Loop to handle interleaved ACKs, notifications, and the targeted response
        for _ in 0..16 {
            let incoming = self.recv_raw_packet()?;
            if incoming.msg_type == MSG_ACK {
                self.seq = incoming.seq;
                continue;
            }
            if incoming.msg_type == MSG_DATA_MDR || incoming.msg_type == MSG_DATA_MDR_NO2 {
                // Acknowledge the device's frame
                let ack_frame = encode_ack(incoming.seq);
                unsafe {
                    send(
                        self.socket,
                        ack_frame.as_ptr(),
                        ack_frame.len() as i32,
                        0,
                    );
                }

                if incoming.msg_type == table_msg_type
                    && !incoming.payload.is_empty()
                    && (expected_opcodes.is_empty() || expected_opcodes.contains(&incoming.payload[0]))
                {
                    return Ok(incoming.payload);
                }
            }
        }
        Err("no matching response received from device".to_string())
    }

    pub fn send_set_command(&mut self, payload: &[u8]) -> Result<(), String> {
        self.send_set_command_table(payload, MSG_DATA_MDR)
    }

    pub fn send_set_command_table(&mut self, payload: &[u8], table_msg_type: u8) -> Result<(), String> {
        let pkt = encode_packet(table_msg_type, self.seq, payload);
        unsafe {
            let sent = send(
                self.socket,
                pkt.as_ptr(),
                pkt.len() as i32,
                0,
            );
            if sent == SOCKET_ERROR {
                return Err("send failed".to_string());
            }
            PACKETS_SENT.fetch_add(1, Ordering::Relaxed);
        }

        // Await ACK from device for SET command
        for _ in 0..8 {
            let incoming = self.recv_raw_packet()?;
            if incoming.msg_type == MSG_ACK {
                self.seq = incoming.seq;
                return Ok(());
            }
            if incoming.msg_type == MSG_DATA_MDR || incoming.msg_type == MSG_DATA_MDR_NO2 {
                let ack_frame = encode_ack(incoming.seq);
                unsafe {
                    send(
                        self.socket,
                        ack_frame.as_ptr(),
                        ack_frame.len() as i32,
                        0,
                    );
                }
                return Ok(());
            }
        }
        Err("timeout waiting for device ack".to_string())
    }

    pub fn send_command(&mut self, payload: &[u8]) -> Result<Vec<u8>, String> {
        self.send_query_command(payload, &[])
    }

    pub fn init_handshake(&mut self) -> Result<bool, String> {
        let resp = self.send_query_command(&[CMD_INIT_REQ, 0x00], &[0x01])?;
        Ok(resp.len() >= 4 && resp[0] == 0x01)
    }

    pub fn query_battery(&mut self) -> Result<BatteryInfo, String> {
        let resp = self.send_query_command(&[CMD_BATTERY_GET, 0x00], &[crate::protocol::CMD_BATTERY_RET])?;
        parse_battery_response(&resp).ok_or_else(|| "invalid battery response payload".to_string())
    }

    pub fn query_anc(&mut self) -> Result<AncMode, String> {
        let resp = self.send_query_command(&[CMD_NCASM_GET, 0x17], &[crate::protocol::CMD_NCASM_RET])?;
        parse_anc_response(&resp).ok_or_else(|| "invalid anc response payload".to_string())
    }

    pub fn set_anc(&mut self, mode: AncMode) -> Result<(), String> {
        let payload = mode.serialize_v2();
        self.send_set_command(&payload)
    }

    pub fn query_eq(&mut self) -> Result<EqInfo, String> {
        let resp = self.send_query_command(&[CMD_EQ_GET, 0x00], &[crate::protocol::CMD_EQ_RET])?;
        parse_eq_response(&resp).ok_or_else(|| "invalid eq response payload".to_string())
    }

    pub fn set_eq(&mut self, preset: EqPreset) -> Result<(), String> {
        let payload = preset.serialize_v2();
        self.send_set_command(&payload)
    }

    pub fn set_clear_bass(&mut self, clear_bass: i8, preset: EqPreset, bands: [i8; 5]) -> Result<(), String> {
        let payload = encode_clear_bass_set(clear_bass, preset, bands);
        self.send_set_command(&payload)
    }

    pub fn query_tws_battery(&mut self) -> Result<(BatteryInfo, BatteryInfo), String> {
        let resp = self.send_query_command(&[CMD_BATTERY_GET, 0x09], &[CMD_BATTERY_RET, CMD_BATTERY_NTFY])?;
        parse_dual_battery_response(&resp).ok_or_else(|| "invalid dual battery response".to_string())
    }

    pub fn query_case_battery(&mut self) -> Result<BatteryInfo, String> {
        let resp = self.send_query_command(&[CMD_BATTERY_GET, 0x0A], &[CMD_BATTERY_RET, CMD_BATTERY_NTFY])?;
        parse_case_battery_response(&resp).ok_or_else(|| "invalid case battery response".to_string())
    }

    pub fn query_firmware(&mut self) -> Result<String, String> {
        let resp = self.send_query_command(&[CMD_FW_GET, 0x02], &[CMD_FW_RET])?;
        parse_firmware_response(&resp).ok_or_else(|| "invalid firmware response".to_string())
    }

    pub fn query_codec(&mut self) -> Result<&'static str, String> {
        let resp = self.send_query_command(&[CMD_CODEC_GET, 0x02], &[CMD_CODEC_RET])?;
        parse_codec_response(&resp).ok_or_else(|| "invalid codec response".to_string())
    }

    pub fn query_dsee(&mut self) -> Result<bool, String> {
        let resp = self.send_query_command(&[CMD_DSEE_GET, 0x01], &[CMD_DSEE_RET])?;
        parse_dsee_response(&resp).ok_or_else(|| "invalid dsee response".to_string())
    }

    pub fn set_dsee(&mut self, enabled: bool) -> Result<(), String> {
        self.send_set_command(&encode_dsee_set(enabled))
    }

    pub fn query_apo(&mut self) -> Result<usize, String> {
        let resp = self.send_query_command(&[CMD_APO_GET, 0x05], &[CMD_APO_RET])?;
        parse_apo_response(&resp).ok_or_else(|| "invalid auto-power-off response".to_string())
    }

    pub fn set_apo(&mut self, index: usize) -> Result<(), String> {
        let payload = encode_apo_set(index).ok_or_else(|| "invalid apo index".to_string())?;
        self.send_set_command(&payload)
    }

    pub fn query_speak_to_chat(&mut self) -> Result<bool, String> {
        let resp = self.send_query_command(&[CMD_BTNMODE_GET, SUB_SPEAK_TO_CHAT], &[CMD_BTNMODE_RET])?;
        parse_speak_to_chat_response(&resp).ok_or_else(|| "invalid speak-to-chat response".to_string())
    }

    pub fn set_speak_to_chat(&mut self, enabled: bool) -> Result<(), String> {
        self.send_set_command(&encode_speak_to_chat_set(enabled))
    }

    pub fn query_wearing_sensor(&mut self) -> Result<bool, String> {
        let resp = self.send_query_command(&[CMD_BTNMODE_GET, SUB_WEARING_SENSOR], &[CMD_BTNMODE_RET])?;
        parse_wearing_sensor_response(&resp).ok_or_else(|| "invalid wearing sensor response".to_string())
    }

    pub fn set_wearing_sensor(&mut self, enabled: bool) -> Result<(), String> {
        self.send_set_command(&encode_wearing_sensor_set(enabled))
    }

    pub fn query_connection_mode(&mut self) -> Result<ConnectionMode, String> {
        let resp = self
            .send_query_command(&[CMD_CONNMODE_GET, 0x01], &[CMD_CONNMODE_RET, 0x11, 0x19])
            .or_else(|_| self.send_query_command(&[CMD_CONNMODE_GET, 0x00], &[CMD_CONNMODE_RET, 0x11, 0x19]))?;
        parse_conn_mode_response(&resp).ok_or_else(|| "invalid connection mode response".to_string())
    }

    pub fn set_connection_mode(&mut self, mode: ConnectionMode) -> Result<(), String> {
        let p1 = encode_conn_mode_set(mode);
        let _ = self.send_set_command(&p1);
        let p2 = vec![CMD_CONNMODE_SET, 0x00, mode as u8];
        let _ = self.send_set_command(&p2);
        Ok(())
    }

    pub fn query_voice_guidance(&mut self) -> Result<bool, String> {
        let resp = self
            .send_query_command(&[CMD_VOICE_GUIDANCE_GET, 0x01], &[CMD_VOICE_GUIDANCE_RET, 0x41, 0x49])
            .or_else(|_| self.send_query_command(&[0x40, 0x01], &[0x41, 0x43]))?;
        parse_voice_guidance_response(&resp).ok_or_else(|| "invalid voice guidance response".to_string())
    }

    pub fn set_voice_guidance(&mut self, enabled: bool) -> Result<(), String> {
        let _ = self.send_set_command(&encode_voice_guidance_set(enabled));
        let _ = self.send_set_command(&[0x42, 0x01, if enabled { 0x01 } else { 0x00 }]);
        Ok(())
    }

    pub fn query_touch_sensor(&mut self) -> Result<bool, String> {
        let resp = self.send_query_command(&[CMD_BTNMODE_GET, SUB_TOUCH_SENSOR], &[CMD_BTNMODE_RET, 0xF7, 0xF9])?;
        parse_touch_sensor_response(&resp).ok_or_else(|| "invalid touch sensor response".to_string())
    }

    pub fn set_touch_sensor(&mut self, enabled: bool) -> Result<(), String> {
        self.send_set_command(&encode_touch_sensor_set(enabled))
    }

    pub fn query_multipoint(&mut self) -> Result<bool, String> {
        let resp = self
            .send_query_command(&[CMD_BTNMODE_GET, SUB_MULTIPOINT], &[CMD_BTNMODE_RET, 0xF7, 0xF9, 0xF1])
            .or_else(|_| self.send_query_command(&[CMD_BTNMODE_GET, 0x01], &[CMD_BTNMODE_RET, 0xF7, 0xF9, 0xF1]))?;
        parse_multipoint_response(&resp).ok_or_else(|| "invalid multipoint response".to_string())
    }

    pub fn set_multipoint(&mut self, enabled: bool) -> Result<(), String> {
        let _ = self.send_set_command(&encode_multipoint_set(enabled));
        let _ = self.send_set_command(&[CMD_BTNMODE_SET, 0x01, if enabled { 0x00 } else { 0x01 }]);
        Ok(())
    }

    pub fn query_device_list(&mut self) -> Result<Vec<PairedDevice>, String> {
        let resp = self.send_query_command_table(&[PERI_GET_PARAM, 0x02], &[PERI_RET_PARAM], MSG_DATA_MDR_NO2)
            .or_else(|_| self.send_query_command_table(&[PERI_GET_PARAM, 0x00], &[PERI_RET_PARAM], MSG_DATA_MDR_NO2))?;
        crate::tray::log_trace(&format!("query_device_list raw response (len={}): {:02x?}", resp.len(), resp));
        parse_device_list(&resp).ok_or_else(|| "failed to parse paired devices list".to_string())
    }

    pub fn switch_playback_to_mac(&mut self, mac: &str) -> Result<(), String> {
        let payload = encode_switch_source(mac);
        self.send_set_command_table(&payload, MSG_DATA_MDR_NO2)
    }

    pub fn pin_focus(&mut self, mac: &str) -> Result<(), String> {
        let payload = encode_switch_source(mac);
        self.send_set_command_table(&payload, MSG_DATA_MDR_NO2)
    }

    pub fn release_fixed_focus(&mut self) -> Result<(), String> {
        let mut p = Vec::with_capacity(3);
        p.push(PERI_SET_EXT_PARAM);
        p.push(0x01);
        p.extend_from_slice(b"00:00:00:00:00:00");
        let _ = self.send_set_command_table(&p, MSG_DATA_MDR_NO2);
        let p_empty = vec![PERI_SET_EXT_PARAM, 0x01];
        let _ = self.send_set_command_table(&p_empty, MSG_DATA_MDR_NO2);
        Ok(())
    }

    pub fn disconnect_device(&mut self, mac: &str) -> Result<(), String> {
        let mut p = Vec::with_capacity(3 + mac.len());
        p.push(PERI_SET_EXT_PARAM);
        p.push(0x02);
        p.push(0x00); // 0x00 = disconnect
        p.extend_from_slice(mac.as_bytes());
        self.send_set_command_table(&p, MSG_DATA_MDR_NO2)
    }

    pub fn connect_paired_device(&mut self, target_mac: &str) -> Result<bool, String> {
        // 1. Send the connect command directly without dropping anything upfront
        let mut p = Vec::with_capacity(3 + target_mac.len());
        p.push(PERI_SET_EXT_PARAM);
        p.push(0x02);
        p.push(0x01); // 0x01 = connect
        p.extend_from_slice(target_mac.as_bytes());
        self.send_set_command_table(&p, MSG_DATA_MDR_NO2)?;

        // 2. Wait for confirmation from the headset (up to 5s)
        let mut handshake_accepted = false;
        let start = std::time::Instant::now();
        while start.elapsed() < std::time::Duration::from_secs(5) {
            if let Ok(pkt) = self.recv_raw_packet() {
                if pkt.msg_type == MSG_ACK {
                    self.seq = pkt.seq;
                    continue;
                }
                if pkt.msg_type == MSG_DATA_MDR || pkt.msg_type == MSG_DATA_MDR_NO2 {
                    let ack_frame = encode_ack(pkt.seq);
                    unsafe {
                        send(self.socket, ack_frame.as_ptr(), ack_frame.len() as i32, 0);
                    }

                    // Check for 0x3D notification (PERI_NTFY_EXT_PARAM)
                    if pkt.msg_type == MSG_DATA_MDR_NO2
                        && pkt.payload.len() >= 4
                        && pkt.payload[0] == PERI_NTFY_EXT_PARAM
                        && pkt.payload[2] == 0x01 // connect action
                    {
                        let mac_matches = if pkt.payload.len() >= 4 + target_mac.len() {
                            pkt.payload[4..4 + target_mac.len()].eq_ignore_ascii_case(target_mac.as_bytes())
                        } else {
                            true
                        };

                        if mac_matches {
                            let result_code = pkt.payload[3];
                            if result_code == 0x11 {
                                // 0x11 is CONNECTION_ERROR / failure: do NOT drop the other device
                                return Ok(false);
                            } else {
                                // anything other than connection failure (0x10 success, 0x12 in progress): handshake accepted
                                handshake_accepted = true;
                                break;
                            }
                        }
                    }

                    // Check for 0x39 / 0x37 device list update
                    if pkt.msg_type == MSG_DATA_MDR_NO2
                        && !pkt.payload.is_empty()
                        && (pkt.payload[0] == PERI_NTFY_PARAM || pkt.payload[0] == PERI_RET_PARAM)
                    {
                        if let Some(devs) = parse_device_list(&pkt.payload) {
                            if let Some(target) = devs.iter().find(|d| d.mac.eq_ignore_ascii_case(target_mac)) {
                                if target.connected {
                                    handshake_accepted = true;
                                    break;
                                }
                            }
                        }
                    }
                }
            } else {
                break;
            }
        }

        // Check live device list if notification was missed
        if !handshake_accepted {
            if let Ok(devs) = self.query_device_list() {
                if let Some(target) = devs.iter().find(|d| d.mac.eq_ignore_ascii_case(target_mac)) {
                    if target.connected {
                        handshake_accepted = true;
                    }
                }
            }
        }

        // 3. Only if the handshake was accepted (anything other than failure):
        // Disconnect the other connected device that isn't this PC so the slot is fully transferred
        if handshake_accepted {
            if let Ok(devs) = self.query_device_list() {
                if let Some(other_dev) = devs.iter().find(|d| {
                    d.connected && !is_this_pc(&d.name) && !d.mac.eq_ignore_ascii_case(target_mac)
                }) {
                    let _ = self.disconnect_device(&other_dev.mac);
                }
            }
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn switch_playback_to_this_pc(&mut self) -> Result<bool, String> {
        let devs = self.query_device_list()?;
        let pc_dev = devs.iter().find(|d| is_this_pc(&d.name));

        if let Some(target) = pc_dev {
            self.switch_playback_to_mac(&target.mac)?;
            Ok(true)
        } else if let Some(first_conn) = devs.iter().find(|d| d.connected && !d.is_playing) {
            self.switch_playback_to_mac(&first_conn.mac)?;
            Ok(true)
        } else {
            Err("could not identify this PC in paired devices list".to_string())
        }
    }
}

impl Drop for SonyRfcomm {
    fn drop(&mut self) {
        unsafe {
            closesocket(self.socket);
            WSACleanup();
        }
    }
}
