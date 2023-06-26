use std::{
    f32::consts::SQRT_2,
    sync::{
        mpsc::{channel, Receiver, Sender},
        Arc, RwLock,
    },
    time::Duration,
};

use egui_inspect::*;

use rodio::{decoder::LoopedDecoder, Source};

use crate::{
    scene::Scene,
    sources::{
        self,
        flanger::flanger,
        noise::NoiseSource,
        owned_source::owned_source,
        takeable_source::{self, TakeableSource},
    },
    RuscMixer,
};

pub struct AudioTest {
    mixer: RuscMixer,
    source_owner: Receiver<()>,
    real_source: Option<Arc<RwLock<Option<LoopedDecoder<std::fs::File>>>>>,
    effects: EnabledEffects,
}

impl AudioTest {
    pub fn new(mixer: RuscMixer) -> Self {
        let (_, source_owner) = channel();

        let source = if let Ok(a) = std::fs::File::open("sound_test.wav") {
            rodio::Decoder::new_looped(a).ok()
        } else {
            None
        }
        .map(|x| TakeableSource::new(x));

        let source = if let Some((source, real_source)) = source {
            mixer.add(source.convert_samples());
            Some(real_source)
        } else {
            None
        };

        Self {
            mixer,
            source_owner,
            effects: Default::default(),
            real_source: source,
        }
    }

    fn apply_effects(
        &self,
        mut source: Box<dyn Source<Item = f32> + Send>,
    ) -> Box<dyn Source<Item = f32> + Send> {
        use crate::sources::*;
        let EnabledEffects {
            volume,
            flanger,
            wobble,
            low_pass,
            high_pass,
            peaking,
            freq,
            bitcrush,
            pitch_shift,
        } = self.effects;

        source = Box::new(source.amplify(volume / 100.0));

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
            source = Box::new(wobble::wobble(source, 4.0, 500.0, 20000.0));
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

        source
    }
}

#[derive(Debug, PartialEq, Clone, Copy, Default, EguiInspect)]
struct EnabledEffects {
    volume: f32,
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
            let source: Box<dyn Source<Item = f32> + Send> =
                if let Some(takeable) = self.real_source.take() {
                    if let Some(source) = takeable.write().unwrap().take() {
                        let (source, taker) = TakeableSource::new(source);
                        self.real_source = Some(taker);

                        Box::new(source.convert_samples())
                    } else {
                        Box::new(NoiseSource::new(44100, 1.0))
                    }
                } else {
                    Box::new(NoiseSource::new(44100, 1.0))
                };

            self.mixer
                .add(owned_source(self.apply_effects(source), marker))
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
