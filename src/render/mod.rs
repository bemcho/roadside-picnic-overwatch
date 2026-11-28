//! Render tuning that keeps the CPU from holding the GPU back.

use bevy::light::cluster::GlobalClusterSettings;
use bevy::prelude::*;

/// Tunes Bevy's renderer once its plugins are built.
pub struct RenderTuningPlugin;

impl Plugin for RenderTuningPlugin {
    fn build(&self, _app: &mut App) {}

    // Plugins finish in the order they were added, so `PbrPlugin::finish` has
    // inserted the cluster settings by now.
    fn finish(&self, app: &mut App) {
        use_cpu_clustering(app.world_mut());
    }
}

/// Assigns lights to clusters on the CPU. Bevy 0.20's GPU clustering rebuilds
/// each view's cluster buffers on the CPU every frame; when the driver holds the
/// GPU at low clocks that took 44 ms a frame on the player's camera alone (8-11
/// FPS, 15-16 with this). The CPU path costs 0.2 ms a frame for the game's few
/// dozen lamps, and the same at full clocks.
pub fn use_cpu_clustering(world: &mut World) {
    if let Some(mut settings) = world.get_resource_mut::<GlobalClusterSettings>() {
        settings.gpu_clustering = None;
    }
}

#[cfg(test)]
mod tests {
    use bevy::light::cluster::GlobalClusterGpuSettings;

    use super::*;

    #[test]
    fn bevy_is_switched_to_cpu_light_clustering() {
        let mut world = World::new();
        world.insert_resource(GlobalClusterSettings {
            supports_storage_buffers: true,
            clustered_decals_are_usable: true,
            gpu_clustering: Some(GlobalClusterGpuSettings {
                initial_z_slice_list_capacity: 1024,
                initial_index_list_capacity: 65536,
            }),
            max_uniform_buffer_clusterable_objects: 256,
            view_cluster_bindings_max_indices: 65536,
        });
        use_cpu_clustering(&mut world);
        let settings = world.resource::<GlobalClusterSettings>();
        assert!(settings.gpu_clustering.is_none());
        assert!(
            settings.supports_storage_buffers,
            "the rest stays as Bevy chose"
        );
    }

    #[test]
    fn without_a_renderer_the_switch_does_nothing() {
        let mut world = World::new();
        use_cpu_clustering(&mut world);
        assert!(!world.contains_resource::<GlobalClusterSettings>());
    }
}
