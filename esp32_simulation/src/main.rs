// =========================================================
// ELECTRONIC NOSE - RUST SIMULATION FIRMWARE (ESP32-S3)
// ✅ Dual Connectivity: USB Serial UART + ThingsBoard Cloud (WiFi / MQTT)
// ✅ Remote RPC Control (START / STOP from ThingsBoard Dashboard)
// ✅ Simulated 8 Gas Sensors (MQ & TGS in mV with realistic 3-phase curves)
// ✅ Simulated Single DHT22 Environment (Temp & Humidity)
// ✅ Simulated & Physical Motor Drivers (3 FASE: OFF 70s -> INLET 100s -> OUTLET 130s)
// ✅ NO PHYSICAL SENSORS REQUIRED!
// =========================================================

use esp_idf_hal::{
    delay::FreeRtos,
    gpio::*,
    peripherals::Peripherals,
};
use esp_idf_svc::{
    eventloop::EspSystemEventLoop,
    mqtt::client::{EspMqttClient, EventPayload, MqttClientConfiguration, QoS},
    nvs::EspDefaultNvsPartition,
    systime::EspSystemTime,
    wifi::{AuthMethod, BlockingWifi, ClientConfiguration, Configuration, EspWifi},
};

use std::sync::{Arc, Mutex};

// ─── KONFIGURASI KREDENSIAL: WIFI & THINGSBOARD ──────────
pub const WIFI_SSID: &str = "hypomone";
pub const WIFI_PASS: &str = "jancok1234";

// Kredensial ThingsBoard Cloud
pub const TB_HOST: &str = "thingsboard.cloud";
pub const TB_PORT: u16 = 1883;
pub const TB_TOKEN: &str = "sIP5Dc6dl1GLn4Enxr9Q";

// ThingsBoard MQTT Topics
pub const TB_TELEMETRY_TOPIC: &str = "v1/devices/me/telemetry";
pub const TB_RPC_SUB_TOPIC: &str = "v1/devices/me/rpc/request/+";
pub const TB_RPC_SUB_TOPIC_PREFIX: &str = "v1/devices/me/rpc/request/";
pub const TB_RPC_RESP_TOPIC_PREFIX: &str = "v1/devices/me/rpc/response/";

// ─── Sensor data ─────────────────────────────────────────
#[derive(Clone, Default)]
pub struct SensorData {
    pub mq6: f32,
    pub mq135: f32,
    pub mq3: f32,
    pub mq7: f32,
    pub tgs2611: f32,
    pub tgs2602: f32,
    pub tgs2600: f32,
    pub tgs2620: f32,
    pub dht_temp: f32,
    pub dht_hum: f32,
}

// ─── System state ─────────────────────────────────────────
pub struct SystemState {
    pub running: bool,
    pub driver1_active: bool, // Inlet  (Driver A)
    pub driver2_active: bool, // Outlet (Driver B)
    pub cycle_start_ms: u64,
    pub last_sensor_read_ms: u64,
    pub sensors: SensorData,
}

impl SystemState {
    pub fn new() -> Self {
        Self {
            running: false,
            driver1_active: false,
            driver2_active: false,
            cycle_start_ms: 0,
            last_sensor_read_ms: 0,
            sensors: SensorData::default(),
        }
    }
}

// ─── Timing helpers ───────────────────────────────────────
pub fn millis() -> u64 {
    EspSystemTime {}.now().as_millis() as u64
}

pub fn get_elapsed_time(state: &SystemState) -> u64 {
    if !state.running {
        return 0;
    }
    let elapsed_ms = millis().saturating_sub(state.cycle_start_ms);
    let elapsed_sec = elapsed_ms / 1000;
    if elapsed_sec == 0 { 1 } else { elapsed_sec }
}

pub fn get_inlet_time(state: &SystemState) -> u64 {
    let elapsed = get_elapsed_time(state);
    if elapsed >= 71 && elapsed <= 170 {
        elapsed - 70
    } else if elapsed > 170 {
        100
    } else {
        0
    }
}

pub fn get_outlet_time(state: &SystemState) -> u64 {
    let elapsed = get_elapsed_time(state);
    if elapsed >= 171 && elapsed <= 300 {
        elapsed - 170
    } else if elapsed > 300 {
        130
    } else {
        0
    }
}

