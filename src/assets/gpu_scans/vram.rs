//! The driver's live VRAM budget (Vulkan's `VK_EXT_memory_budget`), and the
//! scan budget that follows it.

use bevy::prelude::*;
use bevy::render::render_resource::WgpuFeatures;
use bevy::render::renderer::{RenderAdapter, RenderAdapterInfo, RenderDevice};

use crate::launch::{DEFAULT_VRAM_GB, LaunchOptions};

use super::{GpuScans, GpuTier, VramReading, gb, gib};

/// Seconds between two readings of the driver's budget.
const READ_EVERY: f32 = 1.0;

/// What `track_vram_budget` has read and said so far.
#[derive(Default)]
pub struct VramLog {
    /// When to read the driver next.
    next: f32,
    /// A reading came, and was logged.
    read: bool,
    /// A reading came back empty before any came, and was warned about.
    warned: bool,
}

/// About once a second, sets the scan budget from the driver's live reading,
/// capped by `--vram`, unless a scan is still uploading (`apply_reading`).
/// Where the driver cannot report one, the fallback `GpuScansPlugin` set
/// stays. The first reading taken in is logged whenever it comes; an empty
/// one is warned about once, and only while none has come.
pub fn track_vram_budget(
    mut scans: ResMut<GpuScans>,
    tier: Res<GpuTier>,
    device: Res<RenderDevice>,
    launch: Res<LaunchOptions>,
    time: Res<Time<Real>>,
    mut log: Local<VramLog>,
) {
    let now = time.elapsed_secs();
    if now < log.next {
        return;
    }
    log.next = now + READ_EVERY;
    let Some(reading) = read_vram(&device) else {
        if !log.read && !log.warned {
            log.warned = true;
            let assumed = launch.vram_gb.unwrap_or(DEFAULT_VRAM_GB);
            let s = gb(scans.budget());
            warn!(
                "The driver cannot report VRAM; assuming {assumed} GB, so UHD scans may use {s:.1} GB"
            );
        }
        return;
    };
    if !scans.apply_reading(reading, launch.vram_gb.map(gib), now) {
        return;
    }
    if !log.read {
        log.read = true;
        let (b, u, s) = (gb(reading.budget), gb(reading.usage), gb(scans.budget()));
        let (card, level) = (gb(reading.card), tier.level());
        info!("VRAM: driver budget {b:.1} GB, game using {u:.1} GB; UHD scans may use {s:.1} GB");
        info!("GPU: a {card:.1} GB card, effects at the {level:?} tier");
    }
}

/// Decides the session's GPU tier at startup, before the Zone spawns the
/// effects that scale with it: from the card's size, or the fallback where the
/// driver reports none.
pub fn settle_gpu_tier(
    mut tier: ResMut<GpuTier>,
    device: Option<Res<RenderDevice>>,
    adapter: Option<Res<RenderAdapterInfo>>,
    features: Option<Res<RenderAdapter>>,
    launch: Res<LaunchOptions>,
) {
    let reading = device.and_then(|device| read_vram(&device));
    let integrated = adapter.is_some_and(|adapter| shares_system_memory(&adapter));
    let ray_tracing = features.is_some_and(|adapter| {
        adapter
            .features()
            .contains(WgpuFeatures::EXPERIMENTAL_RAY_QUERY)
    });
    tier.settle_with(
        reading,
        launch.vram_gb.map(gib),
        launch.gpu_tier,
        integrated,
        ray_tracing,
    );
    info!("GPU: effects at the {:?} tier", tier.level());
}

/// The budget and usage of the largest device-local heap. `heaps` holds each
/// heap's (device-local, size); `budgets` and `usages` are in the same order.
/// `None` without a device-local heap, or when its budget is 0 (the driver
/// does not report one).
pub fn heap_reading(heaps: &[(bool, u64)], budgets: &[u64], usages: &[u64]) -> Option<VramReading> {
    let (heap, &(_, card)) = heaps
        .iter()
        .enumerate()
        .filter(|(_, (device_local, _))| *device_local)
        .max_by_key(|(_, (_, size))| *size)?;
    let budget = *budgets.get(heap)?;
    let usage = *usages.get(heap)?;
    (budget > 0).then_some(VramReading {
        budget,
        usage,
        card,
    })
}

/// The driver's reading for the game's device, where it runs on Vulkan with
/// `VK_EXT_memory_budget` (wgpu enables it whenever the driver offers it).
/// It is the query wgpu-hal's own `get_memory_properties` makes
/// (`vulkan/device.rs`), through the same `VK_KHR_get_physical_device_properties2`
/// entry point, which wgpu-hal enables on the instance whenever it is offered.
// The platforms where wgpu builds its Vulkan backend.
#[cfg(any(
    windows,
    target_os = "linux",
    target_os = "android",
    target_os = "freebsd"
))]
pub fn read_vram(device: &RenderDevice) -> Option<VramReading> {
    use ash::khr::get_physical_device_properties2 as properties2;
    use ash::vk;
    use wgpu::hal::api::Vulkan;

    // SAFETY: `as_hal` asks that the hal device is not destroyed through the
    // guard and that wgpu-hal's rules hold. Only its instance and physical
    // device handles are read; nothing is created or destroyed, and the guard
    // drops at the end of this function.
    let hal = unsafe { device.wgpu_device().as_hal::<Vulkan>() }?;
    let instance = hal.shared_instance();
    let has_budget = hal
        .enabled_device_extensions()
        .contains(&ash::ext::memory_budget::NAME);
    if !has_budget || !instance.extensions().contains(&properties2::NAME) {
        return None;
    }
    let loader = properties2::Instance::new(instance.entry(), instance.raw_instance());
    let mut budget = vk::PhysicalDeviceMemoryBudgetPropertiesEXT::default();
    let mut properties = vk::PhysicalDeviceMemoryProperties2::default().push_next(&mut budget);
    // SAFETY: `vkGetPhysicalDeviceMemoryProperties2KHR` is loaded from this
    // instance, which enabled `VK_KHR_get_physical_device_properties2`
    // (checked above); the physical device was enumerated from it; the one
    // struct chained to `properties` belongs to `VK_EXT_memory_budget`, which
    // the device enabled (checked above); both structs outlive the call.
    unsafe {
        loader.get_physical_device_memory_properties2(hal.raw_physical_device(), &mut properties);
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

/// Whether the GPU is integrated (or a CPU renderer): the device-local heap
/// `read_vram` reports is then the system's RAM, not a card's.
#[cfg(any(
    windows,
    target_os = "linux",
    target_os = "android",
    target_os = "freebsd"
))]
fn shares_system_memory(adapter: &RenderAdapterInfo) -> bool {
    use wgpu::DeviceType;
    matches!(
        adapter.device_type,
        DeviceType::IntegratedGpu | DeviceType::Cpu
    )
}

/// Without `read_vram` there is no heap reading to mistake for a card's.
#[cfg(not(any(
    windows,
    target_os = "linux",
    target_os = "android",
    target_os = "freebsd"
)))]
fn shares_system_memory(_adapter: &RenderAdapterInfo) -> bool {
    false
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
                usage: gib(3.0),
                card: gib(4.0),
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
