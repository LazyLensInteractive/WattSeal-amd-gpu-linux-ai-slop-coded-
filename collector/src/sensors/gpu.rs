use std::collections::HashMap;

use common::types::InitialInfo;

use super::{Sensor, SensorError, SensorType};
use crate::database::SensorData;

/// GPU hardware vendor identifier.
#[derive(Copy, Clone, PartialEq, Debug)]
pub enum GPUVendor {
    Nvidia,
    Amd,
    Intel,
    Other,
}

impl GPUVendor {
    /// Detects the vendor from a name string (e.g. "NVIDIA GeForce RTX 3070").
    pub fn from_str(vendor_str: &str) -> GPUVendor {
        let vendor_lower = vendor_str.to_lowercase();
        if vendor_lower.contains("nvidia") {
            GPUVendor::Nvidia
        } else if vendor_lower.contains("amd") {
            GPUVendor::Amd
        } else if vendor_lower.contains("intel") {
            GPUVendor::Intel
        } else {
            GPUVendor::Other
        }
    }
}

/// Returns the list of GPU adapter names detected on this system.
#[cfg(target_os = "windows")]
pub fn get_gpu_list() -> Vec<String> {
    use windows::Win32::Graphics::Dxgi::*;

    let mut list = Vec::new();

    unsafe {
        let factory: IDXGIFactory1 = match CreateDXGIFactory1() {
            Ok(f) => f,
            Err(_) => return vec![],
        };

        let mut i = 0;
        loop {
            let adapter = match factory.EnumAdapters1(i) {
                Ok(a) => a,
                Err(_) => break,
            };

            if let Ok(desc) = adapter.GetDesc1() {
                let name = String::from_utf16_lossy(
                    &desc
                        .Description
                        .iter()
                        .take_while(|c| **c != 0)
                        .cloned()
                        .collect::<Vec<u16>>(),
                );
                let name = name.trim();

                // Ignore Microsoft Basic Render Driver fallback driver or empty name
                if name.to_ascii_lowercase().contains("microsoft basic render driver") || name.is_empty() {
                    i += 1;
                    continue;
                }

                list.push(name.to_string());
            }
            i += 1;
        }
    }
    list
}

/// Returns the list of NVIDIA and AMD GPU names detected on Linux.
#[cfg(target_os = "linux")]
pub fn get_gpu_list() -> Vec<String> {
    let mut list: Vec<String> = linux_amd_gpu::list_amd_gpus()
        .into_iter()
        .map(|gpu| gpu.display_name())
        .collect();

    if let Ok(nvml) = nvml_wrapper::Nvml::init() {
        if let Ok(count) = nvml.device_count() {
            list.extend(
                (0..count)
                    .filter_map(|i| nvml.device_by_index(i).ok())
                    .filter_map(|d| d.name().ok()),
            );
        }
    }

    list
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
pub fn get_gpu_list() -> Vec<String> {
    Vec::new()
}

/// Platform-specific GPU power sensor.
pub enum GPUSensor {
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    Nvidia(nvidia_gpu::NvidiaGPUSensor),
    #[cfg(target_os = "windows")]
    Amd(amd_gpu::AmdGPUSensor),
    #[cfg(target_os = "linux")]
    Amd(linux_amd_gpu::LinuxAmdGPUSensor),
    #[cfg(target_os = "windows")]
    Intel { sensor: intel_gpu::IntelGPUSensor },
}

impl Sensor for GPUSensor {
    fn read_full_data(&self) -> Result<SensorData, SensorError> {
        match self {
            #[cfg(any(target_os = "windows", target_os = "linux"))]
            GPUSensor::Nvidia(sensor) => sensor.read_full_data(),
            #[cfg(target_os = "windows")]
            GPUSensor::Amd(sensor) => sensor.read_full_data(),
            #[cfg(target_os = "linux")]
            GPUSensor::Amd(sensor) => sensor.read_full_data(),
            #[cfg(target_os = "windows")]
            GPUSensor::Intel { sensor } => sensor.read_full_data(),
            #[cfg(not(any(target_os = "windows", target_os = "linux")))]
            _ => Err(SensorError::NotSupported),
        }
    }

    fn read_initial_info(&self) -> Result<InitialInfo, SensorError> {
        Ok(InitialInfo::Gpus(get_gpu_list()))
    }

    fn read_name(&self) -> Result<String, SensorError> {
        Ok(format!("Gpu(s): [{}]", get_gpu_list().join(", ")))
    }
}

/// Creates a GPU power sensor appropriate for the given vendor.
pub fn get_gpu_power_sensor(vendor_id: &str, index: u32) -> Result<SensorType, SensorError> {
    let vendor = GPUVendor::from_str(vendor_id);

    #[cfg(target_os = "windows")]
    {
        let sensor = match vendor {
            GPUVendor::Amd => Ok(GPUSensor::Amd(amd_gpu::AmdGPUSensor::new(index)?)),
            GPUVendor::Nvidia => Ok(GPUSensor::Nvidia(nvidia_gpu::NvidiaGPUSensor::new(index)?)),
            GPUVendor::Intel => Ok(GPUSensor::Intel {
                sensor: intel_gpu::IntelGPUSensor::new(index)?,
            }),
            GPUVendor::Other => Err(SensorError::NotSupported),
        };
        return sensor.map(SensorType::GPU);
    }

    #[cfg(target_os = "linux")]
    {
        return match vendor {
            GPUVendor::Amd => linux_amd_gpu::LinuxAmdGPUSensor::new(index).map(|s| SensorType::GPU(GPUSensor::Amd(s))),
            GPUVendor::Nvidia => nvidia_gpu::NvidiaGPUSensor::new(index).map(|s| SensorType::GPU(GPUSensor::Nvidia(s))),
            _ => Err(SensorError::NotSupported),
        };
    }

    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        let _ = (vendor, index);
        Err(SensorError::NotSupported)
    }
}

