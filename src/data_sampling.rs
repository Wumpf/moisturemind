//! Coordinates moisture, battery voltage, and charging status in one sampling task.

use embassy_executor::Spawner;
use embassy_nrf::{Peri, peripherals, saadc};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, watch::Watch};

use crate::{Irqs, battery_sampling, moisture_sampling};

pub const NUM_ADC_SAMPLES: usize = 11;
const SAMPLE_INTERVAL_MS: u64 = 10;

/// Delay between completed sampling batches.
const SAMPLING_INTERVAL_SECS: u64 = 10 * 60;

/// All readings from one completed sampling cycle.
#[derive(Clone, Copy, Debug)]
pub struct Measurements {
    /// Moisture percentage: 0 (dry) to 100 (wet).
    pub moisture: u8,

    pub battery: battery_sampling::BatteryReading,
}

/// Latest complete snapshot, initially empty. Each receiver independently tracks
/// updates; slow or late receivers get the latest snapshot rather than a queue.
/// Two receiver slots: one for the LED task and one for BLE advertising.
pub static MEASUREMENTS: Watch<CriticalSectionRawMutex, Measurements, 2> = Watch::new();

#[embassy_executor::task]
async fn sample_data(
    mut saadc: saadc::Saadc<'static, 2>,
    battery: battery_sampling::BatterySampler,
) -> ! {
    // Allow the newly enabled battery divider to settle before the first read.
    embassy_time::Timer::after_millis(SAMPLE_INTERVAL_MS).await;

    loop {
        // Each ADC operation samples moisture (channel 0) and battery (channel 1).
        //
        // Taking multiple samples is kinda important for moisture, but the only reason we do it for battery is that we have only a single ADC channel.
        let mut moisture_samples = [0_i16; NUM_ADC_SAMPLES];
        let mut battery_samples = [0_i16; NUM_ADC_SAMPLES];
        for index in 0..NUM_ADC_SAMPLES {
            if index > 0 {
                embassy_time::Timer::after_millis(SAMPLE_INTERVAL_MS).await;
            }
            let mut values = [0_i16; 2];
            saadc.sample(&mut values).await;
            moisture_samples[index] = values[0];
            battery_samples[index] = values[1];
        }

        let moisture = moisture_sampling::process_samples(&mut moisture_samples);
        let battery_reading = battery.process_samples(&mut battery_samples);

        // Send latest measurement to all receivers.
        MEASUREMENTS.sender().send(Measurements {
            moisture,
            battery: battery_reading,
        });

        embassy_time::Timer::after_secs(SAMPLING_INTERVAL_SECS).await;
    }
}

pub async fn setup_data_sampling(
    spawner: &Spawner,
    saadc_peripheral: Peri<'static, peripherals::SAADC>,
    moisture_pin: Peri<'static, peripherals::P0_02>,
    read_bat_enable_pin: Peri<'static, peripherals::P0_14>,
    charge_status_pin: Peri<'static, peripherals::P0_17>,
    battery_voltage_pin: Peri<'static, peripherals::P0_31>,
) {
    let (battery, battery_channel) = battery_sampling::setup_battery_sampling(
        read_bat_enable_pin,
        charge_status_pin,
        battery_voltage_pin,
    );

    let moisture_channel = saadc::ChannelConfig::single_ended(moisture_pin);
    let mut config = saadc::Config::default();
    // Battery conversion assumes 12-bit samples; keep moisture calibration unchanged.
    config.resolution = saadc::Resolution::_12bit;
    let saadc = saadc::Saadc::new(
        saadc_peripheral,
        Irqs,
        config,
        [moisture_channel, battery_channel],
    );
    saadc.calibrate().await;

    spawner.spawn(sample_data(saadc, battery).unwrap());
}
