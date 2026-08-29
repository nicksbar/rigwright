//! Transport-neutral I/Q sample types.
//!
//! A radio-specific transport may provide raw I/Q independently of CI-V. This
//! module keeps consumers independent of sample width, endian, and channel
//! interleave while the model profile owns the actual transport negotiation.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IqSampleFormat {
    SignedPcm16Le,
    SignedPcm24Le,
    Float32Le,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IqSampleBlock {
    pub sample_rate_hz: u32,
    pub center_frequency_hz: u64,
    pub samples: Vec<(f32, f32)>,
}

pub fn decode_interleaved_iq(
    bytes: &[u8],
    format: IqSampleFormat,
    sample_rate_hz: u32,
    center_frequency_hz: u64,
) -> anyhow::Result<IqSampleBlock> {
    let width = match format {
        IqSampleFormat::SignedPcm16Le => 2,
        IqSampleFormat::SignedPcm24Le => 3,
        IqSampleFormat::Float32Le => 4,
    };
    anyhow::ensure!(
        bytes.len() % (width * 2) == 0,
        "I/Q payload is not an integral set of samples"
    );
    let mut samples = Vec::with_capacity(bytes.len() / (width * 2));
    for pair in bytes.chunks_exact(width * 2) {
        let read = |data: &[u8]| -> f32 {
            match format {
                IqSampleFormat::SignedPcm16Le => {
                    i16::from_le_bytes([data[0], data[1]]) as f32 / 32768.0
                }
                IqSampleFormat::SignedPcm24Le => {
                    let raw = i32::from_le_bytes([
                        data[0],
                        data[1],
                        data[2],
                        if data[2] & 0x80 != 0 { 0xff } else { 0 },
                    ]);
                    raw as f32 / 8_388_608.0
                }
                IqSampleFormat::Float32Le => {
                    f32::from_le_bytes([data[0], data[1], data[2], data[3]])
                }
            }
        };
        samples.push((read(&pair[..width]), read(&pair[width..])));
    }
    Ok(IqSampleBlock {
        sample_rate_hz,
        center_frequency_hz,
        samples,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decodes_interleaved_pcm16() {
        let block = decode_interleaved_iq(
            &[0, 64, 0, 192],
            IqSampleFormat::SignedPcm16Le,
            48_000,
            14_074_000,
        )
        .unwrap();
        assert_eq!(block.samples, vec![(0.5, -0.5)]);
    }
}