impl GPUSensor {
    /// Returns per-process GPU utilization percentages.
    pub fn get_process_gpu_usage(&self, current_timestamp: u64) -> Result<HashMap<u32, f64>, SensorError> {
        match self {
            #[cfg(any(target_os = "windows", target_os = "linux"))]
            GPUSensor::Nvidia(sensor) => sensor.get_processes_gpu_usage(current_timestamp),
            #[cfg(target_os = "windows")]
            GPUSensor::Amd(_) | GPUSensor::Intel { .. } => Err(SensorError::NotSupported),
            #[cfg(target_os = "linux")]
            GPUSensor::Amd(_) => Err(SensorError::NotSupported),
            #[cfg(not(any(target_os = "windows", target_os = "linux")))]
            _ => Err(SensorError::NotSupported),
        }
    }

    /// Returns `true` when this GPU is a known integrated GPU model.
    pub fn is_integrated(&self) -> bool {
        match self {
            #[cfg(target_os = "windows")]
            GPUSensor::Intel { .. } => true,
            _ => false,
        }
    }
}

#[cfg(target_os = "linux")]
mod linux_amd_gpu {
    use std::{
        fs,
        path::{Path, PathBuf},
    };

    use super::{Sensor, SensorError};
    use crate::database::{GPUData, SensorData};

    const AMD_PCI_VENDOR_ID: &str = "0x1002";

    #[derive(Clone, Debug)]
    pub struct LinuxAmdGpuDevice {
        card: String,
        device_path: PathBuf,
        device_id: Option<String>,
        marketing_name: Option<String>,
    }

    impl LinuxAmdGpuDevice {
        pub fn display_name(&self) -> String {
            let architecture = self
                .device_id
                .as_deref()
                .and_then(radv_architecture_name)
                .map(|name| format!(" / {name}"))
                .unwrap_or_default();
            let pci_id = self
                .device_id
                .as_deref()
                .map(|id| format!("1002:{id}"))
                .unwrap_or_else(|| "1002:unknown".to_string());
            let marketing_name = self.marketing_name.as_deref().unwrap_or("AMD Radeon Graphics");
            let vendor_name = if marketing_name.to_ascii_lowercase().contains("amd") {
                marketing_name.to_string()
            } else {
                format!("AMD {marketing_name}")
            };

            format!("{vendor_name} (RADV{architecture}, {}, {pci_id})", self.card)
        }
    }

    pub struct LinuxAmdGPUSensor {
        device: LinuxAmdGpuDevice,
    }

    impl LinuxAmdGPUSensor {
        pub fn new(index: u32) -> Result<Self, SensorError> {
            let devices = list_amd_gpus();
            let device = devices
                .get(index as usize)
                .cloned()
                .ok_or_else(|| SensorError::ReadError(format!("No AMD GPU found at Linux DRM index {index}")))?;

            Ok(LinuxAmdGPUSensor { device })
        }
    }

