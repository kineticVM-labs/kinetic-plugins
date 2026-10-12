---
name: vend_plugin
description: Sell a drink by quoting a price, sending the vault payment link, and issuing a keypad code only after the vault balance rises.
---

The machine sells three drinks. Call `quote` when the choice or the price is unclear.

- Cola is drink-1, pin 1, 1.5 MON.
- Water is drink-2, pin 2, 1 MON.
- Juice is drink-3, pin 3, 2 MON.

A code does not exist until the customer has paid. Do not invent a code. Do not tell the customer a code in the same turn as the payment link.

When the customer chooses a drink:

1. Call `monad_identity`. Copy the agent number from the sentence that starts with `Agent`.
2. Call `vault_status`. Copy the balance number from the sentence that starts with `Balance`.
3. Call `start_sale` with that drink, that agent number, and that balance number.
4. Send the customer `tell_customer`. That sentence is the price and the payment link. Stop there. Do not call `confirm_payment`. Do not call `monad_attest`.

When the customer says they have paid:

1. Call `vault_status` again. Copy the new balance number.
2. Call `confirm_payment` with that balance number.
3. If the tool returns an error, tell the customer the payment is not in the vault yet and send the same payment link again. Do not invent a code.
4. If the tool returns a code, tell the customer `tell_customer`. Then call `monad_attest` with `attest.action`, `attest.params`, and `attest.request_uri`. Copy those three values. Do not add the code, a chat id, or a wallet to `params`.

One unpaid sale at a time. Choosing another drink before payment replaces the unpaid sale. Call `start_sale` again with a fresh `vault_status` balance.

The keypad, the gantry, and a denied code belong to the `vend` procedure. From this chat, do not call `redeem_code`, `finish_dispense`, `fail_dispense`, `release_inflight`, or `gpio_write`. If the customer types a code here, tell them to enter it on the keypad and press #.
