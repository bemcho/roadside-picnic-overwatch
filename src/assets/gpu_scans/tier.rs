//! How much the GPU can take, judged once a session from the card's size.
//!
//! The live budget (`vram`) drops whenever another program holds VRAM, which
//! would judge a 6 GB card as a 4 GB one; the card's size does not. Effects
//! that scale with the GPU (the Golden Sphere's reflections) read the level.

use bevy::prelude::*;

use super::{VramReading, gib};

/// The card sizes, in GiB, from which a GPU counts as strong (5 GB cards, as the
/// user asked) and exceptional (16 GB cards): drivers report a little under
/// the size sold.
const STRONG_GB: f32 = 4.75;
const EXCEPTIONAL_GB: f32 = 15.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TierLevel {
    /// Under 5 GB (a 4 GB laptop card).
    Modest,
    /// 5 GB and above.
    #[default]
    Strong,
    /// 16 GB and above.
    Exceptional,
}

impl TierLevel {
    /// `--gpu-tier`'s value.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "modest" => Some(Self::Modest),
            "strong" => Some(Self::Strong),
            "exceptional" => Some(Self::Exceptional),
            _ => None,
        }
    }
}

/// The level of a card of `card_bytes`, or of `cap` (`--vram`) if smaller.
pub fn tier_for(card_bytes: u64, cap: Option<u64>) -> TierLevel {
    let card = cap.map_or(card_bytes, |cap| cap.min(card_bytes));
    if card >= gib(EXCEPTIONAL_GB) {
        TierLevel::Exceptional
    } else if card >= gib(STRONG_GB) {
        TierLevel::Strong
    } else {
        TierLevel::Modest
    }
}

/// The session's GPU level: assumed from `--vram` (or the 6 GB fallback) until
/// `settle_gpu_tier` settles it at `Startup`.
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub struct GpuTier {
    level: TierLevel,
    settled: bool,
}

impl GpuTier {
    /// The level a card of `vram_gb` would have, unless `forced` (`--gpu-tier`).
    pub fn fallback(vram_gb: f32, forced: Option<TierLevel>) -> Self {
        Self {
            level: forced.unwrap_or_else(|| tier_for(gib(vram_gb), None)),
            settled: false,
        }
    }

    pub fn level(&self) -> TierLevel {
        self.level
    }

    /// Settles the level the first time: from the card's size in `reading`
    /// (capped by `cap`), or the fallback without one; `forced` always wins.
    /// An `integrated` GPU is modest: the heap it reports is the system's RAM.
    /// Later calls change nothing.
    pub fn settle_with(
        &mut self,
        reading: Option<VramReading>,
        cap: Option<u64>,
        forced: Option<TierLevel>,
        integrated: bool,
    ) {
        if self.settled {
            return;
        }
        self.settled = true;
        if let Some(level) = forced {
            self.level = level;
        } else if integrated {
            self.level = TierLevel::Modest;
        } else if let Some(reading) = reading {
            self.level = tier_for(reading.card, cap);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gib(gb: f32) -> u64 {
        (gb * (1u64 << 30) as f32) as u64
    }

    fn reading(card_gb: f32) -> Option<VramReading> {
        Some(VramReading {
            budget: gib(card_gb - 1.0),
            usage: 0,
            card: gib(card_gb),
        })
    }

    #[test]
    fn the_tier_follows_the_cards_size_capped_by_vram() {
        assert_eq!(tier_for(gib(4.0), None), TierLevel::Modest);
        assert_eq!(
            tier_for(gib(4.9), None),
            TierLevel::Strong,
            "a 5 GB card as its driver reports it"
        );
        assert_eq!(tier_for(gib(6.0), None), TierLevel::Strong);
        assert_eq!(tier_for(gib(16.0), None), TierLevel::Exceptional);
        assert_eq!(tier_for(gib(16.0), Some(gib(4.0))), TierLevel::Modest);
    }

    #[test]
    fn an_integrated_gpu_stays_modest_unless_forced() {
        // Its "device-local" heap is the system's RAM: 15+ GiB on a 32 GB laptop.
        let mut tier = GpuTier::fallback(6.0, None);
        tier.settle_with(reading(16.0), None, None, true);
        assert_eq!(tier.level(), TierLevel::Modest);
        let mut tier = GpuTier::fallback(6.0, None);
        tier.settle_with(None, None, None, true);
        assert_eq!(tier.level(), TierLevel::Modest, "no reading either");
        let forced = Some(TierLevel::Strong);
        let mut tier = GpuTier::fallback(6.0, forced);
        tier.settle_with(reading(16.0), None, forced, true);
        assert_eq!(tier.level(), TierLevel::Strong, "--gpu-tier wins");
    }

    #[test]
    fn the_tier_is_decided_once() {
        let mut tier = GpuTier::fallback(6.0, None);
        assert_eq!(tier.level(), TierLevel::Strong);
        tier.settle_with(reading(4.0), None, None, false);
        assert_eq!(tier.level(), TierLevel::Modest);
        tier.settle_with(reading(16.0), None, None, false);
        assert_eq!(tier.level(), TierLevel::Modest, "settled for the session");
    }

    #[test]
    fn without_a_reading_the_fallback_settles_and_a_forced_tier_wins() {
        let mut tier = GpuTier::fallback(6.0, None);
        tier.settle_with(None, None, None, false);
        assert_eq!(tier.level(), TierLevel::Strong);
        tier.settle_with(reading(4.0), None, None, false);
        assert_eq!(tier.level(), TierLevel::Strong, "settled on the fallback");
        let forced = Some(TierLevel::Exceptional);
        let mut tier = GpuTier::fallback(4.0, forced);
        assert_eq!(tier.level(), TierLevel::Exceptional);
        tier.settle_with(reading(4.0), None, forced, false);
        assert_eq!(tier.level(), TierLevel::Exceptional);
    }
}
