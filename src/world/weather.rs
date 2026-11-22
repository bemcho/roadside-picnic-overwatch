//! Day/night cycle and daily weather. The clock moves the sun and the moon;
//! temperature follows the sun, and relative humidity follows each day's dew
//! point, so the air saturates and fog gathers late at night and at dawn, or
//! all day when it is humid or raining. Each day's breeze picks up in gusts
//! that come and go.

use std::f32::consts::{PI, TAU};

use bevy::camera::Exposure;
use bevy::light::light_consts::lux;
use bevy::pbr::DistanceFog;
use bevy::prelude::*;

use crate::player::PlayerCamera;

use super::terrain::{lattice_hash, lerp, smoothstep};
use super::{Moon, Sun};

/// Real seconds per game hour: a whole day lasts 24 minutes.
const SECONDS_PER_HOUR: f32 = 60.0;
/// Holding T runs the clock this many times faster.
const FAST_FORWARD: f32 = 40.0;
/// The run starts shortly after sunrise, while the night's fog still lies low.
const START_HOUR: f32 = 6.25;
const LATITUDE: f32 = 50.0 * PI / 180.0;
const DECLINATION: f32 = 10.0 * PI / 180.0;
const SEED: u32 = 0x5EA5_0001;
/// Each slot of this many seconds may carry one gust.
const GUST_SLOT: f32 = 24.0;
/// Wind speed, in m/s, that bends plants and trees as far as they go.
const FULL_BEND_SPEED: f32 = 6.0;

/// Moonlight is far brighter than real, so night stays playable.
pub(super) const MOON_LUX: f32 = 1_800.0;
const DAY_EV: f32 = 13.0;
const NIGHT_EV: f32 = 9.0;

/// Game time, in hours since midnight before the first day.
#[derive(Resource, Clone, Copy, Debug)]
pub struct WorldClock {
    hours: f32,
}

impl Default for WorldClock {
    fn default() -> Self {
        Self::at(START_HOUR)
    }
}

impl WorldClock {
    pub fn at(hours: f32) -> Self {
        Self { hours }
    }

    /// Hour of the day, from 0.0 up to 24.0.
    pub fn hour(&self) -> f32 {
        self.hours.rem_euclid(24.0)
    }

