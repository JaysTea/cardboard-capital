//! Live USD -> SGD exchange rate.

use serde_json::Value;

#[derive(Debug, Clone)]
pub struct FxQuote {
    pub rate: f64,
    pub source: String,
    pub date: String,
}

pub fn fetch_usd_sgd(agent: &ureq::Agent) -> Result<FxQuote, String> {
    match frankfurter(agent) {
        Ok(q) => Ok(q),
        Err(e1) => open_er_api(agent).map_err(|e2| format!("{e1}; fallback also failed: {e2}")),
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
fn frankfurter(agent: &ureq::Agent) -> Result<FxQuote, String> {
    let v = get_json(
        agent,
        "https://api.frankfurter.dev/v1/latest?base=USD&symbols=SGD",
    )?;
    let rate = v["rates"]["SGD"]
        .as_f64()
        .ok_or("frankfurter: no SGD rate")?;
    Ok(FxQuote {
        rate,
        source: "Frankfurter (ECB)".into(),
        date: v["date"].as_str().unwrap_or_default().to_string(),
    })
}

fn open_er_api(agent: &ureq::Agent) -> Result<FxQuote, String> {
    let v = get_json(agent, "https://open.er-api.com/v6/latest/USD")?;
    let rate = v["rates"]["SGD"]
        .as_f64()
        .ok_or("open.er-api: no SGD rate")?;
    Ok(FxQuote {
        rate,
        source: "ExchangeRate-API".into(),
        date: v["time_last_update_utc"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
    })
}
