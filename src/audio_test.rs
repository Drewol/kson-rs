use std::{
    f32::consts::SQRT_2,
    sync::mpsc::{channel, Receiver, Sender},
    time::Duration,
};

use egui_inspect::*;

use rodio::Source;

use crate::{
    scene::Scene,
    sources::{self, flanger::flanger, noise::NoiseSource, owned_source::owned_source},
    RuscMixer,
};

pub struct AudioTest {
    mixer: RuscMixer,
    source_owner: Receiver<()>,
    effects: EnabledEffects,
}

impl AudioTest {
    pub fn new(mixer: RuscMixer) -> Self {
        let (_, source_owner) = channel();
        Self {
            mixer,
            source_owner,
            effects: Default::default(),
        }
    }
}

#[derive(Debug, PartialEq, Clone, Copy, Default, EguiInspect)]
struct EnabledEffects {
    flanger: bool,
    wobble: bool,
    low_pass: bool,
    high_pass: bool,
    peaking: bool,
    freq: f32,
    bitcrush: u8,
    #[inspect(min = -48.0, max = 48.0)]
    pitch_shift: i32,
}

impl Scene for AudioTest {
    fn render_ui(&mut self, dt: f64) -> anyhow::Result<()> {
        Ok(())
    }

    fn is_suspended(&self) -> bool {
        false
    }

    fn debug_ui(&mut self, ctx: &egui::Context) -> anyhow::Result<()> {
        let old_effects = self.effects;

        egui::Window::new("Audio Test").show(ctx, |ui| self.effects.inspect_mut("Effects", ui));

        if old_effects != self.effects {
            let (marker, owner) = channel();
            self.source_owner = owner;
            let mut source: Box<dyn Source<Item = f32> + Send> =
                Box::new(NoiseSource::new(44100, 1.0));
            {
                use crate::sources::*;
                let EnabledEffects {
                    flanger,
                    wobble,
                    low_pass,
                    high_pass,
                    peaking,
                    freq,
                    bitcrush,
                    pitch_shift,
                } = self.effects;

                let freq = 20.0f32 * 1000.0f32.powf(freq / 100.0);

                if flanger {
                    source = Box::new(flanger::flanger(
                        source,
                        Duration::from_millis(4),
                        Duration::from_millis(1),
                        0.5,
                    ));
                }

                if wobble {
                    source = Box::new(wobble::wobble(source, 2.0, 500.0, 20000.0));
                }

                if low_pass {
                    source = Box::new(biquad::biquad(
                        source,
                        biquad::BiQuadState::new(biquad::BiQuadType::LowPass, SQRT_2, freq),
                        None,
                    ));
                }

                if high_pass {
                    source = Box::new(biquad::biquad(
                        source,
                        biquad::BiQuadState::new(biquad::BiQuadType::HighPass, SQRT_2, freq),
                        None,
                    ));
                }

                if peaking {
                    source = Box::new(biquad::biquad(
                        source,
                        biquad::BiQuadState::new(biquad::BiQuadType::Peaking(20.0), SQRT_2, freq),
                        None,
                    ));
                }

                if bitcrush != 0 {
                    source = Box::new(bitcrush::bit_crusher(source, bitcrush as u32));
                }

                if pitch_shift != 0 {
                    source = Box::new(pitch_shift::pitch_shift(source, pitch_shift));
                }
            }

            self.mixer.add(owned_source(source, marker))
        }

        Ok(())
    }

    fn closed(&self) -> bool {
        false
    }

    fn name(&self) -> &str {
        "Audio Test"
    }
}
