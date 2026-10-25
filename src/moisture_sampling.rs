//! Moisture filtering and calibration; acquisition is coordinated by data_sampling.

use crate::data_sampling::NUM_ADC_SAMPLES;

/// Convert the median of a batch to moisture: 0 (dry) to 100 (wet).
pub fn process_samples(samples: &mut [i16; NUM_ADC_SAMPLES]) -> u8 {
    // Observed values:
    // 2790-3220 free air
    // 1130-1245 hand
    // 1111-1144 submerged in water
    //
    // Disconnected: 154... occasional spike to 65397 (that's about i16max!), so I guess it's floating!

    const DRY: i64 = 2840;
    const WET: i64 = 1200;

    samples.sort_unstable();
    let median = samples[samples.len() / 2] as i64;
    let mut cleaned_value = median.clamp(WET, DRY);
    cleaned_value = (100 * (DRY - cleaned_value)) / (DRY - WET); // Scale to 0-100 range
    cleaned_value = cleaned_value.clamp(0, 100); // Clamp to 0-100 range just in case.

    cleaned_value as u8
}
