#![no_std]
#![no_main]

mod usb;

use embassy_executor::Spawner;
use embassy_nrf::{
    bind_interrupts, gpio,
    interrupt::{self, typelevel::Interrupt as _},
    peripherals,
};
use nrf_softdevice::{Softdevice, ble};

bind_interrupts!(
    pub struct Irqs {
        USBD => embassy_nrf::usb::InterruptHandler<peripherals::USBD>;
    }
);

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    reset_into_dfu();
}

/// Resets the device into Device Firmware Update mode (DFU).
pub fn reset_into_dfu() -> ! {
    // Via https://github.com/adafruit/Adafruit_nRF52_Bootloader#how-to-use
    // This should allow us to reset into DFU/serial bootloader mode after reset.

    // Bootloader: enter CDC/serial DFU on next reset.
    const GPREGRET_ENTER_SERIAL_DFU: u8 = 0x4E;
    // Bootloader: enter UF2 + CDC bootloader on next reset.
    #[allow(dead_code)]
    const GPREGRET_ENTER_UF2_DFU: u8 = 0x57;
    // Bootloader: enter OTA DFU mode on next reset.
    #[allow(dead_code)]
    const GPREGRET_ENTER_OTA_DFU: u8 = 0xA8;

    // Clear GPREGRET then set exact bootloader value.
    unsafe {
        nrf_softdevice::raw::sd_power_gpregret_clr(0, 0xff);
        nrf_softdevice::raw::sd_power_gpregret_set(0, GPREGRET_ENTER_SERIAL_DFU as u32);
    }
    cortex_m::peripheral::SCB::sys_reset();
}

#[embassy_executor::task]
async fn softdevice_task(
    sd: &'static Softdevice,
    vbus: &'static embassy_nrf::usb::vbus_detect::SoftwareVbusDetect,
) -> ! {
    sd.run_with_callback(|event| {
        use nrf_softdevice::SocEvent;

        // Forward USB events.
        match event {
            SocEvent::PowerUsbDetected => vbus.detected(true),
            SocEvent::PowerUsbRemoved => vbus.detected(false),
            SocEvent::PowerUsbPowerReady => vbus.ready(),
            _ => {}
        }
    })
    .await
}

#[embassy_executor::task]
async fn blink_task(
    mut led_red: gpio::Output<'static>,
    mut led_green: gpio::Output<'static>,
    mut led_blue: gpio::Output<'static>,
) -> ! {
    loop {
        led_red.set_high();
        led_green.set_high();
        led_blue.set_high();
        embassy_time::Timer::after(embassy_time::Duration::from_millis(500)).await;

        led_red.set_low();
        led_green.set_low();
        led_blue.set_low();
        embassy_time::Timer::after(embassy_time::Duration::from_millis(500)).await;
    }
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    // Per https://github.com/embassy-rs/nrf-softdevice/tree/nrf-softdevice-v0.1.0#interrupt-priority
    // Interrupt priorities 0, 1 and 4 are reserved by the Softdevice, so we have to use 2 or 3 for all interrupts.
    let mut config = embassy_nrf::config::Config::default();
    //config.gpiote_interrupt_priority = embassy_nrf::interrupt::Priority::P2;
    config.time_interrupt_priority = interrupt::Priority::P2;
    let peripherals = embassy_nrf::init(config);
    interrupt::typelevel::USBD::set_priority(interrupt::Priority::P2);

    let config = softdevice_config();
    let sd = Softdevice::enable(&config);

    let vbus = usb::setup_usb(&spawner, peripherals.USBD);
    spawner.spawn(softdevice_task(sd, vbus).unwrap());

    setup_blinking_leds(
        &spawner,
        peripherals.P0_26,
        peripherals.P0_30,
        peripherals.P0_06,
    );

    setup_ble_advertising(sd).await;
}

