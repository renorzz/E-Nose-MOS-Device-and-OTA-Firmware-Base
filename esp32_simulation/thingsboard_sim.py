#!/usr/bin/env python3
"""
=============================================================================
ELECTRONIC NOSE - THINGSBOARD SIMULATOR (PYTHON CLI)
Simulasi Pengiriman Data Telemetri E-Nose ke ThingsBoard Cloud tanpa Hardware
=============================================================================
✅ Mengirimkan 8 Sensor Gas (MQ & TGS dalam mV dengan kurva realistis)
✅ Mengirimkan 1 Sensor DHT22 Environment (Suhu & Kelembapan)
✅ Mengirimkan Status Pompa & Valve (Inlet & Outlet)
✅ Otomasi Siklus 3 Fase (70s Idle -> 100s Inlet -> 130s Outlet -> 300s Finish)
✅ Kompatibel 100% dengan ThingsBoard Cloud tanpa perlu install library tambahan!
=============================================================================
"""

import sys
import time
import math
import random
import json
import threading
import urllib.request
import urllib.error

# ─── KONFIGURASI KREDENSIAL THINGSBOARD ─────────────────────────────────────
TB_HOST = "thingsboard.cloud"
TB_TOKEN = "sIP5Dc6dl1GLn4Enxr9Q"  # Token perangkat ThingsBoard Anda
TELEMETRY_URL = f"http://{TB_HOST}/api/v1/{TB_TOKEN}/telemetry"

# ─── BASELINE & PEAK VOLTAGE (dalam mV) ──────────────────────────────────────
BASELINES = {
    "mq6": 460.0,
    "mq135": 530.0,
    "mq3": 390.0,
    "mq7": 610.0,
    "tgs2611": 430.0,
    "tgs2602": 560.0,
    "tgs2600": 400.0,
    "tgs2620": 490.0,
}

PEAKS = {
    "mq6": 1850.0,
    "mq135": 2400.0,
    "mq3": 2950.0,
    "mq7": 1780.0,
    "tgs2611": 2380.0,
    "tgs2602": 2820.0,
    "tgs2600": 2120.0,
    "tgs2620": 2640.0,
}

# ─── STATUS SISTEM ──────────────────────────────────────────────────────────
class SystemState:
    def __init__(self):
        self.running = False
        self.cycle_start_time = 0.0
        self.inlet_pump = False
        self.inlet_valve = False
        self.outlet_pump = False
        self.outlet_valve = False
        self.lock = threading.Lock()

    def start_cycle(self):
        with self.lock:
            self.running = True
            self.cycle_start_time = time.time()
            self.inlet_pump = False
            self.inlet_valve = False
            self.outlet_pump = False
            self.outlet_valve = False

    def stop_cycle(self):
        with self.lock:
            self.running = False
            self.inlet_pump = False
            self.inlet_valve = False
            self.outlet_pump = False
            self.outlet_valve = False

    def get_elapsed(self) -> int:
        if not self.running:
            return 0
        elapsed = int(time.time() - self.cycle_start_time)
        return max(1, elapsed)

    def get_inlet_time(self) -> int:
        el = self.get_elapsed()
        if 71 <= el <= 170:
            return el - 70
        elif el > 170:
            return 100
        return 0

    def get_outlet_time(self) -> int:
        el = self.get_elapsed()
        if 171 <= el <= 300:
            return el - 170
        elif el > 300:
            return 130
        return 0

state = SystemState()

def calculate_sensors(elapsed: int, is_running: bool, inlet_active: bool, outlet_active: bool):
    """Menghitung respons sensor berdasarkan fase pengujian."""
    # Menentukan faktor respons gas (0.0: baseline, 1.0: peak)
    if not is_running or elapsed <= 70:
        factor = 0.0
    elif elapsed <= 170:
        # Fase 2: Inlet ON (Kenaikan gas eksponensial)
        progress = (elapsed - 70) / 100.0
        factor = 1.0 - math.exp(-2.6 * progress)
    elif elapsed <= 300:
        # Fase 3: Outlet ON (Pembersihan / desorpsi chamber)
        progress = (elapsed - 170) / 130.0
        factor = math.exp(-3.2 * progress)
    else:
        factor = 0.0

    sensors = {}
    for key in BASELINES:
        base = BASELINES[key]
        peak = PEAKS[key]
        noise = random.uniform(-10.0, 10.0)
        val = max(50.0, base + (peak - base) * factor + noise)
        sensors[key] = round(val, 2)

    # Suhu & Kelembaban DHT22 Environment
    sensors["dht_temp"] = round(27.5 + random.uniform(-0.2, 0.2), 2)
    sensors["dht_hum"]  = round(62.0 + random.uniform(-0.7, 0.7), 2)

    return sensors

def send_to_thingsboard(payload: dict) -> bool:
    """Mengirim telemetri ke ThingsBoard via HTTP REST API."""
    data_bytes = json.dumps(payload).encode("utf-8")
    req = urllib.request.Request(
        TELEMETRY_URL,
        data=data_bytes,
        headers={"Content-Type": "application/json"},
        method="POST"
    )
    try:
        with urllib.request.urlopen(req, timeout=5) as response:
            return response.status == 200
    except Exception as e:
        print(f"\n❌ [ThingsBoard Error] Gagal mengirim data: {e}")
        return False

