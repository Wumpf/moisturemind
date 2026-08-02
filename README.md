The Moistermind
======================

(Seeed Studio XIAO) nRF52840 + capacitive moisture detector for plants + some Rust code (this repo) == The Moistermind!

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