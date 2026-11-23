//! Saved progress: the ground the stalker has surveyed, the time of day and
//! where the stalker and their car stand. It is saved on F5, every minute and
//! on quitting, and restored on entering the Zone; `--new` on the command line
//! starts afresh. The save is a small text file in the platform's data folder
//! (`progress_path`); the web build keeps none.

use std::io;

use bevy::prelude::*;

use crate::launch::LaunchOptions;
use crate::player::Player;
use crate::player::vehicle::{Driving, Vehicle};
use crate::states::AppState;
use crate::world::{Explored, Terrain, WorldClock};

const HEADER: &str = "roadside-picnic progress 1";
const AUTOSAVE_SECONDS: f32 = 60.0;
/// A stalker saved at the wheel is put back on foot this far to the car's left.
const STEP_OUT: f32 = 3.5;

pub struct SavePlugin;

/// Sent when progress has been written, for the HUD to say so.
#[derive(Message)]
pub struct ProgressSaved;

impl Plugin for SavePlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<ProgressSaved>()
            .add_systems(Startup, read_progress)
            .add_systems(
                Update,
                restore_progress
                    .run_if(in_state(AppState::InGame))
                    .run_if(resource_exists::<SavedProgress>),
            )
            .add_systems(
                Last,
                save_progress
                    .run_if(in_state(AppState::InGame))
                    .run_if(save_due),
            );
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Progress {
    hours: f32,
    stalker: Vec3,
    car: Vec3,
    car_yaw: f32,
    surveyed: Vec<bool>,
}

impl Progress {
    fn to_text(&self) -> String {
        let surveyed: String = self
            .surveyed
            .iter()
            .map(|&seen| if seen { '1' } else { '0' })
            .collect();
        let [x, y, z] = self.stalker.to_array();
        let [cx, cy, cz] = self.car.to_array();
        format!(
            "{HEADER}\nhours {}\nstalker {x} {y} {z}\ncar {cx} {cy} {cz} {}\nsurveyed {surveyed}\n",
            self.hours, self.car_yaw
        )
    }

    fn from_text(text: &str) -> Option<Self> {
        let mut lines = text.lines();
        if lines.next()? != HEADER {
            return None;
        }
        let mut field = |key: &str| {
            let (name, value) = lines.next()?.split_once(' ')?;
            (name == key).then_some(value)
        };
        let [hours] = numbers(field("hours")?)?;
        let [x, y, z] = numbers(field("stalker")?)?;
        let [cx, cy, cz, car_yaw] = numbers(field("car")?)?;
        let surveyed = field("surveyed")?
            .chars()
            .map(|cell| match cell {
                '0' => Some(false),
                '1' => Some(true),
                _ => None,
            })
            .collect::<Option<_>>()?;
        Some(Self {
            hours,
            stalker: Vec3::new(x, y, z),
            car: Vec3::new(cx, cy, cz),
            car_yaw,
            surveyed,
        })
    }
}

/// `N` finite numbers separated by spaces.
fn numbers<const N: usize>(text: &str) -> Option<[f32; N]> {
    let values: Vec<f32> = text
        .split_whitespace()
        .map(|value| value.parse().ok().filter(|value: &f32| value.is_finite()))
        .collect::<Option<_>>()?;
    values.try_into().ok()
}

/// Progress read at startup, waiting for the Zone to be spawned.
#[derive(Resource)]
struct SavedProgress(Progress);

fn read_progress(mut commands: Commands, launch: Res<LaunchOptions>) {
    if launch.new {
        return;
    }
    match read() {
        Ok(text) => match Progress::from_text(&text) {
            Some(progress) => commands.insert_resource(SavedProgress(progress)),
            None => warn!("Ignoring an unreadable save; starting afresh"),
        },
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => warn!("Could not read the save: {error}"),
    }
}

