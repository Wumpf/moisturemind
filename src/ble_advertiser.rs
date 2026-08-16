use embassy_executor::Spawner;
use nrf_softdevice::{Softdevice, ble};

use crate::moisture_sampling::LAST_SAMPLED_MOISTURE;

const BTHOME_MOISTURE_U8: u8 = 0x2F;
const BTHOME_MOISTURE_U16: u8 = 0x14;
const BTHOME_COUNT_I16: u8 = 0x5A;

#[expect(clippy::identity_op)]
async fn advertise_moisture(sd: &'static Softdevice, moisture: u8) {
    // Our custom service UUID `d805d254-01f2-42c2-ab6c-798366bf1a4d` in little endian.
    // Disabled, see below why.
    // const SERVICE_UUID: [u8; 16] = [
    //     0x4d, 0x1a, 0xbf, 0x66, 0x83, 0x79, 0x6c, 0xab, 0xc2, 0x42, 0xf2, 0x01, 0x54, 0xd2, 0x05,
    //     0xd8,
    // ];

    let config = ble::peripheral::Config {
        timeout: Some(200), // Advertise the same value for 200ms (units are in 10ms)
        interval: 160,      // 100ms (Advertising interval in 0.625ms units - doc has a bug.)

        // Enable BLE5 2Mbit connection - this should end up using less battery since racing to sleep radio usually uses least power.
        // Practically my Android BLE scanner app reports no secondary phy used though.
        secondary_phy: ble::Phy::M2,
        // TODO: tweak power config?
        ..ble::peripheral::Config::default()
    };

    // See https://bthome.io/format/
    const BTHOME_UUID: [u8; 2] = 0xFCD2_u16.to_le_bytes(); // 0xFCD2 BTHome service UUID (sponsored by Allterco Robotics)
    const BTHOME_FLAGS: u8 = (0 << 0) + // No encryption.
    (0 << 2) + // Not a trigger based device.
    (2 << 5); // BTHome version 2.0

    let bthome_service_data = [
        BTHOME_UUID[0],
        BTHOME_UUID[1],
        BTHOME_FLAGS,
        BTHOME_MOISTURE_U8,
        moisture,
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

    let _advertise_result = ble::peripheral::advertise(sd, adv, &config).await; // Timeout is one of the errors.
}

#[embassy_executor::task]
async fn advertise_moisture_task(sd: &'static Softdevice) -> ! {
    loop {
        let moisture = LAST_SAMPLED_MOISTURE.load(core::sync::atomic::Ordering::Relaxed);
        advertise_moisture(sd, moisture).await;

        embassy_time::Timer::after_millis(50).await;
    }
}

pub async fn setup_ble_advertising(spawner: Spawner, sd: &'static Softdevice) {
    spawner.spawn(advertise_moisture_task(sd).unwrap());
}
