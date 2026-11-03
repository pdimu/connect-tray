use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::bluetooth::{discover_sony_device, SonyRfcomm};
use crate::protocol::{AncMode, BatteryInfo, ConnectionMode, EqInfo, EqPreset, PairedDevice};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BatteryTopology {
    Single(BatteryInfo),
    Tws {
        left: BatteryInfo,
        right: BatteryInfo,
        case: Option<BatteryInfo>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DeviceCapabilities {
    pub has_dual_battery: bool,
    pub firmware: Option<String>,
    pub codec: Option<String>,
    pub has_dsee: bool,
    pub dsee_enabled: bool,
    pub has_auto_power_off: bool,
    pub auto_power_off_index: usize,
    pub has_speak_to_chat: bool,
    pub speak_to_chat_enabled: bool,
    pub has_in_ear_detection: bool,
    pub in_ear_detection_enabled: bool,
    pub has_connection_mode: bool,
    pub connection_mode: Option<ConnectionMode>,
    pub has_voice_guidance: bool,
    pub voice_guidance_enabled: bool,
    pub has_touch_sensor: bool,
    pub touch_sensor_enabled: bool,
    pub has_multipoint: bool,
    pub multipoint_enabled: bool,
    pub multipoint_devices: Option<Vec<String>>,
    pub multipoint_paired_devices: Option<Vec<PairedDevice>>,
    pub connecting_mac: Option<String>,
    pub pinned_mac: Option<String>,
    pub paired_headsets: Vec<crate::bluetooth::DiscoveredDevice>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceState {
    pub connected: bool,
    pub connecting: bool,
    pub device_name: String,
    pub device_mac: Option<String>,
    pub protocol_version: Option<String>,
    pub battery: Option<BatteryInfo>,
    pub battery_topology: Option<BatteryTopology>,
    pub anc_mode: Option<AncMode>,
    pub eq_info: Option<EqInfo>,
    pub capabilities: DeviceCapabilities,
}

impl Default for DeviceState {
    fn default() -> Self {
        Self {
            connected: false,
            connecting: true,
            device_name: "Sony Headset".to_string(),
            device_mac: None,
            protocol_version: None,
            battery: None,
            battery_topology: None,
            anc_mode: None,
            eq_info: None,
            capabilities: DeviceCapabilities::default(),
        }
    }
}

#[derive(Debug)]
pub enum WorkerCommand {
    SetAnc(AncMode),
    SetEq(EqPreset),
    SetClearBass(i8),
    SetDsee(bool),
    SetApo(usize),
    SetSpeakToChat(bool),
    SetWearingSensor(bool),
    SetConnectionMode(ConnectionMode),
    SetVoiceGuidance(bool),
    SetTouchSensor(bool),
    SetMultipoint(bool),
    SwitchPlaybackToPc,
    SwitchPlaybackToMac(String),
    ConnectPairedDevice(String),
    DisconnectDevice(String),
    PinFocus(String),
    UnpinFocus(String),
    SwitchHeadset { address: u64, name: String },
    Refresh,
    Quit,
}

#[derive(Clone, Debug)]
pub struct DiagnosticsSnapshot {
    pub uptime_secs: u64,
    pub device_name: String,
    pub device_mac: String,
    pub protocol_version: String,
    pub packets_sent: u64,
    pub packets_recv: u64,
}

pub struct DeviceManager {
    pub state: Arc<Mutex<DeviceState>>,
    cmd_tx: Sender<WorkerCommand>,
    running: Arc<AtomicBool>,
    pub start_time: Instant,
}

impl DeviceManager {
    pub fn start<F>(on_state_updated: F) -> Self
    where
        F: Fn() + Send + 'static,
    {
        let state = Arc::new(Mutex::new(DeviceState::default()));
        let (cmd_tx, cmd_rx) = channel::<WorkerCommand>();
        let running = Arc::new(AtomicBool::new(true));
        let start_time = Instant::now();

        let state_clone = Arc::clone(&state);
        let running_clone = Arc::clone(&running);

        thread::spawn(move || {
            worker_loop(state_clone, cmd_rx, running_clone, on_state_updated);
        });

        Self {
            state,
            cmd_tx,
            running,
            start_time,
        }
    }

    pub fn get_diagnostics(&self) -> DiagnosticsSnapshot {
        let uptime_secs = self.start_time.elapsed().as_secs();
        let s = self.state.lock().unwrap();
        DiagnosticsSnapshot {
            uptime_secs,
            device_name: if s.connected { s.device_name.clone() } else { "disconnected".to_string() },
            device_mac: if s.connected { s.device_mac.clone().unwrap_or_else(|| "none".to_string()) } else { "none".to_string() },
            protocol_version: if s.connected { s.protocol_version.clone().unwrap_or_else(|| "none".to_string()) } else { "none".to_string() },
            packets_sent: crate::bluetooth::PACKETS_SENT.load(Ordering::Relaxed),
            packets_recv: crate::bluetooth::PACKETS_RECV.load(Ordering::Relaxed),
        }
    }

    pub fn set_anc(&self, mode: AncMode) {
        {
            let mut s = self.state.lock().unwrap();
            s.anc_mode = Some(mode);
        }
        let _ = self.cmd_tx.send(WorkerCommand::SetAnc(mode));
    }

    pub fn set_eq(&self, preset: EqPreset) {
        {
            let mut s = self.state.lock().unwrap();
            if let Some(ref mut eq) = s.eq_info {
                eq.preset = preset;
            }
        }
        let _ = self.cmd_tx.send(WorkerCommand::SetEq(preset));
    }

    pub fn set_clear_bass(&self, bass: i8) {
        {
            let mut s = self.state.lock().unwrap();
            if let Some(ref mut eq) = s.eq_info {
                eq.clear_bass = bass;
            }
        }
        let _ = self.cmd_tx.send(WorkerCommand::SetClearBass(bass));
    }

    pub fn set_dsee(&self, enabled: bool) {
        {
            let mut s = self.state.lock().unwrap();
            s.capabilities.dsee_enabled = enabled;
        }
        let _ = self.cmd_tx.send(WorkerCommand::SetDsee(enabled));
    }

    pub fn set_apo(&self, index: usize) {
        {
            let mut s = self.state.lock().unwrap();
            s.capabilities.auto_power_off_index = index;
        }
        let _ = self.cmd_tx.send(WorkerCommand::SetApo(index));
    }

    pub fn set_speak_to_chat(&self, enabled: bool) {
        {
            let mut s = self.state.lock().unwrap();
            s.capabilities.speak_to_chat_enabled = enabled;
        }
        let _ = self.cmd_tx.send(WorkerCommand::SetSpeakToChat(enabled));
    }

    pub fn set_wearing_sensor(&self, enabled: bool) {
        {
            let mut s = self.state.lock().unwrap();
            s.capabilities.in_ear_detection_enabled = enabled;
        }
        let _ = self.cmd_tx.send(WorkerCommand::SetWearingSensor(enabled));
    }

    pub fn set_connection_mode(&self, mode: ConnectionMode) {
        {
            let mut s = self.state.lock().unwrap();
            s.capabilities.connection_mode = Some(mode);
        }
        let _ = self.cmd_tx.send(WorkerCommand::SetConnectionMode(mode));
    }

    pub fn set_voice_guidance(&self, enabled: bool) {
        {
            let mut s = self.state.lock().unwrap();
            s.capabilities.voice_guidance_enabled = enabled;
        }
        let _ = self.cmd_tx.send(WorkerCommand::SetVoiceGuidance(enabled));
    }

    pub fn set_touch_sensor(&self, enabled: bool) {
        {
            let mut s = self.state.lock().unwrap();
            s.capabilities.touch_sensor_enabled = enabled;
        }
        let _ = self.cmd_tx.send(WorkerCommand::SetTouchSensor(enabled));
    }

    pub fn set_multipoint(&self, enabled: bool) {
        {
            let mut s = self.state.lock().unwrap();
            s.capabilities.multipoint_enabled = enabled;
        }
        let _ = self.cmd_tx.send(WorkerCommand::SetMultipoint(enabled));
    }

    pub fn switch_playback_to_pc(&self) {
        let pc_mac = {
            let mut s = self.state.lock().unwrap();
            let mut found_mac = None;
            if let Some(ref mut devs) = s.capabilities.multipoint_paired_devices {
                for d in devs.iter_mut() {
                    let is_pc = crate::bluetooth::is_this_pc(&d.name);
                    d.is_playing = is_pc;
                    if is_pc {
                        found_mac = Some(d.mac.clone());
                    }
                }
            }
            found_mac
        };
        if let Some(mac) = pc_mac {
            let _ = self.cmd_tx.send(WorkerCommand::SwitchPlaybackToMac(mac));
        } else {
            let _ = self.cmd_tx.send(WorkerCommand::SwitchPlaybackToPc);
        }
    }

    pub fn switch_playback_to_mac(&self, mac: String) {
        {
            let mut s = self.state.lock().unwrap();
            if let Some(ref mut devs) = s.capabilities.multipoint_paired_devices {
                for d in devs.iter_mut() {
                    d.is_playing = d.mac == mac;
                }
            }
        }
        let _ = self.cmd_tx.send(WorkerCommand::SwitchPlaybackToMac(mac));
    }

    pub fn connect_device(&self, mac: String) {
        {
            let mut s = self.state.lock().unwrap();
            s.capabilities.connecting_mac = Some(mac.clone());
        }
        let _ = self.cmd_tx.send(WorkerCommand::ConnectPairedDevice(mac));
    }

    pub fn disconnect_device(&self, mac: String) {
        let _ = self.cmd_tx.send(WorkerCommand::DisconnectDevice(mac));
    }

    pub fn pin_focus(&self, mac: String) {
        {
            let mut s = self.state.lock().unwrap();
            s.capabilities.pinned_mac = Some(mac.clone());
        }
        let _ = self.cmd_tx.send(WorkerCommand::PinFocus(mac));
    }

    pub fn unpin_focus(&self, mac: String) {
        {
            let mut s = self.state.lock().unwrap();
            if s.capabilities.pinned_mac.as_deref() == Some(&mac) {
                s.capabilities.pinned_mac = None;
            }
        }
        let _ = self.cmd_tx.send(WorkerCommand::UnpinFocus(mac));
    }

    pub fn switch_headset(&self, address: u64, name: String) {
        {
            let mut s = self.state.lock().unwrap();
            s.connecting = true;
            s.device_name = name.clone();
        }
        let _ = self.cmd_tx.send(WorkerCommand::SwitchHeadset { address, name });
    }

    pub fn refresh(&self) {
        let _ = self.cmd_tx.send(WorkerCommand::Refresh);
    }

    pub fn shutdown(&self) {
        self.running.store(false, Ordering::SeqCst);
        let _ = self.cmd_tx.send(WorkerCommand::Quit);
    }
}

fn poll_telemetry(c: &mut SonyRfcomm, shared_state: &Arc<Mutex<DeviceState>>) -> bool {
    let is_tws = {
        let s = shared_state.lock().unwrap();
        s.capabilities.has_dual_battery
    };

    let bat_ok = if is_tws {
        if let Ok((left, right)) = c.query_tws_battery() {
            let case = c.query_case_battery().ok();
            let mut s = shared_state.lock().unwrap();
            s.battery = Some(left);
            s.battery_topology = Some(BatteryTopology::Tws { left, right, case });
            true
        } else {
            false
        }
    } else {
        if let Ok(single) = c.query_battery() {
            let mut s = shared_state.lock().unwrap();
            s.battery = Some(single);
            s.battery_topology = Some(BatteryTopology::Single(single));
            true
        } else {
            false
        }
    };

    let anc_res = c.query_anc();
    if let Ok(a) = anc_res {
        let mut s = shared_state.lock().unwrap();
        s.anc_mode = Some(a);
    }

    let eq_res = c.query_eq();
    if let Ok(eq) = eq_res {
        let mut s = shared_state.lock().unwrap();
        s.eq_info = Some(eq);
    }

    if let Ok(devs) = c.query_device_list() {
        let mut s = shared_state.lock().unwrap();
        s.capabilities.multipoint_paired_devices = Some(devs);
    }

    bat_ok || anc_res.is_ok()
}

fn probe_and_update(c: &mut SonyRfcomm, name: String, addr: u64, shared_state: &Arc<Mutex<DeviceState>>) {
    let _ = c.init_handshake();
    let mut caps = DeviceCapabilities::default();

    let mut bat_top: Option<BatteryTopology> = None;
    let mut single_bat: Option<BatteryInfo> = None;

    if let Ok((left, right)) = c.query_tws_battery() {
        caps.has_dual_battery = true;
        let case = c.query_case_battery().ok();
        single_bat = Some(left);
        bat_top = Some(BatteryTopology::Tws { left, right, case });
    } else if let Ok(single) = c.query_battery() {
        caps.has_dual_battery = false;
        single_bat = Some(single);
        bat_top = Some(BatteryTopology::Single(single));
    }

    let anc_mode = c.query_anc().ok();
    let eq_info = c.query_eq().ok();

    if let Ok(fw) = c.query_firmware() {
        caps.firmware = Some(fw);
    }
    if let Ok(codec) = c.query_codec() {
        caps.codec = Some(codec.to_string());
    }
    if let Ok(dsee) = c.query_dsee() {
        caps.has_dsee = true;
        caps.dsee_enabled = dsee;
    }
    if let Ok(apo) = c.query_apo() {
        caps.has_auto_power_off = true;
        caps.auto_power_off_index = apo;
    }
    if let Ok(stc) = c.query_speak_to_chat() {
        caps.has_speak_to_chat = true;
        caps.speak_to_chat_enabled = stc;
    }
    if let Ok(ws) = c.query_wearing_sensor() {
        caps.has_in_ear_detection = true;
        caps.in_ear_detection_enabled = ws;
    }

    // WH-CH720N and standard Sony MDR headsets support Connection Mode, Multipoint, and Voice Guidance
    caps.has_connection_mode = true;
    caps.connection_mode = Some(ConnectionMode::PriorityOnSoundQuality);
    if let Ok(cm) = c.query_connection_mode() {
        caps.connection_mode = Some(cm);
    }

    caps.has_voice_guidance = true;
    caps.voice_guidance_enabled = true;
    if let Ok(vg) = c.query_voice_guidance() {
        caps.voice_guidance_enabled = vg;
    }

    if let Ok(ts) = c.query_touch_sensor() {
        caps.has_touch_sensor = true;
        caps.touch_sensor_enabled = ts;
    }

    caps.has_multipoint = true;
    caps.multipoint_enabled = true;
    if let Ok(mp) = c.query_multipoint() {
        caps.multipoint_enabled = mp;
    }

    if let Ok(devs) = c.query_device_list() {
        caps.multipoint_paired_devices = Some(devs);
    }

    caps.paired_headsets = crate::bluetooth::find_all_sony_devices();

    let mac = crate::bluetooth::format_mac_addr(addr);
    let proto = match c.protocol_version {
        crate::protocol::SonyProtocolVersion::V2 => "Sony MDR V2".to_string(),
        crate::protocol::SonyProtocolVersion::V1 => "Sony MDR V1".to_string(),
    };

    {
        let mut s = shared_state.lock().unwrap();
        s.connected = true;
        s.connecting = false;
        s.device_name = name;
        s.device_mac = Some(mac);
        s.protocol_version = Some(proto);
        s.battery = single_bat;
        s.battery_topology = bat_top;
        s.anc_mode = anc_mode;
        s.eq_info = eq_info;
        s.capabilities = caps;
    }
}

fn worker_loop<F>(
    shared_state: Arc<Mutex<DeviceState>>,
    cmd_rx: Receiver<WorkerCommand>,
    running: Arc<AtomicBool>,
    notify_update: F,
) where
    F: Fn(),
{
    let mut client: Option<SonyRfcomm> = None;
    let mut target_device: Option<(u64, String)> = None;
    let mut last_poll = Instant::now() - Duration::from_secs(60);
    let poll_interval = Duration::from_secs(30);

    while running.load(Ordering::SeqCst) {
        // 1. Ensure connected (with 3s backoff between reconnect attempts)
        if client.is_none() && last_poll.elapsed() >= Duration::from_secs(3) {
            {
                let mut s = shared_state.lock().unwrap();
                if !s.connecting {
                    s.connecting = true;
                    drop(s);
                    notify_update();
                }
            }
            let maybe_dev = if let Some(ref t) = target_device {
                Some(t.clone())
            } else {
                discover_sony_device()
            };

            let (addr, name) = match maybe_dev {
                Some(pair) => pair,
                None => {
                    let mut s = shared_state.lock().unwrap();
                    let was_connecting = s.connecting;
                    let was_connected = s.connected;
                    s.connected = false;
                    s.connecting = false;
                    s.device_name = "searching...".to_string();
                    s.capabilities.paired_headsets = crate::bluetooth::find_all_sony_devices();
                    drop(s);
                    if was_connecting || was_connected {
                        notify_update();
                    }
                    std::thread::sleep(Duration::from_millis(1500));
                    continue;
                }
            };

            match SonyRfcomm::connect(addr) {
                Ok(mut c) => {
                    probe_and_update(&mut c, name, addr, &shared_state);
                    notify_update();
                    client = Some(c);
                    last_poll = Instant::now();
                }
                Err(_) => {
                    let mut s = shared_state.lock().unwrap();
                    let was_connecting = s.connecting;
                    let was_connected = s.connected;
                    s.connected = false;
                    s.connecting = false;
                    s.capabilities.paired_headsets = crate::bluetooth::find_all_sony_devices();
                    drop(s);
                    if was_connecting || was_connected {
                        notify_update();
                    }
                    last_poll = Instant::now();
                }
            }
        }

        // 2. Fast sub-second socket heartbeat & disconnect detection
        if let Some(ref mut c) = client {
            if !c.is_alive() {
                client = None;
                let mut s = shared_state.lock().unwrap();
                s.connected = false;
                s.connecting = false;
                s.capabilities.paired_headsets = crate::bluetooth::find_all_sony_devices();
                drop(s);
                notify_update();
                last_poll = Instant::now();
            } else {
                // Drain any unsolicited asynchronous notifications from the headset
                let mut changed = false;
                while let Some(pkt) = c.try_recv_async_packet() {
                    if !pkt.payload.is_empty() {
                        let op = pkt.payload[0];
                        // 0x67 / 0x69: NC/ASM response or notification (e.g. physical ANC button press)
                        if op == crate::protocol::CMD_NCASM_RET || op == crate::protocol::PERI_NTFY_PARAM {
                            if let Some(anc) = crate::protocol::parse_anc_response(&pkt.payload) {
                                let mut s = shared_state.lock().unwrap();
                                s.anc_mode = Some(anc);
                                changed = true;
                            }
                        }
                        // 0x23 / 0x25: Battery response or notification
                        if op == crate::protocol::CMD_BATTERY_RET || op == crate::protocol::CMD_BATTERY_NTFY {
                            if let Some(bat) = crate::protocol::parse_battery_response(&pkt.payload) {
                                let mut s = shared_state.lock().unwrap();
                                s.battery = Some(bat);
                                s.battery_topology = Some(BatteryTopology::Single(bat));
                                changed = true;
                            }
                        }
                    }
                }
                if changed {
                    notify_update();
                }
            }
        }

        // 3. Poll periodically if connected
        if client.is_some() && last_poll.elapsed() >= poll_interval {
            if let Some(ref mut c) = client {
                if !poll_telemetry(c, &shared_state) {
                    client = None;
                    let mut s = shared_state.lock().unwrap();
                    s.connected = false;
                    s.connecting = false;
                    drop(s);
                }
                notify_update();
            }
            last_poll = Instant::now();
        }

        // 3. Process commands with tight 80ms timeout for instant responsiveness
        match cmd_rx.recv_timeout(Duration::from_millis(80)) {
            Ok(WorkerCommand::SetAnc(mode)) => {
                if let Some(ref mut c) = client {
                    if c.set_anc(mode).is_ok() {
                        let mut s = shared_state.lock().unwrap();
                        s.anc_mode = Some(mode);
                        drop(s);
                        notify_update();
                    }
                }
            }
            Ok(WorkerCommand::SetEq(preset)) => {
                if let Some(ref mut c) = client {
                    if c.set_eq(preset).is_ok() {
                        let mut s = shared_state.lock().unwrap();
                        if let Some(ref mut eq) = s.eq_info {
                            eq.preset = preset;
                        }
                        drop(s);
                        notify_update();
                    }
                }
            }
            Ok(WorkerCommand::SetClearBass(bass)) => {
                if let Some(ref mut c) = client {
                    let (preset, bands) = {
                        let s = shared_state.lock().unwrap();
                        s.eq_info
                            .as_ref()
                            .map(|e| (e.preset, e.bands))
                            .unwrap_or((EqPreset::Manual, [0; 5]))
                    };
                    if c.set_clear_bass(bass, preset, bands).is_ok() {
                        let mut s = shared_state.lock().unwrap();
                        if let Some(ref mut eq) = s.eq_info {
                            eq.clear_bass = bass;
                            eq.preset = EqPreset::Manual;
                        } else {
                            s.eq_info = Some(EqInfo {
                                preset: EqPreset::Manual,
                                clear_bass: bass,
                                bands,
                            });
                        }
                        drop(s);
                        notify_update();
                    }
                }
            }
            Ok(WorkerCommand::SetDsee(enabled)) => {
                if let Some(ref mut c) = client {
                    if c.set_dsee(enabled).is_ok() {
                        let mut s = shared_state.lock().unwrap();
                        s.capabilities.dsee_enabled = enabled;
                        drop(s);
                        notify_update();
                    }
                }
            }
            Ok(WorkerCommand::SetApo(index)) => {
                if let Some(ref mut c) = client {
                    if c.set_apo(index).is_ok() {
                        let mut s = shared_state.lock().unwrap();
                        s.capabilities.auto_power_off_index = index;
                        drop(s);
                        notify_update();
                    }
                }
            }
            Ok(WorkerCommand::SetSpeakToChat(enabled)) => {
                if let Some(ref mut c) = client {
                    if c.set_speak_to_chat(enabled).is_ok() {
                        let mut s = shared_state.lock().unwrap();
                        s.capabilities.speak_to_chat_enabled = enabled;
                        drop(s);
                        notify_update();
                    }
                }
            }
            Ok(WorkerCommand::SetWearingSensor(enabled)) => {
                if let Some(ref mut c) = client {
                    if c.set_wearing_sensor(enabled).is_ok() {
                        let mut s = shared_state.lock().unwrap();
                        s.capabilities.in_ear_detection_enabled = enabled;
                        drop(s);
                        notify_update();
                    }
                }
            }
            Ok(WorkerCommand::SetConnectionMode(mode)) => {
                if let Some(ref mut c) = client {
                    if c.set_connection_mode(mode).is_ok() {
                        let mut s = shared_state.lock().unwrap();
                        s.capabilities.connection_mode = Some(mode);
                        drop(s);
                        notify_update();
                    }
                }
            }
            Ok(WorkerCommand::SetVoiceGuidance(enabled)) => {
                if let Some(ref mut c) = client {
                    if c.set_voice_guidance(enabled).is_ok() {
                        let mut s = shared_state.lock().unwrap();
                        s.capabilities.voice_guidance_enabled = enabled;
                        drop(s);
                        notify_update();
                    }
                }
            }
            Ok(WorkerCommand::SetTouchSensor(enabled)) => {
                if let Some(ref mut c) = client {
                    if c.set_touch_sensor(enabled).is_ok() {
                        let mut s = shared_state.lock().unwrap();
                        s.capabilities.touch_sensor_enabled = enabled;
                        drop(s);
                        notify_update();
                    }
                }
            }
            Ok(WorkerCommand::SetMultipoint(enabled)) => {
                if let Some(ref mut c) = client {
                    if c.set_multipoint(enabled).is_ok() {
                        let mut s = shared_state.lock().unwrap();
                        s.capabilities.multipoint_enabled = enabled;
                        drop(s);
                        notify_update();
                    }
                }
            }
            Ok(WorkerCommand::SwitchPlaybackToPc) => {
                if let Some(ref mut c) = client {
                    let _ = c.switch_playback_to_this_pc();
                    if let Ok(devs) = c.query_device_list() {
                        let mut s = shared_state.lock().unwrap();
                        s.capabilities.multipoint_paired_devices = Some(devs);
                    }
                }
                notify_update();
            }
            Ok(WorkerCommand::SwitchPlaybackToMac(mac)) => {
                if let Some(ref mut c) = client {
                    let _ = c.switch_playback_to_mac(&mac);
                    if let Ok(devs) = c.query_device_list() {
                        let mut s = shared_state.lock().unwrap();
                        s.capabilities.multipoint_paired_devices = Some(devs);
                    }
                }
                notify_update();
            }
            Ok(WorkerCommand::ConnectPairedDevice(mac)) => {
                notify_update();
                if let Some(ref mut c) = client {
                    let _ = c.connect_paired_device(&mac);
                    std::thread::sleep(std::time::Duration::from_millis(1200));
                    if let Ok(devs) = c.query_device_list() {
                        let mut s = shared_state.lock().unwrap();
                        s.capabilities.multipoint_paired_devices = Some(devs);
                    }
                }
                {
                    let mut s = shared_state.lock().unwrap();
                    s.capabilities.connecting_mac = None;
                }
                notify_update();
            }
            Ok(WorkerCommand::DisconnectDevice(mac)) => {
                if let Some(ref mut c) = client {
                    let _ = c.disconnect_device(&mac);
                    std::thread::sleep(std::time::Duration::from_millis(600));
                    if let Ok(devs) = c.query_device_list() {
                        let mut s = shared_state.lock().unwrap();
                        if s.capabilities.pinned_mac.as_deref() == Some(&mac) {
                            s.capabilities.pinned_mac = None;
                        }
                        s.capabilities.multipoint_paired_devices = Some(devs);
                    }
                }
                notify_update();
            }
            Ok(WorkerCommand::PinFocus(mac)) => {
                if let Some(ref mut c) = client {
                    let _ = c.pin_focus(&mac);
                    std::thread::sleep(std::time::Duration::from_millis(300));
                    if let Ok(devs) = c.query_device_list() {
                        let mut s = shared_state.lock().unwrap();
                        s.capabilities.pinned_mac = Some(mac);
                        s.capabilities.multipoint_paired_devices = Some(devs);
                    }
                }
                notify_update();
            }
            Ok(WorkerCommand::UnpinFocus(mac)) => {
                if let Some(ref mut c) = client {
                    let _ = c.release_fixed_focus();
                    std::thread::sleep(std::time::Duration::from_millis(300));
                    if let Ok(devs) = c.query_device_list() {
                        let mut s = shared_state.lock().unwrap();
                        if s.capabilities.pinned_mac.as_deref() == Some(&mac) {
                            s.capabilities.pinned_mac = None;
                        }
                        s.capabilities.multipoint_paired_devices = Some(devs);
                    }
                }
                notify_update();
            }
            Ok(WorkerCommand::SwitchHeadset { address, name }) => {
                client = None;
                target_device = Some((address, name.clone()));
                let paired = crate::bluetooth::find_all_sony_devices();
                {
                    let mut s = shared_state.lock().unwrap();
                    s.connected = false;
                    s.connecting = true;
                    s.device_name = name;
                    s.battery = None;
                    s.battery_topology = None;
                    s.anc_mode = None;
                    s.eq_info = None;
                    s.capabilities = DeviceCapabilities::default();
                    s.capabilities.paired_headsets = paired;
                }
                notify_update();
                last_poll = Instant::now() - Duration::from_secs(10);
            }
            Ok(WorkerCommand::Refresh) => {
                // Immediate, active in-line query with zero waiting
                if let Some(ref mut c) = client {
                    if poll_telemetry(c, &shared_state) {
                        notify_update();
                        last_poll = Instant::now();
                    } else {
                        // Socket broken or device shut down
                        client = None;
                        let mut s = shared_state.lock().unwrap();
                        s.connected = false;
                        s.connecting = false;
                        drop(s);
                        notify_update();
                    }
                } else {
                    // Force immediate reconnect attempt right now
                    {
                        let mut s = shared_state.lock().unwrap();
                        s.connecting = true;
                        drop(s);
                        notify_update();
                    }
                    let maybe_dev = if let Some(ref t) = target_device {
                        Some(t.clone())
                    } else {
                        discover_sony_device()
                    };
                    if let Some((addr, name)) = maybe_dev {
                        match SonyRfcomm::connect(addr) {
                            Ok(mut c) => {
                                probe_and_update(&mut c, name, addr, &shared_state);
                                notify_update();
                                client = Some(c);
                                last_poll = Instant::now();
                            }
                            Err(_) => {
                                let mut s = shared_state.lock().unwrap();
                                s.connected = false;
                                s.connecting = false;
                                drop(s);
                                notify_update();
                                last_poll = Instant::now();
                            }
                        }
                    } else {
                        let mut s = shared_state.lock().unwrap();
                        s.connected = false;
                        s.connecting = false;
                        s.device_name = "searching...".to_string();
                        drop(s);
                        notify_update();
                        last_poll = Instant::now();
                    }
                }
            }
            Ok(WorkerCommand::Quit) => {
                break;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                break;
            }
        }
    }
}
