//! Navigation sounds for the fullscreen shell: short soft tones made here, so no sound
//! files ship with the app.

/// Samples per second of the tones.
pub const SAMPLE_RATE: u32 = 44_100;

/// A sine tone of `frequency` Hz lasting `millis` ms, as a mono 16-bit PCM WAV file. It
/// rises over its first 5 ms and fades out over the rest, so it clicks softly instead of
/// popping.
pub fn tone_wav(frequency: f64, millis: u32) -> Vec<u8> {
    let count = SAMPLE_RATE * millis / 1000;
    let attack = f64::from(SAMPLE_RATE) * 0.005;
    let data_len = count * 2;
    let mut wav = Vec::with_capacity(44 + data_len as usize);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_len).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&1u16.to_le_bytes()); // mono
    wav.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    wav.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes()); // bytes per second
    wav.extend_from_slice(&2u16.to_le_bytes()); // bytes per frame
    wav.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    for n in 0..count {
        let n = f64::from(n);
        let rise = (n / attack).min(1.0);
        let fall = 1.0 - n / f64::from(count);
        let wave = (std::f64::consts::TAU * frequency * n / f64::from(SAMPLE_RATE)).sin();
        let sample = (wave * rise * fall * 0.3 * f64::from(i16::MAX)) as i16;
        wav.extend_from_slice(&sample.to_le_bytes());
    }
    wav
}

#[cfg(test)]
mod tests {
    use super::*;

    fn le_u32(bytes: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
    }

    #[test]
    fn a_tone_is_a_mono_16_bit_wav_that_fades_in_and_out() {
        let wav = tone_wav(880.0, 40);

        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(le_u32(&wav, 4) as usize, wav.len() - 8);
        assert_eq!(&wav[8..16], b"WAVEfmt ");
        assert_eq!(le_u32(&wav, 24), SAMPLE_RATE);
        assert_eq!(&wav[36..40], b"data");
        let samples: Vec<i16> = wav[44..]
            .chunks(2)
            .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        assert_eq!(samples.len(), (SAMPLE_RATE * 40 / 1000) as usize);
        assert_eq!(samples[0], 0);
        assert!(samples.last().unwrap().abs() < 200);
        assert!(samples.iter().any(|sample| sample.abs() > 3000));
    }
}
