use cpal::Sample as CpalSample;
use rodio::{cpal, ChannelCount, SampleRate, Source};

pub struct ChartAudio {
    /// twice the length of the song, second half is effected
    samples: Vec<f32>,
    cursor: usize,
    channels: ChannelCount,
    sample_rate: SampleRate,
    effec_active: bool,
    effect_offset: usize,
}
#[allow(unused)]
pub trait ChartAudioSource: Source
where
    Self: Sized,
{
    fn chart_audio(self, chart: kson::Chart) -> ChartAudio {
        let channels = self.channels();
        let sample_rate = self.sample_rate();
        let samples = self
            .map(|s| s.to_float_sample().to_sample()) //TODO: idk but it works, should probably make it a generic "Sample" all the way down
            .collect::<Vec<_>>()
            .repeat(2);
        let effect_offset = samples.len();
        let chart_audio = ChartAudio {
            samples,
            cursor: 0,
            channels,
            sample_rate,
            effec_active: false,
            effect_offset,
        };

        //TODO: Render effects
        let _effects = chart.get_effect_tracks();
        // for _effect in effects
        //     .iter()
        //     .filter(|x| matches!(x.track, Some(kson::Track::FX(_))))
        // {}

        chart_audio
    }
}

impl Source for ChartAudio {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> ChannelCount {
        self.channels
    }

    fn sample_rate(&self) -> SampleRate {
        self.sample_rate
    }

    fn total_duration(&self) -> Option<std::time::Duration> {
        Some(std::time::Duration::from_secs_f64(
            (self.effect_offset - self.cursor) as f64 / self.sample_rate.get() as f64,
        ))
    }
}

impl Iterator for ChartAudio {
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        let res = if self.effec_active {
            self.samples.get(self.cursor + self.effect_offset).copied()
        } else {
            self.samples.get(self.cursor).copied()
        };
        self.cursor += 1;

        res
    }
}
