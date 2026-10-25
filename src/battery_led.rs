//! Displays the latest battery voltage as a red-to-green PWM color blend.

use embassy_executor::Spawner;
use embassy_nrf::{gpio, peripherals, pwm};

use crate::{battery_sampling::LAST_SAMPLED_BATTERY_MV, data_sampling::NEW_DATA_SAMPLED};

const BATTERY_MIN_MV: u16 = 3000;
const BATTERY_MAX_MV: u16 = 4200;
const PWM_MAX_DUTY: u16 = BATTERY_MAX_MV - BATTERY_MIN_MV;

#[embassy_executor::task]
async fn battery_led_task(mut led: pwm::SimplePwm<'static>) -> ! {
    loop {
        let millivolts = LAST_SAMPLED_BATTERY_MV.load(core::sync::atomic::Ordering::Relaxed);
        let (red, green, blue) = if millivolts == 0 {
            // No voltage sample yet: show blue, including before the first notification.
            (0, 0, PWM_MAX_DUTY)
        } else {
            let green = millivolts.clamp(BATTERY_MIN_MV, BATTERY_MAX_MV) - BATTERY_MIN_MV;
            (PWM_MAX_DUTY - green, green, 0)
        };
        // XIAO RGB LEDs are active-low. Normal PWM is LOW for `duty` ticks.
        // Blend red -> yellow -> green once a voltage sample is available.
        led.set_all_duties([
            pwm::DutyCycle::normal(red),
            pwm::DutyCycle::normal(green),
            pwm::DutyCycle::normal(blue),
            pwm::DutyCycle::normal(0),
        ]);
        NEW_DATA_SAMPLED.wait().await;
    }
}

pub fn setup_battery_led(
    spawner: &Spawner,
    pwm_peripheral: embassy_nrf::Peri<'static, peripherals::PWM0>,
    red: embassy_nrf::Peri<'static, impl gpio::Pin>,
    green: embassy_nrf::Peri<'static, impl gpio::Pin>,
    blue: embassy_nrf::Peri<'static, impl gpio::Pin>,
) {
    let mut config = pwm::SimpleConfig::default();
    config.max_duty = PWM_MAX_DUTY;
    config.ch0_idle_level = gpio::Level::High;
    config.ch1_idle_level = gpio::Level::High;
    config.ch2_idle_level = gpio::Level::High;
    // Default 1 MHz PWM clock / PWM_MAX_DUTY gives the PWM frequency.
    let led = pwm::SimplePwm::new_3ch(pwm_peripheral, red, green, blue, &config);
    spawner.spawn(battery_led_task(led).unwrap());
}
