// =========================================================
// ELECTRONIC NOSE - RUST VERSION (ESP32-S3)
// ✅ Dual Connectivity: USB Serial UART + ThingsBoard Cloud (WiFi / MQTT)
// ✅ Remote RPC Control (START / STOP from ThingsBoard Dashboard)
// ✅ ADS1115 ADC (8 Gas Sensors: MQ & TGS)
// ✅ Single DHT22 Environment (Temp & Humidity on GPIO 11)
// ✅ 3 FASE: OFF 70s -> INLET 100s -> OUTLET 130s
// =========================================================

use esp_idf_hal::{
    delay::FreeRtos,
    gpio::*,
    i2c::{I2cConfig, I2cDriver},
    peripherals::Peripherals,
    units::*,
};
use esp_idf_svc::{
    eventloop::EspSystemEventLoop,
    mqtt::client::{EspMqttClient, EventPayload, MqttClientConfiguration, QoS},
    nvs::EspDefaultNvsPartition,
    systime::EspSystemTime,
    wifi::{AuthMethod, BlockingWifi, ClientConfiguration, Configuration, EspWifi},
};

use std::sync::{Arc, Mutex};
use dht_sensor::{dht22, DhtReading};

// ─── KONFIGURASI KREDENSIAL: WIFI & THINGSBOARD ──────────
// 1. Ganti WIFI_SSID dan WIFI_PASS dengan WiFi 2.4 GHz Anda
pub const WIFI_SSID: &str = "Punya gweh";
pub const WIFI_PASS: &str = "dicekdulu";

// 2. Kredensial ThingsBoard Cloud
pub const TB_HOST: &str = "thingsboard.cloud";
pub const TB_PORT: u16 = 1883;
pub const TB_TOKEN: &str = "sIP5Dc6dl1GLn4Enxr9Q";

// ThingsBoard MQTT Topics
pub const TB_TELEMETRY_TOPIC: &str = "v1/devices/me/telemetry";
pub const TB_RPC_SUB_TOPIC: &str = "v1/devices/me/rpc/request/+";
pub const TB_RPC_SUB_TOPIC_PREFIX: &str = "v1/devices/me/rpc/request/";
pub const TB_RPC_RESP_TOPIC_PREFIX: &str = "v1/devices/me/rpc/response/";

// ─── ADS1115 I2C addresses ───────────────────────────────
const ADS1_ADDR: u8 = 0x48;
const ADS2_ADDR: u8 = 0x49;

// ─── ADS1115 register / config constants ─────────────────
const ADS_REG_CONVERT: u8 = 0x00;
const ADS_REG_CONFIG: u8 = 0x01;

/// Build ADS1115 config word (gain = TWOTHIRDS ±6.144 V, single-shot)
fn ads_config(channel: u8) -> [u8; 3] {
    let mux: u16 = match channel {
        0 => 0b100, // AIN0 vs GND
        1 => 0b101,
        2 => 0b110,
        3 => 0b111,
        _ => 0b100,
    };
    let high: u8 = (0b1_000_000_1u8) | ((mux as u8) << 4);
    let low: u8 = 0b1000_0011;
    [ADS_REG_CONFIG, high, low]
}

/// Read a single-ended channel from ADS1115 via I2C, returns raw i16
fn ads_read_single(
    i2c: &mut I2cDriver,
    addr: u8,
    channel: u8,
) -> Result<i16, esp_idf_hal::i2c::I2cError> {
    let cfg = ads_config(channel);
    i2c.write(addr, &cfg, 50)?;
    FreeRtos::delay_ms(10);
    i2c.write(addr, &[ADS_REG_CONVERT], 50)?;
    let mut buf = [0u8; 2];
    i2c.read(addr, &mut buf, 50)?;
    Ok(i16::from_be_bytes(buf))
}

/// Convert raw i16 to millivolts (GAIN_TWOTHIRDS = 6.144V FSR, 32767 counts)
fn raw_to_mv(raw: i16) -> f32 {
    (raw as f32) * 6144.0 / 32767.0
}

