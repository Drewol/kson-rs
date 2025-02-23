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
