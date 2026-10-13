# vend_plugin

Quotes a drink, sends the device vault payment link, and issues a four-digit
keypad code only after the vault balance has risen by the price. The code
stays in plugin state. It is not a record field.

## Drinks

| Drink | Slot | Pin | Price |
| --- | --- | --- | --- |
| Cola | drink-1 | 1 | 1.5 MON |
| Water | drink-2 | 2 | 1 MON |
| Juice | drink-3 | 3 | 2 MON |

Pin 0 tells the board the code was refused. Pins 1, 2, and 3 start that drink.
The board answers later on `esp32/dispensed`.

## Firmware

`vend-esp32/` is the sketch for the machine's ESP32. It is not part of the plugin. `kinetic plugin install` does not copy it, flash it, or load it. Flash `vend-esp32/vend-esp32.ino` onto the board with the Arduino IDE, using the same board and the same libraries as the sketch already on the machine. The plugin only runs on the Pi.

## Install

`sop/SOP.toml` runs the keypad as agent `device`. That is the agent
`kinetic quickstart` creates, with its model and risk profile. A keypad
press fails before any tool call when the procedure names a different agent.

```bash
kinetic plugin install /path/to/vend_plugin
```

That loads the tools, copies the `vend` procedure, and writes
`records.example.toml` into the device config. Installing again replaces the
record fields.

## What runs where

Chat uses the skill. `quote` lists the drinks. `start_sale` stores one unpaid
sale and returns the payment link. `confirm_payment` compares a new
`vault_status` balance with the balance from before payment. The code is
stored only when that balance has risen by the drink price. Copy
`attest.params` as given. Do not add the code.

The payment link is `https://vault-deposit.kineticvm.xyz` for the published
testnet vault `0x77102fCAC7927bB64507DF82bf1ae659465B4EE5` and the agent id
from `monad_identity`. The plugin checks the balance the agent read from
`vault_status`. It does not call the chain itself.

The `vend` procedure runs when the board sends `keypad`, `dispensed`,
`error`, or `boot`. A valid code calls `gpio_write` and waits for
`dispensed` before the dispense record is signed. A refused code calls
`gpio_write` with pin 0. A timeout or a reboot puts the reserved code back
so the customer can enter it again.
