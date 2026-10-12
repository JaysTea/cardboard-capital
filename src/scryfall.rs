//! Scryfall API client and card helpers. https://scryfall.com/docs/api

use std::collections::HashMap;
use std::io::Read;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::de::DeserializeOwned;
use serde::Deserialize;

use crate::model::Finish;

pub const API: &str = "https://api.scryfall.com";
/// Scryfall asks for 50–100 ms between requests.
const MIN_INTERVAL: Duration = Duration::from_millis(110);
const MAX_PAGES: usize = 12;

#[derive(Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Prices {
    pub usd: Option<String>,
    pub usd_foil: Option<String>,
    pub usd_etched: Option<String>,
    pub eur: Option<String>,
    pub eur_foil: Option<String>,
}

#[derive(Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct ImageUris {
    pub small: String,
    pub normal: String,
    pub large: String,
}

#[derive(Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct CardFace {
    pub name: String,
    pub type_line: Option<String>,
    pub image_uris: Option<ImageUris>,
    pub colors: Option<Vec<String>>,
}

#[derive(Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Card {
    pub id: String,
    pub name: String,
    pub lang: String,
    pub set: String,
    pub set_name: String,
    pub collector_number: String,
    pub rarity: String,
    pub finishes: Vec<String>,
    pub prices: Prices,
    pub image_uris: Option<ImageUris>,
    pub card_faces: Option<Vec<CardFace>>,
    pub full_art: bool,
    pub textless: bool,
    pub promo: bool,
    pub digital: bool,
    pub border_color: String,
    pub frame: String,
    pub frame_effects: Option<Vec<String>>,
    pub promo_types: Option<Vec<String>>,
    pub type_line: Option<String>,
    pub colors: Option<Vec<String>>,
    pub scryfall_uri: String,
    pub purchase_uris: Option<HashMap<String, String>>,
    pub released_at: String,
    pub prints_search_uri: String,
}

impl Card {
    /// Image URL for a size ("small", "normal", "large"); double-faced cards use the front face.
    pub fn image(&self, size: &str) -> Option<String> {
        let uris = self.image_uris.as_ref().or_else(|| {
            self.card_faces
                .as_ref()
                .and_then(|f| f.first())
                .and_then(|f| f.image_uris.as_ref())
        })?;
        let url = match size {
            "small" => &uris.small,
            "large" => &uris.large,
            _ => &uris.normal,
        };
        (!url.is_empty()).then(|| url.clone())
    }

    pub fn type_line_full(&self) -> String {
        if let Some(t) = &self.type_line {
            return t.clone();
        }
        self.card_faces
            .as_ref()
            .map(|faces| {
                faces
                    .iter()
                    .filter_map(|f| f.type_line.clone())
                    .collect::<Vec<_>>()
                    .join(" // ")
            })
            .unwrap_or_default()
    }

    /// "W", "UB", "Colorless", ... in WUBRG order.
    pub fn colors_label(&self) -> String {
        let colors = self.colors.clone().or_else(|| {
            self.card_faces
                .as_ref()
                .and_then(|f| f.first())
                .and_then(|f| f.colors.clone())
        });
        let colors = colors.unwrap_or_default();
        let s: String = ["W", "U", "B", "R", "G"]
            .iter()
            .filter(|c| colors.iter().any(|x| x == *c))
            .copied()
            .collect();
        if s.is_empty() {
            "Colorless".into()
        } else {
            s
        }
    }

    pub fn has_finish(&self, finish: Finish) -> bool {
        self.finishes.iter().any(|f| f == finish.scryfall_key())
    }

    pub fn tcgplayer_uri(&self) -> Option<String> {
        self.purchase_uris.as_ref()?.get("tcgplayer").cloned()
    }

    fn frame_effects(&self) -> &[String] {
        self.frame_effects.as_deref().unwrap_or(&[])
    }

    fn promo_types(&self) -> &[String] {
        self.promo_types.as_deref().unwrap_or(&[])
    }

    /// True when nothing about the printing is special (regular frame, no promo).
    pub fn is_plain(&self) -> bool {
        treatment(self) == "Normal"
    }
}

/// Scryfall USD price for a finish.
pub fn price_usd(card: &Card, finish: Finish) -> Option<f64> {
    let p = match finish {
        Finish::NonFoil => &card.prices.usd,
        Finish::Foil => &card.prices.usd_foil,
        Finish::Etched => &card.prices.usd_etched,
    };
    p.as_deref().and_then(|s| s.parse::<f64>().ok())
}

