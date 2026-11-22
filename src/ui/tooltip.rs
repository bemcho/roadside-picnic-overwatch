//! Hover descriptions: names and describes what lies under the mouse pointer,
//! from sites and buildings to water, landforms, creatures and artifacts.
//! Places outside the surveyed land show only what kind of place they are.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::player::{PlayerCamera, cast_to_ground};
use crate::states::InGamePhase;
use crate::world::{CreatureKind, Describe, Explored, Place, Terrain, place_at, structure_on_ray};

use super::text::{self, Entry};

/// Seconds between looks at what the pointer is over.
const REFRESH: f32 = 0.08;
const WIDTH: f32 = 320.0;
/// Rough panel height, for keeping it on screen.
const HEIGHT: f32 = 120.0;
const OFFSET: Vec2 = Vec2::new(18.0, 20.0);

#[derive(Component)]
pub(super) struct Tooltip;

#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub(super) enum TooltipLine {
    Kind,
    Title,
    Body,
}

pub(super) fn spawn_tooltip(hud: &mut ChildSpawnerCommands) {
    hud.spawn((
        Tooltip,
        Visibility::Hidden,
        Node {
            position_type: PositionType::Absolute,
            width: Val::Px(WIDTH),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(4.0),
            padding: UiRect::axes(Val::Px(12.0), Val::Px(9.0)),
            border: UiRect::all(Val::Px(1.0)),
            ..default()
        },
        BorderColor::all(Color::srgb(0.66, 0.52, 0.30)),
        BackgroundColor(Color::srgba(0.025, 0.035, 0.042, 0.9)),
    ))
    .with_children(|tip| {
        for (line, size, color) in [
            (TooltipLine::Kind, 12.0, Color::srgb(0.85, 0.68, 0.38)),
            (TooltipLine::Title, 19.0, Color::srgb(0.96, 0.94, 0.88)),
            (TooltipLine::Body, 14.0, Color::srgb(0.80, 0.82, 0.80)),
        ] {
            tip.spawn((
                line,
                Text::default(),
                TextFont {
                    font_size: FontSize::Px(size),
                    ..default()
                },
                TextColor(color),
            ));
        }
    });
}

type TooltipPanel<'w, 's> = (
    Single<'w, 's, (&'static mut Node, &'static mut Visibility), With<Tooltip>>,
    Query<'w, 's, (&'static mut Text, &'static TooltipLine)>,
);

pub(super) fn update_tooltip(
    (time, phase, mouse): (
        Res<Time>,
        Res<State<InGamePhase>>,
        Res<ButtonInput<MouseButton>>,
    ),
    window: Single<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<PlayerCamera>>,
    (terrain, explored): (Res<Terrain>, Option<Res<Explored>>),
    things: Query<(&Describe, &GlobalTransform, &InheritedVisibility)>,
    (mut tooltip, mut lines): TooltipPanel,
    mut since: Local<f32>,
) {
    let (node, visibility) = &mut *tooltip;
    let dragging = mouse.pressed(MouseButton::Right) || mouse.pressed(MouseButton::Middle);
    let (Some(cursor), Ok((camera, eye)), InGamePhase::Running, false) = (
        window.cursor_position(),
        cameras.single(),
        phase.get(),
        dragging,
    ) else {
        visibility.set_if_neq(Visibility::Hidden);
        return;
    };
    let size = window.size();
    let left = if cursor.x + OFFSET.x + WIDTH > size.x {
        cursor.x - OFFSET.x - WIDTH
    } else {
        cursor.x + OFFSET.x
    };
    let top = if cursor.y + OFFSET.y + HEIGHT > size.y {
        cursor.y - OFFSET.y - HEIGHT
    } else {
        cursor.y + OFFSET.y
    };
    node.left = Val::Px(left.max(0.0));
    node.top = Val::Px(top.max(0.0));

    *since += time.delta_secs();
    if *since < REFRESH {
        return;
    }
    *since = 0.0;
    let found = camera.viewport_to_world(eye, cursor).ok().and_then(|ray| {
        pointed_at(
            &terrain,
            ray,
            things
                .iter()
                .filter(|(.., shown)| shown.get())
                .map(|(describe, pose, _)| (describe, pose)),
        )
    });
    let Some((at, place)) = found else {
        visibility.set_if_neq(Visibility::Hidden);
        return;
    };
    let (kind, entry) = describe(place);
    let surveyed = explored.is_some_and(|explored| explored.contains(at));
    let (title, body) = if surveyed {
        (entry.title.to_owned(), entry.description)
    } else {
        (text::unexplored(kind), text::UNEXPLORED_DESCRIPTION)
    };
    for (mut text, line) in &mut lines {
        let wanted = match line {
            TooltipLine::Kind => kind.to_uppercase(),
            TooltipLine::Title => title.clone(),
            TooltipLine::Body => body.to_owned(),
        };
        if text.0 != wanted {
            text.0 = wanted;
        }
    }
    visibility.set_if_neq(Visibility::Inherited);
}

