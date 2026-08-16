use core::sync::atomic::AtomicU8;
use embassy_executor::Spawner;
use embassy_nrf::saadc;

use crate::Irqs;

/// Last sampled moisture value.
///
/// Clean range: 0 (dry) to 100 (wet)
pub static LAST_SAMPLED_MOISTURE: AtomicU8 = AtomicU8::new(0);

#[embassy_executor::task]
async fn sample_moisture(mut saadc: saadc::Saadc<'static, 1>) -> ! {
    // Observed values:
    // 2790-3220 free air
    // 1130-1245 hand
    // 1111-1144 submerged in water
    //
    // Disconnected: 154... occasional spike to 65397 (that's about i16max!), so I guess it's floating!

    const DRY: i64 = 2840;
    const WET: i64 = 1200;

    loop {
        let mut new_value = [0_i16];
        saadc.sample(&mut new_value).await;

        let mut cleaned_value = (new_value[0] as i64).clamp(WET, DRY);
        cleaned_value = (100 * (DRY - cleaned_value)) / (DRY - WET); // Scale to 0-100 range
        cleaned_value = cleaned_value.clamp(0, 100); // Clamp to 0-100 range just in case.

        LAST_SAMPLED_MOISTURE.store(cleaned_value as u8, core::sync::atomic::Ordering::Relaxed);

        embassy_time::Timer::after_millis(100).await; // TODO: moisture doesn't change that often ;-)
    }
}

pub async fn setup_moisture_sampling(
    spawner: &Spawner,
    saadc_peripheral: embassy_nrf::Peri<'static, embassy_nrf::peripherals::SAADC>,
    pin: impl saadc::Input,
) {
    let adc_channel_config = saadc::ChannelConfig::single_ended(pin);
    let saadc = saadc::Saadc::new(
        saadc_peripheral,
        Irqs,
        saadc::Config::default(),
        [adc_channel_config],
    );
    saadc.calibrate().await;

    spawner.spawn(sample_moisture(saadc).unwrap());
}