/// Human label for the printing's visual treatment, e.g. "Borderless, Full Art".
pub fn treatment(card: &Card) -> String {
    let mut parts: Vec<&str> = Vec::new();
    let fx = card.frame_effects();
    let has_fx = |k: &str| fx.iter().any(|f| f == k);

    if card.border_color == "borderless" {
        parts.push("Borderless");
    }
    if card.full_art {
        parts.push("Full Art");
    }
    if has_fx("extendedart") {
        parts.push("Extended Art");
    }
    if has_fx("showcase") {
        parts.push("Showcase");
    }
    if has_fx("inverted") {
        parts.push("Inverted");
    }
    if card.textless {
        parts.push("Textless");
    }
    // Old-style frames on modern prints (Retro frame). Genuinely old cards are left alone.
    if (card.frame == "1993" || card.frame == "1997") && card.released_at.as_str() >= "2020-01-01" {
        parts.push("Retro Frame");
    }

    let promo_labels: &[(&str, &str)] = &[
        ("serialized", "Serialized"),
        ("textured", "Textured"),
        ("galaxyfoil", "Galaxy Foil"),
        ("surgefoil", "Surge Foil"),
        ("halofoil", "Halo Foil"),
        ("confettifoil", "Confetti Foil"),
        ("oilslick", "Oil Slick"),
        ("stepandcompleat", "Step-and-Compleat"),
        ("rainbowfoil", "Rainbow Foil"),
        ("raisedfoil", "Raised Foil"),
        ("fracturefoil", "Fracture Foil"),
        ("doublerainbow", "Double Rainbow"),
        ("gilded", "Gilded"),
        ("neonink", "Neon Ink"),
        ("prerelease", "Prerelease Promo"),
        ("promopack", "Promo Pack"),
        ("buyabox", "Buy-a-Box"),
        ("bundle", "Bundle Promo"),
        ("datestamped", "Datestamped"),
        ("judgegift", "Judge Promo"),
    ];
    let mut known_promo = false;
    for (key, label) in promo_labels {
        if card.promo_types().iter().any(|p| p == key) {
            parts.push(label);
            known_promo = true;
        }
    }
    if card.promo && !known_promo {
        parts.push("Promo");
    }

    if parts.is_empty() {
        "Normal".into()
    } else {
        parts.join(", ")
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct List {
    data: Vec<Card>,
    has_more: bool,
    next_page: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Catalog {
    data: Vec<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ApiError {
    details: String,
}

pub struct Client {
    agent: ureq::Agent,
    last_call: Mutex<Option<Instant>>,
}

impl Client {
    pub fn new() -> Self {
        let agent = ureq::AgentBuilder::new()
            .user_agent(concat!("CardboardCapital/", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(25))
            .build();
        Client {
            agent,
            last_call: Mutex::new(None),
        }
    }

    pub fn agent(&self) -> &ureq::Agent {
        &self.agent
    }

    fn throttle(&self) {
        let mut last = self.last_call.lock().unwrap();
        if let Some(t) = *last {
            let elapsed = t.elapsed();
            if elapsed < MIN_INTERVAL {
                std::thread::sleep(MIN_INTERVAL - elapsed);
            }
        }
        *last = Some(Instant::now());
    }

    fn handle<T: DeserializeOwned>(
        result: Result<ureq::Response, ureq::Error>,
    ) -> Result<T, String> {
        match result {
            Ok(resp) => resp
                .into_json::<T>()
                .map_err(|e| format!("Bad response from Scryfall: {e}")),
            Err(ureq::Error::Status(code, resp)) => {
                let details = resp
                    .into_json::<ApiError>()
                    .ok()
                    .map(|e| e.details)
                    .filter(|d| !d.is_empty());
                Err(details.unwrap_or_else(|| format!("Scryfall returned HTTP {code}")))
            }
            Err(e) => Err(format!("Network error: {e}")),
        }
    }

    fn get<T: DeserializeOwned>(&self, url: &str, query: &[(&str, &str)]) -> Result<T, String> {
        self.throttle();
        let mut req = self.agent.get(url).set("Accept", "application/json");
        for (k, v) in query {
            req = req.query(k, v);
        }
        Self::handle(req.call())
    }

    fn paginate(&self, first: Result<List, String>) -> Result<Vec<Card>, String> {
        let mut page = first?;
        let mut cards = std::mem::take(&mut page.data);
        let mut pages = 1;
        while page.has_more && pages < MAX_PAGES {
            let Some(next) = page.next_page.take() else {
                break;
            };
            page = self.get::<List>(&next, &[])?;
            cards.append(&mut page.data);
            pages += 1;
        }
        cards.retain(|c| !c.digital);
        Ok(cards)
    }

    pub fn autocomplete(&self, q: &str) -> Result<Vec<String>, String> {
        let c: Catalog = self.get(&format!("{API}/cards/autocomplete"), &[("q", q)])?;
        Ok(c.data)
    }

    fn search_prints_exact(&self, name: &str) -> Result<Vec<Card>, String> {
        let q = format!("!\"{}\" game:paper", name.replace('"', ""));
        self.paginate(self.get(
            &format!("{API}/cards/search"),
            &[
                ("q", &q),
                ("unique", "prints"),
                ("order", "released"),
                ("dir", "desc"),
            ],
        ))
    }

    /// All paper printings of a card by name; falls back to fuzzy matching for typos.
    pub fn search_by_name(&self, name: &str) -> Result<Vec<Card>, String> {
        match self.search_prints_exact(name) {
            Ok(cards) if !cards.is_empty() => Ok(cards),
            first => {
                let fuzzy: Result<Card, String> =
                    self.get(&format!("{API}/cards/named"), &[("fuzzy", name)]);
                match fuzzy {
                    Ok(card) => self.search_prints_exact(&card.name),
                    Err(e) => Err(first.err().unwrap_or(e)),
                }
            }
        }
    }

    pub fn card_by_set_number(&self, set: &str, number: &str) -> Result<Card, String> {
        let url = format!(
            "{API}/cards/{}/{}",
            encode_path(&set.trim().to_lowercase()),
            encode_path(number.trim())
        );
        self.get(&url, &[])
    }

    pub fn prints_of(&self, card: &Card) -> Result<Vec<Card>, String> {
        if card.prints_search_uri.is_empty() {
            return Ok(vec![card.clone()]);
        }
        self.paginate(self.get(&card.prints_search_uri, &[]))
    }

    /// Fetches up to any number of cards by Scryfall id, 75 per request.
    pub fn cards_by_id(&self, ids: &[String]) -> Result<Vec<Card>, String> {
        let idents: Vec<serde_json::Value> = ids
            .iter()
            .map(|id| serde_json::json!({ "id": id }))
            .collect();
        self.cards_by_identifiers(&idents)
    }

    /// `POST /cards/collection` with any identifiers ({id}, {set, collector_number}, {name}, …),
    /// 75 per request. Cards that aren't found are simply missing from the result.
    pub fn cards_by_identifiers(
        &self,
        identifiers: &[serde_json::Value],
    ) -> Result<Vec<Card>, String> {
        let mut out = Vec::new();
        for chunk in identifiers.chunks(75) {
            let body = serde_json::json!({ "identifiers": chunk });
            self.throttle();
            let resp = self
                .agent
                .post(&format!("{API}/cards/collection"))
                .set("Accept", "application/json")
                .send_json(body);
            let mut list: List = Self::handle(resp)?;
            out.append(&mut list.data);
        }
        Ok(out)
    }

    /// Any JSON document (used for deck sites). Errors are short, e.g. "HTTP 404".
    pub fn get_value(&self, url: &str) -> Result<serde_json::Value, String> {
        match self.agent.get(url).set("Accept", "application/json").call() {
            Ok(resp) => resp.into_json().map_err(|e| format!("bad response: {e}")),
            Err(ureq::Error::Status(code, _)) => Err(format!("HTTP {code}")),
            Err(e) => Err(format!("network error: {e}")),
        }
    }

    /// Raw bytes of an image (images on *.scryfall.io aren't rate limited).
    pub fn fetch_bytes(&self, url: &str) -> Result<Vec<u8>, String> {
        let resp = self.agent.get(url).call().map_err(|e| e.to_string())?;
        let mut buf = Vec::new();
        resp.into_reader()
            .take(20 * 1024 * 1024)
            .read_to_end(&mut buf)
            .map_err(|e| e.to_string())?;
        Ok(buf)
    }
}

/// Collector numbers to try for what's printed on the card, most likely first.
/// Accepts "0105", "M 0105", "M0105", "105/281", "107s". Newer cards print a rarity
/// letter (C/U/R/M/S/B, L for lands, T for tokens) and leading zeros that Scryfall doesn't use.
pub fn printed_number_candidates(input: &str) -> Vec<String> {
    let mut s = input.trim().to_string();
    // Old style "105/281": only the part before the slash is the collector number.
    if let Some((left, _)) = s.split_once('/') {
        s = left.trim().to_string();
    }

    let mut chars = s.chars();
    if let Some(first) = chars.next() {
        let rest = chars.as_str().trim_start();
        if "CURMSBLT".contains(first.to_ascii_uppercase())
            && rest.starts_with(|c: char| c.is_ascii_digit())
        {
            s = rest.to_string();
        }
    }

    let mut candidates = Vec::new();
    let unpadded = s.trim_start_matches('0');
    if !unpadded.is_empty()
        && unpadded.len() != s.len()
        && unpadded.starts_with(|c: char| c.is_ascii_digit())
    {
        candidates.push(unpadded.to_string());
    }
    candidates.push(s.clone());
    let raw = input.trim().to_string();
    if !candidates.contains(&raw) && !raw.is_empty() {
        candidates.push(raw);
    }
    candidates
}

/// "DSK • EN", "dsk", " MKM " -> "dsk"
pub fn parse_set_code(input: &str) -> String {
    input
        .split(|c: char| !c.is_ascii_alphanumeric())
        .find(|t| !t.is_empty())
        .unwrap_or("")
        .to_lowercase()
}

fn encode_path(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub fn card(json: &str) -> Card {
        serde_json::from_str(json).unwrap()
    }

    pub const BORDERLESS: &str = r#"{
        "id": "b1", "name": "Sheoldred, the Apocalypse", "set": "dmu", "set_name": "Dominaria United",
        "collector_number": "415", "rarity": "mythic", "finishes": ["nonfoil", "foil"],
        "prices": {"usd": "80.12", "usd_foil": "95.50", "usd_etched": null, "eur": "70.00"},
        "image_uris": {"small": "https://cards.scryfall.io/small/b1.jpg", "normal": "https://cards.scryfall.io/normal/b1.jpg"},
        "full_art": false, "border_color": "borderless", "frame": "2015", "frame_effects": ["inverted"],
        "type_line": "Legendary Creature — Phyrexian Praetor", "colors": ["B"],
        "scryfall_uri": "https://scryfall.com/card/dmu/415", "released_at": "2022-09-09",
        "purchase_uris": {"tcgplayer": "https://tcgplayer.example/b1"}
    }"#;

    pub const DFC: &str = r#"{
        "id": "d1", "name": "Delver of Secrets // Insectile Aberration", "set": "isd", "set_name": "Innistrad",
        "collector_number": "51", "rarity": "common", "finishes": ["nonfoil", "foil"],
        "prices": {"usd": "0.25", "usd_foil": null},
        "card_faces": [
            {"name": "Delver of Secrets", "type_line": "Creature — Human Wizard", "colors": ["U"],
             "image_uris": {"small": "https://cards.scryfall.io/small/d1f.jpg", "normal": "https://cards.scryfall.io/normal/d1f.jpg"}},
            {"name": "Insectile Aberration", "type_line": "Creature — Human Insect", "colors": ["U"],
             "image_uris": {"small": "https://cards.scryfall.io/small/d1b.jpg", "normal": "https://cards.scryfall.io/normal/d1b.jpg"}}
        ],
        "border_color": "black", "frame": "2003", "released_at": "2011-09-30"
    }"#;

    pub const ETCHED_FULLART: &str = r#"{
        "id": "e1", "name": "Plains", "set": "unf", "set_name": "Unfinity",
        "collector_number": "235", "rarity": "common", "finishes": ["etched"],
        "prices": {"usd": null, "usd_foil": null, "usd_etched": "3.10"},
        "full_art": true, "border_color": "black", "frame": "2015", "promo": true, "promo_types": ["galaxyfoil"],
        "type_line": "Basic Land — Plains", "released_at": "2022-10-07"
    }"#;

    #[test]
    fn borderless_treatment_and_prices() {
        let c = card(BORDERLESS);
        assert_eq!(treatment(&c), "Borderless, Inverted");
        assert_eq!(price_usd(&c, Finish::NonFoil), Some(80.12));
        assert_eq!(price_usd(&c, Finish::Foil), Some(95.5));
        assert_eq!(price_usd(&c, Finish::Etched), None);
        assert!(c.has_finish(Finish::Foil));
        assert!(!c.has_finish(Finish::Etched));
        assert_eq!(c.colors_label(), "B");
        assert_eq!(
            c.tcgplayer_uri().as_deref(),
            Some("https://tcgplayer.example/b1")
        );
    }

    #[test]
    fn double_faced_uses_front_face() {
        let c = card(DFC);
        assert_eq!(
            c.image("normal").as_deref(),
            Some("https://cards.scryfall.io/normal/d1f.jpg")
        );
        assert_eq!(
            c.type_line_full(),
            "Creature — Human Wizard // Creature — Human Insect"
        );
        assert_eq!(c.colors_label(), "U");
        assert_eq!(treatment(&c), "Normal");
        assert!(c.is_plain());
    }

    #[test]
    fn etched_full_art_promo() {
        let c = card(ETCHED_FULLART);
        assert_eq!(treatment(&c), "Full Art, Galaxy Foil");
        assert_eq!(price_usd(&c, Finish::Etched), Some(3.1));
        assert_eq!(price_usd(&c, Finish::NonFoil), None);
        assert_eq!(c.colors_label(), "Colorless");
        assert_eq!(c.image("normal"), None);
    }

    /// Hits the real APIs: `cargo test -- --ignored live`
    #[test]
    #[ignore]
    fn live_lookups() {
        let client = Client::new();
        let prints = client.search_by_name("Sheoldred, the Apocalypse").unwrap();
        assert!(
            prints.len() > 3,
            "expected many printings, got {}",
            prints.len()
        );
        assert!(prints.iter().any(|c| c.border_color == "borderless"));
        for c in &prints {
            println!(
                "{} #{} {} [{}] usd={:?} foil={:?}",
                c.set,
                c.collector_number,
                c.rarity,
                treatment(c),
                c.prices.usd,
                c.prices.usd_foil
            );
        }

        let typo = client.search_by_name("sheoldred apocalypse").unwrap();
        assert_eq!(typo[0].name, "Sheoldred, the Apocalypse");

        let exact = client.card_by_set_number("DMU", "107").unwrap();
        assert_eq!(exact.name, "Sheoldred, the Apocalypse");
        let others = client.prints_of(&exact).unwrap();
        assert!(others.len() > 3);

        let dfc = client.search_by_name("Delver of Secrets").unwrap();
        assert!(dfc[0].image("normal").is_some());

        let ac = client.autocomplete("sheol").unwrap();
        assert!(ac.iter().any(|n| n.starts_with("Sheoldred")));

        let by_id = client.cards_by_id(&[exact.id.clone()]).unwrap();
        assert_eq!(by_id[0].id, exact.id);

        let img = client.fetch_bytes(&exact.image("small").unwrap()).unwrap();
        assert!(img.len() > 1000);

        let fx = crate::fx::fetch_rate(client.agent(), "SGD").unwrap();
        println!("USD->SGD {} from {} ({})", fx.rate, fx.source, fx.date);
        assert!(fx.rate > 0.8 && fx.rate < 2.5);
    }

    #[test]
    fn printed_numbers() {
        assert_eq!(printed_number_candidates("M 0105")[0], "105");
        assert_eq!(printed_number_candidates("m0105")[0], "105");
        assert_eq!(printed_number_candidates("0105"), vec!["105", "0105"]);
        assert_eq!(printed_number_candidates("105"), vec!["105"]);
        assert_eq!(printed_number_candidates("105/281")[0], "105");
        assert_eq!(printed_number_candidates("U 0042")[0], "42");
        assert_eq!(printed_number_candidates("L 0272")[0], "272");
        // Suffixes and Scryfall's own formats are left alone.
        assert_eq!(printed_number_candidates("107s"), vec!["107s"]);
        assert_eq!(printed_number_candidates("CLB-187"), vec!["CLB-187"]);
        assert_eq!(printed_number_candidates("0"), vec!["0"]);
        assert_eq!(parse_set_code("DSK • EN"), "dsk");
        assert_eq!(parse_set_code(" sos "), "sos");
    }

    #[test]
    fn path_encoding() {
        assert_eq!(encode_path("123★"), "123%E2%98%85");
        assert_eq!(encode_path("dmu"), "dmu");
    }
}
