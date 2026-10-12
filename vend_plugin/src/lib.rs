//! Vend tools. Codes are issued and checked here. The model does not decide
//! whether a code is valid, and the signed record does not contain the code.

mod ledger;

pub use ledger::{fail, finish, issue, quote, redeem, release, Ledger};

#[cfg(target_family = "wasm")]
mod component {
    wit_bindgen::generate!({
        path: "wit/v0",
        world: "tools-plugin",
        features: ["plugins-wit-v0"],
    });

    use exports::kinetic::plugin::plugin_info::Guest as PluginInfo;
    use exports::kinetic::plugin::toolbox::{Guest, ToolInfo, ToolResult};
    use kinetic::plugin::state::{get as state_get, put as state_put, StateError};
    use serde_json::Value;

    use crate::ledger::{fail, finish, issue, quote, redeem, release, Ledger};

    const LEDGER: &str = "ledger";
    const ATTEMPTS: u32 = 4;

    struct Tools;

    impl PluginInfo for Tools {
        fn plugin_name() -> String {
            "vend_plugin".to_string()
        }

        fn plugin_version() -> String {
            "0.1.0".to_string()
        }
    }

    impl Guest for Tools {
        fn list_tools() -> Vec<ToolInfo> {
            vec![
                tool(
                    "quote",
                    "List the three drinks, their slots, pins, and prices in MON. Call this when a customer asks what the machine sells.",
                    r#"{"type":"object","properties":{}}"#,
                ),
                tool(
                    "issue_code",
                    "Issue a random four-digit keypad code for one drink and store it on this device. drink is Cola, Water, or Juice. Tell the customer tell_customer. Then call monad_attest with attest.action, attest.params, and attest.request_uri. Never put the code into the attestation.",
                    r#"{"type":"object","properties":{"drink":{"type":"string"}},"required":["drink"]}"#,
                ),
                tool(
                    "redeem_code",
                    "Check a keypad code. Only the vend procedure calls this, with the four digits from the board. A customer chat must not call it. decision dispense means call gpio_write with pin and value, then stop without attesting. decision deny means call gpio_write with pin 0 and value 0, then monad_attest the attest fields.",
                    r#"{"type":"object","properties":{"code":{"type":"string"}},"required":["code"]}"#,
                ),
                tool(
                    "finish_dispense",
                    "Mark the reserved drink dispensed after the board sends signal dispensed. slot is the payload, such as drink-1. Then call monad_attest with the attest fields.",
                    r#"{"type":"object","properties":{"slot":{"type":"string"}},"required":["slot"]}"#,
                ),
                tool(
                    "fail_dispense",
                    "Return a reserved code to the customer after signal error. reason is the payload, such as pi_timeout. Call monad_attest only when skip_attest is false.",
                    r#"{"type":"object","properties":{"reason":{"type":"string"}},"required":["reason"]}"#,
                ),
                tool(
                    "release_inflight",
                    "Return a reserved code after signal boot. Call monad_attest only when skip_attest is false.",
                    r#"{"type":"object","properties":{}}"#,
                ),
            ]
        }

        fn execute(name: String, args: String) -> Result<ToolResult, String> {
            let parsed: Value = serde_json::from_str(&args)
                .map_err(|error| format!("args are not valid JSON: {error}"))?;
            match name.as_str() {
                "quote" => Ok(done(quote())),
                "issue_code" => mutate(|ledger| issue(ledger, required(&parsed, "drink")?, salt())),
                "redeem_code" => mutate(|ledger| Ok(redeem(ledger, required(&parsed, "code")?))),
                "finish_dispense" => mutate(|ledger| finish(ledger, required(&parsed, "slot")?)),
                "fail_dispense" => mutate(|ledger| Ok(fail(ledger, required(&parsed, "reason")?))),
                "release_inflight" => mutate(|ledger| Ok(release(ledger))),
                other => Err(format!("unknown tool {other}")),
            }
        }
    }

    fn tool(name: &str, description: &str, schema: &str) -> ToolInfo {
        ToolInfo {
            name: name.to_string(),
            description: description.to_string(),
            parameters_schema: schema.to_string(),
        }
    }

    fn done(value: Value) -> ToolResult {
        ToolResult {
            success: true,
            output: value.to_string(),
            error: None,
        }
    }

    fn failed(error: &str) -> ToolResult {
        ToolResult {
            success: false,
            output: String::new(),
            error: Some(error.to_string()),
        }
    }

    fn required<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
        value
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .ok_or_else(|| format!("`{key}` must be a string"))
    }

    fn salt() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos() as u64)
            .unwrap_or(0)
    }

    fn mutate(change: impl Fn(&mut Ledger) -> Result<Value, String>) -> Result<ToolResult, String> {
        for _ in 0..ATTEMPTS {
            let current = state_get(LEDGER).map_err(state_message)?;
            let mut ledger = match current.as_ref() {
                None => Ledger::default(),
                Some(entry) => Ledger::from_bytes(&entry.value)?,
            };
            let before = ledger.clone();
            let output = match change(&mut ledger) {
                Ok(output) => output,
                Err(error) => return Ok(failed(&error)),
            };
            if ledger == before {
                return Ok(done(output));
            }
            let expected = current.as_ref().map(|entry| entry.revision);
            match state_put(LEDGER, &ledger.to_bytes()?, expected) {
                Ok(_) => return Ok(done(output)),
                Err(StateError::Conflict) => continue,
                Err(error) => return Err(state_message(error)),
            }
        }
        Err("the vend ledger was busy".to_string())
    }

    fn state_message(error: StateError) -> String {
        match error {
            StateError::InvalidKey => "state key was rejected".to_string(),
            StateError::AccessDenied => "state access was denied".to_string(),
            StateError::NotFound => "state value was not found".to_string(),
            StateError::Conflict => "state revision conflict".to_string(),
            StateError::QuotaExceeded => "state quota was exceeded".to_string(),
            StateError::Unavailable => "state is unavailable".to_string(),
        }
    }

    export!(Tools);
}