    impl Sensor for LinuxAmdGPUSensor {
        fn read_full_data(&self) -> Result<SensorData, SensorError> {
            let usage_percent = read_f64_file(self.device.device_path.join("gpu_busy_percent"));
            let vram_usage_percent = read_vram_usage_percent(&self.device.device_path);
            let total_power_watts = read_power_watts(&self.device.device_path);

            if usage_percent.is_none() && vram_usage_percent.is_none() && total_power_watts.is_none() {
                return Err(SensorError::ReadError(format!(
                    "amdgpu sysfs telemetry unavailable for {}",
                    self.device.display_name()
                )));
            }

            Ok(GPUData {
                total_power_watts,
                usage_percent,
                vram_usage_percent,
            }
            .into())
        }
    }

    pub fn list_amd_gpus() -> Vec<LinuxAmdGpuDevice> {
        let mut devices: Vec<LinuxAmdGpuDevice> = fs::read_dir("/sys/class/drm")
            .ok()
            .into_iter()
            .flat_map(|entries| entries.filter_map(Result::ok))
            .filter_map(|entry| {
                let card = entry.file_name().to_string_lossy().into_owned();
                if !is_primary_drm_card(&card) {
                    return None;
                }

                let drm_path = entry.path();
                let device_path = drm_path.join("device");
                let vendor = read_trimmed(device_path.join("vendor"))?;
                if !vendor.eq_ignore_ascii_case(AMD_PCI_VENDOR_ID) {
                    return None;
                }

                Some(LinuxAmdGpuDevice {
                    card,
                    device_id: read_trimmed(device_path.join("device")).map(|id| normalize_hex_id(&id)),
                    marketing_name: read_marketing_name(&device_path),
                    device_path,
                })
            })
            .collect();

        devices.sort_by(|a, b| a.card.cmp(&b.card));
        devices
    }

    fn is_primary_drm_card(name: &str) -> bool {
        name.strip_prefix("card")
            .is_some_and(|suffix| !suffix.is_empty() && suffix.chars().all(|ch| ch.is_ascii_digit()))
    }

    fn read_marketing_name(device_path: &Path) -> Option<String> {
        for candidate in ["product_name", "product", "model"] {
            if let Some(value) = read_trimmed(device_path.join(candidate)) {
                if !value.is_empty() {
                    return Some(value);
                }
            }
        }
        None
    }

    fn read_power_watts(device_path: &Path) -> Option<f64> {
        let hwmon_dir = fs::read_dir(device_path.join("hwmon")).ok()?;
        for entry in hwmon_dir.filter_map(Result::ok) {
            let hwmon_path = entry.path();
            let name = read_trimmed(hwmon_path.join("name")).unwrap_or_default();
            if !name.eq_ignore_ascii_case("amdgpu") {
                continue;
            }

            for candidate in ["power1_average", "power1_input"] {
                if let Some(microwatts) = read_f64_file(hwmon_path.join(candidate)) {
                    return Some((microwatts / 1_000_000.0).max(0.0));
                }
            }
        }

        None
    }

    fn read_vram_usage_percent(device_path: &Path) -> Option<f64> {
        let used = read_f64_file(device_path.join("mem_info_vram_used"))?;
        let total = read_f64_file(device_path.join("mem_info_vram_total"))?;
        (total > 0.0).then_some((used / total * 100.0).clamp(0.0, 100.0))
    }

    fn read_f64_file(path: impl AsRef<Path>) -> Option<f64> {
        read_trimmed(path)?.parse::<f64>().ok()
    }

    fn read_trimmed(path: impl AsRef<Path>) -> Option<String> {
        fs::read_to_string(path).ok().map(|s| s.trim().to_string())
    }

    fn normalize_hex_id(id: &str) -> String {
        id.trim_start_matches("0x")
            .trim_start_matches("0X")
            .to_ascii_lowercase()
    }

    fn radv_architecture_name(device_id: &str) -> Option<&'static str> {
        match device_id {
            // Mesa/RADV exposes RDNA 4 as GFX12.  Public Linux reports for
            // Radeon RX 9060 XT show Navi 44 as GFX1200 with PCI ID 1002:7590.
            "7590" => Some("GFX1200 / Navi 44 / RDNA 4"),
            // Keep Navi 48 grouped by architecture even when a board's exact
            // marketing name is unavailable from sysfs.
            "7550" | "7551" | "7552" | "7553" | "7554" | "7555" | "7556" | "7557" | "7558" | "7559" | "755a"
            | "755b" | "755c" | "755d" | "755e" | "755f" => Some("GFX1201 / Navi 48 / RDNA 4"),
            _ => None,
        }
    }
}

