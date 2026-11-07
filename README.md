The Moisturemind
======================

(Seeed Studio XIAO) nRF52840 + [capacitive moisture detector](https://thepihut.com/products/capacitive-soil-moisture-sensor) for plants + some Rust code (this repo) == The Moistermind!

For my first stumbling steps in getting anything out of the Seeed Studio XIAO nRF52840 and how see https://github.com/Wumpf/Seeed-nRF52840-Sense-projects
Some things are directly copied out of that project.

Software Prerequisites
-------------
* [rust](https://www.rust-lang.org/)
* `arm-none-eabi-objcopy` to convert from ELF to ihex (Intel .hex format)
  * rust linker doesn't come with this, on Mac you can install with 
    `brew install armmbed/formulae/arm-none-eabi-gcc`
* [Adafruit-nrfutil](https://github.com/adafruit/Adafruit_nRF52_nrfutil)
  * `pip3 install adafruit-nrfutil`
* (optional) some tool for monitoring & writing the COM port
* For WSL:
  * On windows
    * `usbipd` for passing through COM
    * something to talk serial, e.g. Putty
  * On linux `minicom` for talking to the device

Flashing & Iterating
--------------------
Every time you want to flash you need to double press the reset button,
or write the right byte to the GPREGRET register, see `reset_into_dfu` in the code.
DFU mode can be entered by sending "r" over the com port.

All commands that are needed are in [`build-and-flash.sh`](build-and-flash.sh), but you'll probably
need to change `COM_PORT` to the correct port your device is connected on.
Easiest way to figure it out on Mac is to run `ls /dev/cu.*`

If your SoftDevice setting is different you'll need to adjust the `sd-req` parameter as well, see below.

For WSL:
* pass through COM to WSL. From cmd/powershell:
  * `usbipd list` to find the right BUSID
  * `usbipd bind --busid 1-9`
  * `usbipd attach --wsl --busid 1-9`
* resetting (either from Windows or Linux) will require attaching again
  * I recommend resetting on Windows because resetting on Linux and then attaching booted the device for me sometimes.

Connecting
--------------------
[Pinout of the Seeed Studio XIAO nRF52840](https://wiki.seeedstudio.com/XIAO_BLE/#hardware-overview)

The [moisture detector](https://thepihut.com/products/capacitive-soil-moisture-sensor) uses the board's **3.3 V** supply, with its ground switched by an **IRLZ34NPbF N-channel MOSFET**`*`

| Connection    | Wiring                                       |
| ------------- | -------------------------------------------- |
| Sensor VCC    | XIAO 3V3 (not 5 V)                            |
| Sensor OUT    | P0.02 / D0 / A0                               |
| Sensor GND    | MOSFET drain, **not directly to XIAO GND**     |
| MOSFET source | XIAO GND                                     |
| MOSFET gate   | P0.03 / D1 / A1 through a 1 kΩ series resistor |

`*` Note that this N-channel mosfet might be a bad idea according to my LLM: 3.3 V gate performance isn't guaranteed, and switching ground may allow leakage through OUT.

Current behavior
--------------------
### Sampling and wake-up

* Samples every **5 minutes with USB power present**, or **20 minutes on battery**, regardless of charging status.
* P0.03 enables the sensor for a short warmup, followed by a bunch of samples (of which we take the median), then disables it before advertising and sleeping.
* USB plug/unplug triggers fresh readings and a BLE burst, even when the battery is not charging.
* Charging status is read during each sample
  * convenient since this way we can put everything on a single timer
  * charging change happens rarely enough so no need to wakup

### Home Assistant reporting

Readings are sent via **[BTHome v2](https://bthome.io/format/)** in **2-second BLE bursts** after sampling:

| Reading | Meaning |
| --- | --- |
| Moisture | Soil moisture percentage. |
| Battery voltage | Voltage measured with the sensor powered on. |
| Charging | Whether the charger is actively charging the battery. |
| USB power | Whether USB power is present, independent of USB enumeration; uses BTHome's binary **power** object (`0x10`). |

USB power on with charging off means **plugged in, not charging**—not necessarily full.

### Low-battery behavior and LED

* At **≤3.25 V**, moisture is omitted because readings were observed to become unreliable. Battery voltage, charging status, and USB power reports continue.
* [turned off currently] Battery LED blends from **red (3.0 V)** to **green (4.2 V)**; blue before the first sample.

TODO
--------------------
- [ ] Explore having two moisture sensors connected to the same device.
- [ ] Even longer sleep intervals
