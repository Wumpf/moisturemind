use core::sync::atomic::{AtomicBool, Ordering};

use embassy_executor::Spawner;
use embassy_nrf::{peripherals, usb};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, signal::Signal};
use embassy_usb::{UsbDevice, class::cdc_acm::CdcAcmClass, driver::EndpointError};
use nrf_softdevice::SocEvent;
use static_cell::StaticCell;

use crate::Irqs;

const USB_PACKAGE_SIZE: usize = 64;

/// Tracks VBUS presence independently of enumeration or battery charging,
/// and keeps the USB driver's detection state in sync with SoftDevice events.
pub struct UsbPower {
    vbus: usb::vbus_detect::SoftwareVbusDetect,
    present: AtomicBool,
    changed: Signal<CriticalSectionRawMutex, ()>,
}

impl UsbPower {
    fn new(present: bool, ready: bool) -> Self {
        Self {
            vbus: usb::vbus_detect::SoftwareVbusDetect::new(present, ready),
            present: AtomicBool::new(present),
            changed: Signal::new(),
        }
    }

    pub fn is_present(&self) -> bool {
        self.present.load(Ordering::Relaxed)
    }

    /// Single waiter: the sampling task. This is safe to cancel on timeout.
    /// Recheck state to catch changes before the wait was armed and ignore stale
    /// notifications. Awaiting the signal suspends the task; it does not poll.
    pub async fn wait_for_change(&self, previous: bool) {
        while self.is_present() == previous {
            self.changed.wait().await;
        }
    }

    pub fn handle_event(&self, event: SocEvent) {
        match event {
            SocEvent::PowerUsbDetected => self.set_present(true),
            SocEvent::PowerUsbRemoved => self.set_present(false),
            SocEvent::PowerUsbPowerReady => self.vbus.ready(),
            _ => {}
        }
    }

    fn set_present(&self, present: bool) {
        self.vbus.detected(present);
        if self.present.swap(present, Ordering::Relaxed) != present {
            self.changed.signal(());
        }
    }
}

type UsbDriver = usb::Driver<'static, &'static usb::vbus_detect::SoftwareVbusDetect>;

pub fn setup_usb(
    spawner: &Spawner,
    usbd: embassy_nrf::Peri<'static, peripherals::USBD>,
) -> &'static UsbPower {
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
    static POWER: StaticCell<UsbPower> = StaticCell::new();
    let power = &*POWER.init(UsbPower::new(usb_detected, power_ready));

    let driver = usb::Driver::new(usbd, Irqs, &power.vbus);

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

    power
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