// ─── Pseudo-Random Noise Generator (No extra crate needed) ─
fn next_rand(seed: &mut u32) -> f32 {
    *seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
    ((*seed >> 16) & 0x7FFF) as f32 / 32767.0 // 0.0 .. 1.0
}

fn rand_range(seed: &mut u32, min_val: f32, max_val: f32) -> f32 {
    min_val + (max_val - min_val) * next_rand(seed)
}

// ─── Realistic Sensor Curve Simulation ───────────────────
// Calculates realistic response curve depending on current phase:
// - Idle (not running or 1..70s): baseline + small noise
// - Inlet (71..170s): exponential rise towards peak + noise
// - Outlet (171..300s): exponential decay back to baseline + noise
fn simulate_sensors(state: &mut SystemState, seed: &mut u32) {
    let elapsed = get_elapsed_time(state);

    // Baseline voltages (mV)
    const BASE_MQ6: f32     = 460.0;
    const BASE_MQ135: f32   = 530.0;
    const BASE_MQ3: f32     = 390.0;
    const BASE_MQ7: f32     = 610.0;
    const BASE_TGS2611: f32 = 430.0;
    const BASE_TGS2602: f32 = 560.0;
    const BASE_TGS2600: f32 = 400.0;
    const BASE_TGS2620: f32 = 490.0;

    // Peak voltages under sample gas exposure (mV)
    const PEAK_MQ6: f32     = 1850.0;
    const PEAK_MQ135: f32   = 2400.0;
    const PEAK_MQ3: f32     = 2950.0;
    const PEAK_MQ7: f32     = 1780.0;
    const PEAK_TGS2611: f32 = 2380.0;
    const PEAK_TGS2602: f32 = 2820.0;
    const PEAK_TGS2600: f32 = 2120.0;
    const PEAK_TGS2620: f32 = 2640.0;

    // Response factor (0.0 = baseline, 1.0 = full peak)
    let factor = if !state.running || elapsed <= 70 {
        // Fase 1 / Standby: Baseline
        0.0f32
    } else if elapsed <= 170 {
        // Fase 2: Inlet ON (Gas Hisap) - Kurva naik eksponensial
        let progress = (elapsed - 70) as f32 / 100.0;
        1.0 - (-2.6 * progress).exp()
    } else if elapsed <= 300 {
        // Fase 3: Outlet ON (Pembersihan Chamber) - Kurva turun eksponensial
        let progress = (elapsed - 170) as f32 / 130.0;
        (-3.2 * progress).exp()
    } else {
        0.0f32
    };

    let noise = |s: &mut u32| rand_range(s, -12.0, 12.0);

    state.sensors.mq6     = (BASE_MQ6     + (PEAK_MQ6     - BASE_MQ6)     * factor + noise(seed)).max(50.0);
    state.sensors.mq135   = (BASE_MQ135   + (PEAK_MQ135   - BASE_MQ135)   * factor + noise(seed)).max(50.0);
    state.sensors.mq3     = (BASE_MQ3     + (PEAK_MQ3     - BASE_MQ3)     * factor + noise(seed)).max(50.0);
    state.sensors.mq7     = (BASE_MQ7     + (PEAK_MQ7     - BASE_MQ7)     * factor + noise(seed)).max(50.0);
    state.sensors.tgs2611 = (BASE_TGS2611 + (PEAK_TGS2611 - BASE_TGS2611) * factor + noise(seed)).max(50.0);
    state.sensors.tgs2602 = (BASE_TGS2602 + (PEAK_TGS2602 - BASE_TGS2602) * factor + noise(seed)).max(50.0);
    state.sensors.tgs2600 = (BASE_TGS2600 + (PEAK_TGS2600 - BASE_TGS2600) * factor + noise(seed)).max(50.0);
    state.sensors.tgs2620 = (BASE_TGS2620 + (PEAK_TGS2620 - BASE_TGS2620) * factor + noise(seed)).max(50.0);

    // DHT22 Environment Simulation: Suhu ~27.5°C, Kelembaban ~62%
    state.sensors.dht_temp = 27.5 + rand_range(seed, -0.2, 0.2);
    state.sensors.dht_hum  = 62.0 + rand_range(seed, -0.8, 0.8);
}

