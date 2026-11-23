//! Entry curtain: keeps a loading screen over the freshly spawned Zone until the
//! renderer has compiled its shader pipelines, the plants around the stalker
//! have spawned and the UHD scans are on the GPU (the pinned trees, and any
//! prefill in flight), so the world appears complete instead of popping in.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use bevy::prelude::*;
use bevy::render::render_resource::PipelineCache;
use bevy::render::{Render, RenderApp};

use crate::assets::gpu_scans::GpuScans;
use crate::states::AppState;
use crate::world::PlantsGrowing;

use super::text;

/// How long the pipeline queue and plant spawning must stay idle before the curtain lifts.
const SETTLE_SECONDS: f32 = 0.4;
/// Never hold the player longer than this, even if pipelines keep arriving.
const MAX_SECONDS: f32 = 12.0;

pub(super) struct CurtainPlugin;

/// Render pipelines still compiling, published by the render world.
#[derive(Resource, Clone, Default)]
struct PipelineBacklog(Arc<AtomicUsize>);

#[derive(Component, Default)]
struct EntryCurtain {
    shown: f32,
    settled: f32,
}

impl Plugin for CurtainPlugin {
    fn build(&self, app: &mut App) {
        let backlog = PipelineBacklog::default();
        app.insert_resource(backlog.clone())
            .add_systems(OnEnter(AppState::InGame), spawn_curtain)
            .add_systems(Update, lift_curtain.run_if(in_state(AppState::InGame)));
        if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
            render_app
                .insert_resource(backlog)
                .add_systems(Render, publish_pipeline_backlog);
        }
    }
}

fn publish_pipeline_backlog(pipelines: Res<PipelineCache>, backlog: Res<PipelineBacklog>) {
    backlog
        .0
        .store(pipelines.waiting_pipelines().count(), Ordering::Relaxed);
}

fn spawn_curtain(mut commands: Commands) {
    commands
        .spawn((
            EntryCurtain::default(),
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: Val::VMin(4.0),
                ..default()
            },
            BackgroundColor(Color::srgb(0.035, 0.04, 0.047)),
            GlobalZIndex(i32::MAX),
            DespawnOnExit(AppState::InGame),
        ))
        .with_children(|curtain| {
            curtain.spawn((
                Text::new(text::SPLASH_TITLE),
                TextFont {
                    font_size: FontSize::VMin(7.5),
                    ..default()
                },
                TextColor(Color::srgb(0.96, 0.92, 0.81)),
            ));
            curtain.spawn((
                Text::new(text::ENTERING_ZONE),
                TextFont {
                    font_size: FontSize::VMin(3.2),
                    ..default()
                },
                TextColor(Color::srgb(0.85, 0.84, 0.80)),
            ));
        });
}

