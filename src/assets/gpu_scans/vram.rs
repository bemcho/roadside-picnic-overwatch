//! The driver's live VRAM budget (Vulkan's `VK_EXT_memory_budget`), and the
//! scan budget that follows it.

use bevy::prelude::*;
use bevy::render::renderer::RenderDevice;

use crate::launch::{DEFAULT_VRAM_GB, LaunchOptions};

use super::{GpuScans, VramReading, gb, gib, live_scan_budget};

/// Seconds between two readings of the driver's budget.
const READ_EVERY: f32 = 1.0;

/// About once a second, sets the scan budget from the driver's live reading,
/// capped by `--vram`. Where the driver cannot report one, the fallback
/// `GpuScansPlugin` set stays.
pub fn track_vram_budget(
    mut scans: ResMut<GpuScans>,
    device: Res<RenderDevice>,
    launch: Res<LaunchOptions>,
    time: Res<Time<Real>>,
    // 0 until the first reading.
    mut next: Local<f32>,
) {
    let now = time.elapsed_secs();
    if now < *next {
        return;
    }
    let first = *next == 0.0;
    *next = now + READ_EVERY;
    let Some(reading) = read_vram(&device) else {
        if first {
            let assumed = launch.vram_gb.unwrap_or(DEFAULT_VRAM_GB);
            let s = gb(scans.budget());
            warn!(
                "The driver cannot report VRAM; assuming {assumed} GB, so UHD scans may use {s:.1} GB"
            );
        }
        return;
    };
    let budget = live_scan_budget(reading, scans.resident_bytes(), launch.vram_gb.map(gib));
    scans.set_budget(budget);
    if first {
        let (b, u, s) = (gb(reading.budget), gb(reading.usage), gb(budget));
        info!("VRAM: driver budget {b:.1} GB, game using {u:.1} GB; UHD scans may use {s:.1} GB");
    }
}

/// The budget and usage of the largest device-local heap. `heaps` holds each
/// heap's (device-local, size); `budgets` and `usages` are in the same order.
/// `None` without a device-local heap, or when its budget is 0 (the driver
/// does not report one).
pub fn heap_reading(heaps: &[(bool, u64)], budgets: &[u64], usages: &[u64]) -> Option<VramReading> {
    let (heap, _) = heaps
        .iter()
        .enumerate()
        .filter(|(_, (device_local, _))| *device_local)
        .max_by_key(|(_, (_, size))| *size)?;
    let budget = *budgets.get(heap)?;
    let usage = *usages.get(heap)?;
    (budget > 0).then_some(VramReading { budget, usage })
}

/// The driver's reading for the game's device, where it runs on Vulkan with
/// `VK_EXT_memory_budget` (wgpu enables it whenever the driver offers it).
// The platforms where wgpu builds its Vulkan backend.
#[cfg(any(
    windows,
    target_os = "linux",
    target_os = "android",
    target_os = "freebsd"
))]
pub fn read_vram(device: &RenderDevice) -> Option<VramReading> {
    use ash::vk;
    use wgpu::hal::api::Vulkan;

    // SAFETY: the hal device is only read from (its instance and physical
    // device); nothing is created or destroyed through it, and the guard drops
    // at the end of this function.
    let hal = unsafe { device.wgpu_device().as_hal::<Vulkan>() }?;
    let instance = hal.shared_instance();
    let has_budget = hal
        .enabled_device_extensions()
        .contains(&ash::ext::memory_budget::NAME);
    if !has_budget || instance.instance_api_version() < vk::API_VERSION_1_1 {
        return None;
    }
    let mut budget = vk::PhysicalDeviceMemoryBudgetPropertiesEXT::default();
    let mut properties = vk::PhysicalDeviceMemoryProperties2::default().push_next(&mut budget);
    // SAFETY: the physical device belongs to this instance, the instance is
    // Vulkan 1.1 or later (the query is core there), and the chained budget
    // struct's extension is enabled on the device. It is the query wgpu-hal
    // makes in `vulkan/device.rs` `get_memory_properties`.
    unsafe {
        instance
            .raw_instance()
            .get_physical_device_memory_properties2(hal.raw_physical_device(), &mut properties);
    }
    let memory = properties.memory_properties;
    let count = (memory.memory_heap_count as usize).min(vk::MAX_MEMORY_HEAPS);
    let heaps: Vec<_> = memory.memory_heaps[..count]
        .iter()
        .map(|heap| {
            let device_local = heap.flags.contains(vk::MemoryHeapFlags::DEVICE_LOCAL);
            (device_local, heap.size)
        })
        .collect();
    heap_reading(
        &heaps,
        &budget.heap_budget[..count],
        &budget.heap_usage[..count],
    )
}

/// No Vulkan backend here (the web, macOS): the fallback budget stays.
#[cfg(not(any(
    windows,
    target_os = "linux",
    target_os = "android",
    target_os = "freebsd"
)))]
pub fn read_vram(_device: &RenderDevice) -> Option<VramReading> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// GiB as bytes, independent of the production conversion.
    fn gib(gb: f32) -> u64 {
        (gb * (1u64 << 30) as f32) as u64
    }

    #[test]
    fn the_largest_device_local_heap_is_read() {
        // A 4 GB card: host RAM, the VRAM heap and the small BAR window.
        let heaps = [(false, gib(16.0)), (true, gib(4.0)), (true, gib(0.25))];
        let usages = [gib(1.0), gib(3.0), gib(0.1)];
        assert_eq!(
            heap_reading(&heaps, &[gib(15.0), gib(3.9), gib(0.2)], &usages),
            Some(VramReading {
                budget: gib(3.9),
                usage: gib(3.0)
            })
        );
        assert_eq!(
            heap_reading(&heaps, &[gib(15.0), 0, gib(0.2)], &usages),
            None,
            "no budget from the driver"
        );
        assert_eq!(
            heap_reading(&[(false, gib(16.0))], &[gib(15.0)], &[gib(1.0)]),
            None,
            "no device-local heap"
        );
    }
}
