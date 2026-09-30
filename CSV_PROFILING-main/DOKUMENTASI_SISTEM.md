# 📘 Dokumentasi Lengkap Sistem: CSV Profiler & ESP32-S3 OTA Dashboard

Dokumen ini menyajikan arsitektur lengkap, alur kerja, struktur kode, dan panduan teknis untuk sistem **CSV Profiler + ESP32-S3 OTA Service** yang berada pada folder `CSV_PROFILING-main`.

---

## 📑 Daftar Isi
1. [Ringkasan Eksekutif](#1-ringkasan-eksekutif)
2. [Arsitektur Sistem & Diagram Alur](#2-arsitektur-sistem--diagram-alur)
3. [Struktur Folder & Komponen](#3-struktur-folder--komponen)
4. [Modul 1: CSV Profiler & Data Cleaner (Python/Flask)](#4-modul-1-csv-profiler--data-cleaner-pythonflask)
5. [Modul 2: OTA Server & Build Pipeline (Rust/Axum)](#5-modul-2-ota-server--build-pipeline-rustaxum)
6. [Modul 3: Runtime Klien ESP32-S3 & Mekanisme Rollback](#6-modul-3-runtime-klien-esp32-s3--mekanisme-rollback)
7. [Modul 4: Remote Console (MQTT/TLS ke WebSocket)](#7-modul-4-remote-console-mqtttls-ke-websocket)
8. [Modul 5: Antarmuka Web Dashboard (Frontend)](#8-modul-5-antarmuka-web-dashboard-frontend)
9. [Daftar Lengkap REST API & Endpoints](#9-daftar-lengkap-rest-api--endpoints)
10. [Panduan Konfigurasi (.env) & Cara Menjalankan](#10-panduan-konfigurasi-env--cara-menjalankan)
11. [Integrasi dengan Tugas Akhir E-Nose](#11-integrasi-dengan-tugas-akhir-e-nose)

---

## 1. Ringkasan Eksekutif

Sistem ini dirancang sebagai **layanan dual-backend (hybrid)** yang menyatukan dua kebutuhan krusial dalam siklus hidup perangkat IoT / Embedded System:

1. **Analisis & Pembersihan Data Sensor (Data Pipeline)**:
   - Memproses file CSV hasil akuisisi data sensor gas E-Nose.
   - Melakukan profiling otomatis (*Exploratory Data Analysis / EDA*) menggunakan `ydata_profiling`.
   - Menyediakan fitur *cleaning* (imputasi missing values, penghapusan duplikasi, penanganan outlier IQR, serta konversi tipe data).
   - Menyimpan histori sesi profiling melalui format Sidecar JSON.

2. **Manajemen Firmware & Over-The-Air (OTA Pipeline)**:
   - Memungkinkan pengembang mengunggah kode sumber (*source code*) Rust dalam bentuk file ZIP.
   - Mengompilasi kode menjadi binary ESP32-S3 murni di komputer host (*Local Build*) tanpa memerlukan flash fisik via USB berulang kali.
   - Menyajikan binary firmware via HTTP streaming ke ESP32 di jaringan lokal (LAN).
   - Menyediakan sistem verifikasi hash SHA-256 dan perlindungan *auto-rollback* jika firmware baru mengalami crash saat *booting*.
   - Menyediakan *Remote Console* untuk melihat log serial/aplikasi ESP32 secara nirkabel melalui MQTT dan WebSocket.

---

## 2. Arsitektur Sistem & Diagram Alur

Sistem memisahkan beban kerja analitik berat (Python) dan operasi I/O berkecepatan tinggi / kompilasi sistem (Rust):

```text
                             ┌─────────────────────────────────┐
                             │    Browser Web Dashboard        │
                             │         Port: 5000              │
                             └──────────────┬──────────────────┘
                                            │
                     ┌──────────────────────┴──────────────────────┐
                     │ HTTP / SSE                                  │ HTTP / WS
                     ▼                                             ▼
       ┌───────────────────────────┐                 ┌───────────────────────────┐
       │   Python Backend (Flask)  │                 │    Rust Backend (Axum)    │
       │         Port: 5000        │                 │         Port: 7000        │
       └─────────────┬─────────────┘                 └─────────────┬─────────────┘
                     │                                             │
         ┌───────────┴───────────┐                     ┌───────────┴───────────┐
         ▼                       ▼                     ▼                       ▼
   ┌───────────┐           ┌───────────┐         ┌───────────┐           ┌───────────┐
   │ CSV Data  │           │  Pandas & │         │  Storage  │           │   Local   │
   │  Uploads  │           │ Profiling │         │  SQLite   │           │  Builder  │
   │  & Clean  │           │  Reports  │         │ (ota.db)  │           │(cargo+esp)│
   └───────────┘           └───────────┘         └───────────┘           └─────┬─────┘
                                                                               │
                                                                         Firmware .bin
                                                                               │
                                                        HTTP Poll / Pull OTA   ▼
                                                    ┌────────────────────────────────┐
                                                    │     ESP32-S3 Microcontroller   │
                                                    │    (OTA Runtime + Partition)   │
                                                    └────────────────────────────────┘
```

---

## 3. Struktur Folder & Komponen

```text
CSV_PROFILING-main/
├── python/                          # Backend Analitik Data (Flask)
│   ├── profiler_server.py           # Server utama Flask (EDA, cleaning, sidecar JSON)
│   ├── requirements.txt             # Dependensi Python (pandas, ydata-profiling, flask)
│   ├── uploads/                     # Penyimpanan sementara file CSV upload
│   └── reports/                     # File HTML report dan sidecar JSON (<job_id>.json)
│
├── ota-server/                      # Backend Layanan OTA (Rust + Axum)
│   ├── Cargo.toml                   # Dependensi Rust (axum, tokio, sqlx, rumqttc)
│   ├── migrations/                  # Skema database relasional (001_initial.sql)
│   └── src/
│       ├── main.rs                  # Entry point, router Axum, dan background worker
│       ├── config.rs                # Parsing environment variable (.env)
│       ├── storage.rs               # Lapisan akses database SQLite / PostgreSQL
│       ├── console.rs               # Broker MQTT ingestion & WebSocket streaming
│       ├── errors.rs                # Standarisasi error handling (ApiError)
│       ├── routes/
│       │   ├── builds.rs            # Monitoring status & log build compiler
│       │   ├── devices.rs           # Registrasi device, heartbeat, deploy & status OTA
│       │   ├── firmware.rs          # Metadata firmware, download .bin, & delete
│       │   └── mod.rs               # Agregator router API OTA
│       └── services/
│           ├── builder.rs           # Eksekusi Cargo build + espflash
│           ├── firmware.rs          # Logika bisnis manajemen file binary
│           ├── hashing.rs           # Streaming kalkulator SHA-256
│           └── project.rs           # Ekstraksi ZIP & komposisi kode firmware
│
├── web/                             # Antarmuka Dashboard (Frontend)
│   ├── index.html                   # Halaman Single-Page App (SPA)
│   ├── app.js                       # Logika interaktif CSV Profiler & Cleaning
│   ├── ota.js                       # Logika manajemen Device, Upload ZIP, & Deploy OTA
│   ├── console.js                   # WebSocket client untuk Remote Console
│   └── style.css                    # Styling modern & responsif
│
├── examples/
│   ├── esp32-rust-ota-client/       # Template runtime dasar ESP32-S3 (Wi-Fi, OTA, rollback)
│   ├── uploaded-firmware-basic/     # Contoh firmware user minimal
│   └── uploaded-firmware-with-libs/ # Contoh firmware user dengan dependensi pihak ketiga
│
├── docs/                            # Dokumentasi teknis & buku panduan
│   ├── OTA_GUIDE.md                 # Panduan konfigurasi partisi & OTA
│   ├── remote-console-azure.md      # Panduan MQTT & deployment Azure
│   └── book/                        # Buku panduan lengkap (LaTeX source + PDF)
│
├── run-dev.sh / run-dev.bat         # Launcher otomatis menjalankan server Python & Rust
├── flash-esp32.ps1                  # Helper PowerShell untuk flash USB awal ESP32
└── .env.example                     # Contoh template konfigurasi environment
```

---

## 4. Modul 1: CSV Profiler & Data Cleaner (Python/Flask)

Modul ini diimplementasikan di [`python/profiler_server.py`](file:///Users/moreno/Document/Project%20Coding/TA%20Rafi%20Cepmek/CSV_PROFILING-main/python/profiler_server.py):

### A. Alur Kerja Profiling
1. User mengunggah dataset CSV melalui antarmuka web.
2. Server menghasilkan identitas unik `job_id` (UUIDv4).
3. Thread background menjalankan `ProfileReport(df, minimal=True/False)`.
4. Progress komputasi dilaporkan secara *real-time* ke browser via **Server-Sent Events (SSE)** pada rute `/progress/<job_id>`.
5. Hasil report HTML disimpan di `python/reports/<job_id>.html`.

### B. Sidecar JSON Metadata
Setiap sesi profiling membuat metadata JSON pendamping (`<job_id>.json`) berisi:
- Dimensi data (jumlah baris dan kolom).
- Kolom dengan missing values beserta jumlahnya.
- Jumlah baris duplikat.
- Identifikasi otomatis kolom numerik, kategorikal, konstanta, datetime, dan outlier berbasis **Interquartile Range (IQR)**:
  $$\text{IQR} = Q_3 - Q_1$$
  $$\text{Batas Bawah} = Q_1 - 1.5 \times \text{IQR}, \quad \text{Batas Atas} = Q_3 + 1.5 \times \text{IQR}$$

### C. Fitur Pembersihan Data (Data Cleaning)
Melalui endpoint `POST /clean/<job_id>`, user dapat melakukan transformasi:
- **Missing Value Handling**: `drop`, `mean`, `median`, `mode`, atau `constant`.
- **Outlier Handling**: `drop` (hapus baris outlier) atau `clip` (batasi nilai ke rentang IQR).
- **Duplicate Rows**: Penghapusan baris ganda.
- **Type Conversion**: Konversi string format angka/tanggal menjadi numeric atau datetime murni.
- **Export**: Dataset bersih disimpan dan dapat diunduh via `/download-cleaned/<job_id>`.

---

## 5. Modul 2: OTA Server & Build Pipeline (Rust/Axum)

Modul ini berada di folder [`ota-server/`](file:///Users/moreno/Document/Project%20Coding/TA%20Rafi%20Cepmek/CSV_PROFILING-main/ota-server) dan dibangun dengan performa tinggi menggunakan ekosistem asynchronous Rust:

### A. Skema Database Relasional (`data/ota.db`)
Database SQLite mengelola 5 entitas utama:
1. `projects`: Metadata proyek sumber (nama, target chip, versi, lokasi folder).
2. `builds`: Riwayat proses build, status (`QUEUED`, `BUILDING`, `SUCCESS`, `FAILED`), log compiler, exit code, dan waktu eksekusi.
3. `firmware`: Binary siap pakai, nama file, ukuran byte, checksum SHA-256, dan versi.
4. `devices`: Daftar ESP32 terdaftar, IP address, versi firmware saat ini, target versi yang diinginkan (`desired_version`), dan progress OTA.
5. `deployments`: Riwayat deployment firmware ke setiap perangkat.

### B. Pipeline Kompilasi Firmware (Local Build)
Ketika user memilih **Upload & Build**:
1. **Validasi & Ekstraksi Arsip**:
   - File `.zip` divalidasi ukuran maksimumnya (`OTA_MAX_UPLOAD_MB`).
   - Ekstraksi menggunakan pengecekan keamanan ketat untuk mencegah serangan *Zip-Slip* (path traversal).
2. **Komposisi Proyek Managed**:
   - Kode aplikasi user (`src/main.rs`) digabungkan sebagai library dependency (`device-app`) ke dalam template runtime ESP32 stabil (`examples/esp32-rust-ota-client`).
3. **Eksekusi Cargo**:
   - Dijalankan perintah:
     ```sh
     cargo build --release --target xtensa-esp32s3-espidf
     ```
   - Menggunakan toolchain Espressif (`+esp`). Seluruh stdout dan stderr compiler ditangkap secara real-time dan disimpan ke database agar dapat diinspeksi jika terjadi kesalahan kompilasi.
4. **Ekstraksi Image OTA (.bin)**:
   - Binary ELF release diproses oleh perintah `espflash`:
     ```sh
     espflash save-image --chip esp32s3 --format esp-idf target/.../app output.bin
     ```
   - *Catatan Penting*: Opsi `--merge` sengaja tidak digunakan karena image OTA hanya ditujukan untuk mengisi partisi aplikasi (`ota_0` / `ota_1`), bukan keseluruhan flash (bootloader dan partition table tetap utuh).
5. **Kalkulasi Checksum**:
   - File `.bin` dihitung nilai SHA-256-nya secara streaming untuk memastikan integritas file saat diunduh ESP32.

---

## 6. Modul 3: Runtime Klien ESP32-S3 & Mekanisme Rollback

Template runtime ESP32 berada di [`examples/esp32-rust-ota-client/`](file:///Users/moreno/Document/Project%20Coding/TA%20Rafi%20Cepmek/CSV_PROFILING-main/examples/esp32-rust-ota-client):

### A. Tata Letak Partisi Flash (`partitions.csv`)
Untuk mendukung OTA dan rollback aman, flash memory dibagi menjadi:
- `nvs`: Penyimpanan konfigurasi non-volatile (Wi-Fi credential, dll).
- `otadata`: Metadata partisi boot aktif yang dikelola oleh bootloader Espressif (2 sektor / 8 KB).
- `phy_init`: Data inisialisasi radio RF.
- `ota_0`: Slot firmware aplikasi A (sekitar 3.25 MB).
- `ota_1`: Slot firmware aplikasi B (sekitar 3.25 MB).

### B. Kontrak Aplikasi Pengguna (API Contract)
Kode pengguna mengimplementasikan dua fungsi tingkat atas:
```rust
pub fn setup(context: &mut AppContext) -> anyhow::Result<State>;
pub fn main(context: AppContext, state: State) -> anyhow::Result<()>;
```
- Inisialisasi periferal (I2C ADS1115, GPIO pompa/katup) diletakkan di dalam `setup()`.
- Loop operasional utama berjalan di dalam `main()`.

### C. Alur Download & Rollback Otomatis
1. **Polling**: Thread `ota-supervisor` melakukan query ke `/api/ota/devices/:device_id/update`.
2. **Download Streaming**: Jika tersedia firmware baru, ESP32 mengunduh file binary dalam blok 4 KiB dan langsung menuliskannya ke partisi OTA yang pasif via `esp_ota_write()`. Memori RAM tidak terbebani oleh file firmware.
3. **Verifikasi Hash**: Nilai SHA-256 dihitung selama streaming download dan dicocokkan dengan header dari server.
4. **Boot Baru**: Jika valid, partisi pasif diatur sebagai boot aktif berikutnya, lalu sistem me-reboot diri.
5. **Validasi Image (Anti-Brick)**:
   - Saat boot pertama kali setelah update, firmware berada dalam status `ESP_OTA_IMG_NEW` (belum terverifikasi).
   - Jika `device_app::setup(&mut context)` berhasil tanpa error, aplikasi memanggil `mark_running_image_valid()`.
   - **Jika `setup()` gagal/panic**, aplikasi memanggil `reject_running_image_and_reboot()`. Bootloader Espressif secara otomatis membatalkan partisi tersebut dan langsung beralih kembali (*rollback*) ke partisi versi sebelumnya yang terbukti stabil.

---

## 7. Modul 4: Remote Console (MQTT/TLS ke WebSocket)

Implementasi di [`ota-server/src/console.rs`](file:///Users/moreno/Document/Project%20Coding/TA%20Rafi%20Cepmek/CSV_PROFILING-main/ota-server/src/console.rs):

- **Ingestion Log**: ESP32 mempublikasikan log level (`ERROR`, `WARN`, `INFO`, `DEBUG`) ke topik MQTT:
  ```text
  devices/<device_id>/logs
  ```
- **In-Memory Ring Buffer**: Server Rust menyimpan hingga 200 baris log terakhir per device (maksimal 256 device aktif) di memori RAM untuk membatasi konsumsi resource.
- **WebSocket Streaming**: Browser terhubung ke `/api/ota/console/ws` menggunakan otentikasi header `CONSOLE_VIEWER_TOKEN`. Begitu tersambung, riwayat log langsung dikirim, disusul log baru secara real-time.

---

## 8. Modul 5: Antarmuka Web Dashboard (Frontend)

Antarmuka web single-page di folder [`web/`](file:///Users/moreno/Document/Project%20Coding/TA%20Rafi%20Cepmek/CSV_PROFILING-main/web) terdiri dari navigasi sidebar:

1. **CSV Profiler**: Area drag & drop file CSV, progress bar real-time, dan tampilan iframe hasil visualisasi EDA.
2. **CSV Runs**: Tabel riwayat sesi dataset yang pernah di-profiling, fitur preview metadata, hapus sesi, dan tombol akses panel cleaning data.
3. **OTA Firmware**: Form upload ZIP source code firmware, pemilihan target chip (`esp32s3`), input versi aplikasi, serta daftar binary firmware hasil kompilasi.
4. **Devices**: Tabel pemantauan perangkat ESP32 di jaringan, status online/offline, IP address, versi firmware saat ini, dan tombol **Deploy** untuk mendistribusikan firmware.
5. **Build History**: Log kompilasi lengkap dari Cargo untuk memeriksa apakah terjadi syntax error atau library mismatch.
6. **Remote Console**: Terminal interaktif di browser untuk melihat output serial log ESP32 secara langsung dari jarak jauh.

---

## 9. Daftar Lengkap REST API & Endpoints

### A. Endpoint CSV Profiler (Python / Port 5000)

| Method | Endpoint | Deskripsi |
|---|---|---|
| `POST` | `/upload` | Mengunggah file CSV dan memulai job profiling |
| `GET` | `/progress/<job_id>` | Stream SSE persentase kemajuan profiling |
| `GET` | `/report/<job_id>` | Menyajikan laporan visual HTML hasil profiling |
| `GET` | `/sessions` | Mengambil daftar seluruh riwayat sesi profiling |
| `GET` | `/session/<job_id>` | Mengambil metadata sidecar JSON dari sesi tertentu |
| `POST` | `/clean/<job_id>` | Mengeksekusi instruksi pembersihan data (cleaning) |
| `GET` | `/download-cleaned/<job_id>` | Mengunduh file CSV yang sudah dibersihkan |
| `DELETE`| `/reset/<job_id>` | Menghapus data dan laporan dari satu sesi |
| `DELETE`| `/reset-all` | Menghapus seluruh sesi dari disk |

### B. Endpoint OTA Service (Rust / Port 7000)

| Method | Endpoint | Deskripsi |
|---|---|---|
| `GET` | `/api/ota/status` | Cek status server OTA dan URL publik |
| `GET` | `/api/ota/projects` | Daftar proyek firmware yang diunggah |
| `POST` | `/api/ota/projects` | Upload arsip ZIP kode sumber firmware baru |
| `POST` | `/api/ota/projects/:id/build` | Memulai proses kompilasi (`cargo build`) |
| `GET` | `/api/ota/builds` | Mengambil histori antrean dan log build |
| `GET` | `/api/ota/builds/:id` | Status detail dan teks log kompilasi tertentu |
| `GET` | `/api/ota/firmware` | Daftar binary firmware (`.bin`) yang siap digunakan |
| `GET` | `/api/ota/firmware/latest` | Mengambil metadata firmware terbaru berdasarkan target chip |
| `GET` | `/api/ota/firmware/:id` | Metadata lengkap firmware (ukuran, hash SHA-256) |
| `GET` | `/api/ota/firmware/:id/download` | Stream download file `.bin` ke ESP32 / browser |
| `DELETE`| `/api/ota/firmware/:id` | Menghapus file binary yang tidak lagi digunakan |
| `POST` | `/api/ota/devices/register` | Endpoint registrasi atau update info perangkat ESP32 |
| `POST` | `/api/ota/devices/:id/heartbeat` | Heartbeat berkala status perangkat |
| `GET` | `/api/ota/devices` | Daftar semua perangkat yang terdaftar di database |
| `POST` | `/api/ota/devices/:id/deploy` | Menetapkan target versi firmware untuk suatu perangkat |
| `GET` | `/api/ota/devices/:id/update` | Endpoint polling ESP32 untuk memeriksa ketersediaan update |
| `POST` | `/api/ota/devices/:id/ota-status` | Callback dari ESP32 saat proses flashing berjalan/selesai |
| `GET` | `/api/ota/console/ws` | Koneksi WebSocket untuk streaming log konsol |

---

## 10. Panduan Konfigurasi (.env) & Cara Menjalankan

### A. Konfigurasi Lingkungan (`.env`)
Salin file `.env.example` ke `.env` pada root direktori:

```env
# IP Komputer Host di Jaringan Lokal (PENTING: Jangan gunakan localhost untuk perangkat fisik)
OTA_PUBLIC_BASE_URL=http://192.168.1.10:7000
OTA_BIND_ADDRESS=0.0.0.0
OTA_PORT=7000

# Kredensial Wi-Fi yang akan disuntikkan ke runtime ESP32 saat kompilasi
WIFI_SSID=Nama_WiFi_Anda
WIFI_PASS=Password_WiFi_Anda
DEVICE_NAME=ESP32-S3 E-Nose Node

# Target toolchain Rust
OTA_RUST_TOOLCHAIN=esp
```

### B. Prasyarat Sistem
1. **Python 3.10+** dengan virtual environment di `python/.venv`.
2. **Rust & Cargo** terpasang (`rustup`).
3. **Toolchain Espressif**:
   ```sh
   espup install
   cargo install espflash
   ```

### C. Menjalankan Server Pengembangan

**Linux / macOS:**
```bash
chmod +x run-dev.sh
./run-dev.sh
```

**Windows:**
```cmd
run-dev.bat
```

Atau dijalankan secara terpisah pada dua terminal:
```bash
# Terminal 1 (Python Profiler)
python3 python/profiler_server.py

# Terminal 2 (Rust OTA Server)
cargo run --manifest-path ota-server/Cargo.toml
```

Akses web browser pada: **`http://localhost:5000`**

---

## 11. Integrasi dengan Tugas Akhir E-Nose

Folder `CSV_PROFILING-main` ini terhubung langsung secara fungsional dengan pilar utama proyek Tugas Akhir E-Nose:

1. **Pemrosesan Data Akuisisi E-Nose**:
   Data pengujian respons 8 sensor gas (MQ-6, MQ-135, MQ-3, MQ-7, TGS 2611, TGS 2602, TGS 2600, TGS 2620) dan sensor suhu/kelembaban DHT22 yang diekspor dari aplikasi desktop GUI (`gui_rust`) ke format CSV dapat di-upload ke web profiler ini. Anda dapat:
   - Menganalisis kurva distribusi tegangan baseline vs chamber.
   - Menghitung matriks korelasi antar sensor gas.
   - Membersihkan data noise/outlier sebelum masuk ke tahap pelatihan model klasifikasi (*Machine Learning*).

2. **Pengembangan Firmware Fleksibel**:
   Ketika algoritma timing siklus pembilasan (*purging*), hisap (*sampling*), atau kecepatan PWM pompa perlu diubah, Anda cukup memodifikasi kode Rust, men-zip folder project, dan mengunggahnya ke dashboard OTA. ESP32-S3 pada modul E-Nose akan memperbarui dirinya sendiri secara nirkabel melalui jaringan Wi-Fi laboratorium tanpa perlu membongkar casing atau mencolokkan kabel data.
