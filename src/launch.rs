//! Command-line launch options, parsed once in `main` and shared as a resource.
//!
//! `--new` ignores the saved progress; `--vram <GB>` caps the GPU memory the
//! game may use; `--gpu-tier modest|strong|exceptional` forces the GPU level
//! effects scale to, for testing. Unknown arguments are ignored.

use bevy::prelude::*;

use crate::assets::gpu_scans::TierLevel;

/// GB of GPU memory the game assumes where the driver cannot report it and
/// `--vram` is not given.
pub const DEFAULT_VRAM_GB: f32 = 6.0;

#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct LaunchOptions {
    /// Start afresh instead of restoring the saved progress.
    pub new: bool,
    /// `--vram`: the most GPU memory in GB the game may use; `None` takes what
    /// the driver reports.
    pub vram_gb: Option<f32>,
    /// `--gpu-tier`: the GPU level to assume, whatever the card.
    pub gpu_tier: Option<TierLevel>,
}

impl LaunchOptions {
    pub fn parse(args: impl IntoIterator<Item = String>) -> Self {
        let mut options = Self::default();
        let mut args = args.into_iter().peekable();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--new" => options.new = true,
                "--vram" => {
                    // A following flag is not the value: leave it for the next turn.
                    let value = args.next_if(|next| !next.starts_with("--"));
                    options.vram_gb = vram_gb(value.as_deref());
                }
                "--gpu-tier" => {
                    let value = args.next_if(|next| !next.starts_with("--"));
                    options.gpu_tier = value.as_deref().and_then(TierLevel::parse);
                    if options.gpu_tier.is_none() {
                        warn!("--gpu-tier needs modest, strong or exceptional; ignoring it");
                    }
                }
                _ => {}
            }
        }
        options
    }

    pub fn from_env() -> Self {
        Self::parse(std::env::args().skip(1))
    }
}

/// A positive, finite number of GB; anything else warns and gives `None`.
fn vram_gb(value: Option<&str>) -> Option<f32> {
    let parsed = value
        .and_then(|value| value.parse::<f32>().ok())
        .filter(|gb| gb.is_finite() && *gb > 0.0);
    if parsed.is_none() {
        match value {
            Some(value) => warn!("--vram needs a positive number of GB, not {value}; ignoring it"),
            None => warn!("--vram needs a positive number of GB after it; ignoring it"),
        }
    }
    parsed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> LaunchOptions {
        LaunchOptions::parse(args.iter().map(|arg| arg.to_string()))
    }

    #[test]
    fn defaults_are_a_saved_run_without_a_vram_cap() {
        assert_eq!(
            parse(&[]),
            LaunchOptions {
                new: false,
                vram_gb: None,
                gpu_tier: None,
            }
        );
    }

    #[test]
    fn flags_parse_in_any_order() {
        assert_eq!(
            parse(&["--vram", "12", "--new"]),
            LaunchOptions {
                new: true,
                vram_gb: Some(12.0),
                gpu_tier: None,
            }
        );
        assert_eq!(
            parse(&["--new", "--vram", "7.5"]),
            LaunchOptions {
                new: true,
                vram_gb: Some(7.5),
                gpu_tier: None,
            }
        );
    }

    #[test]
    fn a_bad_vram_value_is_ignored() {
        for args in [
            &["--vram", "abc"][..],
            &["--vram"],
            &["--vram", "-3"],
            &["--vram", "0"],
            &["--vram", "NaN"],
        ] {
            assert_eq!(parse(args).vram_gb, None, "{args:?}");
        }
    }

    #[test]
    fn a_flag_after_vram_is_not_its_value() {
        assert_eq!(
            parse(&["--vram", "--new"]),
            LaunchOptions {
                new: true,
                vram_gb: None,
                gpu_tier: None,
            }
        );
    }

    #[test]
    fn unknown_arguments_are_ignored() {
        assert_eq!(parse(&["--bogus", "12", "-x"]), LaunchOptions::default());
    }

    #[test]
    fn a_gpu_tier_can_be_forced() {
        assert_eq!(
            parse(&["--gpu-tier", "exceptional"]).gpu_tier,
            Some(TierLevel::Exceptional)
        );
        assert_eq!(parse(&["--gpu-tier", "huge", "--new"]).gpu_tier, None);
        assert!(parse(&["--gpu-tier", "huge", "--new"]).new);
    }
}
