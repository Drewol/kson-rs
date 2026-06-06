use rodio::{ChannelCount, SampleRate};
use rodio::{Sample, Source};
pub mod biquad;
pub mod bitcrush;
pub mod effected_part;
pub mod flanger;
pub mod gate;
pub mod mix_source;
pub mod noise;
pub mod owned_source;
pub mod phaser;
#[cfg(not(target_os = "android"))]
pub mod pitch_shift;
#[cfg(target_os = "android")]
pub mod pitch_shift_passthrough;
#[cfg(target_os = "android")]
pub use pitch_shift_passthrough as pitch_shift;

pub mod re_trigger;
pub mod side_chain;
pub mod takeable_source;
pub mod tape_stop;
pub mod triangle;
pub mod wobble;

// Copied from rodio
fn lerp(first: f32, second: f32, numerator: u32, denominator: u32) -> f32 {
    first + (second - first) * numerator as f32 / denominator as f32
}
