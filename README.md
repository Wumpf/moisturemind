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

Flashing & Iterating
--------------------
Every time you want to flash you need to double press the reset button,
or write the right byte to the GPREGRET register, see `reset_into_dfu` in the code.
DFU mode can be entered by sending "r" over the com port.

All commands that are needed are in [`build-and-flash.sh`](build-and-flash.sh), but you'll probably
need to change `COM_PORT` to the correct port your device is connected on.
Easiest way to figure it out on Mac is to run `ls /dev/cu.*`

If your SoftDevice setting is different you'll need to adjust the `sd-req` parameter as well, see below.


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
* Samples moisture, battery voltage, and charging status every **5 minutes while charging**, or **20 minutes otherwise**.
* P0.03 enables the sensor for a **500 ms warm-up** and the **11-sample batch** (~100 ms), then disables it before advertising and sleeping.
* Charger status changes wake sampling via a GPIO interrupt (with a 100 ms debounce), triggering a fresh reading and BLE burst.
* Reports moisture, battery voltage, and charging status to Home Assistant via **[BTHome v2](https://bthome.io/format/)** in **2-second BLE bursts** after sampling.
* Battery LED blends from **red (3.0 V)** to **green (4.2 V)**; blue before the first sample.

TODO
--------------------
- [ ] Explore having two moisture sensors connected to the same device.
- [ ] Even longer sleep intervals
