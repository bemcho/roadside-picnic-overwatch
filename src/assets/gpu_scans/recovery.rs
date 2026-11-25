//! Surviving the GPU running out of memory. Bevy quits on any render error by
//! default; on an out-of-memory error the game instead drops its UHD scans
//! (`GpuScans::out_of_memory`), which it can load again from RAM, and the
//! render world's per-frame textures, where the failed allocation (a shadow
//! map, on the 2060) would otherwise be reused every frame. Then it renders on.
//!
//! A full device recovery (`RenderErrorPolicy::Recover`) is no way out: the
//! terrain, roads and scenery meshes are render-only, with no copy left in the
//! main world to upload again.

use bevy::prelude::*;
use bevy::render::error_handler::{ErrorType, RenderError, RenderErrorPolicy};
use bevy::render::texture::TextureCache;

use super::GpuScans;
use super::residency::ScanResidency;

/// Recoveries allowed within `TROUBLE_SECONDS` before the game quits as Bevy
/// would: an error that keeps coming back makes the screen flash.
const MAX_RECOVERIES: usize = 3;
const TROUBLE_SECONDS: f32 = 60.0;
/// Seconds after an out-of-memory error in which a validation error is its
/// echo (the failed object used before it was replaced), not a bug of its own.
const ECHO_SECONDS: f32 = 2.0;

/// When the game recovered from render errors, in seconds of real time.
#[derive(Resource, Default)]
pub struct RenderTrouble {
    recoveries: Vec<f32>,
}

impl RenderTrouble {
    /// Whether an error of type `ty` at `now` is one to recover from: an
    /// out-of-memory error, or a validation error just after one, unless the
    /// game recovered `MAX_RECOVERIES` times within `TROUBLE_SECONDS` already.
    /// Notes the recovery when it is.
    fn recovers(&mut self, ty: ErrorType, now: f32) -> bool {
        let echo = self
            .recoveries
            .last()
            .is_some_and(|&last| now - last < ECHO_SECONDS);
        let recoverable =
            matches!(ty, ErrorType::OutOfMemory) || (matches!(ty, ErrorType::Validation) && echo);
        self.recoveries.retain(|&at| now - at < TROUBLE_SECONDS);
        if !recoverable || self.recoveries.len() >= MAX_RECOVERIES {
            return false;
        }
        self.recoveries.push(now);
        true
    }
}

/// The game's `RenderErrorHandler`: recovers from running out of GPU memory
/// (see the module docs) and quits on anything else, as Bevy's default does.
pub fn handle_render_error(
    error: &RenderError,
    main_world: &mut World,
    render_world: &mut World,
) -> RenderErrorPolicy {
    let now = main_world.resource::<Time<Real>>().elapsed_secs();
    if !main_world
        .get_resource_or_init::<RenderTrouble>()
        .recovers(error.ty, now)
    {
        error!("Quitting the application due to {:?} RenderError", error.ty);
        main_world.write_message(AppExit::error());
        return RenderErrorPolicy::StopRendering;
    }
    render_world.insert_resource(TextureCache::default());
    let residency = main_world.resource::<ScanResidency>().clone();
    let mut scans = main_world.resource_mut::<GpuScans>();
    let dropped = scans.out_of_memory(now);
    for &path in &dropped {
        scans.unload(path);
        residency.lock().forget(path);
    }
    warn!(
        "The GPU ran out of memory ({:?}); dropped {} UHD scans, which may now use {} MB",
        error.ty,
        dropped.len(),
        scans.budget() >> 20,
    );
    RenderErrorPolicy::Ignore
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovers_from_running_out_of_memory_a_few_times_only() {
        let mut trouble = RenderTrouble::default();
        assert!(
            !trouble.recovers(ErrorType::Validation, 1.0),
            "a bug of its own"
        );
        assert!(trouble.recovers(ErrorType::OutOfMemory, 10.0));
        assert!(trouble.recovers(ErrorType::Validation, 10.5), "its echo");
        assert!(
            !trouble.recovers(ErrorType::Validation, 13.0),
            "too late for an echo"
        );
        assert!(!trouble.recovers(ErrorType::DeviceLost, 14.0));
        assert!(trouble.recovers(ErrorType::OutOfMemory, 20.0));
        assert!(
            !trouble.recovers(ErrorType::OutOfMemory, 30.0),
            "the third in a minute"
        );
        assert!(
            trouble.recovers(ErrorType::OutOfMemory, 75.0),
            "a minute on"
        );
    }

    #[test]
    fn running_out_of_memory_drops_the_scans_and_renders_on() {
        let mut main_world = World::new();
        main_world.init_resource::<Time<Real>>();
        main_world.init_resource::<ScanResidency>();
        main_world.init_resource::<Messages<AppExit>>();
        let mut scans = GpuScans::new(1 << 30);
        scans.insert_resident("a", Handle::default());
        main_world.insert_resource(scans);
        let mut render_world = World::new();
        let error = |ty| RenderError {
            ty,
            description: String::new(),
            source: None,
        };

        let policy = handle_render_error(
            &error(ErrorType::OutOfMemory),
            &mut main_world,
            &mut render_world,
        );
        assert!(matches!(policy, RenderErrorPolicy::Ignore));
        assert!(render_world.contains_resource::<TextureCache>());
        assert_eq!(main_world.resource::<GpuScans>().scene("a"), None);

        let policy = handle_render_error(
            &error(ErrorType::Internal),
            &mut main_world,
            &mut render_world,
        );
        assert!(matches!(policy, RenderErrorPolicy::StopRendering));
        assert!(
            !main_world.resource::<Messages<AppExit>>().is_empty(),
            "quits"
        );
    }
}