// ─── JSON output (via UART0 / stdout) ────────────────────
fn send_json(state: &SystemState) {
    let s = &state.sensors;
    println!(
        "{{\"sensors\":{{\"mq6\":{:.2},\"mq135\":{:.2},\"mq3\":{:.2},\"mq7\":{:.2},\
         \"tgs2611\":{:.2},\"tgs2602\":{:.2},\"tgs2600\":{:.2},\"tgs2620\":{:.2},\
         \"dht_temp\":{:.2},\"dht_hum\":{:.2}}},\
         \"status\":{{\"inlet_pump\":{},\"inlet_valve\":{},\
         \"outlet_pump\":{},\"outlet_valve\":{},\
         \"inlet_time\":{},\"outlet_time\":{},\"elapsed_time\":{}}},\
         \"timestamp\":{}}}",
        s.mq6, s.mq135, s.mq3, s.mq7,
        s.tgs2611, s.tgs2602, s.tgs2600, s.tgs2620,
        s.dht_temp, s.dht_hum,
        state.driver1_active, state.driver1_active,
        state.driver2_active, state.driver2_active,
        get_inlet_time(state),
        get_outlet_time(state),
        get_elapsed_time(state),
        millis(),
    );
}

// ─── ThingsBoard Telemetry ───────────────────────────────
fn send_thingsboard_telemetry(state: &SystemState, mqtt: &mut Option<EspMqttClient>) {
    if let Some(client) = mqtt {
        let s = &state.sensors;
        let payload = format!(
            "{{\"mq6\":{:.2},\"mq135\":{:.2},\"mq3\":{:.2},\"mq7\":{:.2},\
             \"tgs2611\":{:.2},\"tgs2602\":{:.2},\"tgs2600\":{:.2},\"tgs2620\":{:.2},\
             \"dht_temp\":{:.2},\"dht_hum\":{:.2},\
             \"inlet_pump\":{},\"inlet_valve\":{},\
             \"outlet_pump\":{},\"outlet_valve\":{},\
             \"inlet_time\":{},\"outlet_time\":{},\"elapsed_time\":{},\
             \"running\":{}}}",
            s.mq6, s.mq135, s.mq3, s.mq7,
            s.tgs2611, s.tgs2602, s.tgs2600, s.tgs2620,
            s.dht_temp, s.dht_hum,
            state.driver1_active, state.driver1_active,
            state.driver2_active, state.driver2_active,
            get_inlet_time(state),
            get_outlet_time(state),
            get_elapsed_time(state),
            state.running,
        );
        let res = client.publish(
            TB_TELEMETRY_TOPIC,
            QoS::AtLeastOnce,
            false,
            payload.as_bytes(),
        );
        match res {
            Ok(msg_id) => println!(">>> [ThingsBoard] Telemetry sent! (ID: {})", msg_id),
            Err(e) => println!(">>> [ThingsBoard] Gagal kirim telemetry: {:?}", e),
        }
    }
}

// ─── Motor driver helpers ─────────────────────────────
macro_rules! driver_start {
    ($in1:expr, $in2:expr, $in3:expr, $in4:expr) => {{
        $in1.set_high().ok();
        $in2.set_low().ok();
        $in3.set_high().ok();
        $in4.set_low().ok();
    }};
}

macro_rules! driver_stop {
    ($in1:expr, $in2:expr, $in3:expr, $in4:expr) => {{
        $in1.set_low().ok();
        $in2.set_low().ok();
        $in3.set_low().ok();
        $in4.set_low().ok();
    }};
}

