//! Coordinates moisture, battery voltage, and charging status in one sampling task.

use embassy_executor::Spawner;
use embassy_nrf::{Peri, gpio, peripherals, saadc};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, watch::Watch};

use crate::{Irqs, battery_sampling, moisture_sampling, usb};

pub const NUM_ADC_SAMPLES: usize = 11;
const SAMPLE_INTERVAL_MS: u64 = 10;
const SENSOR_WARMUP_MS: u64 = 150;

/// Delay between completed sampling batches (each triggers an advertising burst).
const UPDATE_INTERVAL_SECS_USB: u64 = 5 * 60;
const UPDATE_INTERVAL_SECS_BATTERY: u64 = 20 * 60;

/// All readings from one completed sampling cycle.
#[derive(Clone, Copy, Debug)]
pub struct Measurements {
    /// Moisture percentage: 0 (dry) to 100 (wet).
    pub moisture: u8,

    pub battery: battery_sampling::BatteryReading,

    /// USB VBUS detected, regardless of whether the battery is charging.
    pub usb_power: bool,
}

/// Latest complete snapshot, initially empty. Each receiver independently tracks
/// updates; slow or late receivers get the latest snapshot rather than a queue.
/// Two receiver slots: one for the LED task and one for BLE advertising.
pub static MEASUREMENTS: Watch<CriticalSectionRawMutex, Measurements, 2> = Watch::new();

#[embassy_executor::task]
async fn sample_data(
    mut saadc: saadc::Saadc<'static, 2>,
    battery_sampler: battery_sampling::BatterySampler,
    mut sensor_power: gpio::Output<'static>,
    usb_power_state: &'static usb::UsbPower,
) -> ! {
    // Allow the newly enabled battery divider to settle before the first read.
    embassy_time::Timer::after_millis(SAMPLE_INTERVAL_MS).await;

    loop {
        // Enable sensor power and allow its output to settle before sampling.
        sensor_power.set_high();
        embassy_time::Timer::after_millis(SENSOR_WARMUP_MS).await;

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

        // All ADC conversions have completed; leave the sensor off through BLE
        // advertising and the timer/USB wait. Never sample moisture while off.
        sensor_power.set_low();

        let moisture = moisture_sampling::process_samples(&mut moisture_samples);
        let battery = battery_sampler.process_samples(&mut battery_samples);
        let usb_power = usb_power_state.is_present();

        // Send latest measurement to all receivers.
        MEASUREMENTS.sender().send(Measurements {
            moisture,
            battery,
            usb_power,
        });

        let interval_secs = if usb_power {
            UPDATE_INTERVAL_SECS_USB
        } else {
            UPDATE_INTERVAL_SECS_BATTERY
        };
        // Wake for the regular interval or USB power changes.
        // A change triggers fresh measurements and an immediate advertising burst.
        let _ = embassy_time::with_timeout(
            embassy_time::Duration::from_secs(interval_secs),
            usb_power_state.wait_for_change(usb_power),
        )
        .await;
    }
}

pub async fn setup_data_sampling(
    spawner: &Spawner,
    saadc_peripheral: Peri<'static, peripherals::SAADC>,
    moisture_pin: Peri<'static, peripherals::P0_02>,
    sensor_power: gpio::Output<'static>,
    read_bat_enable_pin: Peri<'static, peripherals::P0_14>,
    charge_status_pin: Peri<'static, peripherals::P0_17>,
    battery_voltage_pin: Peri<'static, peripherals::P0_31>,
    usb_power: &'static usb::UsbPower,
) {
    let (battery, battery_channel) = battery_sampling::setup_battery_sampling(
        read_bat_enable_pin,
        charge_status_pin,
        battery_voltage_pin,
    );

    // Keep the analog input free of GPIO pulls:
    // - Pulls can bias the measurement by loading the sensor output.
    // - With low-side power switching, a pull-down could provide an unintended
    //   current return path through sensor OUT while the sensor is off.
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

    spawner.spawn(sample_data(saadc, battery, sensor_power, usb_power).unwrap());
}
