# 🧪 E-Nose MOS Device and OTA Firmware Base

Sistem komprehensif **Electronic Nose (Hidung Elektronik)** berbasis mikrokontroler **ESP32-S3 (Rust)**, dilengkapi dengan arsitektur **Over-The-Air (OTA) Firmware Management & Remote Console (Rust Axum)**, **CSV Profiling & Analytical Server (Python/Flask/Pandas)**, serta dataset aroma kopi riil untuk klasifikasi mutu berbasis pola gas Metal Oxide Semiconductor (MOS).

---

## 📑 Daftar Isi
1. [Arsitektur Sistem Terintegrasi](#1-arsitektur-sistem-terintegrasi)
2. [Struktur Folder Repositori](#2-struktur-folder-repositori)
3. [Perangkat Keras & Firmware ESP32-S3](#3-perangkat-keras--firmware-esp32-s3)
   - [Matriks Pinout GPIO & Antarmuka](#31-matriks-pinout-gpio--antarmuka)
   - [Pemetaan Sensor Gas ke 2x ADS1115 (16-Bit ADC)](#32-pemetaan-sensor-gas-ke-2x-ads1115-16-bit-adc)
   - [Pemetaan Driver Motor Pompa & Katup Solenoid](#33-pemetaan-driver-motor-pompa--katup-solenoid)
   - [Skema Daya & Distribusi Listrik](#34-skema-daya--distribusi-listrik)
4. [Siklus Pengujian Sensor (Timeline 360 Detik)](#4-siklus-pengujian-sensor-timeline-360-detik)
5. [Protokol Komunikasi & Format JSON](#5-protokol-komunikasi--format-json)
6. [Sistem OTA Server & Remote Console (Rust Axum)](#6-sistem-ota-server--remote-console-rust-axum)
7. [CSV Profiler & Engine Analisis Data (Python)](#7-csv-profiler--engine-analisis-data-python)
8. [Dataset Aroma Kopi (Raw Coffee Data)](#8-dataset-aroma-kopi-raw-coffee-data)
9. [Panduan Menjalankan Sistem (Quickstart)](#9-panduan-menjalankan-sistem-quickstart)
10. [Dokumentasi Laporan & Buku Panduan](#10-dokumentasi-laporan--buku-panduan)

---

## 1. Arsitektur Sistem Terintegrasi

Sistem ini dirancang untuk menghubungkan siklus operasional *edge-device* hingga *cloud/server dashboard*:

```text
┌────────────────────────────────────────────────────────────────────────┐
│                        Web Browser Dashboard (:5000)                   │
│  - Single-Page Application (HTML/CSS/JS)                              │
│  - CSV Profiling & Interactive Data Explorer                           │
│  - OTA Management, Project Upload, Firmware Build Trigger & Console   │
└───────────────────▲────────────────────────────────▲───────────────────┘
                    │ REST / WebSocket (:5000)       │ REST / WebSocket (:7000)
┌───────────────────▼──────────────────────┐   ┌─────▼──────────────────────────┐
│ Python CSV Profiler Server (:5000)       │   │ Rust Axum OTA Service (:7000)  │
│ - Flask + Pandas + Sweetviz/Profiling    │   │ - Automated `cargo build`      │
│ - Automated Baseline & Peak Feature Ext. │   │ - SQLite / PostgreSQL (Azure)  │
│ - Data Sanitization & Cleaning Engine    │   │ - MQTT Remote Console Listener │
└──────────────────────────────────────────┘   └────────────────▲───────────────┘
                                                                │ MQTT (TLS) / HTTP OTA
                                                                │ Wi-Fi LAN
┌───────────────────────────────────────────────────────────────▼───────────────┐
│                           ESP32-S3 Edge Device                                │
│ - Rust Firmware (esp-idf-hal / FreeRTOS)                                      │
│ - State Machine 3-Phase Cycle Controller (360s Timeline)                      │
│ - Safe Dual-Partition OTA Rollback Engine                                     │
│ - Multi-Sensor Gas Acquisition (MQ & TGS Arrays)                              │
└──────────────┬────────────────────────────────────────────────▲───────────────┘
               │ I2C (400kHz)                                   │ Digital One-Wire
┌──────────────▼──────────┐                            ┌────────┴─────────────┐
│ 2x ADS1115 ADC (16-Bit) │                            │ DHT22 Ambien Sensor  │
│ - Unit #1: 4x MQ Array  │                            │ - Suhu & Kelembaban  │
│ - Unit #2: 4x TGS Array │                            └──────────────────────┘
└─────────────────────────┘
               │ PWM / Digital GPIO Out
┌──────────────▼────────────────────────────────────────────────────────────────┐
│ 2x Motor Driver (L298N / MC33886)                                             │
│ - Driver A: Inlet Pump (Hisap) + Inlet Valve (Solenoid)                       │
│ - Driver B: Outlet Pump (Purging) + Outlet Valve (Solenoid)                   │
└───────────────────────────────────────────────────────────────────────────────┘
```

---

## 2. Struktur Folder Repositori

```text
.
├── 📁 esp32/                        # Firmware ESP32-S3 fisik (Rust esp-idf-hal)
│   ├── 📁 .cargo/                  # Konfigurasi cross-compiler xtensa-esp32s3-espidf
│   ├── 📄 Cargo.toml               # Dependensi crate: esp-idf-hal, esp-idf-svc, dht-sensor
│   ├── 📄 rust-toolchain.toml      # Toolchain ESP Rust (channel: "esp")
│   └── 📁 src/
│       ├── 📄 main.rs              # Firmware utama siklus 360s, I2C ADC, DHT22, Driver Pompa
│       └── 📄 main_light.rs        # Versi optimasi stack FreeRTOS + kontrol PWM LEDC
│
├── 📁 esp32_simulation/             # Firmware simulasi ESP32 & ThingsBoard Gateway
│   ├── 📁 src/main.rs              # Simulasi loop sensor tanpa hardware fisik
│   ├── 📄 thingsboard_sim.py       # Gateway simulator ThingsBoard via MQTT telemetry
│   ├── 📄 partitions.csv           # Skema partisi flash ESP32 untuk simulasi
│   └── 📄 sdkconfig.defaults       # Default konfigurasi ESP-IDF
│
├── 📁 CSV_PROFILING-main/           # Platform OTA & Profiling Web Dashboard
│   ├── 📁 ota-server/              # Backend OTA Server berbasis Rust & Axum framework
│   │   ├── 📁 src/
│   │   │   ├── 📁 routes/          # API endpoint: projects, builds, firmware, devices
│   │   │   ├── 📁 services/        # Service compiler builder, ZIP extraction, SHA-256
│   │   │   ├── 📄 storage.rs       # Integrasi database SQLite / PostgreSQL
│   │   │   └── 📄 main.rs          # Server entrypoint port :7000
│   │   └── 📄 Cargo.toml
│   ├── 📁 python/                  # Backend Profiling CSV berbasis Flask & Pandas
│   │   ├── 📄 profiler_server.py   # HTTP API upload CSV, normalisasi, dan pembuatan report
│   │   ├── 📄 requirements.txt     # Dependensi Python: flask, pandas, ydata-profiling
│   │   ├── 📁 uploads/             # Folder staging file CSV yang diunggah
│   │   └── 📁 reports/             # Arsip laporan profiling interaktif (HTML/JSON)
│   ├── 📁 web/                     # Frontend Web Dashboard terpadu (HTML5, Vanilla CSS, JS)
│   ├── 📁 examples/                # Template proyek firmware OTA untuk ESP32
│   │   └── 📁 esp32-rust-ota-client/ # Client runtime OTA yang di-flash via USB
│   ├── 📄 run-dev.sh / run-dev.bat # Skrip otomasi menjalankan server lokal
│   └── 📁 docs/                    # Dokumentasi mendalam dan LaTeX Handbook
│
├── 📁 raw_coffee_data/              # Dataset hasil pengujian riil sampel kopi
│   ├── 📁 high_grade/              # Sampel kopi grade tinggi (Sample 1 s/d 6)
│   │   └── (Arabica Gayo, Situbondo, Jember, Javateng Super, Kerinci, Wamena)
│   └── 📁 low_grade/               # Sampel kopi grade rendah (Sample 1 s/d 6) + Data Validasi
│
└── 📁 docs/                         # Laporan akademis & publikasi
    ├── 📄 laporan_progress.tex     # Naskah LaTeX Laporan Progress E-Nose & OTA
    ├── 📄 laporan_progress.pdf     # Dokumen PDF hasil kompilasi
    └── 📁 image/                   # Diagram alur, visualisasi respon sensor, & GUI
```

---

## 3. Perangkat Keras & Firmware ESP32-S3

### 3.1 Matriks Pinout GPIO & Antarmuka

| Komponen / Modul | Jalur / Pin Hardware | Pin ESP32-S3 | Fungsi & Spesifikasi |
| :--- | :--- | :--- | :--- |
| **I2C Bus Sensor** | SDA | `GPIO 8` | Data line bersama untuk 2 unit ADS1115 (400 kHz) |
| **I2C Bus Sensor** | SCL | `GPIO 9` | Clock line bersama untuk 2 unit ADS1115 |
| **DHT22 Environment**| Data (One-Wire) | `GPIO 11` | Monitoring Suhu & Kelembaban ruang ambien |
| **Driver A (Inlet)** | IN1 / IN2 | `GPIO 5` / `GPIO 17` | Kontrol arah/status Pompa Hisap (Inlet) |
| **Driver A (Inlet)** | IN3 / IN4 | `GPIO 14` / `GPIO 19`| Kontrol arah/status Katup Solenoid Masuk |
| **Driver A (Inlet)** | ENA / ENB | `GPIO 18` / `GPIO 15`| Kontrol PWM Kecepatan Pompa/Katup Masuk (Opsional) |
| **Driver B (Outlet)**| IN1 / IN2 | `GPIO 16` / `GPIO 4`  | Kontrol arah/status Pompa Buang (Outlet / Purging) |
| **Driver B (Outlet)**| IN3 / IN4 | `GPIO 45` / `GPIO 20`| Kontrol arah/status Katup Solenoid Buang |
| **Driver B (Outlet)**| ENA / ENB | `GPIO 2` / `GPIO 21`  | Kontrol PWM Kecepatan Pompa/Katup Buang (Opsional) |
| **UART Serial Comm** | TX / RX | `GPIO 43` / `GPIO 44`| Komunikasi Serial JSON ke GUI / PC |

---

### 3.2 Pemetaan Sensor Gas ke 2x ADS1115 (16-Bit ADC)

Sistem menggunakan **2 unit modul ADS1115** dengan FSR $\pm 6.144\text{V}$:

#### Unit #1 — Alamat I2C `0x48` (Pin ADDR terhubung ke GND)
| Channel ADC | Sensor Gas | Target Deteksi Gas Utama | Rentang Sinyal |
| :--- | :--- | :--- | :--- |
| `AIN0` | **MQ-6** | LPG, Iso-butana, Propana | $0 - 5\text{V}$ (Analog) |
| `AIN1` | **MQ-135** | Air Quality, Amonia, Benzena, Asap | $0 - 5\text{V}$ (Analog) |
| `AIN2` | **MQ-3** | Uap Alkohol, Etanol | $0 - 5\text{V}$ (Analog) |
| `AIN3` | **MQ-7** | Karbon Monoksida (CO) | $0 - 5\text{V}$ (Analog) |

#### Unit #2 — Alamat I2C `0x49` (Pin ADDR terhubung ke VDD/3.3V)
| Channel ADC | Sensor Gas | Target Deteksi Gas Utama | Rentang Sinyal |
| :--- | :--- | :--- | :--- |
| `AIN0` | **TGS 2611** | Metana ($\text{CH}_4$), Gas Alam | $0 - 5\text{V}$ (Analog) |
| `AIN1` | **TGS 2602** | VOCs, Amonia, $\text{H}_2\text{S}$, Bau Busuk | $0 - 5\text{V}$ (Analog) |
| `AIN2` | **TGS 2600** | Kontaminan Udara, Hidrogen ($\text{H}_2$), CO | $0 - 5\text{V}$ (Analog) |
| `AIN3` | **TGS 2620** | Uap Pelarut Organik, Alkohol | $0 - 5\text{V}$ (Analog) |

---

### 3.3 Pemetaan Driver Motor Pompa & Katup Solenoid

- **Driver A (Inlet System)**:
  - Pompa Inlet: `GPIO 5` (IN1) & `GPIO 17` (IN2)
  - Katup Inlet: `GPIO 14` (IN3) & `GPIO 19` (IN4)
- **Driver B (Outlet System / Purging)**:
  - Pompa Outlet: `GPIO 16` (IN1) & `GPIO 4` (IN2)
  - Katup Outlet: `GPIO 45` (IN3) & `GPIO 20` (IN4)

---

### 3.4 Skema Daya & Distribusi Listrik

```text
[ Adaptor Daya 12V DC / 3A-5A ]
        │
        ├──► [ Step-Down Regulator LM2596 (12V -> 5V / 3A) ]
        │            │
        │            ├──► VCC Array Sensor Gas MQ & TGS (Heater & Logic 5V)
        │            ├──► VCC 2x ADS1115 (5V)
        │            └──► VCC ESP32-S3 (Pin 5V / VBUS)
        │
        └──► V+ Power Motor Driver L298N / MC33886 (12V untuk Pompa & Solenoid)
```

> [!IMPORTANT]
> **Common Ground**: Seluruh jalur ground (GND 12V, GND 5V, GND ESP32, GND ADC, dan GND Sensor) **wajib disatukan** untuk menjaga kestabilan referensi tegangan ADC.

---

## 4. Siklus Pengujian Sensor (Timeline 360 Detik)

Siklus pengujian otomatis berjalan teratur selama **360 detik**:

```text
       0s                  70s                170s               230s                360s
TIMELINE: ├───────────────────┼───────────────────┼───────────────────┼───────────────────┤
FASE:        Fase 1: Baseline     Fase 2: Inlet        Fase Sensing       Fase 3: Outlet
POMPA:       Semua Pompa OFF      Inlet Pump ON       Semua Pompa OFF     Outlet Pump ON
KATUP:       Semua Katup Tutup    Katup Masuk Buka    Semua Katup Tutup   Katup Buang Buka
DURASI:          70 Detik           100 Detik            60 Detik            130 Detik
TUJUAN:     Baseline Stabil       Hisap Uap Sampel    Reaksi Puncak Gas   Pembersihan Chamber
```

1. **Fase 1 — Baseline (Detik 0 – 70)**: Semua aktuator mati. Pengambilan tegangan referensi udara bersih.
2. **Fase 2 — Intake / Hisap (Detik 71 – 170)**: Pompa inlet dan solenoid masuk aktif selama 100 detik membawa uap aroma ke ruang sensor.
3. **Fase Sensing — Pengukuran Puncak (Detik 171 – 230)**: Pompa dimatikan selama 60 detik agar uap aroma terperangkap dan sensor mencapai respon saturasi optimal.
4. **Fase 3 — Purging / Pembersihan (Detik 231 – 360)**: Pompa buang dan katup outlet aktif selama 130 detik membilas chamber dengan udara bersih.

---

## 5. Protokol Komunikasi & Format JSON

ESP32 mengirimkan paket telemetri JSON secara periodik (1 Hz) ke Serial/MQTT:

```json
{
  "sensors": {
    "mq6": 1250.45,
    "mq135": 820.10,
    "mq3": 450.30,
    "mq7": 610.80,
    "tgs2611": 1540.20,
    "tgs2602": 1120.00,
    "tgs2600": 980.50,
    "tgs2620": 1310.75,
    "dht_temp": 28.70,
    "dht_hum": 64.90
  },
  "status": {
    "inlet_pump": true,
    "inlet_valve": true,
    "outlet_pump": false,
    "outlet_valve": false,
    "inlet_time": 45,
    "outlet_time": 0,
    "elapsed_time": 115
  },
  "timestamp": 115000
}
```

---

## 6. Sistem OTA Server & Remote Console (Rust Axum)

Sub-sistem `CSV_PROFILING-main/ota-server` menyediakan infrastruktur manajemen firmware:
- **Kompilasi Firmware di Server**: Pengguna mengunggah arsip proyek ZIP; server mengompilasi biner ESP32 secara terisolasi menggunakan `cargo espflash`.
- **Dual Partition OTA Rollback**: Menggunakan skema partisi `ota_0` dan `ota_1`. Jika firmware baru mengalami crash/panic, ESP32 otomatis *rollback* ke partisi valid sebelumnya.
- **Remote Console via MQTT & WebSocket**: Log output firmware dikirimkan via MQTT (TLS) dan disiarkan secara *realtime* ke antarmuka web melalui WebSocket.
- **Dukungan Basis Data**: Default SQLite (`ota.db`) dan siap diintegrasikan ke PostgreSQL (termasuk Azure Database for PostgreSQL).

---

## 7. CSV Profiler & Engine Analisis Data (Python)

Sub-sistem `CSV_PROFILING-main/python/profiler_server.py` bertugas mengolah data hasil uji:
- **Automated Feature Extraction**: Menghitung tegangan baseline ($V_0$), respon puncak ($V_{max}$), diferensial ($\Delta V = V_{max} - V_0$), rasio respon ($V_{max}/V_0$), dan integral area di bawah kurva (AUC).
- **Interactive EDA Reports**: Menghasilkan profil distribusi, korelasi Pearson, dan ringkasan multivariat interaktif.
- **Data Preprocessing & Cleaning**: Pembersihan noise, penanganan *missing values*, serta formatting siap pakai untuk pemodelan klasifikasi Machine Learning (SVM, Random Forest, ANN).

---

## 8. Dataset Aroma Kopi (Raw Coffee Data)

Folder `raw_coffee_data/` memuat data riil pengukuran E-Nose untuk komparasi mutu biji kopi:
- **`high_grade/`**: 6 varietas kopi mutu tinggi (Sample 1 s/d Sample 6), mencakup *Arabica Gayo, Situbondo, Jember, Javateng Super, Kerinci, dan Wamena*.
- **`low_grade/`**: 6 varietas kopi mutu komersial/rendah, disertai data pengujian validasi (`Val_...csv`) untuk benchmarking algoritma klasifikasi aroma.

---

## 9. Panduan Menjalankan Sistem (Quickstart)

### Prasyarat
1. **Rust Toolchain**: `rustup` + target `xtensa-esp32s3-espidf` atau `riscv32imc-esp-espidf`.
2. **ESP Toolchain**: `cargo install espup cargo-espflash espflash && espup install`.
3. **Python 3.10+**: `pip install -r CSV_PROFILING-main/python/requirements.txt`.

### Menjalankan Platform OTA & Web Profiler
Cukup jalankan script launcher otomatis:

**Linux / macOS:**
```bash
cd CSV_PROFILING-main
./run-dev.sh
```

**Windows:**
```bat
cd CSV_PROFILING-main
run-dev.bat
```
- Web Dashboard: `http://localhost:5000`
- OTA Axum Service: `http://localhost:7000`

### Mem-flash Firmware ESP32 Fisik
```bash
cd esp32
cargo espflash flash --release --monitor
```

---

## 10. Dokumentasi Laporan & Buku Panduan

- **Laporan Progress Lengkap (PDF)**: [`docs/laporan_progress.pdf`](docs/laporan_progress.pdf)
- **Source Naskah Laporan (LaTeX)**: [`docs/laporan_progress.tex`](docs/laporan_progress.tex)
- **Illustrated OTA Handbook (Bahasa Indonesia)**: [`CSV_PROFILING-main/docs/book/csv-profiler-ota-handbook-id.pdf`](CSV_PROFILING-main/docs/book/csv-profiler-ota-handbook-id.pdf)
- **Illustrated OTA Handbook (English)**: [`CSV_PROFILING-main/docs/book/csv-profiler-ota-handbook.pdf`](CSV_PROFILING-main/docs/book/csv-profiler-ota-handbook.pdf)