    fn day(&self) -> i32 {
        (self.hours / 24.0).floor() as i32
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sky {
    Clear,
    Humid,
    Overcast,
    Rain,
}

/// The weather right now, derived from the clock.
#[derive(Resource, Clone, Copy, Debug)]
pub struct Weather {
    pub sky: Sky,
    /// Air temperature, in °C.
    pub temperature: f32,
    /// Relative humidity, from 0.0 to 1.0.
    pub humidity: f32,
    /// How readily ground fog forms, from 0.0 (dry air) to 1.0 (saturated).
    pub fog: f32,
    /// How much cloud dims the sun and moon, from 0.0 to 1.0.
    pub cloud: f32,
    /// Breeze that drifts the fog, in m/s.
    pub wind: Vec2,
}

impl Default for Weather {
    fn default() -> Self {
        Weather::at(WorldClock::default())
    }
}

/// The air moving trees, plants, the stalker's coat and the fog: the day's
/// breeze, with gusts that come and go.
#[derive(Resource, Clone, Copy, Default, Debug)]
pub struct Wind {
    /// Horizontal air velocity, in m/s, gusts included.
    pub velocity: Vec2,
    /// How far the current gust has risen, from 0.0 (none) to 1.0.
    pub gust: f32,
    /// Seconds the wind has blown, which pace the flutter of leaves and cloth.
    pub seconds: f32,
}

impl Wind {
    /// How far the wind bends things: about 0.1–0.3 in a breeze, near 1.0 in a
    /// strong gust.
    pub fn strength(&self) -> f32 {
        (self.velocity.length() / FULL_BEND_SPEED).min(1.5)
    }

    /// Where the wind blows to, on the ground plane.
    pub fn direction(&self) -> Vec2 {
        self.velocity.normalize_or(Vec2::X)
    }
}

/// The gust blowing `seconds` into the run: a swell and fade of 4–10 s in
/// about two of every three slots, from 0.0 to its peak.
fn gust(seconds: f32) -> f32 {
    let slot = (seconds / GUST_SLOT).floor();
    let roll = |n| lattice_hash(slot as i32, n, SEED + 40);
    if roll(0) > 0.65 {
        return 0.0;
    }
    let length = lerp(4.0, 10.0, roll(1));
    let start = roll(2) * (GUST_SLOT - length);
    let t = (seconds - slot * GUST_SLOT - start) / length;
    if !(0.0..1.0).contains(&t) {
        return 0.0;
    }
    (PI * t).sin().powi(2) * lerp(0.6, 1.0, roll(3))
}

/// Settings that stay fixed for a whole day.
struct Day {
    sky: Sky,
    mean: f32,
    swing: f32,
    /// Dew point relative to the day's coldest temperature.
    dew_offset: f32,
    cloud: f32,
    wind: Vec2,
}

impl Day {
    fn of(index: i32) -> Self {
        // The first day is humid, so the opening morning shows the valley fog.
        let roll = if index == 0 {
            0.5
        } else {
            lattice_hash(index, 7, SEED)
        };
        let warmth = (lattice_hash(index, 11, SEED) - 0.5) * 4.0;
        let heading = lattice_hash(index, 13, SEED) * TAU;
        let breeze = 0.4 + 1.6 * lattice_hash(index, 17, SEED);
        let (sky, mean, swing, dew_offset, cloud) = match roll {
            r if r < 0.4 => (Sky::Clear, 14.0, 7.0, -1.6, 0.0),
            r if r < 0.65 => (Sky::Humid, 16.0, 5.0, 0.3, 0.15),
            r if r < 0.85 => (Sky::Overcast, 12.0, 3.0, -0.6, 0.65),
            _ => (Sky::Rain, 10.0, 1.8, 0.0, 0.85),
        };
        Self {
            sky,
            mean: mean + warmth,
            swing,
            dew_offset,
            cloud,
            wind: Vec2::from_angle(heading) * breeze,
        }
    }

    fn weather(&self, hour: f32) -> Weather {
        let temperature = self.mean + self.swing * diurnal(hour);
        let dew_point = if self.sky == Sky::Rain {
            // Falling rain keeps the air close to saturation all day.
            temperature - 0.6
        } else {
            (self.mean - self.swing + self.dew_offset).min(temperature)
        };
        let humidity = relative_humidity(temperature, dew_point);
        let rain = f32::from(self.sky == Sky::Rain);
        Weather {
            sky: self.sky,
            temperature,
            humidity,
            fog: smoothstep(0.84, 0.995, humidity).max(rain * 0.8),
            cloud: self.cloud,
            wind: self.wind,
        }
    }
}

impl Weather {
    /// Blends each day's settings into the next over the four hours around midnight.
    pub fn at(clock: WorldClock) -> Self {
        let hour = clock.hour();
        let today = Day::of(clock.day()).weather(hour);
        let (other, blend) = if hour < 2.0 {
            (Day::of(clock.day() - 1), 0.5 - hour / 4.0)
        } else if hour > 22.0 {
            (Day::of(clock.day() + 1), (hour - 22.0) / 4.0)
        } else {
            return today;
        };
        let other = other.weather(hour);
        let blend = smoothstep(0.0, 1.0, blend);
        Weather {
            sky: if blend > 0.5 { other.sky } else { today.sky },
            temperature: today.temperature.lerp(other.temperature, blend),
            humidity: today.humidity.lerp(other.humidity, blend),
            fog: today.fog.lerp(other.fog, blend),
            cloud: today.cloud.lerp(other.cloud, blend),
            wind: today.wind.lerp(other.wind, blend),
        }
    }
}

/// -1.0 at the coldest hour (05:00) to 1.0 at the warmest (15:00).
fn diurnal(hour: f32) -> f32 {
    let since_coldest = (hour - 5.0).rem_euclid(24.0);
    if since_coldest < 10.0 {
        -(PI * since_coldest / 10.0).cos()
    } else {
        (PI * (since_coldest - 10.0) / 14.0).cos()
    }
}

/// Magnus approximation, from air temperature and dew point in °C.
fn relative_humidity(temperature: f32, dew_point: f32) -> f32 {
    let saturation = |t: f32| (17.62 * t / (243.12 + t)).exp();
    (saturation(dew_point) / saturation(temperature)).clamp(0.0, 1.0)
}

/// Direction toward a body on the celestial equator's `declination`, with
/// +X east, +Z north, at hour angle `hour_angle` (0 at its highest).
fn sky_direction(hour_angle: f32, declination: f32) -> Vec3 {
    let east = -declination.cos() * hour_angle.sin();
    let north =
        declination.sin() * LATITUDE.cos() - declination.cos() * LATITUDE.sin() * hour_angle.cos();
    let up =
        declination.sin() * LATITUDE.sin() + declination.cos() * LATITUDE.cos() * hour_angle.cos();
    Vec3::new(east, up, north)
}

pub(super) fn sun_direction(hour: f32) -> Vec3 {
    sky_direction((hour - 12.0) / 24.0 * TAU, DECLINATION)
}

/// A full moon, opposite the sun.
pub(super) fn moon_direction(hour: f32) -> Vec3 {
    sky_direction((hour - 12.0) / 24.0 * TAU + PI, -DECLINATION)
}

/// 0.0 at night to 1.0 once the sun is well clear of the horizon.
pub(super) fn daylight(hour: f32) -> f32 {
    smoothstep(-0.12, 0.25, sun_direction(hour).y)
}

pub(super) fn start_clock(mut commands: Commands) {
    let clock = WorldClock::default();
    commands.insert_resource(clock);
    commands.insert_resource(Weather::at(clock));
    commands.insert_resource(Wind::default());
}

/// Gusts the day's breeze up, veering it a little while they blow.
pub(super) fn blow_wind(time: Res<Time>, weather: Res<Weather>, mut wind: ResMut<Wind>) {
    wind.seconds += time.delta_secs();
    wind.gust = gust(wind.seconds);
    let veer = Vec2::from_angle(0.35 * wind.gust * (wind.seconds * 0.21).sin());
    wind.velocity = veer.rotate(weather.wind) * (1.0 + 2.5 * wind.gust);
}

pub(super) fn advance_clock(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mut clock: ResMut<WorldClock>,
    mut weather: ResMut<Weather>,
) {
    let rate = if keys.pressed(KeyCode::KeyT) {
        FAST_FORWARD
    } else {
        1.0
    };
    clock.hours += time.delta_secs() * rate / SECONDS_PER_HOUR;
    *weather = Weather::at(*clock);
}

/// A sky light (`Of`), kept apart from the other one (`Not`).
type SkyLight<'w, 's, Of, Not> = Query<
    'w,
    's,
    (&'static mut Transform, &'static mut DirectionalLight),
    (With<Of>, Without<Not>),
>;

/// Places the sun and moon, hands shadows to whichever is up, and adapts the
/// camera's exposure and haze to the light.
pub(super) fn light_the_sky(
    clock: Res<WorldClock>,
    weather: Res<Weather>,
    mut suns: SkyLight<Sun, Moon>,
    mut moons: SkyLight<Moon, Sun>,
    mut cameras: Query<(&mut Exposure, &mut DistanceFog), With<PlayerCamera>>,
) {
    let hour = clock.hour();
    let sun = sun_direction(hour);
    let moon = moon_direction(hour);
    let sun_up = sun.y > -0.03;
    let shade = 1.0 - weather.cloud * 0.7;
    for (mut transform, mut light) in &mut suns {
        *transform = Transform::from_translation(sun).looking_at(Vec3::ZERO, Vec3::Y);
        light.illuminance = lux::RAW_SUNLIGHT * shade;
        light.shadow_maps_enabled = sun_up;
    }
    for (mut transform, mut light) in &mut moons {
        *transform = Transform::from_translation(moon).looking_at(Vec3::ZERO, Vec3::Y);
        light.illuminance = MOON_LUX * shade;
        light.shadow_maps_enabled = !sun_up && moon.y > 0.0;
    }
    let day = daylight(hour);
    let twilight = smoothstep(-0.14, 0.0, sun.y) * (1.0 - smoothstep(0.04, 0.3, sun.y));
    let haze = Vec3::new(0.72, 0.80, 0.76).lerp(Vec3::new(0.62, 0.66, 0.68), weather.cloud);
    let dusk = Vec3::new(0.78, 0.52, 0.40) * 0.55;
    let night = Vec3::new(0.030, 0.042, 0.065);
    let color = night.lerp(haze, day).lerp(dusk, twilight * 0.6);
    for (mut exposure, mut fog) in &mut cameras {
        exposure.ev100 = NIGHT_EV.lerp(DAY_EV, day) - weather.cloud * day;
        fog.color = Color::linear_rgb(color.x, color.y, color.z);
        fog.directional_light_color = Color::linear_rgba(
            1.0,
            0.74,
            0.38,
            0.18 * smoothstep(-0.02, 0.1, sun.y) * (1.0 - weather.cloud),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sun_rises_in_the_east_peaks_south_and_sets_in_the_west() {
        let dawn = sun_direction(6.0);
        let noon = sun_direction(12.0);
        let dusk = sun_direction(18.0);
        let midnight = sun_direction(0.0);
        assert!(dawn.x > 0.5 && dawn.y > 0.0, "dawn {dawn}");
        assert!(noon.z < 0.0 && noon.y > 0.7, "noon {noon}");
        assert!(dusk.x < -0.5 && dusk.y > 0.0, "dusk {dusk}");
        assert!(midnight.y < -0.4, "midnight {midnight}");
        assert!(moon_direction(0.0).y > 0.4);
        assert!(daylight(0.0) == 0.0 && daylight(12.0) == 1.0);
    }

    #[test]
    fn humidity_peaks_at_dawn_and_fog_burns_off_on_a_clear_day() {
        let clear = (1..60)
            .map(Day::of)
            .find(|day| day.sky == Sky::Clear)
            .expect("a clear day");
        let dawn = clear.weather(5.0);
        let afternoon = clear.weather(15.0);
        assert!(dawn.temperature < afternoon.temperature);
        assert!(
            dawn.humidity > 0.85 && afternoon.humidity < 0.7,
            "{dawn:?} {afternoon:?}"
        );
        assert!(dawn.fog > afternoon.fog);
        assert_eq!(afternoon.fog, 0.0);
    }

    #[test]
    fn humid_and_rainy_days_keep_the_air_foggy() {
        let rain = (1..80)
            .map(Day::of)
            .find(|day| day.sky == Sky::Rain)
            .expect("a rainy day");
        assert!(rain.weather(14.0).fog >= 0.8);
        let opening = Day::of(0);
        assert_eq!(opening.sky, Sky::Humid);
        assert!(opening.weather(START_HOUR).fog > 0.9);
        assert!(opening.weather(3.0).fog > opening.weather(14.0).fog);
    }

    #[test]
    fn weather_changes_smoothly_across_midnight() {
        let before = Weather::at(WorldClock::at(24.0 * 3.0 - 0.01));
        let after = Weather::at(WorldClock::at(24.0 * 3.0 + 0.01));
        assert!((before.temperature - after.temperature).abs() < 0.2);
        assert!((before.humidity - after.humidity).abs() < 0.02);
    }
}
