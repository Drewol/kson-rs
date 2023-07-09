use std::time::Duration;

use itertools::Itertools;
use rodio::{Sample, Source};

pub fn tape_stop<I: Source<Item = D>, D: Sample>(
    mut input: I,
    duration: Duration,
) -> TapeStop<I, D> {
    let duration = duration.as_secs_f64();
    let step = 1.0f64 / input.sample_rate() as f64;
    let channels = input.channels();
    let held_samples = (0..channels).map(|_| input.next()).collect_vec();

    TapeStop {
        input,
        sample_advance: 1.0,
        held_samples,
        channel: 0,
        channels,
        duration,
        countdown: duration,
        step,
    }
}

pub struct TapeStop<I: Source<Item = D>, D: Sample> {
    input: I,
    sample_advance: f64,
    held_samples: Vec<Option<D>>,
    channel: u16,
    channels: u16,
    duration: f64,
    countdown: f64,
    step: f64,
}

impl<I, D> Iterator for TapeStop<I, D>
where
    I: Source<Item = D>,
    D: Sample,
{
    type Item = D;

    fn next(&mut self) -> Option<Self::Item> {
        let c = self.channel as usize;
        self.channel += 1;
        if self.channel >= self.channels {
            self.channel = 0;
            if self.countdown > 0.0 {
                self.countdown -= self.step;
                self.sample_advance -= self.countdown / self.duration;

                while self.sample_advance <= 0.0 {
                    self.sample_advance += 1.0;
                    for i in 0..self.channels {
                        self.held_samples[i as usize] = self.input.next()
                    }
                }
            }
        }

        self.held_samples[c]
    }
}

impl<I, D> Source for TapeStop<I, D>
where
    I: Source<Item = D>,
    D: Sample,
{
    fn current_frame_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> u16 {
        self.channels
    }

    fn sample_rate(&self) -> u32 {
        self.input.sample_rate()
    }

    fn total_duration(&self) -> Option<std::time::Duration> {
        None
    }
}