// ─── Sensor data ─────────────────────────────────────────
#[derive(Clone, Default)]
struct SensorData {
    mq6: f32,
    mq135: f32,
    mq3: f32,
    mq7: f32,
    tgs2611: f32,
    tgs2602: f32,
    tgs2600: f32,
    tgs2620: f32,
    dht_temp: f32,
    dht_hum: f32,
}

// ─── System state ─────────────────────────────────────────
struct SystemState {
    running: bool,
    driver1_active: bool, // Inlet  (Driver A)
    driver2_active: bool, // Outlet (Driver B)
    cycle_start_ms: u64,
    last_sensor_read_ms: u64,
    sensors: SensorData,
}

impl SystemState {
    fn new() -> Self {
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
fn millis() -> u64 {
    EspSystemTime {}.now().as_millis() as u64
}

fn get_elapsed_time(state: &SystemState) -> u64 {
    if !state.running {
        return 0;
    }
    let elapsed_ms = millis().saturating_sub(state.cycle_start_ms);
    let elapsed_sec = elapsed_ms / 1000;
    if elapsed_sec == 0 { 1 } else { elapsed_sec }
}

fn get_inlet_time(state: &SystemState) -> u64 {
    let elapsed = get_elapsed_time(state);
    if elapsed >= 71 && elapsed <= 170 {
        elapsed - 70
    } else if elapsed > 170 {
        100
    } else {
        0
    }
}

fn get_outlet_time(state: &SystemState) -> u64 {
    let elapsed = get_elapsed_time(state);
    if elapsed >= 171 && elapsed <= 300 {
        elapsed - 170
    } else if elapsed > 300 {
        130
    } else {
        0
    }
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
        let _ = client.publish(
            TB_TELEMETRY_TOPIC,
            QoS::AtLeastOnce,
            false,
            payload.as_bytes(),
        );
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
        println!(">>> [WiFi] Silakan ganti WIFI_SSID, WIFI_PASS, dan TB_TOKEN di src/main.rs.");
        println!(">>> [WiFi] Berjalan dalam mode offline (Serial UART Only).");
        return Ok(None);
    }

    println!(">>> [WiFi] Menghubungkan ke SSID: '{}'...", WIFI_SSID);
    let esp_wifi = EspWifi::new(modem, sys_loop.clone(), Some(nvs))?;
    let mut wifi = BlockingWifi::wrap(esp_wifi, sys_loop)?;

    let ssid_h = WIFI_SSID.try_into().map_err(|_| anyhow::anyhow!("SSID max 32 bytes"))?;
    let pass_h = WIFI_PASS.try_into().map_err(|_| anyhow::anyhow!("Password max 64 bytes"))?;

    // Gunakan AuthMethod::None sebagai threshold agar ESP32 bisa connect ke WPA, WPA2, WPA3, maupun Hotspot HP
    wifi.set_configuration(&Configuration::Client(ClientConfiguration {
        ssid: ssid_h,
        password: pass_h,
        auth_method: AuthMethod::None,
        ..Default::default()
    }))?;

    wifi.start()?;
    println!(">>> [WiFi] Driver started. Memindai jaringan sekitar...");

    if let Ok(ap_list) = wifi.scan() {
        let found = ap_list.iter().any(|ap| ap.ssid.as_str() == WIFI_SSID);
        if found {
            println!(">>> [WiFi] Jaringan '{}' DITEMUKAN pada sinyal 2.4 GHz!", WIFI_SSID);
        } else {
            println!(">>> [WiFi] PERINGATAN: '{}' tidak terdeteksi dalam jangkauan 2.4 GHz!", WIFI_SSID);
            println!(">>> [WiFi] AP yang terdeteksi:");
            for ap in ap_list.iter().take(8) {
                println!("    - SSID: '{}' (RSSI: {} dBm)", ap.ssid, ap.signal_strength);
            }
        }
    }

    let mut connected = false;
    for attempt in 1..=3 {
        println!(">>> [WiFi] Mencoba koneksi ke '{}' (Percobaan {}/3)...", WIFI_SSID, attempt);
        match wifi.connect() {
            Ok(_) => {
                println!(">>> [WiFi] Terhubung ke AP! Menunggu IP DHCP...");
                match wifi.wait_netif_up() {
                    Ok(_) => {
                        let ip_info = wifi.wifi().sta_netif().get_ip_info()?;
                        println!(">>> [WiFi] Berhasil mendapatkan IP: {}", ip_info.ip);
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
        println!(">>> [WiFi] Pastikan WiFi memancarkan frekuensi 2.4 GHz (ESP32 tidak mendukung 5 GHz).");
        println!(">>> [WiFi] Jika menggunakan Hotspot HP, aktifkan opsi 'Maximize Compatibility / Maksimalkan Kompatibilitas'.");
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
        println!(">>> [ThingsBoard] Silakan isi TB_TOKEN di src/main.rs untuk mengaktifkan MQTT.");
        return Ok(None);
    }

    let broker_url = format!("mqtt://{}:{}", TB_HOST, TB_PORT);
    println!(">>> [ThingsBoard] Menghubungkan ke {} dengan Token...", broker_url);

    let mqtt_cfg = MqttClientConfiguration {
        client_id: Some("esp32_enose"),
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
                    println!(">>> [ThingsBoard MQTT] Connected successfully!");
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

    // Subscribe ke RPC Thingsboard
    let _ = client.subscribe(TB_RPC_SUB_TOPIC, QoS::AtLeastOnce);
    println!(">>> [ThingsBoard] Subscribed ke RPC Topic: {}", TB_RPC_SUB_TOPIC);

    Ok(Some(client))
}

// ─── Main ─────────────────────────────────────────────────
fn main() -> anyhow::Result<()> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    println!("==================================================");
    println!("🧪 ELECTRONIC NOSE - ESP32-S3 FIRMWARE");
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

    // ── I2C (SDA=8, SCL=9) ───────────────────────────────
    let i2c = I2cDriver::new(
        peripherals.i2c0,
        pins.gpio8,  // SDA
        pins.gpio9,  // SCL
        &I2cConfig::new().baudrate(400_u32.kHz().into()),
    )?;
    let i2c = Arc::new(Mutex::new(i2c));

    // ── Driver A – Inlet ────
    let mut in1_a = PinDriver::output(pins.gpio5)?;
    let mut in2_a = PinDriver::output(pins.gpio17)?;
    let mut in3_a = PinDriver::output(pins.gpio14)?;
    let mut in4_a = PinDriver::output(pins.gpio19)?;

    // ── Driver B – Outlet ───
    let mut in1_b = PinDriver::output(pins.gpio16)?;
    let mut in2_b = PinDriver::output(pins.gpio4)?;
    let mut in3_b = PinDriver::output(pins.gpio45)?;
    let mut in4_b = PinDriver::output(pins.gpio20)?;

    driver_stop!(in1_a, in2_a, in3_a, in4_a);
    driver_stop!(in1_b, in2_b, in3_b, in4_b);

    // ── DHT22 Environment sensor (GPIO 11, GPIO 10 dibebaskan) ───
    let mut dht_pin = PinDriver::input_output_od(pins.gpio11, Pull::Up)?;

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

    let mut state = SystemState::new();
    let mut cmd_buf = String::new();

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

        // 2. Baca Perintah dari ThingsBoard RPC (jika belum ada dari UART)
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
                println!(">>> START (Siklus Pengujian Dimulai)");
                state.running = true;
                state.cycle_start_ms = millis();

                driver_stop!(in1_a, in2_a, in3_a, in4_a);
                driver_stop!(in1_b, in2_b, in3_b, in4_b);
                state.driver1_active = false;
                state.driver2_active = false;

                println!(">>> Fase 1: Semua mati (1-70s)");

                read_sensors(&mut state, &i2c);
                send_json(&state);
                send_thingsboard_telemetry(&state, &mut mqtt_client);
                state.last_sensor_read_ms = millis();
                println!(">>> First data sent!");

            } else if cmd == "STOP" && state.running {
                println!(">>> STOP (Siklus Pengujian Dihentikan)");
                state.running = false;

                driver_stop!(in1_a, in2_a, in3_a, in4_a);
                driver_stop!(in1_b, in2_b, in3_b, in4_b);
                state.driver1_active = false;
                state.driver2_active = false;

                send_json(&state);
                send_thingsboard_telemetry(&state, &mut mqtt_client);
            }
        }

        // 4. Pembacaan Sensor Rutin (Interval 5 Detik)
        if now.saturating_sub(state.last_sensor_read_ms) >= 5000 {
            let mut delay = esp_idf_hal::delay::Ets;
            if let Ok(reading) = dht22::Reading::read(&mut delay, &mut dht_pin) {
                state.sensors.dht_temp = reading.temperature;
                state.sensors.dht_hum = reading.relative_humidity;
            }

            read_sensors(&mut state, &i2c);
            send_json(&state);
            send_thingsboard_telemetry(&state, &mut mqtt_client);
            state.last_sensor_read_ms = now;
        }

        // 5. Otomasi Siklus 3 Fase
        if state.running {
            let elapsed = get_elapsed_time(&state);

            if elapsed >= 1 && elapsed <= 70 {
                // Fase 1: Chamber istirahat / purge off
                if state.driver1_active {
                    driver_stop!(in1_a, in2_a, in3_a, in4_a);
                    state.driver1_active = false;
                }
                if state.driver2_active {
                    driver_stop!(in1_b, in2_b, in3_b, in4_b);
                    state.driver2_active = false;
                }

            } else if elapsed >= 71 && elapsed <= 170 {
                // Fase 2: Inlet ON (Hisap Sampel Gas)
                if !state.driver1_active {
                    driver_start!(in1_a, in2_a, in3_a, in4_a);
                    state.driver1_active = true;
                    state.driver2_active = false;
                    println!(">>> Fase 2: Inlet ON (71-170s) - 100 detik");
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
                    println!(">>> Fase 3: Outlet ON (171-300s) - 130 detik");
                    send_thingsboard_telemetry(&state, &mut mqtt_client);
                }

            } else if elapsed > 300 {
                // Selesai 1 siklus penuh (300 detik)
                println!(">>> Cycle complete!");
                state.running = false;

                driver_stop!(in1_a, in2_a, in3_a, in4_a);
                driver_stop!(in1_b, in2_b, in3_b, in4_b);
                state.driver1_active = false;
                state.driver2_active = false;

                send_json(&state);
                send_thingsboard_telemetry(&state, &mut mqtt_client);
            }
        }

        FreeRtos::delay_ms(1);
    }
}

// ─── Sensor reading ──────────────────────────────────────
fn read_sensors(state: &mut SystemState, i2c: &Arc<Mutex<I2cDriver>>) {
    let mut bus = i2c.lock().unwrap();

    let raw_mq6    = ads_read_single(&mut bus, ADS1_ADDR, 0).unwrap_or(0);
    let raw_mq135  = ads_read_single(&mut bus, ADS1_ADDR, 1).unwrap_or(0);
    let raw_mq3    = ads_read_single(&mut bus, ADS1_ADDR, 2).unwrap_or(0);
    let raw_mq7    = ads_read_single(&mut bus, ADS1_ADDR, 3).unwrap_or(0);

    let raw_tgs2611 = ads_read_single(&mut bus, ADS2_ADDR, 0).unwrap_or(0);
    let raw_tgs2602 = ads_read_single(&mut bus, ADS2_ADDR, 1).unwrap_or(0);
    let raw_tgs2600 = ads_read_single(&mut bus, ADS2_ADDR, 2).unwrap_or(0);
    let raw_tgs2620 = ads_read_single(&mut bus, ADS2_ADDR, 3).unwrap_or(0);

    state.sensors = SensorData {
        mq6:     raw_to_mv(raw_mq6),
        mq135:   raw_to_mv(raw_mq135),
        mq3:     raw_to_mv(raw_mq3),
        mq7:     raw_to_mv(raw_mq7),
        tgs2611: raw_to_mv(raw_tgs2611),
        tgs2602: raw_to_mv(raw_tgs2602),
        tgs2600: raw_to_mv(raw_tgs2600),
        tgs2620: raw_to_mv(raw_tgs2620),
        ..state.sensors
    };
}