#[cfg(target_os = "windows")]
mod amd_gpu {
    use adlx::{gpu_metrics::GpuMetrics, helper::AdlxHelper};

    use super::{Sensor, SensorError};
    use crate::database::{GPUData, SensorData};

    pub struct AmdGPUSensor {
        _helper: AdlxHelper,
        gpu_metrics: GpuMetrics,
    }

    impl AmdGPUSensor {
        pub fn new(index: u32) -> Result<Self, SensorError> {
            let helper = AdlxHelper::new().map_err(|e| SensorError::ReadError(e.to_string()))?;
            let system = helper.system();
            let perfo = system
                .performance_monitoring_services()
                .map_err(|e| SensorError::ReadError(e.to_string()))?;
            let gpu_list = system.gpus().map_err(|e| SensorError::ReadError(e.to_string()))?;

            let gpu = gpu_list.at(index).map_err(|e| SensorError::ReadError(e.to_string()))?;
            let gpu_metrics = perfo
                .current_gpu_metrics(&gpu)
                .map_err(|e| SensorError::ReadError(e.to_string()))?;

            Ok(AmdGPUSensor {
                _helper: helper,
                gpu_metrics,
            })
        }
    }

    impl Sensor for AmdGPUSensor {
        fn read_full_data(&self) -> Result<SensorData, SensorError> {
            // Read AMD GPU data here
            let power_mw = self
                .gpu_metrics
                .power()
                .map_err(|e| SensorError::ReadError(e.to_string()))?;
            let usage = self
                .gpu_metrics
                .usage()
                .map_err(|e| SensorError::ReadError(e.to_string()))?;
            let memory = self
                .gpu_metrics
                .vram()
                .map_err(|e| SensorError::ReadError(e.to_string()))?;

            let data = GPUData {
                total_power_watts: Some(power_mw as f64 / 1000.0),
                usage_percent: Some(usage as f64),
                vram_usage_percent: Some(memory as f64),
            };

            Ok(data.into())
        }
    }
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
mod nvidia_gpu {
    use std::{cell::RefCell, collections::HashMap};

    use nvml_wrapper::Nvml;

    use super::{Sensor, SensorError};
    use crate::database::{GPUData, SensorData};

    pub struct NvidiaGPUSensor {
        nvml: Nvml,
        device_index: u32,
        last_timestamp: RefCell<u64>,
    }

    impl NvidiaGPUSensor {
        pub fn new(index: u32) -> Result<Self, SensorError> {
            let nvml = Nvml::init().map_err(|e| SensorError::ReadError(e.to_string()))?;
            // Validate that the device exists
            let device = nvml
                .device_by_index(index)
                .map_err(|e| SensorError::ReadError(e.to_string()))?;

            let power_probe = device.power_usage();
            let utilization_probe = device.utilization_rates();

            if let (Err(power_err), Err(util_err)) = (&power_probe, &utilization_probe) {
                return Err(SensorError::ReadError(format!(
                    "NVML telemetry unavailable for device index {}: power='{}', utilization='{}'",
                    index, power_err, util_err
                )));
            }

            if let Err(power_err) = power_probe {
                return Err(SensorError::ReadError(format!(
                    "⚠ NVIDIA GPU {} power telemetry unavailable at initialization: {}",
                    index, power_err
                )));
            }
            if let Err(util_err) = utilization_probe {
                return Err(SensorError::ReadError(format!(
                    "⚠ NVIDIA GPU {} utilization telemetry unavailable at initialization: {}",
                    index, util_err
                )));
            }

            Ok(NvidiaGPUSensor {
                nvml,
                device_index: index,
                last_timestamp: RefCell::new(0),
            })
        }

        pub fn get_processes_gpu_usage(&self, current_timestamp: u64) -> Result<HashMap<u32, f64>, SensorError> {
            let mut last_timestamp = self
                .last_timestamp
                .try_borrow_mut()
                .map_err(|_| SensorError::ReadError("Failed to borrow last_timestamp".to_string()))?;
            if *last_timestamp == 0 {
                *last_timestamp = current_timestamp;
                return Ok(HashMap::new());
            }
            let device = self
                .nvml
                .device_by_index(self.device_index)
                .map_err(|e| SensorError::ReadError(e.to_string()))?;
            let processes = device.process_utilization_stats(*last_timestamp);
            *last_timestamp = current_timestamp;
            let mut usage_map = HashMap::new();
            match processes {
                Ok(procs) => {
                    for proc in procs {
                        usage_map.insert(proc.pid, proc.sm_util as f64);
                    }
                    Ok(usage_map)
                }
                Err(e) => Err(SensorError::ReadError(format!(
                    "Failed to get process utilization stats: {}",
                    e
                ))),
            }
        }
    }

