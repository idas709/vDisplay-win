use anyhow::{anyhow, Context, Result};
use parsec_vdd_rust::{close_device_handle, display::ParsecDisplay, get_all_displays, open_device_handle, query_device_status, vdd_add_display, vdd_remove_display, vdd_update, DeviceStatus, VDD_ADAPTER_GUID, VDD_CLASS_GUID, VDD_HARDWARE_ID};
use std::sync::{atomic::{AtomicBool, Ordering}, Arc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::HANDLE;

pub struct DriverSession {
    handle: HANDLE,
    display_index: i32,
    pub display: ParsecDisplay,
    stopping: Arc<AtomicBool>,
    heartbeat: Option<JoinHandle<()>>,
}

impl DriverSession {
    pub fn resolution_options(&mut self) -> Result<(Vec<parsec_vdd_rust::display::Mode>, Option<parsec_vdd_rust::display::Mode>)> {
        let display = self.refresh_display().context("仮想ディスプレイが見つかりません。")?;
        self.check_independent_output(&display)?;
        let current = display.current_mode.clone();
        let modes = display.supported_resolutions.iter().filter_map(|resolution| {
            let hz = current.as_ref().map(|m| m.hz).filter(|hz| resolution.refresh_rates.contains(hz))
                .or_else(|| resolution.refresh_rates.iter().copied().find(|hz| *hz == 60))
                .or_else(|| resolution.refresh_rates.first().copied())?;
            Some(parsec_vdd_rust::display::Mode::new(resolution.width, resolution.height, hz))
        }).collect();
        Ok((modes, current))
    }

    fn check_independent_output(&self, target: &ParsecDisplay) -> Result<()> {
        use windows::core::PCWSTR;
        use windows::Win32::Graphics::Gdi::{EnumDisplayDevicesW, DISPLAY_DEVICEW, DISPLAY_DEVICE_ACTIVE};
        anyhow::ensure!(target.active && !target.device_name.is_empty(), "仮想ディスプレイが切断されています。Windowsの表示設定で拡張表示にしてください。");
        let name: Vec<u16> = target.device_name.encode_utf16().chain(Some(0)).collect();
        let mut active = Vec::new();
        for index in 0..64 {
            let mut device = DISPLAY_DEVICEW::default();
            device.cb = std::mem::size_of_val(&device) as u32;
            if !unsafe { EnumDisplayDevicesW(PCWSTR(name.as_ptr()), index, &mut device, 1) }.as_bool() { break; }
            if device.StateFlags.contains(DISPLAY_DEVICE_ACTIVE) {
                active.push(String::from_utf16_lossy(&device.DeviceID).trim_end_matches('\0').to_lowercase());
            }
        }
        tracing::debug!(?active, monitor_instance = %target.monitor_instance, "resolution output identity check");
        anyhow::ensure!(active.len() == 1 && active[0].contains(&target.monitor_instance.to_lowercase().replace('\\', "#")),
            "仮想ディスプレイが複製表示または未接続のため、解像度を変更できません。Windowsの表示設定で拡張表示にしてください。");
        Ok(())
    }

    pub fn set_resolution(&mut self, requested: &parsec_vdd_rust::display::Mode) -> Result<()> {
        use windows::core::PCWSTR;
        use windows::Win32::Graphics::Gdi::*;
        let (options, _) = self.resolution_options()?;
        anyhow::ensure!(options.contains(requested), "この解像度は現在の仮想ディスプレイでは使用できません。");
        let name: Vec<u16> = self.display.device_name.encode_utf16().chain(Some(0)).collect();
        let mut mode = DEVMODEW::default();
        mode.dmSize = std::mem::size_of_val(&mode) as u16;
        anyhow::ensure!(unsafe { EnumDisplaySettingsW(PCWSTR(name.as_ptr()), ENUM_CURRENT_SETTINGS, &mut mode) }.as_bool(), "現在の解像度を取得できません。");
        mode.dmPelsWidth = requested.width as u32;
        mode.dmPelsHeight = requested.height as u32;
        mode.dmDisplayFrequency = requested.hz as u32;
        mode.dmFields = DM_PELSWIDTH | DM_PELSHEIGHT | DM_DISPLAYFREQUENCY;
        for flags in [CDS_TEST, CDS_TYPE(0)] {
            let result = unsafe { ChangeDisplaySettingsExW(PCWSTR(name.as_ptr()), Some(&mode), None, flags, None) };
            tracing::info!(name = %self.display.device_name, ?requested, flags = flags.0, result = result.0, "virtual resolution change");
            anyhow::ensure!(result == DISP_CHANGE_SUCCESSFUL, "解像度を変更できませんでした (DISP_CHANGE: {})。", result.0);
        }
        Ok(())
    }

    pub fn start() -> Result<Self> {
        let reported_status = query_device_status(&VDD_CLASS_GUID, VDD_HARDWARE_ID);
        if reported_status != DeviceStatus::Ok {
            // SetupAPI hardware-ID discovery varies between Parsec package and
            // Windows versions. The device interface is the authoritative test:
            // continue and try opening it instead of rejecting a usable driver.
            tracing::warn!(status = ?reported_status,
                "Parsec VDD status probe was not ready; attempting the device interface directly");
        }
        let handle = open_device_handle(&VDD_ADAPTER_GUID).ok_or_else(||
            anyhow!("failed to open Parsec VDD adapter (status probe: {reported_status:?})"))?;
        tracing::info!(status = ?reported_status, "Parsec VDD device interface opened");
        let stopping = Arc::new(AtomicBool::new(false));
        let heartbeat_stop = Arc::clone(&stopping);
        let heartbeat_handle = handle.0 as isize;
        // Keep the driver alive even while monitor arrival/identification is pending.
        let heartbeat = match thread::Builder::new().name("parsec-vdd-heartbeat".to_string()).spawn(move || {
            let handle = HANDLE(heartbeat_handle as *mut std::ffi::c_void);
            let mut last_log = Instant::now();
            let mut failures = 0u64;
            while !heartbeat_stop.load(Ordering::Acquire) {
                match vdd_update(handle) {
                    Ok(()) => {
                        if failures != 0 || last_log.elapsed() >= Duration::from_secs(5) {
                            tracing::debug!(failures, "Parsec VDD heartbeat succeeded");
                            last_log = Instant::now();
                        }
                        failures = 0;
                    }
                    Err(error) => {
                        failures += 1;
                        if failures == 1 || last_log.elapsed() >= Duration::from_secs(5) {
                            tracing::warn!(%error, failures, "Parsec VDD heartbeat failed; retrying");
                            last_log = Instant::now();
                        }
                    }
                }
                thread::sleep(Duration::from_millis(50));
            }
        }) {
            Ok(thread) => thread,
            Err(error) => { close_device_handle(handle); return Err(error).context("failed to start Parsec VDD heartbeat"); }
        };
        let mut added_index = None;
        let result = (|| -> Result<(i32, ParsecDisplay)> {
            let index = vdd_add_display(handle)?;
            added_index = Some(index);
            for _ in 0..50 {
                let candidates: Vec<_> = get_all_displays().into_iter().filter(|d| d.display_index() == index).collect();
                if candidates.len() == 1 {
                    return Ok((index, candidates.into_iter().next().unwrap()));
                }
                thread::sleep(Duration::from_millis(100));
            }
            Err(anyhow!("Parsec monitor with driver index {index} did not arrive within 5 seconds"))
        })();
        let (display_index, mut monitor) = match result {
            Ok(value) => value,
            Err(error) => {
                stopping.store(true, Ordering::Release);
                let _ = heartbeat.join();
                if let Some(index) = added_index { let _ = vdd_remove_display(handle, index); }
                close_device_handle(handle);
                return Err(error);
            }
        };
        tracing::info!(display_index, address = monitor.address, monitor_instance = %monitor.monitor_instance, name = %monitor.device_name, active = monitor.active, "Parsec monitor identified");
        if !monitor.change_mode(None, None, None, Some((1920, 0)), None) {
            tracing::warn!(name = %monitor.device_name, "could not place the Parsec monitor in the extended desktop");
        }
        Ok(Self { handle, display_index, display: monitor, stopping, heartbeat: Some(heartbeat) })
    }

    pub fn refresh_display(&mut self) -> Option<ParsecDisplay> {
        let displays = get_all_displays();
        for monitor in &displays {
            tracing::info!(address = monitor.address, monitor_instance = %monitor.monitor_instance,
                name = %monitor.device_name, active = monitor.active, clone_of = monitor.clone_of,
                hmonitor = ?monitor.get_hmonitor_id(), mode = ?monitor.current_mode, "Parsec monitor enumerated");
        }
        let candidates: Vec<_> = displays.into_iter().filter(|d|
            d.monitor_instance == self.display.monitor_instance
        ).collect();
        if candidates.len() != 1 {
            tracing::warn!(address = self.display.address, matches = candidates.len(), "owned Parsec monitor missing or ambiguous");
            return None;
        }
        let monitor = candidates.into_iter().next().unwrap();
        self.display = monitor.clone();
        Some(monitor)
    }
}

impl Drop for DriverSession {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::Release);
        if let Some(heartbeat) = self.heartbeat.take() { let _ = heartbeat.join(); }
        if let Err(error) = vdd_remove_display(self.handle, self.display_index) { tracing::warn!(%error, "failed to remove Parsec virtual monitor"); }
        close_device_handle(self.handle);
    }
}