// ─── Wi-Fi Initializer ───────────────────────────────────
fn init_wifi<'d>(
    modem: esp_idf_hal::modem::Modem<'d>,
    sys_loop: EspSystemEventLoop,
    nvs: EspDefaultNvsPartition,
) -> Result<Option<BlockingWifi<EspWifi<'d>>>, anyhow::Error> {
    if WIFI_SSID == "YOUR_WIFI_SSID" || WIFI_SSID.is_empty() {
        println!(">>> [WiFi] Placeholder SSID detected ('{}').", WIFI_SSID);
        println!(">>> [WiFi] Mode offline (Serial UART Only).");
        return Ok(None);
    }

    println!(">>> [WiFi] Menghubungkan ke SSID: '{}'...", WIFI_SSID);
    let esp_wifi = EspWifi::new(modem, sys_loop.clone(), Some(nvs))?;
    let mut wifi = BlockingWifi::wrap(esp_wifi, sys_loop)?;

    let ssid_h = WIFI_SSID.try_into().map_err(|_| anyhow::anyhow!("SSID max 32 bytes"))?;
    let pass_h = WIFI_PASS.try_into().map_err(|_| anyhow::anyhow!("Password max 64 bytes"))?;

    wifi.set_configuration(&Configuration::Client(ClientConfiguration {
        ssid: ssid_h,
        password: pass_h,
        auth_method: AuthMethod::None,
        ..Default::default()
    }))?;

    wifi.start()?;
    println!(">>> [WiFi] Driver started. Mencoba koneksi...");

    let mut connected = false;
    for attempt in 1..=3 {
        println!(">>> [WiFi] Mencoba koneksi ke '{}' (Percobaan {}/3)...", WIFI_SSID, attempt);
        match wifi.connect() {
            Ok(_) => {
                println!(">>> [WiFi] Terhubung ke AP! Menunggu IP DHCP...");
                match wifi.wait_netif_up() {
                    Ok(_) => {
                        let ip_info = wifi.wifi().sta_netif().get_ip_info()?;
                        println!(">>> [WiFi] ✅ Berhasil mendapatkan IP: {}", ip_info.ip);
                        connected = true;
                        break;
                    }
                    Err(e) => {
                        println!(">>> [WiFi] Timeout menunggu IP DHCP: {:?}", e);
                    }
                }
            }
            Err(e) => {
                println!(">>> [WiFi] Percobaan {} gagal (error: {:?})", attempt, e);
            }
        }
        FreeRtos::delay_ms(1500);
    }

    if connected {
        Ok(Some(wifi))
    } else {
        println!(">>> [WiFi] Gagal terhubung setelah 3 kali percobaan.");
        println!(">>> [WiFi] Berjalan dalam mode offline serial UART.");
        Ok(None)
    }
}

