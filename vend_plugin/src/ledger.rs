//! Vend codes and the record the device is allowed to sign.
//!
//! The four-digit code stays in this ledger. The attest object has the drink,
//! the price, and the result. It does not have the code.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const MAX_OPEN: usize = 20;
const MAX_USED: usize = 80;
const ISSUE_ATTEMPTS: u32 = 32;

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

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ledger {
    #[serde(default)]
    counter: u64,
    #[serde(default)]
    open: Vec<Ticket>,
    #[serde(default)]
    used: Vec<Ticket>,
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

pub fn issue(ledger: &mut Ledger, drink_name: &str, salt: u64) -> Result<Value, String> {
    let drink =
        find_drink(drink_name).ok_or_else(|| "drink must be Cola, Water, or Juice".to_string())?;
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
        ledger.open.push(Ticket {
            code: code.clone(),
            slot: drink.slot.to_string(),
            item: drink.item.to_string(),
            pin: drink.pin.to_string(),
            price_mon: drink.price_mon.to_string(),
            status: Some(CodeStatus::Issued),
        });
        return Ok(json!({
            "code": code,
            "item": drink.item,
            "slot": drink.slot,
            "price_mon": drink.price_mon,
            "tell_customer": format!("Your code is {code}. Enter it on the keypad and press #."),
            "attest": attest("issue", drink.item, drink.slot, drink.pin, drink.price_mon, "issued", "ok"),
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
    fn issue_returns_a_four_digit_code_that_the_record_does_not_contain() {
        let mut ledger = Ledger::default();
        let output = issue(&mut ledger, "cola", 7).expect("issue");
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
            let output = issue(&mut ledger, "water", 3).expect("issue");
            seen.push(output["code"].as_str().expect("code").to_string());
        }
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), 20);
    }

    #[test]
    fn an_unknown_drink_issues_nothing() {
        let mut ledger = Ledger::default();
        let error = issue(&mut ledger, "coffee", 1).expect_err("drink");
        assert!(error.contains("Cola"));
        assert!(ledger.open.is_empty());
    }

    #[test]
    fn redeem_reserves_a_code_and_a_second_code_stays_busy() {
        let mut ledger = Ledger::default();
        let first = issue(&mut ledger, "Juice", 1).expect("first")["code"]
            .as_str()
            .expect("code")
            .to_string();
        let second = issue(&mut ledger, "cola", 1).expect("second")["code"]
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
        let code = issue(&mut ledger, "cola", 9).expect("issue")["code"]
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
        let code = issue(&mut ledger, "water", 4).expect("issue")["code"]
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
        let code = issue(&mut ledger, "1", 2).expect("issue")["code"]
            .as_str()
            .expect("code")
            .to_string();
        let restored = Ledger::from_bytes(&ledger.to_bytes().expect("bytes")).expect("parse");
        assert_eq!(restored.open[0].code, code);
        assert!(Ledger::from_bytes(b"nope").is_err());
    }
}
