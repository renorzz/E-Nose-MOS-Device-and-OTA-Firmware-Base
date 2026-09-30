# 🧪 Electronic Nose - Simulation & ThingsBoard Cloud

Folder ini berisi program simulasi untuk Electronic Nose (E-Nose) yang mengirimkan data sensor gas, suhu, kelembaban, serta status pompa/valve ke **ThingsBoard Cloud**.

Program ini dirancang khusus agar Anda dapat menguji aliran data dan dashboard ThingsBoard **tanpa memerlukan sensor fisik terpasang**.

---

## 📁 Struktur File

| File / Folder | Deskripsi |
| :--- | :--- |
| **`thingsboard_sim.py`** | **Simulator Desktop (Python)**. Dapat langsung dijalankan di laptop (macOS / Windows / Linux) tanpa hardware ESP32. Menggunakan HTTP REST API bawaan Python (tanpa perlu install library tambahan). |
| **`src/main.rs`** | **Firmware ESP32 (Rust)**. Firmware simulasi yang dapat di-flash ke ESP32. Terkoneksi ke WiFi & ThingsBoard via MQTT, serta menerima kontrol RPC START/STOP. |
| **`Cargo.toml`** | Konfigurasi package & dependensi ESP-IDF Rust. |
| **`.cargo/config.toml`** | Konfigurasi target arsitektur ESP32-S3 dan runner `espflash`. |

---

## 🚀 Opsi 1: Menjalankan Simulator Python di Laptop (Paling Cepat)

Anda dapat langsung mengecek apakah ThingsBoard menerima data dalam hitungan detik tanpa perlu menghubungkan ESP32 ke komputer.

### Cara Menjalankan:
Buka Terminal dan masuk ke folder `esp32_simulation`:
```bash
cd "/Users/moreno/Document/Project Coding/TA Rafi Cepmek/esp32_simulation"
python3 thingsboard_sim.py
```

### Opsi Perintah Terminal saat Program Berjalan:
- Ketik **`start`** lalu tekan Enter: Memulai siklus 3-fase (300 detik).
- Ketik **`stop`** lalu tekan Enter: Menghentikan siklus pengujian kapan saja.
- Ketik **`status`** lalu tekan Enter: Menampilkan pembacaan tegangan sensor (mV) dan suhu saat ini.
- Ketik **`quit`** lalu tekan Enter: Keluar dari program.

> **Tips Auto-Start:** Jika ingin langsung memulai siklus otomatis saat dijalankan:
> ```bash
> python3 thingsboard_sim.py --auto-start
> ```

---

## ⚡ Opsi 2: Flash Firmware Simulasi ke Mikrokontroler ESP32

Jika Anda ingin menguji langsung pada board ESP32 tanpa sensor terpasang:

### 1. Kredensial WiFi & ThingsBoard
Kredensial sudah diset di dalam [src/main.rs](file:///Users/moreno/Document/Project%20Coding/TA%20Rafi%20Cepmek/esp32_simulation/src/main.rs):
```rust
pub const WIFI_SSID: &str = "Punya gweh";
pub const WIFI_PASS: &str = "dicekdulu";
pub const TB_HOST: &str   = "thingsboard.cloud";
pub const TB_TOKEN: &str  = "sIP5Dc6dl1GLn4Enxr9Q";
```

### 2. Build & Flash ke ESP32
Gunakan perintah standard Cargo (karena runner `espflash` sudah dikonfigurasi di `.cargo/config.toml`):
```bash
cd "/Users/moreno/Document/Project Coding/TA Rafi Cepmek/esp32_simulation"
cargo run --release
```

*Catatan: Jika Anda ingin menggunakan perintah `cargo espflash flash --release --monitor`, install subcommand-nya terlebih dahulu dengan:*
```bash
cargo install cargo-espflash
```

ESP32 akan:
1. Menghubungkan diri ke WiFi 2.4 GHz.
2. Terkoneksi ke ThingsBoard via MQTT Broker.
3. Mengirimkan telemetri baseline secara rutin setiap 5 detik.
4. Menerima kendali perintah `START` dan `STOP` dari Serial USB maupun dari Dashboard ThingsBoard (RPC).

---

## 📊 Format Data Telemetri ThingsBoard

Data yang dikirimkan ke topik `v1/devices/me/telemetry` setiap 5 detik:

```json
{
  "mq6": 462.15,
  "mq135": 531.40,
  "mq3": 388.90,
  "mq7": 612.30,
  "tgs2611": 431.25,
  "tgs2602": 560.80,
  "tgs2600": 401.10,
  "tgs2620": 492.50,
  "dht1_temp": 27.25,
  "dht1_hum": 62.40,
  "dht2_temp": 27.52,
  "dht2_hum": 61.85,
  "inlet_pump": false,
  "inlet_valve": false,
  "outlet_pump": false,
  "outlet_valve": false,
  "inlet_time": 0,
  "outlet_time": 0,
  "elapsed_time": 0,
  "running": false
}
```

### Siklus 3 Fase Otomatis:
1. **Fase 1 (1–70s - Purge / Stabilisasi)**: Pompa mati, sensor di baseline (~380–610 mV).
2. **Fase 2 (71–170s - Inlet ON)**: Pompa & valve inlet menyala, sensor mengalami kenaikan eksponensial respons gas hingga 1800–2950 mV.
3. **Fase 3 (171–300s - Outlet Purge ON)**: Pompa & valve outlet menyala, sensor mengalami penurunan eksponensial kembali ke baseline.
4. **> 300s**: Siklus selesai, sistem kembali ke status Standby.