// ─── ThingsBoard MQTT Initializer ─────────────────────────
fn init_mqtt(
    rpc_cmd_queue: Arc<Mutex<Vec<String>>>,
    latest_rpc_id: Arc<Mutex<Option<String>>>,
) -> Result<Option<EspMqttClient<'static>>, anyhow::Error> {
    if TB_TOKEN == "YOUR_THINGSBOARD_TOKEN" || TB_TOKEN.is_empty() {
        println!(">>> [ThingsBoard] Placeholder token detected ('{}').", TB_TOKEN);
        return Ok(None);
    }

    let broker_url = format!("mqtt://{}:{}", TB_HOST, TB_PORT);
    println!(">>> [ThingsBoard] Menghubungkan ke {}...", broker_url);

    let mqtt_cfg = MqttClientConfiguration {
        client_id: Some("esp32_enose_sim"),
        username: Some(TB_TOKEN),
        ..Default::default()
    };

    let rpc_cmd_cb = rpc_cmd_queue.clone();
    let rpc_id_cb = latest_rpc_id.clone();

    let mut client = EspMqttClient::new_cb(
        &broker_url,
        &mqtt_cfg,
        move |event| {
            match event.payload() {
                EventPayload::Connected(_) => {
                    println!(">>> [ThingsBoard MQTT] ✅ Connected successfully!");
                }
                EventPayload::Disconnected => {
                    println!(">>> [ThingsBoard MQTT] Disconnected.");
                }
                EventPayload::Received { topic, data, .. } => {
                    if let Some(t) = topic {
                        if t.starts_with(TB_RPC_SUB_TOPIC_PREFIX) {
                            let req_id = &t[TB_RPC_SUB_TOPIC_PREFIX.len()..];
                            *rpc_id_cb.lock().unwrap() = Some(req_id.to_string());

                            if let Ok(payload_str) = core::str::from_utf8(data) {
                                println!(">>> [ThingsBoard RPC] ID: {}, Data: {}", req_id, payload_str);
                                let upper = payload_str.to_uppercase();
                                if upper.contains("START") {
                                    rpc_cmd_cb.lock().unwrap().push("START".to_string());
                                } else if upper.contains("STOP") {
                                    rpc_cmd_cb.lock().unwrap().push("STOP".to_string());
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        },
    )?;

    let _ = client.subscribe(TB_RPC_SUB_TOPIC, QoS::AtLeastOnce);
    println!(">>> [ThingsBoard] Subscribed ke RPC Topic: {}", TB_RPC_SUB_TOPIC);

    Ok(Some(client))
}

// ─── Main ─────────────────────────────────────────────────
fn main() -> anyhow::Result<()> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    println!("==================================================");
    println!("🧪 ELECTRONIC NOSE - SIMULASI SENSOR & THINGSBOARD");
    println!("ℹ️  Mode Simulasi Aktif (Tidak perlu sensor fisik)");
    println!("==================================================");

    let peripherals = Peripherals::take()?;
    let pins = peripherals.pins;

    // ── NVS & EventLoop (Wajib untuk WiFi & MQTT) ─────────
    let nvs = EspDefaultNvsPartition::take()?;
    let sys_loop = EspSystemEventLoop::take()?;

    // ── Wi-Fi Connection ─────────────────────────────────
    let _wifi = init_wifi(peripherals.modem, sys_loop, nvs)?;

    // ── ThingsBoard MQTT Setup & RPC Queues ───────────────
    let rpc_cmd_queue: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let latest_rpc_id: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));

    let mut mqtt_client = if _wifi.is_some() {
        init_mqtt(rpc_cmd_queue.clone(), latest_rpc_id.clone())?
    } else {
        None
    };

    // ── Inisialisasi Driver Motor (Opsional jika terpasang) ───
    let mut in1_a = PinDriver::output(pins.gpio5)?;
    let mut in2_a = PinDriver::output(pins.gpio17)?;
    let mut in3_a = PinDriver::output(pins.gpio14)?;
    let mut in4_a = PinDriver::output(pins.gpio19)?;

    let mut in1_b = PinDriver::output(pins.gpio16)?;
    let mut in2_b = PinDriver::output(pins.gpio4)?;
    let mut in3_b = PinDriver::output(pins.gpio45)?;
    let mut in4_b = PinDriver::output(pins.gpio20)?;

    driver_stop!(in1_a, in2_a, in3_a, in4_a);
    driver_stop!(in1_b, in2_b, in3_b, in4_b);

    println!(">>> System ready. Menunggu perintah START via UART atau ThingsBoard RPC...");
    unsafe {
        esp_idf_sys::uart_driver_install(
            esp_idf_sys::uart_port_t_UART_NUM_0,
            256,
            0,
            0,
            core::ptr::null_mut(),
            0,
        );
    }

    let sim_thread = std::thread::Builder::new()
        .stack_size(32768)
        .name("sim_task".into())
        .spawn(move || {
            let mut state = SystemState::new();
            let mut cmd_buf = String::new();
            let mut rand_seed: u32 = 123456789 ^ (millis() as u32);

            // Inisialisasi nilai awal sensor baseline
            simulate_sensors(&mut state, &mut rand_seed);
            send_json(&state);
            send_thingsboard_telemetry(&state, &mut mqtt_client);
            state.last_sensor_read_ms = millis();

            loop {
                let now = millis();

                // 1. Baca Perintah dari UART Serial USB
                let mut incoming_cmd: Option<String> = None;
                {
                    let ch = unsafe {
                        let mut byte: u8 = 0;
                        let n = esp_idf_sys::uart_read_bytes(
                            esp_idf_sys::uart_port_t_UART_NUM_0,
                            &mut byte as *mut u8 as *mut core::ffi::c_void,
                            1,
                            0,
                        );
                        if n > 0 { Some(byte) } else { None }
                    };

                    if let Some(b) = ch {
                        if b == b'\n' || b == b'\r' {
                            let c = cmd_buf.trim().to_uppercase();
                            if !c.is_empty() {
                                incoming_cmd = Some(c);
                            }
                            cmd_buf.clear();
                        } else {
                            cmd_buf.push(b as char);
                        }
                    }
                }

                // 2. Baca Perintah dari ThingsBoard RPC
                if incoming_cmd.is_none() {
                    let mut q = rpc_cmd_queue.lock().unwrap();
                    if !q.is_empty() {
                        let rpc_cmd = q.remove(0);
                        println!(">>> [Control] Memproses perintah dari ThingsBoard RPC: {}", rpc_cmd);
                        incoming_cmd = Some(rpc_cmd);

                        // Kirim respons RPC jika ada request ID
                        if let Some(req_id) = latest_rpc_id.lock().unwrap().take() {
                            if let Some(ref mut client) = mqtt_client {
                                let resp_topic = format!("{}{}", TB_RPC_RESP_TOPIC_PREFIX, req_id);
                                let _ = client.publish(
                                    &resp_topic,
                                    QoS::AtLeastOnce,
                                    false,
                                    b"{\"status\":\"ok\"}",
                                );
                            }
                        }
                    }
                }

                // 3. Eksekusi Perintah START / STOP
                if let Some(cmd) = incoming_cmd {
                    if cmd == "START" && !state.running {
                        println!(">>> 🚀 START: Siklus Pengujian Dimulai (300 Detik)");
                        state.running = true;
                        state.cycle_start_ms = millis();

                        driver_stop!(in1_a, in2_a, in3_a, in4_a);
                        driver_stop!(in1_b, in2_b, in3_b, in4_b);
                        state.driver1_active = false;
                        state.driver2_active = false;

                        println!(">>> Fase 1: Chamber Purge/Off (1-70s) - Baseline Stabilisasi");
                        simulate_sensors(&mut state, &mut rand_seed);
                        send_json(&state);
                        send_thingsboard_telemetry(&state, &mut mqtt_client);
                        state.last_sensor_read_ms = millis();

                    } else if cmd == "STOP" && state.running {
                        println!(">>> ⏹️ STOP: Siklus Pengujian Dihentikan!");
                        state.running = false;

                        driver_stop!(in1_a, in2_a, in3_a, in4_a);
                        driver_stop!(in1_b, in2_b, in3_b, in4_b);
                        state.driver1_active = false;
                        state.driver2_active = false;

                        simulate_sensors(&mut state, &mut rand_seed);
                        send_json(&state);
                        send_thingsboard_telemetry(&state, &mut mqtt_client);
                    }
                }

                // 4. Pembacaan Sensor Rutin Tiap 1 Detik (Kontinu)
                if now.saturating_sub(state.last_sensor_read_ms) >= 1000 {
                    simulate_sensors(&mut state, &mut rand_seed);
                    send_json(&state);
                    send_thingsboard_telemetry(&state, &mut mqtt_client);
                    state.last_sensor_read_ms = now;
                }

                // 5. Otomasi Siklus 3 Fase
                if state.running {
                    let elapsed = get_elapsed_time(&state);

                    if elapsed >= 1 && elapsed <= 70 {
                        // Fase 1: Chamber istirahat / purge off
                        if state.driver1_active || state.driver2_active {
                            driver_stop!(in1_a, in2_a, in3_a, in4_a);
                            driver_stop!(in1_b, in2_b, in3_b, in4_b);
                            state.driver1_active = false;
                            state.driver2_active = false;
                        }

                    } else if elapsed >= 71 && elapsed <= 170 {
                        // Fase 2: Inlet ON (Hisap Sampel Gas)
                        if !state.driver1_active {
                            driver_start!(in1_a, in2_a, in3_a, in4_a);
                            state.driver1_active = true;
                            state.driver2_active = false;
                            println!(">>> 🟢 Fase 2: Inlet ON (71-170s) - Hisap Sampel Gas (100 detik)");
                            simulate_sensors(&mut state, &mut rand_seed);
                            send_thingsboard_telemetry(&state, &mut mqtt_client);
                        }

                    } else if elapsed >= 171 && elapsed <= 300 {
                        // Fase 3: Outlet ON (Buang / Pembersihan Chamber)
                        if state.driver1_active {
                            driver_stop!(in1_a, in2_a, in3_a, in4_a);
                            state.driver1_active = false;
                        }
                        if !state.driver2_active {
                            driver_start!(in1_b, in2_b, in3_b, in4_b);
                            state.driver2_active = true;
                            println!(">>> 🔵 Fase 3: Outlet ON (171-300s) - Buang Gas / Purge (130 detik)");
                            simulate_sensors(&mut state, &mut rand_seed);
                            send_thingsboard_telemetry(&state, &mut mqtt_client);
                        }

                    } else if elapsed > 300 {
                        // Selesai 1 siklus penuh (300 detik)
                        println!(">>> 🏁 Siklus Selesai Penuh (300 Detik)! Kembali ke Standby.");
                        state.running = false;

                        driver_stop!(in1_a, in2_a, in3_a, in4_a);
                        driver_stop!(in1_b, in2_b, in3_b, in4_b);
                        state.driver1_active = false;
                        state.driver2_active = false;

                        simulate_sensors(&mut state, &mut rand_seed);
                        send_json(&state);
                        send_thingsboard_telemetry(&state, &mut mqtt_client);
                    }
                }

                FreeRtos::delay_ms(50);
            }
        })?;

    sim_thread.join().unwrap();
    Ok(())
}
