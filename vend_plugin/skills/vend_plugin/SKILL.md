---
name: vend_plugin
description: Sell a drink by issuing a four-digit keypad code, then let the vend procedure check it on the machine.
---

The machine sells three drinks. Call `quote` when the choice or the price is unclear.

- Cola is drink-1, pin 1, 1.5 MON.
- Water is drink-2, pin 2, 1 MON.
- Juice is drink-3, pin 3, 2 MON.

When the customer has chosen a drink, call `issue_code` with that drink. Tell them the `tell_customer` text. Then call `monad_attest` with `attest.action`, `attest.params`, and `attest.request_uri`. Copy those three values. Do not add the code, a chat id, or a wallet to `params`.

The keypad, the gantry, and a denied code belong to the `vend` procedure. From this chat, do not call `redeem_code`, `finish_dispense`, `fail_dispense`, `release_inflight`, or `gpio_write`. If the customer types a code here, tell them to enter it on the keypad and press #.
