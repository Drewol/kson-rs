use std::time::Duration;

use rodio::{source::Delay, Source};

pub trait ConsumeOne<R> {
    /// Consume the first sample of a source as a workaround for initializing(?) some decoders
    fn consume_one(self) -> Result<R, rodio::decoder::DecoderError>;
}

impl<T> ConsumeOne<Delay<T>> for T
where
    T: Source,
{
    fn consume_one(mut self) -> Result<Delay<T>, rodio::decoder::DecoderError> {
        // Read the first sample, bug with decoder initialization?
        for _ in 0..self.channels().get() {
            self.next()
                .ok_or(rodio::decoder::DecoderError::DecodeError("Empty audio"))?;
        }

        let delay = 1_000_000_000u128 / self.sample_rate().get() as u128;

        // Compensate read sample
        Ok(self.delay(Duration::from_nanos(delay as u64)))
    }
}
