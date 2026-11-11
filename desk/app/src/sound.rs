//! The app's few sounds: short runs of soft notes, made here as they are wanted (there are no sound files) and
//! handed to Windows to play while the app goes on.

/// What there is to hear.
#[derive(Clone, Copy, PartialEq)]
pub enum Sound {
    /// A wild mon has come.
    Came,
    /// A ball has closed for good.
    Caught,
    /// A mon has gone up a level.
    Level,
    /// A focus session was kept.
    Kept,
    /// A focus session was not.
    Lost,
    /// A break was earned.
    Rested,
}

#[cfg(target_os="windows")]
#[link(name = "winmm")]
extern "system" {
    fn PlaySoundW(sound: *const u8, module: *mut core::ffi::c_void, flags: u32) -> i32;
}

#[cfg(target_os="windows")]
const SND_ASYNC: u32 = 0x1;
#[cfg(target_os="windows")]
const SND_NODEFAULT: u32 = 0x2;
#[cfg(target_os="windows")]
const SND_MEMORY: u32 = 0x4;
const RATE: u32 = 22050;

/// The notes of a sound: each its pitch in hertz and how long it rings, in seconds.
fn notes(sound: Sound) -> &'static [(f32, f32)] {
    match sound {
        Sound::Came => &[(659.3, 0.09), (880.0, 0.16)],
        Sound::Caught => &[(523.3, 0.09), (659.3, 0.09), (784.0, 0.09), (1046.5, 0.26)],
        Sound::Level => &[(784.0, 0.08), (1046.5, 0.2)],
        Sound::Kept => &[(523.3, 0.12), (784.0, 0.12), (1046.5, 0.12), (1318.5, 0.34)],
        Sound::Lost => &[(392.0, 0.16), (311.1, 0.3)],
        Sound::Rested => &[(659.3, 0.2), (523.3, 0.34)],
    }
}

/// A sound as the bytes of a WAV file: one channel of 16-bit samples, each note a soft sine that dies away.
pub fn wav(sound: Sound) -> Vec<u8> {
    let mut samples: Vec<i16> = Vec::new();
    for (pitch, long) in notes(sound) {
        let count = (long * RATE as f32) as usize;
        for index in 0..count {
            let (time, along) = (index as f32 / RATE as f32, index as f32 / count as f32);
            // In quickly so that it does not click, out slowly so that it rings; an octave over it for a bell's edge.
            let swell = (along * 40.0).min(1.0) * (1.0 - along).powf(1.6);
            let wave = (time * pitch * std::f32::consts::TAU).sin() + 0.3 * (time * pitch * 2.0 * std::f32::consts::TAU).sin();
            samples.push((wave * swell * 0.16 * i16::MAX as f32) as i16);
        }
    }
    let data = samples.len() as u32 * 2;
    let mut bytes = Vec::with_capacity(44 + data as usize);
    bytes.extend(b"RIFF");
    bytes.extend((36 + data).to_le_bytes());
    bytes.extend(b"WAVEfmt ");
    bytes.extend(16u32.to_le_bytes());
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(RATE.to_le_bytes());
    bytes.extend((RATE * 2).to_le_bytes());
    bytes.extend(2u16.to_le_bytes());
    bytes.extend(16u16.to_le_bytes());
    bytes.extend(b"data");
    bytes.extend(data.to_le_bytes());
    for sample in samples {
        bytes.extend(sample.to_le_bytes());
    }
    bytes
}

/// Plays a sound and comes straight back. What is played has to outlive the playing of it, so each sound is made
/// once and kept for good.
pub fn play(sound: Sound) {
    use std::sync::OnceLock;
    static MADE: [OnceLock<Vec<u8>>; 6] = [const { OnceLock::new() }; 6];
    let bytes = MADE[sound as usize].get_or_init(|| wav(sound));
    #[cfg(target_os="windows")]
    unsafe {
        PlaySoundW(bytes.as_ptr(), std::ptr::null_mut(), SND_ASYNC | SND_NODEFAULT | SND_MEMORY);
    }
    #[cfg(target_os="macos")]
    crate::desk::play_wav(bytes,sound as usize);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sound_is_a_whole_wav_file_as_long_as_its_notes() {
        let bytes = wav(Sound::Caught);
        assert_eq!(&bytes[..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        let said = u32::from_le_bytes(bytes[40..44].try_into().unwrap()) as usize;
        assert_eq!(said, bytes.len() - 44);
        let long: f32 = notes(Sound::Caught).iter().map(|(_, long)| long).sum();
        assert!((said as f32 / 2.0 / RATE as f32 - long).abs() < 0.01);
    }
}
