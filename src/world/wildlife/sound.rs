use bevy::prelude::*;

use super::Kind;
use super::movement::Wildlife;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Cue {
    Step(f32),
    Call,
    Alarm,
    Growl,
    Strike,
    Impact,
    Wings,
    Land,
}

#[derive(Clone, Copy, Debug, Message)]
pub(crate) struct CreatureSound {
    pub kind: Kind,
    pub cue: Cue,
    pub at: Vec3,
}

impl CreatureSound {
    pub(super) fn new(kind: Kind, cue: Cue, at: Vec3) -> Self {
        Self { kind, cue, at }
    }
}

pub(super) struct MotionSound {
    at: Vec3,
    age: f32,
    gait: f32,
    air: f32,
}

impl MotionSound {
    pub(super) fn before(at: Vec3, motion: &Wildlife) -> Self {
        Self {
            at,
            age: motion.age,
            gait: motion.gait,
            air: motion.air,
        }
    }

    pub(super) fn emit(
        &self,
        at: Vec3,
        motion: &Wildlife,
        alert: bool,
        sounds: &mut MessageWriter<CreatureSound>,
    ) {
        if motion.age <= self.age {
            return;
        }
        let kind = motion.kind;
        let mut emit = |cue, height| {
            sounds.write(CreatureSound::new(kind, cue, at + Vec3::Y * height));
        };
        let grounded = motion.air <= 0.025;
        let was_grounded = self.air <= 0.025;
        if !was_grounded && grounded {
            emit(Cue::Land, 0.05);
        } else if was_grounded && !grounded {
            emit(Cue::Wings, 0.55);
        } else if grounded && kind != Kind::Chameleon && motion.lunge <= 0.25 {
            let actual = self.at.xz().distance(at.xz());
            let contacts = if kind.is_bird()
                || (matches!(kind, Kind::Mutt | Kind::Chimera) && motion.run >= 0.75)
            {
                2.0
            } else {
                4.0
            };
            if (0.001..1.7).contains(&actual)
                && (self.gait * contacts).floor() != (motion.gait * contacts).floor()
            {
                emit(Cue::Step(motion.run), 0.04);
            }
        }
        if !alert {
            let seed = motion.call_seed();
            let period = if kind.is_bird() { 7.0 } else { 13.0 } + seed * 9.0;
            let offset = seed * period;
            if ((motion.age + offset) / period).floor() > ((self.age + offset) / period).floor() {
                emit(
                    Cue::Call,
                    if grounded { kind.model().1 * 0.36 } else { 0.0 },
                );
            }
        }
    }
}