fn lift_curtain(
    mut commands: Commands,
    time: Res<Time<Real>>,
    backlog: Res<PipelineBacklog>,
    plants: Res<PlantsGrowing>,
    mut scans: ResMut<GpuScans>,
    curtain: Option<Single<(Entity, &mut EntryCurtain)>>,
) {
    let Some(curtain) = curtain else {
        return;
    };
    let (entity, mut curtain) = curtain.into_inner();
    let dt = time.delta_secs();
    curtain.shown += dt;
    // `settled` is false while any scan loads or uploads, a pinned tree's or a
    // prefill's, and while a pinned one is not yet on the GPU: the curtain
    // holds until prefill has no more room to fill, but `MAX_SECONDS` caps the
    // wait, and a tree shows its crown until its scan is resident.
    let busy = backlog.0.load(Ordering::Relaxed) > 0 || plants.0 || !scans.settled();
    curtain.settled = if busy { 0.0 } else { curtain.settled + dt };
    if curtain.settled >= SETTLE_SECONDS || curtain.shown >= MAX_SECONDS {
        commands.entity(entity).despawn();
        // The world's own uploads have landed: scans may use the whole budget.
        scans.entered();
        info!("Entered the Zone");
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::state::app::StatesPlugin;
    use bevy::time::TimeUpdateStrategy;

    use super::*;

    /// An app that has just entered the Zone, with its pipeline backlog.
    fn entered_app() -> (App, PipelineBacklog) {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, StatesPlugin, CurtainPlugin))
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
                100,
            )))
            .init_resource::<PlantsGrowing>()
            .init_resource::<GpuScans>()
            .init_state::<AppState>();
        app.world_mut()
            .resource_mut::<NextState<AppState>>()
            .set(AppState::InGame);
        let backlog = app.world().resource::<PipelineBacklog>().clone();
        (app, backlog)
    }

    fn curtains(app: &mut App) -> usize {
        app.world_mut()
            .query::<&EntryCurtain>()
            .iter(app.world())
            .count()
    }

    #[test]
    fn curtain_waits_for_compiling_pipelines_then_lifts() {
        let (mut app, backlog) = entered_app();
        backlog.0.store(3, Ordering::Relaxed);

        for _ in 0..10 {
            app.update();
        }
        assert_eq!(curtains(&mut app), 1, "pipelines are still compiling");

        backlog.0.store(0, Ordering::Relaxed);
        for _ in 0..6 {
            app.update();
        }
        assert_eq!(
            curtains(&mut app),
            0,
            "the curtain lifts once compiling settles"
        );
    }

    #[test]
    fn curtain_waits_for_plants_to_spawn_then_lifts() {
        let (mut app, _) = entered_app();
        app.world_mut().resource_mut::<PlantsGrowing>().0 = true;

        for _ in 0..10 {
            app.update();
        }
        assert_eq!(curtains(&mut app), 1, "plants are still spawning");

        app.world_mut().resource_mut::<PlantsGrowing>().0 = false;
        for _ in 0..6 {
            app.update();
        }
        assert_eq!(
            curtains(&mut app),
            0,
            "the curtain lifts once plants settle"
        );
    }

    #[test]
    fn curtain_waits_for_pinned_scans_then_lifts() {
        const TREE: &str = "models/trees/uhd/tree.glb";
        let (mut app, _) = entered_app();
        app.world_mut().resource_mut::<GpuScans>().pin(TREE);

        for _ in 0..10 {
            app.update();
        }
        assert_eq!(curtains(&mut app), 1, "a pinned scan is not on the GPU");

        app.world_mut()
            .resource_mut::<GpuScans>()
            .insert_resident(TREE, Handle::default());
        for _ in 0..6 {
            app.update();
        }
        assert_eq!(
            curtains(&mut app),
            0,
            "the curtain lifts once the scan is resident"
        );
    }

    #[test]
    fn lifting_the_curtain_lets_wanted_scans_fill_the_whole_budget() {
        const PLANT: &str = "models/plants/uhd/plant.glb";
        let (mut app, _) = entered_app();
        app.insert_resource(GpuScans::entering(100 << 20));
        app.world_mut()
            .resource_mut::<GpuScans>()
            .estimate(PLANT, 50 << 20);
        let plan = |app: &mut App| {
            let mut scans = app.world_mut().resource_mut::<GpuScans>();
            scans.want(PLANT, 0.0);
            scans.plan(0.0).load
        };
        assert!(
            plan(&mut app).is_empty(),
            "behind the curtain it leaves headroom"
        );

        for _ in 0..6 {
            app.update();
        }
        assert_eq!(curtains(&mut app), 0);
        assert_eq!(plan(&mut app), [PLANT]);
    }

    #[test]
    fn curtain_lifts_after_the_cap_though_a_pinned_scan_is_still_loading() {
        let (mut app, _) = entered_app();
        app.world_mut()
            .resource_mut::<GpuScans>()
            .pin("models/trees/uhd/tree.glb");

        for _ in 0..(MAX_SECONDS * 10.0) as usize + 2 {
            app.update();
        }
        assert_eq!(curtains(&mut app), 0, "the cap lifts it");
    }
}
