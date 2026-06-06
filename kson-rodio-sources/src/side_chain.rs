use rodio::{ChannelCount, SampleRate};
use rodio::{Sample, Source};
use std::ops::Mul;
use std::time::Duration;

use super::mix_source::MixSource;

pub struct SideChain<I: Source> {
    input: I,
    time: u64,
    channel: u16,
    channels: ChannelCount,
    length: u64,
    attack: u64,
    hold: u64,
    release: u64,
    countdown: u128,
    mix: f32,
    ratio: f32,
}

pub fn side_chain<I: Source>(
    source: I,
    start: Duration,
    duration: Duration,
    attack: Duration,
    hold: Duration,
    release: Duration,
    ratio: f32,
) -> SideChain<I> {
    let channels = source.channels();
    let sample_rate = source.sample_rate().get() as f64;

    let dur_to_u64 = |dur: &Duration| (dur.as_secs_f64() * sample_rate) as u64;

    SideChain {
        input: source,
        time: 0,
        channel: 0,
        channels,
        length: dur_to_u64(&duration),
        attack: dur_to_u64(&attack),
        hold: dur_to_u64(&hold),
        release: dur_to_u64(&release),
        countdown: (start.as_secs_f64() * sample_rate * channels.get() as f64) as u128,
        mix: 1.0,
        ratio,
    }
}

impl<I> Iterator for SideChain<I>
where
    I: Source,
{
    type Item = I::Item;

    fn next(&mut self) -> Option<Self::Item> {
        let original = self.input.next();
        if self.countdown > 0 || self.mix < f32::EPSILON {
            self.countdown = self.countdown.saturating_sub(1);
            return original;
        }

        let volume = if self.time < self.attack {
            1.0 - (self.time as f32 / self.attack as f32)
        } else if self.time < self.attack + self.hold {
            0.0
        } else if self.time < self.attack + self.hold + self.release {
            (self.time - self.attack - self.hold) as f32 / self.release as f32
        } else {
            1.0
        };
        // range from 1/ratio (volume=0) to 1 (volume=1)
        let sample_gain =
            self.mix * ((1.0 / self.ratio) + (1.0 - 1.0 / self.ratio) * volume) + (1.0 - self.mix);

        self.channel = (self.channel + 1) % self.channels;

        if self.channel == 0 {
            self.time = (self.time + 1) % self.length;
        }

        original.map(|x| x.mul(sample_gain))
    }
}

impl<I> Source for SideChain<I>
where
    I: Source,
{
    fn current_span_len(&self) -> Option<usize> {
        self.input.current_span_len()
    }

    fn channels(&self) -> ChannelCount {
        self.input.channels()
    }

    fn sample_rate(&self) -> SampleRate {
        self.input.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.input.total_duration()
    }
}

impl<I> MixSource for SideChain<I>
where
    I: Source,
{
    fn set_mix(&mut self, mix: f32) {
        self.mix = mix;
    }
}