async fn setup_ble_advertising(sd: &'static Softdevice) {
    // Our custom service UUID `d805d254-01f2-42c2-ab6c-798366bf1a4d` in little endian.
    // Disabled, see below why.
    // const SERVICE_UUID: [u8; 16] = [
    //     0x4d, 0x1a, 0xbf, 0x66, 0x83, 0x79, 0x6c, 0xab, 0xc2, 0x42, 0xf2, 0x01, 0x54, 0xd2, 0x05,
    //     0xd8,
    // ];

    let mut config = ble::peripheral::Config::default();
    config.interval = 160; // 100ms (Advertising interval in 0.625ms units - doc has a bug.)
    config.primary_phy = ble::Phy::M1;
    // Enable BLE5 2Mbit connection - this should end up using less battery since racing to sleep radio usually uses least power.
    // Practically my Android BLE scanner app reports no secondary phy used though.
    config.secondary_phy = ble::Phy::M2;
    // TODO: tweak power config?

    let moisture = 42_u16.to_le_bytes(); // TODO: advertise actual sensor data here.

    // See https://bthome.io/format/
    const BTHOME_UUID: [u8; 2] = 0xFCD2_u16.to_le_bytes(); // 0xFCD2 BTHome service UUID (sponsored by Allterco Robotics)
    const BTHOME_FLAGS: u8 = (0 << 0) + // No encryption.
                             (0 << 2) + // Not a trigger based device.
                             (2 << 5); // BTHome version 2.0
    const BTHOME_MOISTURE16: u8 = 0x14; // BTHome moisture sensor type, 16bit value.

    let bthome_service_data = [
        BTHOME_UUID[0],
        BTHOME_UUID[1],
        BTHOME_FLAGS,
        BTHOME_MOISTURE16,
        moisture[0],
        moisture[1],
    ];

    // I tried using extended advertisng (part of BLE5). But neither the scanner on my phone nor HA's scanner could see the device anymore.
    // ("legacy" just means we have the older 31 byte advertisment package)
    let advertisement_data = ble::advertisement_builder::LegacyAdvertisementBuilder::new()
        .flags(&[
            // TODO: Using `LimitedDiscovery` would make sense. Llm says we have to set a timeout for this!
            ble::advertisement_builder::Flag::GeneralDiscovery,
            // Tell everyone we're only supporting BLE, not classic Bluetooth.
            ble::advertisement_builder::Flag::LE_Only,
        ])
        // 16bit service IDs are used for standard services.
        // Our device doesn't fit any in particular, so we should set a 128bit custom service ID instead.
        // However, it gets a bit hard to squeeze this into the 31 advertisment package
        // .services_128(
        //     ble::advertisement_builder::ServiceList::Complete,
        //     &[SERVICE_UUID],
        // )
        .raw(
            // Let's be nice and set an appearance value.
            ble::advertisement_builder::AdvertisementDataType::APPEARANCE,
            &0x0540_u16.to_le_bytes(), // Appearance flag for Generic Sensor (see https://www.bluetooth.com/wp-content/uploads/Files/Specification/HTML/Assigned_Numbers/out/en/Assigned_Numbers.pdf)
        )
        .raw(
            ble::advertisement_builder::AdvertisementDataType::SERVICE_DATA_16,
            &bthome_service_data,
        )
        .full_name("Moisturemind")
        .build();

    // Not doing allowing any scanning, everywhing we have to say is in the advertisement.
    // let adv = ble::peripheral::NonconnectableAdvertisement::ExtendedNonscannableUndirected {
    //     set_id: 0,
    //     anonymous: false,
    //     adv_data: &advertisement_data,
    // };
    let adv = ble::peripheral::NonconnectableAdvertisement::NonscannableUndirected {
        adv_data: &advertisement_data,
    };

    ble::peripheral::advertise(sd, adv, &config).await.unwrap();
}

fn setup_blinking_leds(
    spawner: &Spawner,
    red: embassy_nrf::Peri<'static, impl gpio::Pin>,
    green: embassy_nrf::Peri<'static, impl gpio::Pin>,
    blue: embassy_nrf::Peri<'static, impl gpio::Pin>,
) {
    let led_red = gpio::Output::new(red, gpio::Level::Low, gpio::OutputDrive::Standard);
    let led_green = gpio::Output::new(green, gpio::Level::Low, gpio::OutputDrive::Standard);
    let led_blue = gpio::Output::new(blue, gpio::Level::Low, gpio::OutputDrive::Standard);
    spawner.spawn(blink_task(led_red, led_green, led_blue).unwrap());
}

fn softdevice_config() -> nrf_softdevice::Config {
    use nrf_softdevice::raw;

    const NAME: &[u8] = b"Moisturemind";

    nrf_softdevice::Config {
        // What clock the softdevice should use.
        clock: Some(raw::nrf_clock_lf_cfg_t {
            source: raw::NRF_CLOCK_LF_SRC_RC as u8, // TODO: switch to external? NRF_CLOCK_LF_SRC_XTAL?
            rc_ctiv: 16,
            rc_temp_ctiv: 2,
            accuracy: raw::NRF_CLOCK_LF_ACCURACY_500_PPM as u8,
        }),

        // Configure BLE roles.
        gap_role_count: Some(raw::ble_gap_cfg_role_count_t {
            adv_set_count: 1,
            periph_role_count: 0, // Disable connections as a pheripheral. Default is 1 (raw::BLE_GAP_ROLE_COUNT_PERIPH_DEFAULT)
            central_role_count: 0, // Disable connections as a central (we're only advertising). Default is 3 (raw::BLE_GAP_ROLE_COUNT_CENTRAL_DEFAULT),
            central_sec_count: 0, // Disable concurrent central-role security negotiations. Default is 1 (raw::BLE_GAP_ROLE_COUNT_CENTRAL_SEC_DEFAULT),
            _bitfield_1: raw::ble_gap_cfg_role_count_t::new_bitfield_1(0),
        }),

        // Configure GAP (Generic Access Profile) device name.
        gap_device_name: Some(raw::ble_gap_cfg_device_name_t {
            p_value: NAME.as_ptr() as *const u8 as _,
            current_len: NAME.len() as _,
            max_len: NAME.len() as _,
            write_perm: unsafe { core::mem::zeroed() }, // Not writable.
            _bitfield_1: raw::ble_gap_cfg_device_name_t::new_bitfield_1(
                raw::BLE_GATTS_VLOC_STACK as u8,
            ),
        }),

        // Configure uuid count.
        common_vs_uuid: Some(raw::ble_common_cfg_vs_uuid_t {
            vs_uuid_count: 1, // We have one custom service UUID.
        }),

        ..Default::default()
    }
}
