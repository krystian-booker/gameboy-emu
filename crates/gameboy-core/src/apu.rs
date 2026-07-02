pub const SAMPLE_RATE: u32 = 44_100;

const CPU_HZ: u32 = 4_194_304;
const FRAME_SEQUENCER_PERIOD: u32 = CPU_HZ / 512;
const DUTY_PATTERNS: [[u8; 8]; 4] = [
    [0, 0, 0, 0, 0, 0, 0, 1],
    [1, 0, 0, 0, 0, 0, 0, 1],
    [1, 0, 0, 0, 0, 1, 1, 1],
    [0, 1, 1, 1, 1, 1, 1, 0],
];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AudioSample {
    pub left: i16,
    pub right: i16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Apu {
    powered: bool,
    frame_cycles: u32,
    frame_step: u8,
    sample_cycles: u32,
    sample_buffer: Vec<AudioSample>,
    nr50: u8,
    nr51: u8,
    pulse1: PulseChannel,
    pulse2: PulseChannel,
    wave: WaveChannel,
    noise: NoiseChannel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PulseChannel {
    has_sweep: bool,
    enabled: bool,
    dac_enabled: bool,
    duty: u8,
    length_timer: u16,
    length_enabled: bool,
    volume: u8,
    envelope_initial: u8,
    envelope_increase: bool,
    envelope_period: u8,
    envelope_timer: u8,
    frequency: u16,
    period_timer: u16,
    duty_step: u8,
    sweep_period: u8,
    sweep_negate: bool,
    sweep_shift: u8,
    sweep_timer: u8,
    sweep_shadow: u16,
    sweep_enabled: bool,
    sweep_negated_since_trigger: bool,
    nr10: u8,
    nr11: u8,
    nr12: u8,
    nr13: u8,
    nr14: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WaveChannel {
    enabled: bool,
    dac_enabled: bool,
    length_timer: u16,
    length_enabled: bool,
    volume_code: u8,
    frequency: u16,
    period_timer: u16,
    sample_index: u8,
    wave_ram: [u8; 16],
    nr30: u8,
    nr31: u8,
    nr32: u8,
    nr33: u8,
    nr34: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NoiseChannel {
    enabled: bool,
    dac_enabled: bool,
    length_timer: u16,
    length_enabled: bool,
    volume: u8,
    envelope_initial: u8,
    envelope_increase: bool,
    envelope_period: u8,
    envelope_timer: u8,
    clock_shift: u8,
    width_mode: bool,
    divisor_code: u8,
    period_timer: u16,
    lfsr: u16,
    nr41: u8,
    nr42: u8,
    nr43: u8,
    nr44: u8,
}

impl Default for Apu {
    fn default() -> Self {
        Self {
            powered: false,
            frame_cycles: 0,
            frame_step: 0,
            sample_cycles: 0,
            sample_buffer: Vec::new(),
            nr50: 0,
            nr51: 0,
            pulse1: PulseChannel::new(true),
            pulse2: PulseChannel::new(false),
            wave: WaveChannel::default(),
            noise: NoiseChannel::default(),
        }
    }
}

impl Apu {
    pub fn advance_cycles(&mut self, cycles: u32) {
        if !self.powered {
            return;
        }

        for _ in 0..cycles {
            self.clock_channels();
            self.frame_cycles += 1;
            if self.frame_cycles >= FRAME_SEQUENCER_PERIOD {
                self.frame_cycles -= FRAME_SEQUENCER_PERIOD;
                self.clock_frame_sequencer();
            }

            self.sample_cycles += SAMPLE_RATE;
            if self.sample_cycles >= CPU_HZ {
                self.sample_cycles -= CPU_HZ;
                self.sample_buffer.push(self.mix_sample());
            }
        }
    }

    pub fn drain_samples(&mut self) -> Vec<AudioSample> {
        self.sample_buffer.drain(..).collect()
    }

    pub fn read_register(&self, address: u16) -> Option<u8> {
        Some(match address {
            0xFF10 => self.pulse1.nr10 | 0x80,
            0xFF11 => self.pulse1.nr11 | 0x3F,
            0xFF12 => self.pulse1.nr12,
            0xFF13 => 0xFF,
            0xFF14 => self.pulse1.nr14 | 0xBF,
            0xFF15 => 0xFF,
            0xFF16 => self.pulse2.nr11 | 0x3F,
            0xFF17 => self.pulse2.nr12,
            0xFF18 => 0xFF,
            0xFF19 => self.pulse2.nr14 | 0xBF,
            0xFF1A => self.wave.nr30 | 0x7F,
            0xFF1B => 0xFF,
            0xFF1C => self.wave.nr32 | 0x9F,
            0xFF1D => 0xFF,
            0xFF1E => self.wave.nr34 | 0xBF,
            0xFF1F => 0xFF,
            0xFF20 => 0xFF,
            0xFF21 => self.noise.nr42,
            0xFF22 => self.noise.nr43,
            0xFF23 => self.noise.nr44 | 0xBF,
            0xFF24 => self.nr50,
            0xFF25 => self.nr51,
            0xFF26 => self.read_nr52(),
            0xFF27..=0xFF2F => 0xFF,
            0xFF30..=0xFF3F => self.wave.read_wave_ram(address - 0xFF30),
            _ => return None,
        })
    }

    pub fn write_register(&mut self, address: u16, value: u8) -> bool {
        if address == 0xFF26 {
            self.write_nr52(value);
            return true;
        }

        if matches!(address, 0xFF30..=0xFF3F) {
            self.wave.write_wave_ram(address - 0xFF30, value);
            return true;
        }

        if matches!(address, 0xFF15 | 0xFF1F | 0xFF27..=0xFF2F) {
            return true;
        }

        if !self.powered {
            return matches!(address, 0xFF10..=0xFF25);
        }

        match address {
            0xFF10 => self.pulse1.write_nr10(value),
            0xFF11 => self.pulse1.write_nr11(value),
            0xFF12 => self.pulse1.write_nr12(value),
            0xFF13 => self.pulse1.write_nr13(value),
            0xFF14 => self.write_pulse1_nr14(value),
            0xFF16 => self.pulse2.write_nr11(value),
            0xFF17 => self.pulse2.write_nr12(value),
            0xFF18 => self.pulse2.write_nr13(value),
            0xFF19 => self.write_pulse2_nr14(value),
            0xFF1A => self.wave.write_nr30(value),
            0xFF1B => self.wave.write_nr31(value),
            0xFF1C => self.wave.write_nr32(value),
            0xFF1D => self.wave.write_nr33(value),
            0xFF1E => self.write_wave_nr34(value),
            0xFF20 => self.noise.write_nr41(value),
            0xFF21 => self.noise.write_nr42(value),
            0xFF22 => self.noise.write_nr43(value),
            0xFF23 => self.write_noise_nr44(value),
            0xFF24 => self.nr50 = value,
            0xFF25 => self.nr51 = value,
            _ => return false,
        }

        true
    }

    pub fn read_nr52(&self) -> u8 {
        0x70 | (u8::from(self.powered) << 7)
            | u8::from(self.pulse1.enabled)
            | (u8::from(self.pulse2.enabled) << 1)
            | (u8::from(self.wave.enabled) << 2)
            | (u8::from(self.noise.enabled) << 3)
    }

    fn write_nr52(&mut self, value: u8) {
        let power = value & 0x80 != 0;
        if self.powered == power {
            return;
        }

        self.powered = power;
        if power {
            self.frame_cycles = 0;
            self.frame_step = 0;
        } else {
            let wave_ram = self.wave.wave_ram;
            let pulse1_length = self.pulse1.length_timer;
            let pulse2_length = self.pulse2.length_timer;
            let wave_length = self.wave.length_timer;
            let noise_length = self.noise.length_timer;
            *self = Self::default();
            self.wave.wave_ram = wave_ram;
            self.pulse1.length_timer = pulse1_length;
            self.pulse2.length_timer = pulse2_length;
            self.wave.length_timer = wave_length;
            self.noise.length_timer = noise_length;
        }
    }

    fn write_pulse1_nr14(&mut self, value: u8) {
        let was_enabled = self.pulse1.length_enabled;
        self.pulse1.write_nr14(value);
        if !was_enabled && self.pulse1.length_enabled && self.extra_length_clock_on_enable() {
            self.pulse1.clock_length();
        }
    }

    fn write_pulse2_nr14(&mut self, value: u8) {
        let was_enabled = self.pulse2.length_enabled;
        self.pulse2.write_nr14(value);
        if !was_enabled && self.pulse2.length_enabled && self.extra_length_clock_on_enable() {
            self.pulse2.clock_length();
        }
    }

    fn write_wave_nr34(&mut self, value: u8) {
        let was_enabled = self.wave.length_enabled;
        self.wave.write_nr34(value);
        if !was_enabled && self.wave.length_enabled && self.extra_length_clock_on_enable() {
            self.wave.clock_length();
        }
    }

    fn write_noise_nr44(&mut self, value: u8) {
        let was_enabled = self.noise.length_enabled;
        self.noise.write_nr44(value);
        if !was_enabled && self.noise.length_enabled && self.extra_length_clock_on_enable() {
            self.noise.clock_length();
        }
    }

    fn extra_length_clock_on_enable(&self) -> bool {
        !matches!((self.frame_step + 1) & 0x07, 0 | 2 | 4 | 6)
    }

    fn clock_channels(&mut self) {
        self.pulse1.clock_timer();
        self.pulse2.clock_timer();
        self.wave.clock_timer();
        self.noise.clock_timer();
    }

    fn clock_frame_sequencer(&mut self) {
        self.frame_step = (self.frame_step + 1) & 0x07;

        if matches!(self.frame_step, 0 | 2 | 4 | 6) {
            self.pulse1.clock_length();
            self.pulse2.clock_length();
            self.wave.clock_length();
            self.noise.clock_length();
        }

        if matches!(self.frame_step, 2 | 6) {
            self.pulse1.clock_sweep();
        }

        if self.frame_step == 7 {
            self.pulse1.clock_envelope();
            self.pulse2.clock_envelope();
            self.noise.clock_envelope();
        }
    }

    fn mix_sample(&self) -> AudioSample {
        let outputs = [
            self.pulse1.output(),
            self.pulse2.output(),
            self.wave.output(),
            self.noise.output(),
        ];
        let mut left = 0.0;
        let mut right = 0.0;

        for (index, output) in outputs.into_iter().enumerate() {
            let sample = output / 15.0;
            if self.nr51 & (1 << index) != 0 {
                right += sample;
            }
            if self.nr51 & (1 << (index + 4)) != 0 {
                left += sample;
            }
        }

        let left_volume = ((self.nr50 >> 4) & 0x07) as f32 + 1.0;
        let right_volume = (self.nr50 & 0x07) as f32 + 1.0;
        AudioSample {
            left: pcm_sample((left / 4.0) * (left_volume / 8.0)),
            right: pcm_sample((right / 4.0) * (right_volume / 8.0)),
        }
    }
}

impl PulseChannel {
    fn new(has_sweep: bool) -> Self {
        Self {
            has_sweep,
            enabled: false,
            dac_enabled: false,
            duty: 0,
            length_timer: 0,
            length_enabled: false,
            volume: 0,
            envelope_initial: 0,
            envelope_increase: false,
            envelope_period: 0,
            envelope_timer: 0,
            frequency: 0,
            period_timer: 0,
            duty_step: 0,
            sweep_period: 0,
            sweep_negate: false,
            sweep_shift: 0,
            sweep_timer: 0,
            sweep_shadow: 0,
            sweep_enabled: false,
            sweep_negated_since_trigger: false,
            nr10: 0,
            nr11: 0,
            nr12: 0,
            nr13: 0,
            nr14: 0,
        }
    }

    fn write_nr10(&mut self, value: u8) {
        if self.has_sweep
            && self.sweep_negated_since_trigger
            && self.sweep_negate
            && value & 0x08 == 0
        {
            self.enabled = false;
        }

        self.nr10 = value & 0x7F;
        self.sweep_period = (value >> 4) & 0x07;
        self.sweep_negate = value & 0x08 != 0;
        self.sweep_shift = value & 0x07;
    }

    fn write_nr11(&mut self, value: u8) {
        self.nr11 = value;
        self.duty = value >> 6;
        self.length_timer = 64 - (value & 0x3F) as u16;
    }

    fn write_nr12(&mut self, value: u8) {
        self.nr12 = value;
        self.envelope_initial = value >> 4;
        self.envelope_increase = value & 0x08 != 0;
        self.envelope_period = value & 0x07;
        self.dac_enabled = value & 0xF8 != 0;
        if !self.dac_enabled {
            self.enabled = false;
        }
    }

    fn write_nr13(&mut self, value: u8) {
        self.nr13 = value;
        self.frequency = (self.frequency & 0x0700) | value as u16;
    }

    fn write_nr14(&mut self, value: u8) {
        self.nr14 = value & 0xC7;
        self.length_enabled = value & 0x40 != 0;
        self.frequency = (self.frequency & 0x00FF) | (((value & 0x07) as u16) << 8);
        if value & 0x80 != 0 {
            self.trigger();
        }
    }

    fn trigger(&mut self) {
        self.enabled = self.dac_enabled;
        if self.length_timer == 0 {
            self.length_timer = 64;
        }
        self.period_timer = self.period();
        self.volume = self.envelope_initial;
        self.envelope_timer = envelope_reload(self.envelope_period);
        self.sweep_shadow = self.frequency;
        self.sweep_timer = envelope_reload(self.sweep_period);
        self.sweep_enabled = self.sweep_period != 0 || self.sweep_shift != 0;
        self.sweep_negated_since_trigger = false;

        if self.has_sweep && self.sweep_shift != 0 {
            let _ = self.calculate_sweep();
        }
    }

    fn clock_timer(&mut self) {
        if self.period_timer == 0 {
            self.period_timer = self.period();
            self.duty_step = (self.duty_step + 1) & 0x07;
        } else {
            self.period_timer -= 1;
        }
    }

    fn clock_length(&mut self) {
        if self.length_enabled && self.length_timer > 0 {
            self.length_timer -= 1;
            if self.length_timer == 0 {
                self.enabled = false;
            }
        }
    }

    fn clock_envelope(&mut self) {
        if self.envelope_period == 0 {
            return;
        }

        self.envelope_timer = self.envelope_timer.saturating_sub(1);
        if self.envelope_timer == 0 {
            self.envelope_timer = envelope_reload(self.envelope_period);
            if self.envelope_increase && self.volume < 15 {
                self.volume += 1;
            } else if !self.envelope_increase && self.volume > 0 {
                self.volume -= 1;
            }
        }
    }

    fn clock_sweep(&mut self) {
        if !self.has_sweep {
            return;
        }

        self.sweep_timer = self.sweep_timer.saturating_sub(1);
        if self.sweep_timer != 0 {
            return;
        }

        self.sweep_timer = envelope_reload(self.sweep_period);
        if self.sweep_enabled && self.sweep_period != 0 {
            if let Some(next) = self.calculate_sweep() {
                if self.sweep_shift != 0 {
                    self.frequency = next;
                    self.sweep_shadow = next;
                    let _ = self.calculate_sweep();
                }
            }
        }
    }

    fn calculate_sweep(&mut self) -> Option<u16> {
        let delta = self.sweep_shadow >> self.sweep_shift;
        let frequency = if self.sweep_negate {
            self.sweep_negated_since_trigger = true;
            self.sweep_shadow.wrapping_sub(delta)
        } else {
            self.sweep_shadow.wrapping_add(delta)
        };

        if frequency > 2047 {
            self.enabled = false;
            None
        } else {
            Some(frequency)
        }
    }

    fn output(&self) -> f32 {
        if !self.enabled
            || !self.dac_enabled
            || DUTY_PATTERNS[self.duty as usize][self.duty_step as usize] == 0
        {
            0.0
        } else {
            self.volume as f32
        }
    }

    fn period(&self) -> u16 {
        (2048 - self.frequency).max(1) * 4
    }
}

impl Default for WaveChannel {
    fn default() -> Self {
        Self {
            enabled: false,
            dac_enabled: false,
            length_timer: 0,
            length_enabled: false,
            volume_code: 0,
            frequency: 0,
            period_timer: 0,
            sample_index: 0,
            wave_ram: [0; 16],
            nr30: 0,
            nr31: 0,
            nr32: 0,
            nr33: 0,
            nr34: 0,
        }
    }
}

impl WaveChannel {
    fn read_wave_ram(&self, offset: u16) -> u8 {
        if self.enabled {
            self.wave_ram[(self.sample_index / 2) as usize]
        } else {
            self.wave_ram[offset as usize]
        }
    }

    fn write_wave_ram(&mut self, offset: u16, value: u8) {
        let index = if self.enabled {
            (self.sample_index / 2) as usize
        } else {
            offset as usize
        };
        self.wave_ram[index] = value;
    }

    fn write_nr30(&mut self, value: u8) {
        self.nr30 = value & 0x80;
        self.dac_enabled = value & 0x80 != 0;
        if !self.dac_enabled {
            self.enabled = false;
        }
    }

    fn write_nr31(&mut self, value: u8) {
        self.nr31 = value;
        self.length_timer = 256 - value as u16;
    }

    fn write_nr32(&mut self, value: u8) {
        self.nr32 = value & 0x60;
        self.volume_code = (value >> 5) & 0x03;
    }

    fn write_nr33(&mut self, value: u8) {
        self.nr33 = value;
        self.frequency = (self.frequency & 0x0700) | value as u16;
    }

    fn write_nr34(&mut self, value: u8) {
        self.nr34 = value & 0xC7;
        self.length_enabled = value & 0x40 != 0;
        self.frequency = (self.frequency & 0x00FF) | (((value & 0x07) as u16) << 8);
        if value & 0x80 != 0 {
            self.trigger();
        }
    }

    fn trigger(&mut self) {
        self.enabled = self.dac_enabled;
        if self.length_timer == 0 {
            self.length_timer = 256;
        }
        self.period_timer = self.period();
        self.sample_index = 0;
    }

    fn clock_timer(&mut self) {
        if self.period_timer == 0 {
            self.period_timer = self.period();
            self.sample_index = (self.sample_index + 1) & 0x1F;
        } else {
            self.period_timer -= 1;
        }
    }

    fn clock_length(&mut self) {
        if self.length_enabled && self.length_timer > 0 {
            self.length_timer -= 1;
            if self.length_timer == 0 {
                self.enabled = false;
            }
        }
    }

    fn output(&self) -> f32 {
        if !self.enabled || !self.dac_enabled {
            return 0.0;
        }

        let byte = self.wave_ram[(self.sample_index / 2) as usize];
        let sample = if self.sample_index & 1 == 0 {
            byte >> 4
        } else {
            byte & 0x0F
        };

        match self.volume_code {
            0 => 0.0,
            1 => sample as f32,
            2 => (sample >> 1) as f32,
            3 => (sample >> 2) as f32,
            _ => 0.0,
        }
    }

    fn period(&self) -> u16 {
        (2048 - self.frequency).max(1) * 2
    }
}

impl Default for NoiseChannel {
    fn default() -> Self {
        Self {
            enabled: false,
            dac_enabled: false,
            length_timer: 0,
            length_enabled: false,
            volume: 0,
            envelope_initial: 0,
            envelope_increase: false,
            envelope_period: 0,
            envelope_timer: 0,
            clock_shift: 0,
            width_mode: false,
            divisor_code: 0,
            period_timer: 0,
            lfsr: 0x7FFF,
            nr41: 0,
            nr42: 0,
            nr43: 0,
            nr44: 0,
        }
    }
}

impl NoiseChannel {
    fn write_nr41(&mut self, value: u8) {
        self.nr41 = value & 0x3F;
        self.length_timer = 64 - (value & 0x3F) as u16;
    }

    fn write_nr42(&mut self, value: u8) {
        self.nr42 = value;
        self.envelope_initial = value >> 4;
        self.envelope_increase = value & 0x08 != 0;
        self.envelope_period = value & 0x07;
        self.dac_enabled = value & 0xF8 != 0;
        if !self.dac_enabled {
            self.enabled = false;
        }
    }

    fn write_nr43(&mut self, value: u8) {
        self.nr43 = value;
        self.clock_shift = value >> 4;
        self.width_mode = value & 0x08 != 0;
        self.divisor_code = value & 0x07;
    }

    fn write_nr44(&mut self, value: u8) {
        self.nr44 = value & 0xC0;
        self.length_enabled = value & 0x40 != 0;
        if value & 0x80 != 0 {
            self.trigger();
        }
    }

    fn trigger(&mut self) {
        self.enabled = self.dac_enabled;
        if self.length_timer == 0 {
            self.length_timer = 64;
        }
        self.period_timer = self.period();
        self.volume = self.envelope_initial;
        self.envelope_timer = envelope_reload(self.envelope_period);
        self.lfsr = 0x7FFF;
    }

    fn clock_timer(&mut self) {
        if self.period_timer == 0 {
            self.period_timer = self.period();
            let xor_bit = (self.lfsr & 1) ^ ((self.lfsr >> 1) & 1);
            self.lfsr = (self.lfsr >> 1) | (xor_bit << 14);
            if self.width_mode {
                self.lfsr = (self.lfsr & !(1 << 6)) | (xor_bit << 6);
            }
        } else {
            self.period_timer -= 1;
        }
    }

    fn clock_length(&mut self) {
        if self.length_enabled && self.length_timer > 0 {
            self.length_timer -= 1;
            if self.length_timer == 0 {
                self.enabled = false;
            }
        }
    }

    fn clock_envelope(&mut self) {
        if self.envelope_period == 0 {
            return;
        }

        self.envelope_timer = self.envelope_timer.saturating_sub(1);
        if self.envelope_timer == 0 {
            self.envelope_timer = envelope_reload(self.envelope_period);
            if self.envelope_increase && self.volume < 15 {
                self.volume += 1;
            } else if !self.envelope_increase && self.volume > 0 {
                self.volume -= 1;
            }
        }
    }

    fn output(&self) -> f32 {
        if !self.enabled || !self.dac_enabled || self.lfsr & 1 != 0 {
            0.0
        } else {
            self.volume as f32
        }
    }

    fn period(&self) -> u16 {
        let divisor = match self.divisor_code {
            0 => 8,
            code => code as u16 * 16,
        };
        (divisor << self.clock_shift).max(1)
    }
}

fn envelope_reload(period: u8) -> u8 {
    if period == 0 {
        8
    } else {
        period
    }
}

fn pcm_sample(sample: f32) -> i16 {
    (sample.clamp(0.0, 1.0) * i16::MAX as f32) as i16
}

#[cfg(test)]
mod tests {
    use super::*;

    fn power_on(apu: &mut Apu) {
        apu.write_register(0xFF26, 0x80);
        apu.write_register(0xFF24, 0x77);
        apu.write_register(0xFF25, 0xFF);
    }

    #[test]
    fn power_control_gates_register_writes_and_channel_status() {
        let mut apu = Apu::default();

        assert_eq!(apu.read_register(0xFF26), Some(0x70));
        apu.write_register(0xFF12, 0xF0);
        assert_eq!(apu.read_register(0xFF12), Some(0x00));

        power_on(&mut apu);
        apu.write_register(0xFF12, 0xF0);
        apu.write_register(0xFF14, 0x80);
        assert_eq!(apu.read_register(0xFF26).unwrap() & 0x81, 0x81);

        apu.write_register(0xFF26, 0x00);
        assert_eq!(apu.read_register(0xFF26), Some(0x70));
        assert_eq!(apu.read_register(0xFF12), Some(0x00));
    }

    #[test]
    fn pulse_channel_generates_mixed_samples() {
        let mut apu = Apu::default();
        power_on(&mut apu);
        apu.write_register(0xFF11, 0x80);
        apu.write_register(0xFF12, 0xF0);
        apu.write_register(0xFF13, 0x00);
        apu.write_register(0xFF14, 0x87);

        apu.advance_cycles(CPU_HZ / 60);
        let samples = apu.drain_samples();

        assert!(!samples.is_empty());
        assert!(samples
            .iter()
            .any(|sample| sample.left != 0 || sample.right != 0));
    }

    #[test]
    fn length_counter_disables_channel() {
        let mut apu = Apu::default();
        power_on(&mut apu);
        apu.write_register(0xFF11, 0x3F);
        apu.write_register(0xFF12, 0xF0);
        apu.write_register(0xFF14, 0xC0);

        apu.advance_cycles(FRAME_SEQUENCER_PERIOD * 8);

        assert_eq!(apu.read_register(0xFF26).unwrap() & 0x01, 0);
    }

    #[test]
    fn wave_ram_and_wave_channel_are_functional() {
        let mut apu = Apu::default();
        power_on(&mut apu);
        apu.write_register(0xFF30, 0xF0);
        apu.write_register(0xFF1A, 0x80);
        apu.write_register(0xFF1C, 0x20);
        apu.write_register(0xFF1E, 0x80);

        assert_eq!(apu.read_register(0xFF30), Some(0xF0));
        assert_eq!(apu.read_register(0xFF26).unwrap() & 0x04, 0x04);
    }

    #[test]
    fn noise_channel_triggers_and_produces_samples() {
        let mut apu = Apu::default();
        power_on(&mut apu);
        apu.write_register(0xFF21, 0xF0);
        apu.write_register(0xFF22, 0x00);
        apu.write_register(0xFF23, 0x80);

        apu.advance_cycles(CPU_HZ / 120);

        assert_eq!(apu.read_register(0xFF26).unwrap() & 0x08, 0x08);
        assert!(!apu.drain_samples().is_empty());
    }
}