type Stalker<'w, 's> = Query<'w, 's, (&'static mut Transform, Has<Driving>), With<Player>>;
type Car<'w, 's> = Query<'w, 's, &'static mut Transform, (With<Vehicle>, Without<Player>)>;

/// Puts the saved progress into the freshly spawned Zone.
fn restore_progress(
    mut commands: Commands,
    saved: Res<SavedProgress>,
    explored: Option<ResMut<Explored>>,
    clock: Option<ResMut<WorldClock>>,
    mut stalkers: Stalker,
    mut cars: Car,
) {
    let (Some(mut explored), Some(mut clock), Ok((mut stalker, _)), Ok(mut car)) =
        (explored, clock, stalkers.single_mut(), cars.single_mut())
    else {
        return;
    };
    let progress = &saved.0;
    if let Some(survey) = Explored::from_cells(progress.surveyed.clone()) {
        *explored = survey;
    }
    *clock = WorldClock::at(progress.hours);
    stalker.translation = progress.stalker;
    car.translation = progress.car;
    car.rotation = Quat::from_rotation_y(progress.car_yaw);
    commands.remove_resource::<SavedProgress>();
}

/// Whether to save this frame: on F5, every `AUTOSAVE_SECONDS` and when the
/// game quits, but not before a save read at startup has been restored.
fn save_due(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mut exits: MessageReader<AppExit>,
    mut since: Local<f32>,
    pending: Option<Res<SavedProgress>>,
) -> bool {
    *since += time.delta_secs();
    let quitting = exits.read().count() > 0;
    let due = quitting || keys.just_pressed(KeyCode::F5) || *since >= AUTOSAVE_SECONDS;
    if due {
        *since = 0.0;
    }
    due && pending.is_none()
}

fn save_progress(
    terrain: Res<Terrain>,
    explored: Option<Res<Explored>>,
    clock: Option<Res<WorldClock>>,
    stalkers: Query<(&Transform, Has<Driving>), With<Player>>,
    cars: Query<&Transform, (With<Vehicle>, Without<Player>)>,
    mut saved: MessageWriter<ProgressSaved>,
) {
    let (Some(explored), Some(clock), Ok((stalker, driving)), Ok(car)) =
        (explored, clock, stalkers.single(), cars.single())
    else {
        return;
    };
    let stalker = if driving {
        let beside = car.translation + car.rotation * Vec3::X * STEP_OUT;
        beside
            .xz()
            .extend(terrain.height_at(beside.x, beside.z))
            .xzy()
    } else {
        stalker.translation
    };
    let forward = car.rotation * Vec3::Z;
    let progress = Progress {
        hours: clock.hours(),
        stalker,
        car: car.translation,
        car_yaw: forward.x.atan2(forward.z),
        surveyed: explored.cells().collect(),
    };
    match write(&progress.to_text()) {
        Ok(()) => {
            saved.write(ProgressSaved);
        }
        Err(error) => warn!("Could not save progress: {error}"),
    }
}

/// The save file: in `$XDG_DATA_HOME` (or `~/.local/share`) on Linux,
/// `%APPDATA%` on Windows and `~/Library/Application Support` on macOS.
#[cfg(not(target_arch = "wasm32"))]
fn progress_path() -> io::Result<std::path::PathBuf> {
    use std::path::PathBuf;

    let var = |name| std::env::var_os(name).map(PathBuf::from);
    let data = if cfg!(target_os = "windows") {
        var("APPDATA")
    } else if cfg!(target_os = "macos") {
        var("HOME").map(|home| home.join("Library/Application Support"))
    } else {
        var("XDG_DATA_HOME")
            .filter(|path| path.is_absolute())
            .or_else(|| var("HOME").map(|home| home.join(".local/share")))
    };
    data.map(|dir| dir.join("roadside-picnic").join("progress.txt"))
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no data folder"))
}

#[cfg(not(target_arch = "wasm32"))]
fn read() -> io::Result<String> {
    std::fs::read_to_string(progress_path()?)
}

/// Writes beside the save, then swaps it in, so a crash never leaves half a file.
#[cfg(not(target_arch = "wasm32"))]
fn write(text: &str) -> io::Result<()> {
    let path = progress_path()?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let fresh = path.with_extension("tmp");
    std::fs::write(&fresh, text)?;
    std::fs::rename(fresh, path)
}

#[cfg(target_arch = "wasm32")]
fn read() -> io::Result<String> {
    Err(io::ErrorKind::NotFound.into())
}

#[cfg(target_arch = "wasm32")]
fn write(_: &str) -> io::Result<()> {
    Err(io::ErrorKind::Unsupported.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn progress() -> Progress {
        Progress {
            hours: 31.25,
            stalker: Vec3::new(840.5, 2.25, -113.0),
            car: Vec3::new(-12.0, 0.5, 400.125),
            car_yaw: -1.5,
            surveyed: Explored::default().cells().map(|_| false).collect(),
        }
    }

    #[test]
    fn progress_reads_back_as_written() {
        let mut saved = progress();
        saved.surveyed[7] = true;
        saved.surveyed[1000] = true;
        assert_eq!(Progress::from_text(&saved.to_text()), Some(saved));
    }

    #[test]
    fn a_damaged_or_foreign_save_is_ignored() {
        let text = progress().to_text();
        assert!(Progress::from_text(&text.replace("progress 1", "progress 2")).is_none());
        assert!(Progress::from_text(&text.replace("hours 31.25", "hours lots")).is_none());
        assert!(Progress::from_text(&text.replace("hours 31.25", "hours NaN")).is_none());
        assert!(Progress::from_text(&text.replace("surveyed 0", "surveyed x")).is_none());
        let cut = &text[..text.len() / 2];
        let restored = Progress::from_text(cut).map(|progress| progress.surveyed);
        assert!(restored.is_none_or(|cells| Explored::from_cells(cells).is_none()));
    }

    #[test]
    fn restoring_moves_the_stalker_and_car_and_restores_the_survey() {
        let mut world = World::new();
        world.insert_resource(Explored::default());
        world.insert_resource(WorldClock::default());
        let stalker = world.spawn((Player::default(), Transform::default())).id();
        let car = world.spawn((Vehicle::default(), Transform::default())).id();
        let mut saved = progress();
        saved.surveyed[0] = true;
        world.insert_resource(SavedProgress(saved.clone()));
        world
            .run_system_cached(restore_progress)
            .expect("the system runs");
        world.flush();
        assert!(!world.contains_resource::<SavedProgress>());
        assert_eq!(world.resource::<WorldClock>().hours(), saved.hours);
        assert!(world.resource::<Explored>().cells().next() == Some(true));
        assert_eq!(
            world.get::<Transform>(stalker).map(|t| t.translation),
            Some(saved.stalker)
        );
        let parked = world.get::<Transform>(car).copied().unwrap_or_default();
        assert_eq!(parked.translation, saved.car);
        let heading = Vec3::new(saved.car_yaw.sin(), 0.0, saved.car_yaw.cos());
        assert!((parked.rotation * Vec3::Z).distance(heading) < 1e-5);
    }
}
