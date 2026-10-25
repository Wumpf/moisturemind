//! Samples battery status. See https://wiki.seeedstudio.com/XIAO_BLE/#q3-what-are-the-considerations-when-using-xiao-nrf52840-sense-for-battery-charging

use crate::data_sampling::NUM_ADC_SAMPLES;
use core::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use embassy_nrf::{Peri, gpio, peripherals, saadc};

/// Whether the charger was active at the last sample.
///
/// False means not charging, not necessarily fully charged: USB power or the battery may be absent.
/// See https://wiki.seeedstudio.com/XIAO_BLE/#q4-how-does-the-green-light-behave-when-powered-on
pub static LAST_SAMPLED_CHARGING: AtomicBool = AtomicBool::new(false);

/// Battery voltage in millivolts.
///
/// If USB is connected, the voltage may be wrong, in particular if there's no battery.
pub static LAST_SAMPLED_BATTERY_MV: AtomicU16 = AtomicU16::new(0);

/// Keeps the divider enabled and owns the charging-status input for the shared task.
pub struct BatterySampler {
    _read_bat_enable: gpio::Output<'static>,
    charge_status: gpio::Input<'static>,
}

impl BatterySampler {
    /// Publish median battery voltage and current charging status in the same cycle.
    pub fn record_samples(&self, samples: &mut [i16; NUM_ADC_SAMPLES]) {
        samples.sort_unstable();
        let median_raw = samples[samples.len() / 2].max(0) as u32;

        // 12-bit ADC, internal 600 mV reference, gain 1/6 => 3600 mV range.
        // XIAO divider: R16 = 1 MOhm, R17 = 510 kOhm to P0.14 (LOW).
        // Vbat = Vadc * (1000 + 510) / 510. Round to nearest mV.
        let denominator = 4096 * 510;
        let millivolts = (u64::from(median_raw) * 3600 * 1510 + denominator / 2) / denominator;
        LAST_SAMPLED_BATTERY_MV.store(millivolts as u16, Ordering::Relaxed);

        // Charging status we can just read off the input pin.
        LAST_SAMPLED_CHARGING.store(self.charge_status.is_low(), Ordering::Relaxed);
    }
}

pub fn setup_battery_sampling(
    read_bat_enable_pin: Peri<'static, peripherals::P0_14>,
    charge_status_pin: Peri<'static, peripherals::P0_17>,
    battery_voltage_pin: Peri<'static, peripherals::P0_31>,
) -> (BatterySampler, saadc::ChannelConfig<'static>) {
    // Keep LOW even between readings: Seeed warns that disabling the divider
    // during charging can expose P0.31 to excessive voltage.
    let read_bat_enable = gpio::Output::new(
        read_bat_enable_pin,
        gpio::Level::Low,
        gpio::OutputDrive::Standard,
    );

    let mut channel = saadc::ChannelConfig::single_ended(battery_voltage_pin);
    channel.reference = saadc::Reference::Internal;
    channel.gain = saadc::Gain::Gain1_6;
    // The 1 MOhm / 510 kOhm divider has a high source impedance (~338 kOhm).
    channel.time = saadc::Time::_40US;

    // The XIAO nRF52840 routes the charger's active-low, open-drain status
    // output to P0.17 (also connected to the dedicated charge LED).
    // Read it as an input; never drive the charger's output from the MCU.
    let charge_status = gpio::Input::new(charge_status_pin, gpio::Pull::Up);
    (
        BatterySampler {
            _read_bat_enable: read_bat_enable,
            charge_status,
        },
        channel,
    )
}
