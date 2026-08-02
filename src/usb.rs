use embassy_executor::Spawner;
use embassy_nrf::{peripherals, usb};
use embassy_usb::{UsbDevice, class::cdc_acm::CdcAcmClass, driver::EndpointError};
use static_cell::StaticCell;

use crate::Irqs;

const USB_PACKAGE_SIZE: usize = 64;

type UsbDriver = usb::Driver<'static, &'static usb::vbus_detect::SoftwareVbusDetect>;

pub fn setup_usb(
    spawner: &Spawner,
    usbd: embassy_nrf::Peri<'static, peripherals::USBD>,
) -> &'static usb::vbus_detect::SoftwareVbusDetect {
    // Enable USB events on softdevice.
    unsafe {
        nrf_softdevice::raw::sd_power_usbdetected_enable(1);
        nrf_softdevice::raw::sd_power_usbremoved_enable(1);
        nrf_softdevice::raw::sd_power_usbpwrrdy_enable(1);
    };

    // Create the driver.
    // We can't use usb::vbus_detect::HardwareVbusDetect with SoftDevice, so we have to feed in status ourselves.
    // This happens as part of the `softdevice_task` callback, which is called on USB events.
    let mut usbregstatus: u32 = 0;
    unsafe {
        nrf_softdevice::raw::sd_power_usbregstatus_get(&mut usbregstatus);
    }
    let usb_detected = (usbregstatus & 1) != 0;
    let power_ready = (usbregstatus & (1 << 1)) != 0;
    static VBUS: StaticCell<usb::vbus_detect::SoftwareVbusDetect> = StaticCell::new();
    let vbus = &*VBUS.init(usb::vbus_detect::SoftwareVbusDetect::new(
        usb_detected,
        power_ready,
    ));

    let driver = usb::Driver::new(usbd, Irqs, vbus);

    // Create embassy-usb Config
    let mut config = embassy_usb::Config::new(0xc0de, 0xcafe);
    config.manufacturer = Some("Wumpftech");
    config.product = Some("Wumpftech Serial");
    config.serial_number = Some("wumpf1");
    config.max_packet_size_0 = USB_PACKAGE_SIZE as u8;

    // Create embassy-usb DeviceBuilder using the driver and config.
    static CONFIG_DESC: StaticCell<[u8; 256]> = StaticCell::new();
    static BOS_DESC: StaticCell<[u8; 256]> = StaticCell::new();
    static MSOS_DESC: StaticCell<[u8; 128]> = StaticCell::new();
    static CONTROL_BUF: StaticCell<[u8; 128]> = StaticCell::new();
    let mut builder = embassy_usb::Builder::new(
        driver,
        config,
        &mut CONFIG_DESC.init([0; 256])[..],
        &mut BOS_DESC.init([0; 256])[..],
        &mut MSOS_DESC.init([0; 128])[..],
        &mut CONTROL_BUF.init([0; 128])[..],
    );

    // Create classes on the builder.
    static STATE: StaticCell<embassy_usb::class::cdc_acm::State> = StaticCell::new();
    let state = STATE.init(embassy_usb::class::cdc_acm::State::new());
    let class = CdcAcmClass::new(&mut builder, state, 64);

    let device = builder.build();

    spawner.spawn(usb_task(device).unwrap());
    spawner.spawn(usb_read_write_task(class).unwrap());

    vbus
}

struct Disconnected {}

impl From<EndpointError> for Disconnected {
    fn from(val: EndpointError) -> Self {
        match val {
            EndpointError::BufferOverflow => panic!("Buffer overflow"),
            EndpointError::Disabled => Disconnected {},
        }
    }
}

#[embassy_executor::task]
async fn usb_task(mut device: UsbDevice<'static, UsbDriver>) -> ! {
    device.run().await;
}

#[embassy_executor::task]
async fn usb_read_write_task(mut class: CdcAcmClass<'static, UsbDriver>) -> ! {
    // Reconnect if we loose connection.
    loop {
        class.wait_connection().await;
        let _ = echo_and_reset_on_r(&mut class).await;
    }
}

async fn echo_and_reset_on_r(
    class: &mut CdcAcmClass<'static, UsbDriver>,
) -> Result<(), Disconnected> {
    let mut buf = [0; USB_PACKAGE_SIZE];
    loop {
        let n = class.read_packet(&mut buf).await?;
        let data = &buf[..n];
        class.write_packet(data).await?;

        if data.contains(&b'r') {
            class.write_packet(b"\nresetting...").await?;
            // There doesn't seem to be a reliable way to ensure that the packet arrived.
            cortex_m::asm::delay(6_400_000); // ~100ms at 64 MHz

            crate::reset_into_dfu();
        }
    }
}
