#![allow(unused)]
/// Copied from rodio source and modified with fade out logic.
/// TODO: Upstream?
use std::{
    sync::{atomic::AtomicUsize, Arc},
    time::Duration,
};

use rodio::{Sample, Source};

/// Internal function that builds a `TakeDuration` object.
pub fn take_duration_fade<I>(
    input: I,
    duration: Duration,
    fade: Duration,
    signal: Arc<AtomicUsize>,
) -> TakeDurationFade<I>
where
    I: Source,
    I::Item: Sample,
{
    TakeDurationFade {
        current_frame_len: input.current_frame_len(),
        duration_per_sample: TakeDurationFade::get_duration_per_sample(&input),
        input,
        remaining_duration: duration,
        requested_duration: duration,
        filter: Some(DurationFilter::FadeOut(fade)),
        signal,
        signal_sent: false,
    }
}

/// A filter that can be applied to a `TakeDuration`.
#[derive(Clone, Debug)]
enum DurationFilter {
    FadeOut(Duration),
}
impl DurationFilter {
    fn apply<I: Iterator>(
        &self,
        sample: <I as Iterator>::Item,
        parent: &TakeDurationFade<I>,
    ) -> (<I as Iterator>::Item, bool)
    where
        I::Item: Sample,
    {
        use self::DurationFilter::*;
        match self {
            FadeOut(fade) => {
                let remaining = parent.remaining_duration.as_millis() as f32;
                let fade = fade.as_millis() as f32;
                (
                    sample.amplify((remaining / fade).min(1.0)),
                    remaining < fade,
                )
            }
        }
    }
}

const NANOS_PER_SEC: u64 = 1_000_000_000;

/// A source that truncates the given source to a certain duration.
#[derive(Clone, Debug)]
pub struct TakeDurationFade<I> {
    input: I,
    remaining_duration: Duration,
    requested_duration: Duration,
    filter: Option<DurationFilter>,
    // Remaining samples in current frame.
    current_frame_len: Option<usize>,
    // Only updated when the current frame len is exhausted.
    duration_per_sample: Duration,
    signal: Arc<AtomicUsize>,
    signal_sent: bool,
}

impl<I> TakeDurationFade<I>
where
    I: Source,
    I::Item: Sample,
{
    /// Returns the duration elapsed for each sample extracted.
    #[inline]
    fn get_duration_per_sample(input: &I) -> Duration {
        let ns = NANOS_PER_SEC / (input.sample_rate() as u64 * input.channels() as u64);
        // \|/ the maximum value of `ns` is one billion, so this can't fail
        Duration::new(0, ns as u32)
    }

    /// Returns a reference to the inner source.
    #[inline]
    pub const fn inner(&self) -> &I {
        &self.input
    }

    /// Returns a mutable reference to the inner source.
    #[inline]
    pub fn inner_mut(&mut self) -> &mut I {
        &mut self.input
    }

    /// Returns the inner source.
    #[inline]
    pub fn into_inner(self) -> I {
        self.input
    }

    pub fn set_filter_fadeout(&mut self, fade: Duration) {
        self.filter = Some(DurationFilter::FadeOut(fade));
    }

    pub fn clear_filter(&mut self) {
        self.filter = None;
    }

    fn send_signal(&mut self) {
        if !self.signal_sent {
            self.signal
                .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
            self.signal_sent = true;
        }
    }
}

impl<I> Iterator for TakeDurationFade<I>
where
    I: Source,
    I::Item: Sample,
{
    type Item = <I as Iterator>::Item;

    fn next(&mut self) -> Option<<I as Iterator>::Item> {
        if let Some(frame_len) = self.current_frame_len.take() {
            if frame_len > 0 {
                self.current_frame_len = Some(frame_len - 1);
            } else {
                self.current_frame_len = self.input.current_frame_len();
                // Sample rate might have changed
                self.duration_per_sample = Self::get_duration_per_sample(&self.input);
            }
        }

        if self.remaining_duration <= self.duration_per_sample {
            self.send_signal();
            None
        } else if let Some(sample) = self.input.next() {
            let (sample, send_signal) = match &self.filter {
                Some(filter) => filter.apply(sample, self),
                None => (sample, false),
            };
            if send_signal {
                self.send_signal();
            }

            self.remaining_duration -= self.duration_per_sample;

            Some(sample)
        } else {
            self.send_signal();
            None
        }
    }

    // TODO: size_hint
}

impl<I> Source for TakeDurationFade<I>
where
    I: Iterator + Source,
    I::Item: Sample,
{
    #[inline]
    fn current_frame_len(&self) -> Option<usize> {
        let remaining_nanos = self
            .remaining_duration
            .as_secs()
            .saturating_mul(NANOS_PER_SEC)
            .saturating_add(self.remaining_duration.subsec_nanos() as u64);
        let nanos_per_sample = self
            .duration_per_sample
            .as_secs()
            .saturating_mul(NANOS_PER_SEC)
            .saturating_add(self.duration_per_sample.subsec_nanos() as u64);
        let remaining_samples = (remaining_nanos / nanos_per_sample) as usize;

        self.input
            .current_frame_len()
            .filter(|value| *value < remaining_samples)
            .or(Some(remaining_samples))
    }

    #[inline]
    fn channels(&self) -> u16 {
        self.input.channels()
    }

    #[inline]
    fn sample_rate(&self) -> u32 {
        self.input.sample_rate()
    }

    #[inline]
    fn total_duration(&self) -> Option<Duration> {
        if let Some(duration) = self.input.total_duration() {
            if duration < self.requested_duration {
                Some(duration)
            } else {
                Some(self.requested_duration)
            }
        } else {
            None
        }
    }
}
