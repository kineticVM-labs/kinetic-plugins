# vend_plugin

Issues a four-digit keypad code on the device, checks that code when the
keypad sends it, and returns an attestation the device can sign. The code
stays in plugin state. It is not a record field.

## Drinks

| Drink | Slot | Pin | Price |
| --- | --- | --- | --- |
| Cola | drink-1 | 1 | 1.5 MON |
| Water | drink-2 | 2 | 1 MON |
| Juice | drink-3 | 3 | 2 MON |

Pin 0 tells the board the code was refused. Pins 1, 2, and 3 start that drink.
The board answers later on `esp32/dispensed`.

## Install

The device agent alias in `sop/SOP.toml` is `default`. Change it to the alias
in your Kinetic config before installing.

```bash
kinetic plugin install /path/to/vend_plugin
```

That loads the tools, copies the `vend` procedure, and writes
`records.example.toml` into the device config. Installing again replaces the
record fields.

## What runs where

Chat uses the skill. `quote` lists the drinks. `issue_code` stores a code and
returns the sentence to tell the customer, plus `attest` fields for
`monad_attest`. Copy `attest.params` as given. Do not add the code.

The `vend` procedure runs when the board sends `keypad`, `dispensed`,
`error`, or `boot`. A valid code calls `gpio_write` and waits for
`dispensed` before the dispense record is signed. A refused code calls
`gpio_write` with pin 0. A timeout or a reboot puts the reserved code back
so the customer can enter it again.
