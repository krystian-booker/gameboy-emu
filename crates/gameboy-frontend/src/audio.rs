use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    FromSample, SizedSample, Stream,
};
use gameboy_core::apu::{AudioSample, SAMPLE_RATE};

/// Interleaved stereo f32 samples shared between the emulator thread (producer)
/// and the cpal audio callback (consumer).
type SharedBuffer = Arc<Mutex<VecDeque<f32>>>;

/// Owns the live audio output stream and the plumbing needed to feed it.
///
/// The cpal stream must stay alive for playback to continue, so callers keep
/// this struct around for the lifetime of the emulator loop.
pub struct AudioOutput {
    _stream: Stream,
    buffer: SharedBuffer,
    resampler: Resampler,
    channels: usize,
    /// Target maximum number of interleaved samples to keep buffered. Used to
    /// throttle emulation to the rate the audio device drains samples.
    max_buffered: usize,
}

impl AudioOutput {
    /// Opens the default output device and starts playback. Returns `None`
    /// (and logs a message) if no device is available or the stream fails to
    /// build, so the frontend can fall back to running without sound.
    pub fn new() -> Option<Self> {
        let host = cpal::default_host();
        let device = host.default_output_device()?;
        let supported = device.default_output_config().ok()?;

        let sample_format = supported.sample_format();
        let config: cpal::StreamConfig = supported.into();
        let channels = config.channels as usize;
        let out_rate = config.sample_rate.0;

        let buffer: SharedBuffer = Arc::new(Mutex::new(VecDeque::new()));

        let stream = match sample_format {
            cpal::SampleFormat::F32 => build_stream::<f32>(&device, &config, buffer.clone()),
            cpal::SampleFormat::I16 => build_stream::<i16>(&device, &config, buffer.clone()),
            cpal::SampleFormat::U16 => build_stream::<u16>(&device, &config, buffer.clone()),
            other => {
                eprintln!("unsupported audio sample format: {other:?}");
                return None;
            }
        }?;

        if let Err(err) = stream.play() {
            eprintln!("failed to start audio stream: {err}");
            return None;
        }

        // Keep roughly 100ms of audio buffered before throttling the emulator.
        let max_buffered = (out_rate as usize / 10) * channels;

        Some(Self {
            _stream: stream,
            buffer,
            resampler: Resampler::new(SAMPLE_RATE, out_rate),
            channels,
            max_buffered,
        })
    }

    /// Resamples a batch of APU samples to the device rate and queues them.
    pub fn queue(&mut self, samples: &[AudioSample]) {
        if samples.is_empty() {
            return;
        }

        let mut stereo = Vec::new();
        self.resampler.process(samples, &mut stereo);

        let mut buffer = self.buffer.lock().unwrap();
        if self.channels == 2 {
            buffer.extend(stereo);
        } else {
            // Fan the stereo pair out to however many channels the device wants.
            for pair in stereo.chunks_exact(2) {
                for channel in 0..self.channels {
                    buffer.push_back(pair[channel.min(1)]);
                }
            }
        }
    }

    /// Blocks until the audio device has drained the queue below the target
    /// fill level. This paces emulation to real time using the audio clock.
    pub fn wait_for_drain(&self) {
        loop {
            let len = self.buffer.lock().unwrap().len();
            if len <= self.max_buffered {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }
}

fn build_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    buffer: SharedBuffer,
) -> Option<Stream>
where
    T: SizedSample + FromSample<f32>,
{
    let err_fn = |err| eprintln!("audio stream error: {err}");
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                let mut buffer = buffer.lock().unwrap();
                for slot in data.iter_mut() {
                    let sample = buffer.pop_front().unwrap_or(0.0);
                    *slot = T::from_sample(sample);
                }
            },
            err_fn,
            None,
        )
        .map_err(|err| eprintln!("failed to build audio stream: {err}"))
        .ok()
}

/// Streaming linear resampler from the APU's fixed rate to the device rate.
struct Resampler {
    /// Input samples consumed per output sample (`in_rate / out_rate`).
    step: f64,
    /// Fractional position between the previous and current input sample.
    frac: f64,
    prev_left: f32,
    prev_right: f32,
    started: bool,
}

impl Resampler {
    fn new(in_rate: u32, out_rate: u32) -> Self {
        Self {
            step: in_rate as f64 / out_rate as f64,
            frac: 0.0,
            prev_left: 0.0,
            prev_right: 0.0,
            started: false,
        }
    }

    /// Appends resampled interleaved stereo f32 samples to `out`.
    fn process(&mut self, input: &[AudioSample], out: &mut Vec<f32>) {
        for sample in input {
            let cur_left = sample.left as f32 / 32768.0;
            let cur_right = sample.right as f32 / 32768.0;

            if !self.started {
                self.prev_left = cur_left;
                self.prev_right = cur_right;
                self.started = true;
            }

            while self.frac < 1.0 {
                let t = self.frac as f32;
                out.push(self.prev_left + (cur_left - self.prev_left) * t);
                out.push(self.prev_right + (cur_right - self.prev_right) * t);
                self.frac += self.step;
            }
            self.frac -= 1.0;

            self.prev_left = cur_left;
            self.prev_right = cur_right;
        }
    }
}
