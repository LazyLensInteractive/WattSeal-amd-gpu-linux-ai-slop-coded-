<div align="center">

<img src="resources/svg/banner.svg" alt="WattSeal, real-time Linux power monitoring" width="100%"/>

# WattSeal

Real-time Linux power monitoring for CPUs, GPUs, memory, storage, network, and processes.

**This branch keeps the existing quick-install flow and adds AMD GPU telemetry on Linux, with the AMD path built around the open-source Linux `amdgpu`/DRM sysfs interfaces used alongside Mesa RADV rather than AMD-only proprietary tooling.**

[![Linux](https://img.shields.io/badge/Linux-x86__64-FCC624?style=flat-square&logo=linux&logoColor=black)](https://github.com/daminoup88/wattseal/releases)
[![AMD RDNA 4](https://img.shields.io/badge/AMD_RDNA_4-GFX12-red?style=flat-square)](https://docs.mesa3d.org/drivers/radv.html)
[![GPL-3.0](https://img.shields.io/badge/License-GPL--3.0-blue?style=flat-square)](LICENSE)

<img src="resources/dashboard.png" alt="WattSeal app dashboard showing real-time power consumption breakdown by application and component" width="80%" style="border-radius: 12px; box-shadow: 0 4px 12px rgba(0, 0, 0, 0.1); margin-top: 20px;"/>

</div>

---

## Quick install

Keep using the same Linux executable that was already available:

```bash
chmod +x WattSeal-linux
sudo ./WattSeal-linux
```

Why `sudo` is still recommended:

- Linux CPU energy counters can require elevated access depending on kernel and distro policy.
- AMD GPU telemetry is read from `/sys/class/drm/card*/device` and `/sys/class/drm/card*/device/hwmon/hwmon*`; many distros expose those files to normal users, but power files can be restricted by udev rules.
- NVIDIA telemetry still uses NVML where available.

If you need a desktop launcher, keep using `resources/linux/WattSeal.desktop` and point it at the same `WattSeal-linux` binary.

---

## What changed for AMD GPUs on Linux

WattSeal now detects Linux AMD DRM devices directly from sysfs:

- GPU discovery: `/sys/class/drm/card*/device/vendor` with AMD PCI vendor ID `0x1002`.
- Utilization: `gpu_busy_percent` from the `amdgpu` kernel driver.
- VRAM usage: `mem_info_vram_used` and `mem_info_vram_total`.
- Board/GPU power: `hwmon` files such as `power1_average` or `power1_input` when the kernel exposes them.

This keeps the Linux AMD path aligned with open-source driver stacks:

- Kernel side: DRM + `amdgpu` sysfs/hwmon telemetry.
- Userspace graphics side: Mesa RADV naming and GFX generation conventions.
- No ADLX, ROCm, AMDGPU-PRO, or proprietary AMD library is required for this telemetry path.

---

## RDNA 4 / RADV focus

RDNA 4 support is treated as a first-class Linux target:

| RDNA 4 family | RADV/GFX label used by WattSeal when known | Notes |
|---|---|---|
| Navi 44 | `GFX1200 / Navi 44 / RDNA 4` | Includes public RX 9060 XT Linux reports such as PCI ID `1002:7590`. |
| Navi 48 | `GFX1201 / Navi 48 / RDNA 4` | Grouped by the known Navi 48/RDNA 4 ID range when sysfs does not expose a board marketing name. |
| Unknown/new RDNA 4 IDs | `AMD Radeon Graphics (RADV, cardN, 1002:xxxx)` | Still detected and monitored through `amdgpu` sysfs if the kernel exposes telemetry. |

The app does **not** require the Vulkan RADV driver to be loaded before it can read power/usage telemetry. RADV is used here as the open-source naming and architecture reference point, while the actual readings come from the kernel DRM device.

---

## Current sensor support

| Component | Linux support | Data source |
|---|---:|---|
| CPU | ✅ | RAPL/scaphandre path when available, otherwise estimates |
| AMD GPU | ✅ | Open-source `amdgpu` DRM sysfs + hwmon |
| AMD RDNA 4 GPU | ✅ | Same AMD path, with RADV/GFX12-friendly labels |
| NVIDIA GPU | ✅ | NVML |
| Intel GPU | Planned | Not currently enabled on Linux |
| RAM | ✅ | Estimated from memory usage |
| Disk | ✅ | Estimated from read/write activity |
| Network | ✅ | Estimated from throughput |
| Per-process | ✅ | CPU, memory, and I/O; NVIDIA process GPU usage when NVML supports it |

> AMD per-process GPU attribution is not yet available in this branch because the open sysfs path provides device-level telemetry, not per-process GPU engine utilization.

---

## Linux requirements

Recommended for AMD RDNA 4 systems:

- A recent Linux kernel with RDNA 4 `amdgpu` support enabled.
- Recent Mesa with RADV support for GFX12/RDNA 4 if you also want Vulkan workloads to run through RADV.
- `amdgpu` loaded for the GPU you want to monitor.
- Access to `/sys/class/drm/card*/device/gpu_busy_percent` and any available `hwmon` power files.

Optional runtime dependency:

- An X11 tray library. If either `libappindicator` or `libayatana-appindicator` is installed, WattSeal shows a tray icon with menu items. Without it, WattSeal still runs and the dashboard can be reopened by running the app again.

---

## Troubleshooting AMD GPU detection

### Check that Linux sees the AMD card

```bash
for card in /sys/class/drm/card*/device; do
  printf '%s vendor=' "$card"
  cat "$card/vendor" 2>/dev/null || true
  printf '%s device=' "$card"
  cat "$card/device" 2>/dev/null || true
done
```

AMD cards should report vendor `0x1002`.

### Check open `amdgpu` telemetry files

```bash
for card in /sys/class/drm/card*/device; do
  [ "$(cat "$card/vendor" 2>/dev/null)" = "0x1002" ] || continue
  echo "== $card =="
  cat "$card/gpu_busy_percent" 2>/dev/null || echo "gpu_busy_percent unavailable"
  cat "$card/mem_info_vram_used" 2>/dev/null || true
  cat "$card/mem_info_vram_total" 2>/dev/null || true
  for hwmon in "$card"/hwmon/hwmon*; do
    [ -d "$hwmon" ] || continue
    echo "hwmon: $hwmon ($(cat "$hwmon/name" 2>/dev/null))"
    cat "$hwmon/power1_average" 2>/dev/null || cat "$hwmon/power1_input" 2>/dev/null || true
  done
done
```

If the files exist but WattSeal cannot read power as a normal user, run WattSeal with `sudo` or add distro-specific udev permissions for the relevant `hwmon` files.

### The GPU appears but power is blank

Some boards or kernel versions expose utilization and VRAM counters but not instantaneous board power. WattSeal will still report the available usage fields and leave total GPU watts empty rather than fabricating an AMD GPU power estimate.

---

## Development notes

Build the Linux collector/app with Cargo as before:

```bash
cargo check
cargo build --release
```

The AMD Linux implementation is intentionally dependency-light. It uses Rust standard-library filesystem reads and existing Linux kernel interfaces so it remains compatible with open Mesa/RADV setups and avoids binding the project to proprietary AMD SDKs.

---

## License

WattSeal is released under the [GPL-3.0 license](LICENSE).