    impl Sensor for NvidiaGPUSensor {
        fn read_full_data(&self) -> Result<SensorData, SensorError> {
            // Read NVIDIA GPU data here
            let device = self
                .nvml
                .device_by_index(self.device_index)
                .map_err(|e| SensorError::ReadError(e.to_string()))?;
            let power_usage_mw = device
                .power_usage()
                .map_err(|e| SensorError::ReadError(e.to_string()))?;
            let utilization = device
                .utilization_rates()
                .map_err(|e| SensorError::ReadError(e.to_string()))?;

            let data = GPUData {
                total_power_watts: Some(power_usage_mw as f64 / 1000.0),
                usage_percent: Some(utilization.gpu as f64),
                vram_usage_percent: Some(utilization.memory as f64),
            };

            Ok(data.into())
        }
    }
}

#[cfg(target_os = "windows")]
mod intel_gpu {
    use std::slice;

    use windows::{
        Win32::System::Performance::{
            PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY, PdhAddEnglishCounterW,
            PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW, PdhOpenQueryW,
        },
        core::PCWSTR,
    };

    use super::{Sensor, SensorError};
    use crate::database::{GPUData, SensorData};

    const PDH_MORE_DATA: u32 = 0x800007D2;

    pub struct IntelGPUSensor {
        query: PDH_HQUERY,
        counter: PDH_HCOUNTER,
        initialized: std::cell::Cell<bool>,
    }

    impl IntelGPUSensor {
        pub fn new(_index: u32) -> Result<Self, SensorError> {
            unsafe {
                let mut query = std::mem::MaybeUninit::<PDH_HQUERY>::uninit();
                if PdhOpenQueryW(None, 0, query.as_mut_ptr()) != 0 {
                    return Err(SensorError::ReadError("PdhOpenQuery failed".to_string()));
                }
                let query = query.assume_init();

                let path: Vec<u16> = "\\GPU Engine(*)\\Utilization Percentage\0".encode_utf16().collect();
                let mut counter = std::mem::MaybeUninit::<PDH_HCOUNTER>::uninit();
                if PdhAddEnglishCounterW(query, PCWSTR(path.as_ptr()), 0, counter.as_mut_ptr()) != 0 {
                    let _ = PdhCloseQuery(query);
                    return Err(SensorError::ReadError("PdhAddEnglishCounter failed".to_string()));
                }
                let counter = counter.assume_init();

                Ok(IntelGPUSensor {
                    query,
                    counter,
                    initialized: std::cell::Cell::new(false),
                })
            }
        }
    }

    impl Drop for IntelGPUSensor {
        fn drop(&mut self) {
            unsafe {
                let _ = PdhCloseQuery(self.query);
            }
        }
    }

    impl Sensor for IntelGPUSensor {
        fn read_full_data(&self) -> Result<SensorData, SensorError> {
            unsafe {
                PdhCollectQueryData(self.query);
                if !self.initialized.get() {
                    self.initialized.set(true);
                    PdhCollectQueryData(self.query);
                }
                let (mut size, mut count) = (0u32, 0u32);
                if PdhGetFormattedCounterArrayW(self.counter, PDH_FMT_DOUBLE, &mut size, &mut count, None)
                    != PDH_MORE_DATA
                {
                    return Ok(GPUData {
                        total_power_watts: None,
                        usage_percent: Some(0.0),
                        vram_usage_percent: None,
                    }
                    .into());
                }
                let mut buf = vec![0u8; size as usize];
                let items = buf.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W;
                if PdhGetFormattedCounterArrayW(self.counter, PDH_FMT_DOUBLE, &mut size, &mut count, Some(items)) != 0 {
                    return Ok(GPUData {
                        total_power_watts: None,
                        usage_percent: Some(0.0),
                        vram_usage_percent: None,
                    }
                    .into());
                }
                let max = slice::from_raw_parts(items, count as usize)
                    .iter()
                    .filter(|i| i.FmtValue.CStatus == 0)
                    .filter_map(|i| {
                        let v = i.FmtValue.Anonymous.doubleValue;
                        v.is_finite().then_some(v)
                    })
                    .fold(0.0f64, f64::max);
                Ok(GPUData {
                    total_power_watts: None,
                    usage_percent: Some(max.clamp(0.0, 100.0)),
                    vram_usage_percent: None,
                }
                .into())
            }
        }
    }
}