/// What the ray points at, and where: a described thing first (even inside a
/// building's outline), then the nearest building or site, then named ground.
fn pointed_at<'a>(
    terrain: &Terrain,
    ray: Ray3d,
    things: impl Iterator<Item = (&'a Describe, &'a GlobalTransform)>,
) -> Option<(Vec2, Place)> {
    let ground = cast_to_ground(terrain, ray);
    let reach = ground.map_or(f32::INFINITY, |hit| hit.distance(ray.origin) + 0.5);
    let thing = things
        .filter_map(|(describe, pose)| {
            let center = pose.translation() + Vec3::Y * describe.height * 0.5;
            enter_sphere(ray, center, describe.radius.max(describe.height * 0.5))
                .filter(|&t| t < reach)
                .map(|t| (t, pose.translation().xz(), describe.place))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0));
    let structure =
        structure_on_ray(terrain, ray, reach).map(|(t, place)| (t, ray.get_point(t).xz(), place));
    thing
        .or(structure)
        .map(|(_, at, place)| (at, place))
        .or_else(|| ground.and_then(|hit| place_at(hit.xz()).map(|place| (hit.xz(), place))))
}

fn enter_sphere(ray: Ray3d, center: Vec3, radius: f32) -> Option<f32> {
    let from = ray.origin - center;
    let b = from.dot(*ray.direction);
    let disc = b * b - (from.length_squared() - radius * radius);
    let t = -b - disc.max(0.0).sqrt();
    (disc >= 0.0 && t >= 0.0).then_some(t)
}

/// The kind label and copy for a place.
fn describe(place: Place) -> (&'static str, &'static Entry) {
    let sites: &'static [Entry] = &text::SITE_COPY;
    let creatures: &'static [Entry] = &text::CREATURE_COPY;
    let artifacts: &'static [Entry] = &text::ARTIFACT_COPY;
    match place {
        Place::Site(kind) if kind.is_landmark() => (text::KIND_LANDMARK, &sites[kind as usize]),
        Place::Site(kind) => (text::KIND_SITE, &sites[kind as usize]),
        Place::DerailedTrain => (text::KIND_LANDMARK, &text::DERAILED_TRAIN),
        Place::Hangar => (text::KIND_BUILDING, &text::HANGAR),
        Place::Checkpoint => (text::KIND_BUILDING, &text::CHECKPOINT),
        Place::Yard => (text::KIND_SITE, &text::YARD),
        Place::Quarry => (text::KIND_LANDMARK, &text::QUARRY),
        Place::BaldHill => (text::KIND_LANDMARK, &text::BALD_HILL),
        Place::StagnantFlats => (text::KIND_SWAMP, &text::STAGNANT_FLATS),
        Place::Swamp => (text::KIND_SWAMP, &text::SWAMP),
        Place::Lake => (text::KIND_LAKE, &text::LAKE),
        Place::WestLake => (text::KIND_LAKE, &text::WEST_LAKE),
        Place::River => (text::KIND_RIVER, &text::RIVER),
        Place::Ford => (text::KIND_RIVER, &text::FORD),
        Place::Tributary => (text::KIND_RIVER, &text::TRIBUTARY),
        Place::Coast => (text::KIND_COAST, &text::COAST),
        Place::Creature(kind) => {
            let label = match kind {
                CreatureKind::Bovine
                | CreatureKind::Chimera
                | CreatureKind::Fleshdeer
                | CreatureKind::Mutt => text::KIND_MUTANT,
                _ => text::KIND_WILDLIFE,
            };
            (label, &creatures[kind as usize])
        }
        Place::Vehicle => (text::KIND_VEHICLE, &text::VEHICLE),
        Place::Artifact(index) => (
            text::KIND_ARTIFACT,
            &artifacts[index.min(artifacts.len() - 1)],
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pointer_names_a_creature_before_the_ground_behind_it() {
        let terrain = Terrain::new();
        let ground = Vec2::new(40.0, -80.0);
        let feet = ground.extend(terrain.mesh_height_at(ground)).xzy();
        let eye = feet + Vec3::new(0.0, 30.0, 30.0);
        let ray = Ray3d::new(
            eye,
            Dir3::new(feet + Vec3::Y - eye).expect("toward the wolf"),
        );
        let wolf = Describe {
            place: Place::Creature(CreatureKind::Frostwolf),
            radius: 1.1,
            height: 1.5,
        };
        let pose = GlobalTransform::from_translation(feet);
        assert_eq!(
            pointed_at(&terrain, ray, [(&wolf, &pose)].into_iter()),
            Some((ground, wolf.place))
        );
        // With the wolf gone, the same pointer names the hangar yard beneath.
        assert_eq!(
            pointed_at(&terrain, ray, std::iter::empty()).map(|(_, place)| place),
            Some(Place::Yard)
        );
    }

    #[test]
    fn each_kind_of_place_has_its_own_label() {
        assert_eq!(describe(Place::Swamp).0, text::KIND_SWAMP);
        assert_eq!(describe(Place::Ford).1.title, text::FORD.title);
        assert_eq!(
            describe(Place::Creature(CreatureKind::Mutt)).0,
            text::KIND_MUTANT
        );
        assert_eq!(
            describe(Place::Artifact(3)).1.title,
            text::ARTIFACT_COPY[3].title
        );
    }
}
