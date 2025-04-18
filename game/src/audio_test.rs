use std::{
    f32::consts::SQRT_2,
    sync::{mpsc::channel, Arc, RwLock},
    time::Duration,
};

use di::ServiceProvider;

use rodio::{
    decoder::LoopedDecoder,
    dynamic_mixer::{self},
    Source,
};

use kson_rodio_sources::{
    biquad::{biquad, BiQuadState, BiquadController},
    noise::NoiseSource,
    owned_source::{self, owned_source},
    takeable_source::TakeableSource,
};

use crate::{scene::Scene, InnerRuscMixer, RuscMixer};

pub struct AudioTest {
    mixer: RuscMixer,
    source_owner: owned_source::Marker,
    _master_owner: owned_source::Marker,
    real_source: Option<Arc<RwLock<Option<LoopedDecoder<std::fs::File>>>>>,
    effects: EnabledEffects,
    biquad_controllers: [BiquadController; 3],
}

impl AudioTest {
    pub fn new(services: ServiceProvider) -> Self {
        let (inner_mixer, mixer_source) = dynamic_mixer::mixer(2, 44100);
        inner_mixer.add(rodio::source::Zero::new(2, 44100));
        let [a, b, c] = [channel(), channel(), channel()];

        let mixer_source = biquad(mixer_source, Default::default(), Some(a.1));
        let mixer_source = biquad(mixer_source, Default::default(), Some(b.1));
        let mixer_source = biquad(mixer_source, Default::default(), Some(c.1));
        let master_owner = owned_source::Marker::new();
        let source_owner = owned_source::Marker::new();

        services
            .get_required::<InnerRuscMixer>()
            .add(owned_source(mixer_source, &master_owner));
        let mixer = inner_mixer;

        let source = if let Ok(a) = std::fs::File::open("sound_test.wav") {
            rodio::Decoder::new_looped(a).ok()
        } else {
            None
        }
        .map(TakeableSource::new);

        let source = if let Some((source, real_source)) = source {
            mixer.add(source.convert_samples());
            Some(real_source)
        } else {
            None
        };

        Self {
            mixer,
            source_owner,
            _master_owner: master_owner,
            effects: Default::default(),
            real_source: source,
            biquad_controllers: [a.0, b.0, c.0],
        }
    }

    fn apply_effects(
        &self,
        mut source: Box<dyn Source<Item = f32> + Send>,
    ) -> Box<dyn Source<Item = f32> + Send> {
        use kson_rodio_sources::*;
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
            tape_stop,
        } = self.effects;

        source = Box::new(source.amplify(volume / 100.0));

        let freq = 20.0f32 * 1000.0f32.powf(freq / 100.0);
        if flanger {
            source = Box::new(flanger::flanger(
                source,
                Duration::from_millis(4),
                Duration::from_millis(1),
                0.5,
                0.05,
            ));
        }
        if wobble {
            source = Box::new(wobble::wobble(source, 4.0, 500.0, 20000.0));
        }
        if low_pass {
            _ = self.biquad_controllers[2].send((
                Some(BiQuadState::new(biquad::BiQuadType::LowPass, SQRT_2, freq)),
                Some(1.0),
            ))
        } else {
            _ = self.biquad_controllers[2].send((None, Some(0.0)))
        }

        if high_pass {
            _ = self.biquad_controllers[1].send((
                Some(BiQuadState::new(biquad::BiQuadType::HighPass, SQRT_2, freq)),
                Some(1.0),
            ))
        } else {
            _ = self.biquad_controllers[1].send((None, Some(0.0)))
        }

        if peaking {
            _ = self.biquad_controllers[0].send((
                Some(BiQuadState::new(
                    biquad::BiQuadType::Peaking(20.0),
                    SQRT_2,
                    freq,
                )),
                Some(1.0),
            ));
        } else {
            _ = self.biquad_controllers[0].send((None, Some(0.0)))
        }

        if bitcrush != 0 {
            source = Box::new(bitcrush::bit_crusher(source, bitcrush as u32));
        }
        if pitch_shift != 0 {
            source = Box::new(pitch_shift::pitch_shift(source, pitch_shift));
        }

        if tape_stop {
            source = Box::new(tape_stop::tape_stop(
                source,
                Duration::ZERO,
                Duration::from_secs(1),
            ));
        }

        source
    }
}

#[derive(Debug, PartialEq, Clone, Copy, Default, serde::Serialize, serde::Deserialize)]
struct EnabledEffects {
    volume: f32,
    flanger: bool,
    wobble: bool,
    low_pass: bool,
    high_pass: bool,
    peaking: bool,
    freq: f32,
    bitcrush: u8,
    pitch_shift: i32,
    tape_stop: bool,
}

impl Scene for AudioTest {
    fn render_ui(&mut self, _dt: f64) -> anyhow::Result<()> {
        Ok(())
    }

    fn is_suspended(&self) -> bool {
        false
    }

    fn debug_ui(&mut self, _ctx: &egui::Context) -> anyhow::Result<()> {
        let old_effects = self.effects;

        // TODO: egui::Window::new("Audio Test").show(ctx, |ui|  self.effects.inspect_mut("Effects", ui));

        if old_effects != self.effects {
            self.source_owner = owned_source::Marker::new();
            let source: Box<dyn Source<Item = f32> + Send> =
                if let Some(takeable) = self.real_source.take() {
                    if let Some(source) = takeable.write().expect("Lock error").take() {
                        let (source, taker) = TakeableSource::new(source);
                        self.real_source = Some(taker);

                        Box::new(source.convert_samples())
                    } else {
                        Box::new(NoiseSource::new(44100, 1.0, 2))
                    }
                } else {
                    Box::new(NoiseSource::new(44100, 1.0, 2))
                };

            self.mixer
                .add(owned_source(self.apply_effects(source), &self.source_owner))
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
