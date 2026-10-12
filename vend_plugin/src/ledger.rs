//! Vend codes and the record the device is allowed to sign.
//!
//! The four-digit code stays in this ledger. The attest object has the drink,
//! the price, and the result. It does not have the code.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const MAX_OPEN: usize = 20;
const MAX_USED: usize = 80;
const ISSUE_ATTEMPTS: u32 = 32;
const WEI_PER_MON: u128 = 1_000_000_000_000_000_000;
const VAULT: &str = "0x77102fCAC7927bB64507DF82bf1ae659465B4EE5";

struct Drink {
    slot: &'static str,
    item: &'static str,
    pin: &'static str,
    price_mon: &'static str,
    names: &'static [&'static str],
}

const DRINKS: &[Drink] = &[
    Drink {
        slot: "drink-1",
        item: "Cola",
        pin: "1",
        price_mon: "1.5",
        names: &["drink-1", "1", "cola", "coke"],
    },
    Drink {
        slot: "drink-2",
        item: "Water",
        pin: "2",
        price_mon: "1",
        names: &["drink-2", "2", "water"],
    },
    Drink {
        slot: "drink-3",
        item: "Juice",
        pin: "3",
        price_mon: "2",
        names: &["drink-3", "3", "juice"],
    },
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum CodeStatus {
    Issued,
    Reserved,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Ticket {
    code: String,
    slot: String,
    item: String,
    pin: String,
    price_mon: String,
    #[serde(default)]
    status: Option<CodeStatus>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct PendingSale {
    agent: String,
    slot: String,
    item: String,
    pin: String,
    price_mon: String,
    price_wei: String,
    balance_wei: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ledger {
    #[serde(default)]
    counter: u64,
    #[serde(default)]
    open: Vec<Ticket>,
    #[serde(default)]
    used: Vec<Ticket>,
    #[serde(default)]
    pending: Option<PendingSale>,
}

impl Ledger {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        serde_json::from_slice(bytes).map_err(|_| "the vend ledger could not be read".to_string())
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, String> {
        serde_json::to_vec(self).map_err(|_| "the vend ledger could not be stored".to_string())
    }
}

pub fn quote() -> Value {
    json!({
        "drinks": DRINKS.iter().map(|drink| json!({
            "item": drink.item,
            "slot": drink.slot,
            "pin": drink.pin,
            "price_mon": drink.price_mon,
        })).collect::<Vec<_>>(),
    })
}

pub fn start_sale(
    ledger: &mut Ledger,
    drink_name: &str,
    agent_raw: &str,
    balance_raw: &str,
) -> Result<Value, String> {
    let drink =
        find_drink(drink_name).ok_or_else(|| "drink must be Cola, Water, or Juice".to_string())?;
    let agent = agent_id(agent_raw)?;
    let balance_wei = mon_to_wei(balance_raw)?;
    let price_wei = mon_to_wei(drink.price_mon)?;
    let payment_url = format!("https://vault-deposit.kineticvm.xyz?address={VAULT}&agent={agent}");
    ledger.pending = Some(PendingSale {
        agent,
        slot: drink.slot.to_string(),
        item: drink.item.to_string(),
        pin: drink.pin.to_string(),
        price_mon: drink.price_mon.to_string(),
        price_wei: price_wei.to_string(),
        balance_wei: balance_wei.to_string(),
    });
    Ok(json!({
        "awaiting_payment": true,
        "item": drink.item,
        "slot": drink.slot,
        "price_mon": drink.price_mon,
        "payment_url": payment_url,
        "tell_customer": format!(
            "{item} is {price} MON. Pay here: {payment_url} Reply when the transfer is done.",
            item = drink.item,
            price = drink.price_mon,
        ),
    }))
}

pub fn confirm_payment(ledger: &mut Ledger, balance_raw: &str, salt: u64) -> Result<Value, String> {
    let Some(pending) = ledger.pending.clone() else {
        return Err("no sale is waiting for payment. Call start_sale first.".to_string());
    };
    let paid = received(&pending, balance_raw)?;
    if !paid {
        return Err(format!(
            "the vault has not received {} MON yet",
            pending.price_mon
        ));
    }
    issue_paid(ledger, &pending, salt)
}

fn received(pending: &PendingSale, balance_raw: &str) -> Result<bool, String> {
    let balance_wei = mon_to_wei(balance_raw)?;
    let before: u128 = pending
        .balance_wei
        .parse()
        .map_err(|_| "the saved vault balance could not be read".to_string())?;
    let price: u128 = pending
        .price_wei
        .parse()
        .map_err(|_| "the saved price could not be read".to_string())?;
    Ok(balance_wei >= before && balance_wei - before >= price)
}

fn issue_paid(ledger: &mut Ledger, pending: &PendingSale, salt: u64) -> Result<Value, String> {
    if ledger.open.len() >= MAX_OPEN {
        return Err("too many unused codes".to_string());
    }
    let mut counter = ledger.counter;
    for _ in 0..ISSUE_ATTEMPTS {
        counter = counter.saturating_add(1);
        let code = four_digits(counter, salt);
        if taken(ledger, &code) {
            continue;
        }
        ledger.counter = counter;
        ledger.pending = None;
        ledger.open.push(Ticket {
            code: code.clone(),
            slot: pending.slot.clone(),
            item: pending.item.clone(),
            pin: pending.pin.clone(),
            price_mon: pending.price_mon.clone(),
            status: Some(CodeStatus::Issued),
        });
        return Ok(json!({
            "code": code,
            "item": pending.item,
            "slot": pending.slot,
            "price_mon": pending.price_mon,
            "tell_customer": format!("Your code is {code}. Enter it on the keypad and press #."),
            "attest": attest(
                "issue",
                &pending.item,
                &pending.slot,
                &pending.pin,
                &pending.price_mon,
                "issued",
                "paid",
            ),
        }));
    }
    Err("could not issue a code".to_string())
}

pub fn redeem(ledger: &mut Ledger, raw_code: &str) -> Value {
    let code = raw_code.trim();
    if !is_code(code) {
        return deny(None, "invalid");
    }
    if ledger
        .open
        .iter()
        .any(|ticket| ticket.status == Some(CodeStatus::Reserved))
    {
        return deny(lookup(ledger, code), "busy");
    }
    if let Some(ticket) = ledger.used.iter().find(|ticket| ticket.code == code) {
        return deny(Some(ticket), "used");
    }
    let Some(index) = ledger.open.iter().position(|ticket| ticket.code == code) else {
        return deny(None, "unknown");
    };
    if ledger.open[index].status != Some(CodeStatus::Issued) {
        return deny(Some(&ledger.open[index]), "busy");
    }
    ledger.open[index].status = Some(CodeStatus::Reserved);
    let ticket = &ledger.open[index];
    let pin = ticket.pin.parse::<u64>().unwrap_or(0);
    json!({
        "decision": "dispense",
        "pin": pin,
        "value": 1,
        "slot": ticket.slot,
        "skip_attest": true,
    })
}

pub fn finish(ledger: &mut Ledger, raw_slot: &str) -> Result<Value, String> {
    let slot = raw_slot.trim();
    let Some(index) = ledger
        .open
        .iter()
        .position(|ticket| ticket.status == Some(CodeStatus::Reserved) && ticket.slot == slot)
    else {
        return Err("no reserved drink matches that slot".to_string());
    };
    let ticket = ledger.open.remove(index);
    let output = json!({
        "slot": ticket.slot,
        "attest": attest(
            "dispense",
            &ticket.item,
            &ticket.slot,
            &ticket.pin,
            &ticket.price_mon,
            "dispensed",
            "ok",
        ),
    });
    remember_used(ledger, ticket);
    Ok(output)
}

pub fn fail(ledger: &mut Ledger, raw_reason: &str) -> Value {
    let detail = match raw_reason.trim() {
        "pi_timeout" => "timeout",
        "unknown_drink" => "unknown_drink",
        _ => "reset",
    };
    release_reserved(ledger, detail)
}

pub fn release(ledger: &mut Ledger) -> Value {
    release_reserved(ledger, "reset")
}

fn release_reserved(ledger: &mut Ledger, detail: &str) -> Value {
    let Some(index) = ledger
        .open
        .iter()
        .position(|ticket| ticket.status == Some(CodeStatus::Reserved))
    else {
        return json!({ "skip_attest": true });
    };
    let ticket = &mut ledger.open[index];
    ticket.status = Some(CodeStatus::Issued);
    let ticket = ticket.clone();
    json!({
        "skip_attest": false,
        "attest": attest(
            "release",
            &ticket.item,
            &ticket.slot,
            &ticket.pin,
            &ticket.price_mon,
            "released",
            detail,
        ),
    })
}

fn deny(ticket: Option<&Ticket>, detail: &str) -> Value {
    let (item, slot, pin, price) = ticket
        .map(|ticket| {
            (
                ticket.item.as_str(),
                ticket.slot.as_str(),
                ticket.pin.as_str(),
                ticket.price_mon.as_str(),
            )
        })
        .unwrap_or(("none", "none", "0", "0"));
    json!({
        "decision": "deny",
        "pin": 0,
        "value": 0,
        "skip_attest": false,
        "attest": attest("deny", item, slot, pin, price, "denied", detail),
    })
}

fn attest(
    action: &str,
    item: &str,
    slot: &str,
    pin: &str,
    price_mon: &str,
    result: &str,
    detail: &str,
) -> Value {
    let params = json!({
        "action": action,
        "detail": detail,
        "item": item,
        "machine": "vending",
        "pin": pin,
        "price_mon": price_mon,
        "result": result,
        "slot": slot,
    });
    json!({
        "action": action,
        "params": serde_json::to_string(&params).unwrap_or_else(|_| "{}".to_string()),
        "request_uri": format!("kinetic://vend/{slot}"),
    })
}

fn agent_id(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if !trimmed.is_empty() && trimmed.chars().all(|c| c.is_ascii_digit()) {
        return Ok(trimmed.to_string());
    }
    let Some(after) = trimmed.split("Agent ").nth(1) else {
        return Err("agent must be the agent number from monad_identity".to_string());
    };
    let id: String = after
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    if id.is_empty() {
        return Err("agent must be the agent number from monad_identity".to_string());
    }
    Ok(id)
}

fn mon_to_wei(raw: &str) -> Result<u128, String> {
    let token = raw
        .split(|c: char| !c.is_ascii_digit() && c != '.')
        .find(|part| part.chars().any(|c| c.is_ascii_digit()))
        .ok_or_else(|| "balance must be a MON amount".to_string())?;
    let (whole, frac) = token.split_once('.').unwrap_or((token, ""));
    if frac.len() > 18
        || frac.chars().any(|c| !c.is_ascii_digit())
        || whole.chars().any(|c| !c.is_ascii_digit())
    {
        return Err("balance must be a MON amount".to_string());
    }
    let whole_wei = if whole.is_empty() {
        0
    } else {
        whole
            .parse::<u128>()
            .map_err(|_| "balance must be a MON amount".to_string())?
            .checked_mul(WEI_PER_MON)
            .ok_or_else(|| "balance is too large".to_string())?
    };
    let frac_wei = if frac.is_empty() {
        0
    } else {
        format!("{frac:0<18}")
            .parse::<u128>()
            .map_err(|_| "balance must be a MON amount".to_string())?
    };
    whole_wei
        .checked_add(frac_wei)
        .ok_or_else(|| "balance is too large".to_string())
}

fn find_drink(raw: &str) -> Option<&'static Drink> {
    let key = raw.trim().to_ascii_lowercase();
    DRINKS
        .iter()
        .find(|drink| drink.names.iter().any(|name| *name == key))
}

fn taken(ledger: &Ledger, code: &str) -> bool {
    ledger.open.iter().any(|ticket| ticket.code == code)
        || ledger.used.iter().any(|ticket| ticket.code == code)
}

fn lookup<'a>(ledger: &'a Ledger, code: &str) -> Option<&'a Ticket> {
    ledger
        .open
        .iter()
        .chain(ledger.used.iter())
        .find(|ticket| ticket.code == code)
}

fn remember_used(ledger: &mut Ledger, mut ticket: Ticket) {
    ticket.status = None;
    ledger.used.push(ticket);
    if ledger.used.len() > MAX_USED {
        let extra = ledger.used.len() - MAX_USED;
        ledger.used.drain(0..extra);
    }
}

fn is_code(code: &str) -> bool {
    code.len() == 4 && code.bytes().all(|byte| byte.is_ascii_digit())
}

fn four_digits(counter: u64, salt: u64) -> String {
    format!("{:04}", mix(counter ^ salt.rotate_left(17)) % 10_000)
}

fn mix(mut value: u64) -> u64 {
    value ^= value >> 30;
    value = value.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value ^= value >> 27;
    value = value.wrapping_mul(0x94D0_49BB_1331_11EB);
    value ^= value >> 31;
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(output: &Value) -> Value {
        let text = output["attest"]["params"].as_str().expect("params");
        serde_json::from_str(text).expect("params json")
    }

    fn paid(ledger: &mut Ledger, drink: &str, salt: u64) -> Result<Value, String> {
        start_sale(ledger, drink, "4", "0")?;
        let price = ledger
            .pending
            .as_ref()
            .map(|sale| sale.price_mon.clone())
            .ok_or_else(|| "no sale".to_string())?;
        confirm_payment(ledger, &price, salt)
    }

    #[test]
    fn quote_lists_the_three_drinks() {
        let menu = quote();
        let drinks = menu["drinks"].as_array().expect("drinks");
        assert_eq!(drinks.len(), 3);
        assert_eq!(drinks[0]["item"], "Cola");
        assert_eq!(drinks[1]["slot"], "drink-2");
        assert_eq!(drinks[2]["price_mon"], "2");
    }

    #[test]
    fn a_sale_returns_a_payment_link_and_no_code() {
        let mut ledger = Ledger::default();
        let sale = start_sale(
            &mut ledger,
            "cola",
            "Agent 12. Owner 0x64772107fC23f7370C90EA0aBd29ee6117B97f77. Device 0xabc.",
            "Balance 0.000000000000000000 MON. Spent in this window 0.000000000000000000 MON.",
        )
        .expect("sale");
        assert!(sale.get("code").is_none());
        assert!(sale.get("attest").is_none());
        assert_eq!(sale["price_mon"], "1.5");
        assert_eq!(
            sale["payment_url"],
            "https://vault-deposit.kineticvm.xyz?address=0x77102fCAC7927bB64507DF82bf1ae659465B4EE5&agent=12"
        );
        let told = sale["tell_customer"].as_str().expect("tell");
        assert!(told.contains("https://vault-deposit.kineticvm.xyz"));
        assert!(!told.contains("Your code"));
        assert!(ledger.open.is_empty());
        let restored = Ledger::from_bytes(&ledger.to_bytes().expect("bytes")).expect("parse");
        assert_eq!(restored.pending.expect("pending").price_mon, "1.5");
    }

    #[test]
    fn an_unpaid_balance_issues_no_code() {
        let mut ledger = Ledger::default();
        start_sale(&mut ledger, "cola", "12", "10").expect("sale");
        let short = confirm_payment(&mut ledger, "Balance 11.400000000000000000 MON.", 7)
            .expect_err("short");
        assert!(short.contains("1.5"));
        assert!(ledger.open.is_empty());
        assert!(ledger.pending.is_some());
        let same = confirm_payment(&mut ledger, "10", 7).expect_err("same");
        assert!(same.contains("not received"));
        let none = confirm_payment(&mut Ledger::default(), "2", 1).expect_err("none");
        assert!(none.contains("start_sale"));
        assert!(start_sale(&mut ledger, "cola", "device", "0").is_err());
        assert!(start_sale(&mut Ledger::default(), "cola", "12", "nope").is_err());
    }

    #[test]
    fn payment_issues_one_code_and_a_later_sale_replaces_an_unpaid_one() {
        let mut ledger = Ledger::default();
        start_sale(&mut ledger, "cola", "12", "10").expect("sale");
        let output = confirm_payment(
            &mut ledger,
            "Balance 11.500000000000000000 MON. Spent in this window 0.000000000000000000 MON.",
            7,
        )
        .expect("paid");
        let code = output["code"].as_str().expect("code");
        assert!(is_code(code), "{code}");
        assert!(ledger.pending.is_none());
        let signed = params(&output);
        assert_eq!(signed["detail"], "paid");
        assert!(!signed.to_string().contains(code));
        assert!(confirm_payment(&mut ledger, "13", 8).is_err());
        assert_eq!(ledger.open.len(), 1);

        start_sale(&mut ledger, "cola", "12", "0").expect("cola");
        start_sale(&mut ledger, "water", "12", "0").expect("water");
        assert!(confirm_payment(&mut ledger, "0.5", 3).is_err());
        let water = confirm_payment(&mut ledger, "1", 3).expect("water");
        assert_eq!(water["item"], "Water");
        assert_eq!(ledger.open.len(), 2);
    }

    #[test]
    fn issue_returns_a_four_digit_code_that_the_record_does_not_contain() {
        let mut ledger = Ledger::default();
        let output = paid(&mut ledger, "cola", 7).expect("issue");
        let code = output["code"].as_str().expect("code");
        assert!(is_code(code), "{code}");
        assert_eq!(output["item"], "Cola");
        assert_eq!(output["slot"], "drink-1");
        let signed = params(&output);
        assert!(signed.get("code").is_none());
        assert!(!signed.to_string().contains(code), "{signed}");
        assert_eq!(signed["action"], "issue");
        assert_eq!(signed["result"], "issued");
        assert_eq!(signed["machine"], "vending");
        assert_eq!(signed["price_mon"], "1.5");
        assert_eq!(signed["pin"], "1");
    }

    #[test]
    fn twenty_codes_stay_unique() {
        let mut ledger = Ledger::default();
        let mut seen = Vec::new();
        for _ in 0..20 {
            let output = paid(&mut ledger, "water", 3).expect("issue");
            seen.push(output["code"].as_str().expect("code").to_string());
        }
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), 20);
    }

    #[test]
    fn an_unknown_drink_issues_nothing() {
        let mut ledger = Ledger::default();
        let error = paid(&mut ledger, "coffee", 1).expect_err("drink");
        assert!(error.contains("Cola"));
        assert!(ledger.open.is_empty());
    }

    #[test]
    fn redeem_reserves_a_code_and_a_second_code_stays_busy() {
        let mut ledger = Ledger::default();
        let first = paid(&mut ledger, "Juice", 1).expect("first")["code"]
            .as_str()
            .expect("code")
            .to_string();
        let second = paid(&mut ledger, "cola", 1).expect("second")["code"]
            .as_str()
            .expect("code")
            .to_string();

        let accepted = redeem(&mut ledger, &first);
        assert_eq!(accepted["decision"], "dispense");
        assert_eq!(accepted["pin"], 3);
        assert_eq!(accepted["value"], 1);
        assert_eq!(accepted["skip_attest"], true);
        assert!(accepted.get("code").is_none());

        let busy = redeem(&mut ledger, &second);
        assert_eq!(busy["decision"], "deny");
        assert_eq!(busy["pin"], 0);
        assert_eq!(params(&busy)["detail"], "busy");
        assert!(!params(&busy).to_string().contains(&second));
        assert_eq!(ledger.open[1].status, Some(CodeStatus::Issued));
    }

    #[test]
    fn an_unknown_or_short_code_is_denied_without_a_write() {
        let mut ledger = Ledger::default();
        let before = ledger.clone();
        let denied = redeem(&mut ledger, "12");
        assert_eq!(params(&denied)["detail"], "invalid");
        assert_eq!(ledger, before);
        let unknown = redeem(&mut ledger, "0000");
        assert_eq!(params(&unknown)["detail"], "unknown");
        assert_eq!(params(&unknown)["item"], "none");
        assert_eq!(ledger, before);
    }

    #[test]
    fn finish_dispenses_once_and_the_code_cannot_be_reused() {
        let mut ledger = Ledger::default();
        let code = paid(&mut ledger, "cola", 9).expect("issue")["code"]
            .as_str()
            .expect("code")
            .to_string();
        redeem(&mut ledger, &code);
        let done = finish(&mut ledger, "drink-1").expect("finish");
        let signed = params(&done);
        assert_eq!(signed["result"], "dispensed");
        assert_eq!(signed["item"], "Cola");
        assert!(!signed.to_string().contains(&code));
        assert!(ledger.open.is_empty());

        let again = redeem(&mut ledger, &code);
        assert_eq!(params(&again)["detail"], "used");
        assert_eq!(params(&again)["item"], "Cola");
        let mismatch = finish(&mut Ledger::default(), "drink-1");
        assert!(mismatch.is_err());
    }

    #[test]
    fn a_timeout_returns_the_code_and_boot_clears_a_reservation() {
        let mut ledger = Ledger::default();
        let code = paid(&mut ledger, "water", 4).expect("issue")["code"]
            .as_str()
            .expect("code")
            .to_string();
        redeem(&mut ledger, &code);
        let failed = fail(&mut ledger, "pi_timeout");
        assert_eq!(params(&failed)["detail"], "timeout");
        assert_eq!(params(&failed)["result"], "released");
        assert_eq!(ledger.open[0].status, Some(CodeStatus::Issued));

        redeem(&mut ledger, &code);
        let idle = fail(&mut Ledger::default(), "pi_timeout");
        assert_eq!(idle["skip_attest"], true);

        let released = release(&mut ledger);
        assert_eq!(params(&released)["detail"], "reset");
        assert_eq!(ledger.open[0].status, Some(CodeStatus::Issued));
        let quiet = release(&mut ledger);
        assert_eq!(quiet["skip_attest"], true);
    }

    #[test]
    fn the_ledger_round_trips_without_the_signed_record() {
        let mut ledger = Ledger::default();
        let code = paid(&mut ledger, "1", 2).expect("issue")["code"]
            .as_str()
            .expect("code")
            .to_string();
        let restored = Ledger::from_bytes(&ledger.to_bytes().expect("bytes")).expect("parse");
        assert_eq!(restored.open[0].code, code);
        assert!(Ledger::from_bytes(b"nope").is_err());
    }
}
