//! Live USD -> <user's currency> exchange rate.

use serde_json::Value;

#[derive(Debug, Clone)]
pub struct FxQuote {
    /// Currency code the rate converts USD into.
    pub currency: String,
    pub rate: f64,
    pub source: String,
    pub date: String,
}

/// 1 USD in `code`. Frankfurter (ECB) first; ExchangeRate-API covers currencies the ECB doesn't (TWD, VND, …).
pub fn fetch_rate(agent: &ureq::Agent, code: &str) -> Result<FxQuote, String> {
    let code = code.to_uppercase();
    if code == "USD" {
        return Ok(FxQuote {
            currency: code,
            rate: 1.0,
            source: "Prices are already in USD".into(),
            date: String::new(),
        });
    }
    match frankfurter(agent, &code) {
        Ok(q) => Ok(q),
        Err(e1) => open_er_api(agent, &code)
            .map_err(|e2| format!("{e1}; fallback also failed: {e2}")),
    }
}

fn get_json(agent: &ureq::Agent, url: &str) -> Result<Value, String> {
    agent
        .get(url)
        .call()
        .map_err(|e| e.to_string())?
        .into_json::<Value>()
        .map_err(|e| e.to_string())
}

/// ECB reference rates (updated each working day).
fn frankfurter(agent: &ureq::Agent, code: &str) -> Result<FxQuote, String> {
    let v = get_json(
        agent,
        &format!("https://api.frankfurter.dev/v1/latest?base=USD&symbols={code}"),
    )?;
    let rate = v["rates"][code]
        .as_f64()
        .ok_or_else(|| format!("frankfurter: no {code} rate"))?;
    Ok(FxQuote {
        currency: code.to_string(),
        rate,
        source: "Frankfurter (ECB)".into(),
        date: v["date"].as_str().unwrap_or_default().to_string(),
    })
}

fn open_er_api(agent: &ureq::Agent, code: &str) -> Result<FxQuote, String> {
    let v = get_json(agent, "https://open.er-api.com/v6/latest/USD")?;
    let rate = v["rates"][code]
        .as_f64()
        .ok_or_else(|| format!("open.er-api: no {code} rate"))?;
    Ok(FxQuote {
        currency: code.to_string(),
        rate,
        source: "ExchangeRate-API".into(),
        date: v["time_last_update_utc"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every supported currency must be fetchable. `cargo test -- --ignored live`
    #[test]
    #[ignore]
    fn live_all_currencies() {
        let client = crate::scryfall::Client::new();
        let mut failed = Vec::new();
        for c in crate::model::CURRENCIES {
            match fetch_rate(client.agent(), c.code) {
                Ok(q) => println!("{} {} ({})", c.code, q.rate, q.source),
                Err(e) => failed.push(format!("{}: {e}", c.code)),
            }
        }
        assert!(failed.is_empty(), "{failed:?}");
    }
}