def simulation_loop():
    """Loop simulasi telemetri tiap interval."""
    interval = 1.0  # kirim terus menerus tiap 1 detik
    last_phase = ""

    while True:
        elapsed = state.get_elapsed()
        running = state.running

        # Logika Fase
        if running:
            if 1 <= elapsed <= 70:
                phase_name = "FASE 1: PURGE / IDLE (1-70s)"
                state.inlet_pump = False
                state.inlet_valve = False
                state.outlet_pump = False
                state.outlet_valve = False
            elif 71 <= elapsed <= 170:
                phase_name = "FASE 2: INLET ON (71-170s) - HISAP SAMPEL"
                state.inlet_pump = True
                state.inlet_valve = True
                state.outlet_pump = False
                state.outlet_valve = False
            elif 171 <= elapsed <= 300:
                phase_name = "FASE 3: OUTLET ON (171-300s) - BUANG / PURGING"
                state.inlet_pump = False
                state.inlet_valve = False
                state.outlet_pump = True
                state.outlet_valve = True
            else:
                phase_name = "SIKLUS SELESAI (300s)"
                state.stop_cycle()
                phase_name = "STANDBY"
        else:
            phase_name = "STANDBY (Idle)"

        if phase_name != last_phase and running:
            print(f"\n🔔 >>> PERUBAHAN STATUS: {phase_name}")
            last_phase = phase_name

        # Hitung sensor & paket telemetri
        sensors = calculate_sensors(elapsed, running, state.inlet_pump, state.outlet_pump)
        payload = {
            **sensors,
            "inlet_pump": state.inlet_pump,
            "inlet_valve": state.inlet_valve,
            "outlet_pump": state.outlet_pump,
            "outlet_valve": state.outlet_valve,
            "inlet_time": state.get_inlet_time(),
            "outlet_time": state.get_outlet_time(),
            "elapsed_time": elapsed,
            "running": running
        }

        # Kirim ke Thingsboard
        success = send_to_thingsboard(payload)
        status_sym = "✅ OK (200)" if success else "❌ FAILED"

        # Format visual terminal ringkas
        print(f"\r[{time.strftime('%H:%M:%S')}] Status: {phase_name:<28} | Elapsed: {elapsed:>3}s | Inlet: {state.inlet_pump} | Outlet: {state.outlet_pump} | TB: {status_sym}", end="")

        time.sleep(interval)

def main():
    print("=" * 70)
    print("  🧪 ELECTRONIC NOSE - THINGSBOARD SIMULATOR")
    print(f"  🌐 Host : {TB_HOST}")
    print(f"  🔑 Token: {TB_TOKEN[:6]}...{TB_TOKEN[-4:]}")
    print("=" * 70)
    print("Perintah yang tersedia:")
    print("  'start' : Mulai siklus pengujian 3-fase (300 detik)")
    print("  'stop'  : Hentikan siklus pengujian")
    print("  'status': Lihat detail data sensor saat ini")
    print("  'quit'  : Keluar dari program simulasi")
    print("-" * 70)

    # Jalankan background sender thread
    t = threading.Thread(target=simulation_loop, daemon=True)
    t.start()

    # Opsi jika dijalankan dengan flag --auto-start
    if "--auto-start" in sys.argv:
        print("\n🚀 [Auto-Start] Memulai siklus pengujian...")
        state.start_cycle()

    try:
        while True:
            cmd = input().strip().lower()
            if cmd == "start":
                if not state.running:
                    print("\n🚀 Memulai Siklus Pengujian...")
                    state.start_cycle()
                else:
                    print("\n⚠️ Siklus sedang berjalan!")
            elif cmd == "stop":
                if state.running:
                    print("\n⏹️ Menghentikan Siklus Pengujian...")
                    state.stop_cycle()
                else:
                    print("\nℹ️ Siklus sudah dalam keadaan mati/standby.")
            elif cmd == "status":
                el = state.get_elapsed()
                s = calculate_sensors(el, state.running, state.inlet_pump, state.outlet_pump)
                print("\n--- DATA SENSOR TERAKHIR ---")
                print(f"MQ-6: {s['mq6']} mV | MQ-135: {s['mq135']} mV | MQ-3: {s['mq3']} mV | MQ-7: {s['mq7']} mV")
                print(f"TGS2611: {s['tgs2611']} mV | TGS2602: {s['tgs2602']} mV | TGS2600: {s['tgs2600']} mV | TGS2620: {s['tgs2620']} mV")
                print(f"DHT Env: {s['dht_temp']} °C, {s['dht_hum']} %")
                print(f"Inlet Time: {state.get_inlet_time()}s | Outlet Time: {state.get_outlet_time()}s | Elapsed: {el}s | Running: {state.running}")
                print("----------------------------")
            elif cmd in ["quit", "exit", "q"]:
                print("\nKeluar dari program simulasi. Sampai jumpa!")
                break
            elif cmd == "":
                continue
            else:
                print(f"\nPerintah '{cmd}' tidak dikenali. Ketik: start / stop / status / quit")
    except (KeyboardInterrupt, EOFError):
        print("\nProgram dihentikan oleh pengguna.")

if __name__ == "__main__":
    main()
