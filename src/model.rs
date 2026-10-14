use serde::{Deserialize, Serialize};

use crate::scryfall::{self, Card};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum Finish {
    #[default]
    NonFoil,
    Foil,
    Etched,
}

impl Finish {
    pub const ALL: [Finish; 3] = [Finish::NonFoil, Finish::Foil, Finish::Etched];

    pub fn label(self) -> &'static str {
        match self {
            Finish::NonFoil => "Non-foil",
            Finish::Foil => "Foil",
            Finish::Etched => "Etched",
        }
    }

    /// The value Scryfall uses in a card's `finishes` array.
    pub fn scryfall_key(self) -> &'static str {
        match self {
            Finish::NonFoil => "nonfoil",
            Finish::Foil => "foil",
            Finish::Etched => "etched",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum Condition {
    #[default]
    NM,
    LP,
    MP,
    HP,
    DMG,
}

impl Condition {
    pub const ALL: [Condition; 5] = [
        Condition::NM,
        Condition::LP,
        Condition::MP,
        Condition::HP,
        Condition::DMG,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Condition::NM => "NM",
            Condition::LP => "LP",
            Condition::MP => "MP",
            Condition::HP => "HP",
            Condition::DMG => "DMG",
        }
    }

    pub fn long_label(self) -> &'static str {
        match self {
            Condition::NM => "Near Mint",
            Condition::LP => "Lightly Played",
            Condition::MP => "Moderately Played",
            Condition::HP => "Heavily Played",
            Condition::DMG => "Damaged",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum RarityFilter {
    #[default]
    Any,
    Common,
    Uncommon,
    Rare,
    Mythic,
    Special,
}

impl RarityFilter {
    pub const ALL: [RarityFilter; 6] = [
        RarityFilter::Any,
        RarityFilter::Common,
        RarityFilter::Uncommon,
        RarityFilter::Rare,
        RarityFilter::Mythic,
        RarityFilter::Special,
    ];

    pub fn label(self) -> &'static str {
        match self {
            RarityFilter::Any => "Any",
            RarityFilter::Common => "Common",
            RarityFilter::Uncommon => "Uncommon",
            RarityFilter::Rare => "Rare",
            RarityFilter::Mythic => "Mythic",
            RarityFilter::Special => "Special / Bonus",
        }
    }

    pub fn matches(self, rarity: &str) -> bool {
        match self {
            RarityFilter::Any => true,
            RarityFilter::Common => rarity == "common",
            RarityFilter::Uncommon => rarity == "uncommon",
            RarityFilter::Rare => rarity == "rare",
            RarityFilter::Mythic => rarity == "mythic",
            RarityFilter::Special => rarity == "special" || rarity == "bonus",
        }
    }
}

/// "mythic" -> "Mythic"
pub fn rarity_label(rarity: &str) -> String {
    let mut c = rarity.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// One row of the collection: a specific printing in a specific finish and condition.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Entry {
    pub uid: u64,
    pub scryfall_id: String,
    pub name: String,
    pub set_code: String,
    pub set_name: String,
    pub collector_number: String,
    pub rarity: String,
    pub finish: Finish,
    pub condition: Condition,
    /// Auto-detected from Scryfall (e.g. "Borderless, Full Art").
    pub treatment: String,
    /// User override; empty means use `treatment`.
    pub treatment_override: String,
    pub colors: String,
    pub type_line: String,
    pub qty: u32,
    /// Scryfall USD price for this finish.
    pub usd: Option<f64>,
    /// Manual unit price in the user's currency; overrides the converted Scryfall price.
    /// (Saved as "manual_sgd" so older versions can still read it.)
    #[serde(rename = "manual_sgd")]
    pub manual_price: Option<f64>,
    pub checked: bool,
    pub notes: String,
    pub scryfall_uri: String,
    pub tcgplayer_uri: String,
    pub image_small: String,
    pub image_normal: String,
    pub added: String,
    pub price_updated: String,
}

impl Entry {
    pub fn from_card(card: &Card, finish: Finish, condition: Condition, qty: u32) -> Entry {
        let mut e = Entry {
            finish,
            condition,
            qty: qty.max(1),
            added: now_string(),
            ..Default::default()
        };
        e.update_from_card(card);
        e
    }

    /// Refreshes everything that comes from Scryfall (price, metadata, images).
    pub fn update_from_card(&mut self, card: &Card) {
        self.scryfall_id = card.id.clone();
        self.name = card.name.clone();
        self.set_code = card.set.to_uppercase();
        self.set_name = card.set_name.clone();
        self.collector_number = card.collector_number.clone();
        self.rarity = card.rarity.clone();
        self.treatment = scryfall::treatment(card);
        self.colors = card.colors_label();
        self.type_line = card.type_line_full();
        self.usd = scryfall::price_usd(card, self.finish);
        self.scryfall_uri = card.scryfall_uri.clone();
        self.tcgplayer_uri = card.tcgplayer_uri().unwrap_or_default();
        self.image_small = card.image("small").unwrap_or_default();
        self.image_normal = card.image("normal").unwrap_or_default();
        self.price_updated = now_string();
    }

    pub fn treatment_display(&self) -> &str {
        if self.treatment_override.trim().is_empty() {
            &self.treatment
        } else {
            &self.treatment_override
        }
    }

    pub fn unit_price(&self, rate: f64) -> Option<f64> {
        self.manual_price.or(self.usd.map(|u| u * rate))
    }

    pub fn total_price(&self, rate: f64) -> Option<f64> {
        self.unit_price(rate).map(|u| u * self.qty as f64)
    }

    pub fn price_source(&self) -> &'static str {
        if self.manual_price.is_some() {
            "Manual"
        } else if self.usd.is_some() {
            "Scryfall"
        } else {
            "No price"
        }
    }

    fn same_stack(&self, other: &Entry) -> bool {
        self.scryfall_id == other.scryfall_id
            && self.finish == other.finish
            && self.condition == other.condition
    }
}

/// Adds `entry`, or merges it into an existing row for the same printing/finish/condition.
/// Returns (merged, resulting quantity).
pub fn add_or_merge(collection: &mut Vec<Entry>, entry: Entry) -> (bool, u32) {
    if let Some(existing) = collection.iter_mut().find(|e| e.same_stack(&entry)) {
        existing.qty += entry.qty;
        if entry.manual_price.is_some() {
            existing.manual_price = entry.manual_price;
        }
        if !entry.treatment_override.trim().is_empty() {
            existing.treatment_override = entry.treatment_override;
        }
        let note = entry.notes.trim();
        if !note.is_empty() && !existing.notes.contains(note) {
            if existing.notes.trim().is_empty() {
                existing.notes = note.to_string();
            } else {
                existing.notes = format!("{}; {}", existing.notes.trim(), note);
            }
        }
        existing.usd = entry.usd;
        existing.price_updated = entry.price_updated;
        (true, existing.qty)
    } else {
        let qty = entry.qty;
        collection.push(entry);
        (false, qty)
    }
}

/// Card count and value of a set of entries.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Value {
    pub cards: u32,
    /// In the user's currency. Entries without a price count as 0.
    pub total: f64,
    /// Entries without any price.
    pub missing: usize,
}

pub fn collection_value<'a>(entries: impl IntoIterator<Item = &'a Entry>, rate: f64) -> Value {
    let mut v = Value::default();
    for e in entries {
        v.cards += e.qty;
        match e.total_price(rate) {
            Some(t) => v.total += t,
            None => v.missing += 1,
        }
    }
    v
}

/// A trading card game with its own collection. Only Magic for now.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Game {
    Mtg,
}

impl Game {
    pub const ALL: [Game; 1] = [Game::Mtg];

    /// Folder name under `data\`.
    pub fn id(self) -> &'static str {
        match self {
            Game::Mtg => "mtg",
        }
    }

    pub fn short_name(self) -> &'static str {
        match self {
            Game::Mtg => "MTG",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Game::Mtg => "Magic: The Gathering",
        }
    }
}

/// Each game's share of the total value, skipping games worth nothing. Empty when nothing has a value.
pub fn value_shares(totals: &[(Game, f64)]) -> Vec<(Game, f64)> {
    let sum: f64 = totals.iter().map(|(_, v)| v.max(0.0)).sum();
    if sum <= 0.0 {
        return Vec::new();
    }
    totals
        .iter()
        .filter(|(_, v)| *v > 0.0)
        .map(|&(g, v)| (g, v / sum))
        .collect()
}

/// How the home screen shows each game's share of the collection's value.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum HomeChart {
    /// A stacked bar above the game tiles.
    #[default]
    Bar,
    /// A ring in its own panel.
    Donut,
}

impl HomeChart {
    pub const ALL: [HomeChart; 2] = [HomeChart::Bar, HomeChart::Donut];

    pub fn label(self) -> &'static str {
        match self {
            HomeChart::Bar => "Bar",
            HomeChart::Donut => "Donut",
        }
    }
}

/// Which releases the app offers as updates.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum UpdateChannel {
    Stable,
    Beta,
    Off,
}

impl UpdateChannel {
    pub const ALL: [UpdateChannel; 3] = [UpdateChannel::Stable, UpdateChannel::Beta, UpdateChannel::Off];

    pub fn label(self) -> &'static str {
        match self {
            UpdateChannel::Stable => "Stable",
            UpdateChannel::Beta => "Beta",
            UpdateChannel::Off => "Off",
        }
    }

    pub fn help(self) -> &'static str {
        match self {
            UpdateChannel::Stable => "Tested releases only (recommended).",
            UpdateChannel::Beta => "Also get betas: new features early, may have bugs.",
            UpdateChannel::Off => "Never check. Stay on this version.",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Settings {
    /// Legacy full path of the spreadsheet (pre-1.2). Used only when `export_dir` is empty.
    pub export_path: String,
    /// Folder the spreadsheet is saved in. Empty = the default `exports` folder.
    pub export_dir: String,
    /// The "where should the spreadsheet go?" question has been answered.
    pub export_prompted: bool,
    pub auto_export: bool,
    /// ISO code prices are shown in (see `CURRENCIES`).
    pub currency: String,
    /// USD -> `currency` rate.
    pub fx_rate: f64,
    pub fx_updated: String,
    pub fx_source: String,
    pub fx_manual: bool,
    pub fx_manual_rate: f64,
    /// Rows whose total (in the user's currency) is at least this are highlighted in the sheet.
    #[serde(rename = "highlight_sgd")]
    pub highlight_value: f64,
    pub dark_mode: bool,
    /// None = follow the build: beta builds check the Beta channel, others Stable.
    pub update_channel: Option<UpdateChannel>,
    /// Tag of an update the user chose to skip (e.g. "v1.2.0").
    pub skipped_version: String,
    pub home_chart: HomeChart,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            export_path: String::new(),
            export_dir: String::new(),
            export_prompted: false,
            auto_export: true,
            currency: "SGD".into(),
            fx_rate: 1.30,
            fx_updated: String::new(),
            fx_source: "default (not fetched yet)".into(),
            fx_manual: false,
            fx_manual_rate: 1.30,
            highlight_value: 20.0,
            dark_mode: true,
            update_channel: None,
            skipped_version: String::new(),
            home_chart: HomeChart::Bar,
        }
    }
}

impl Settings {
    pub fn currency(&self) -> &'static Currency {
        currency(&self.currency)
    }

    pub fn channel(&self) -> UpdateChannel {
        self.update_channel.unwrap_or(if crate::update::current_version().pre.is_empty() {
            UpdateChannel::Stable
        } else {
            UpdateChannel::Beta
        })
    }

    pub fn rate(&self) -> f64 {
        if self.fx_manual {
            self.fx_manual_rate
        } else {
            self.fx_rate
        }
    }
}

pub fn now_string() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M").to_string()
}

/// Parses a user-typed money amount ("12.5", "S$12.50", "1,234"). Empty -> Ok(None).
pub fn parse_money(s: &str) -> Result<Option<f64>, ()> {
    let cleaned: String = s
        .trim()
        // Drop a leading currency symbol or code: "S$", "$", "€", "RM ", "EUR "…
        .trim_start_matches(|c: char| !c.is_ascii_digit() && c != '.')
        .chars()
        .filter(|c| *c != ',' && !c.is_whitespace())
        .collect();
    if cleaned.is_empty() {
        // Blank means "no price"; something with no digits at all ("abc") is a typo.
        return if s.trim().is_empty() { Ok(None) } else { Err(()) };
    }
    match cleaned.parse::<f64>() {
        Ok(v) if v.is_finite() && v >= 0.0 => Ok(Some(v)),
        _ => Err(()),
    }
}

/// A currency prices can be shown in. Prices are USD from Scryfall, converted at a live rate.
#[derive(Debug, PartialEq)]
pub struct Currency {
    pub code: &'static str,
    pub symbol: &'static str,
    pub name: &'static str,
    /// Decimal places shown (0 for yen, won, …).
    pub decimals: usize,
}

const fn c(code: &'static str, symbol: &'static str, name: &'static str, decimals: usize) -> Currency {
    Currency { code, symbol, name, decimals }
}

/// Supported currencies. All are available from the exchange-rate sources in `fx.rs`.
pub const CURRENCIES: &[Currency] = &[
    c("SGD", "S$", "Singapore dollar", 2),
    c("USD", "US$", "US dollar", 2),
    c("EUR", "€", "Euro", 2),
    c("GBP", "£", "British pound", 2),
    c("AUD", "A$", "Australian dollar", 2),
    c("CAD", "C$", "Canadian dollar", 2),
    c("NZD", "NZ$", "New Zealand dollar", 2),
    c("JPY", "¥", "Japanese yen", 0),
    c("CNY", "CN¥", "Chinese yuan", 2),
    c("HKD", "HK$", "Hong Kong dollar", 2),
    c("TWD", "NT$", "New Taiwan dollar", 0),
    c("KRW", "₩", "South Korean won", 0),
    c("MYR", "RM", "Malaysian ringgit", 2),
    c("IDR", "Rp", "Indonesian rupiah", 0),
    c("PHP", "₱", "Philippine peso", 2),
    c("THB", "THB", "Thai baht", 2),
    c("VND", "₫", "Vietnamese dong", 0),
    c("INR", "₹", "Indian rupee", 2),
    c("CHF", "CHF", "Swiss franc", 2),
    c("SEK", "SEK", "Swedish krona", 2),
    c("NOK", "NOK", "Norwegian krone", 2),
    c("DKK", "DKK", "Danish krone", 2),
    c("PLN", "zł", "Polish złoty", 2),
    c("BRL", "R$", "Brazilian real", 2),
    c("MXN", "MX$", "Mexican peso", 2),
    c("ZAR", "R", "South African rand", 2),
];

static DISPLAY_CURRENCY: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// The currency the UI is showing (set by the app from settings each frame).
pub fn cur() -> &'static Currency {
    &CURRENCIES[DISPLAY_CURRENCY.load(std::sync::atomic::Ordering::Relaxed).min(CURRENCIES.len() - 1)]
}

pub fn set_display_currency(code: &str) {
    let i = CURRENCIES.iter().position(|c| c.code.eq_ignore_ascii_case(code)).unwrap_or(0);
    DISPLAY_CURRENCY.store(i, std::sync::atomic::Ordering::Relaxed);
}

/// Looks up a currency by code; unknown codes fall back to SGD.
pub fn currency(code: &str) -> &'static Currency {
    CURRENCIES
        .iter()
        .find(|c| c.code.eq_ignore_ascii_case(code))
        .unwrap_or(&CURRENCIES[0])
}

impl Currency {
    /// 1234.5 -> "S$ 1,234.50", "¥ 1,235"
    pub fn fmt(&self, v: f64) -> String {
        format!("{} {}", self.symbol, self.amount(v))
    }

    /// Just the number, with this currency's decimals: "1,234.50"
    pub fn amount(&self, v: f64) -> String {
        if self.decimals == 0 {
            let s = money(v.round());
            s.trim_end_matches(".00").to_string()
        } else {
            money(v)
        }
    }

    /// Excel number format, e.g. `"S$"#,##0.00`.
    pub fn excel_format(&self) -> String {
        let sym = self.symbol.replace('"', "");
        if self.decimals == 0 {
            format!("\"{sym}\"#,##0")
        } else {
            format!("\"{sym}\"#,##0.00")
        }
    }
}

/// 1234.5 -> "1,234.50"
pub fn money(v: f64) -> String {
    let neg = v < 0.0;
    let cents = (v.abs() * 100.0).round() as u64;
    let whole = (cents / 100).to_string();
    let mut grouped = String::new();
    for (i, ch) in whole.chars().enumerate() {
        if i > 0 && (whole.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    format!(
        "{}{}.{:02}",
        if neg { "-" } else { "" },
        grouped,
        cents % 100
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, finish: Finish, cond: Condition, qty: u32) -> Entry {
        Entry {
            scryfall_id: id.into(),
            finish,
            condition: cond,
            qty,
            ..Default::default()
        }
    }

    #[test]
    fn merge_same_stack_increments_qty() {
        let mut c = vec![entry("a", Finish::Foil, Condition::NM, 2)];
        let (merged, qty) = add_or_merge(&mut c, entry("a", Finish::Foil, Condition::NM, 2));
        assert!(merged);
        assert_eq!(qty, 4);
        assert_eq!(c.len(), 1);
    }

    #[test]
    fn different_finish_or_condition_is_new_row() {
        let mut c = vec![entry("a", Finish::Foil, Condition::NM, 1)];
        add_or_merge(&mut c, entry("a", Finish::NonFoil, Condition::NM, 1));
        add_or_merge(&mut c, entry("a", Finish::Foil, Condition::LP, 1));
        assert_eq!(c.len(), 3);
    }

    #[test]
    fn merge_keeps_manual_price_and_appends_notes() {
        let mut first = entry("a", Finish::Foil, Condition::NM, 1);
        first.notes = "binder 1".into();
        let mut c = vec![first];
        let mut second = entry("a", Finish::Foil, Condition::NM, 1);
        second.manual_price = Some(12.0);
        second.notes = "from trade".into();
        add_or_merge(&mut c, second);
        assert_eq!(c[0].manual_price, Some(12.0));
        assert_eq!(c[0].notes, "binder 1; from trade");
    }

    #[test]
    fn pricing() {
        let mut e = entry("a", Finish::Foil, Condition::NM, 3);
        e.usd = Some(10.0);
        assert_eq!(e.unit_price(1.3), Some(13.0));
        assert_eq!(e.total_price(1.3), Some(39.0));
        assert_eq!(e.price_source(), "Scryfall");
        e.manual_price = Some(5.0);
        assert_eq!(e.total_price(1.3), Some(15.0));
        assert_eq!(e.price_source(), "Manual");
    }

    #[test]
    fn currencies() {
        assert_eq!(currency("eur").code, "EUR");
        assert_eq!(currency("XXX").code, "SGD");
        assert_eq!(currency("SGD").fmt(1234.5), "S$ 1,234.50");
        assert_eq!(currency("JPY").fmt(1234.5), "¥ 1,235");
        assert_eq!(currency("JPY").excel_format(), "\"¥\"#,##0");
        assert_eq!(parse_money("€ 12.5"), Ok(Some(12.5)));
        assert_eq!(parse_money("RM3"), Ok(Some(3.0)));
        // Old settings files (no currency field) stay SGD.
        let s: Settings = serde_json::from_str("{\"fx_rate\": 1.3}").unwrap();
        assert_eq!(s.currency().code, "SGD");
    }

    #[test]
    fn value_counts_missing_prices() {
        let mut a = entry("a", Finish::NonFoil, Condition::NM, 2);
        a.usd = Some(1.0);
        let mut b = entry("b", Finish::NonFoil, Condition::NM, 1);
        b.manual_price = Some(5.0);
        let c = entry("c", Finish::NonFoil, Condition::NM, 4);
        let v = collection_value(&[a, b, c], 2.0);
        assert_eq!(v, Value { cards: 7, total: 9.0, missing: 1 });
        assert_eq!(collection_value(&[], 2.0), Value::default());
    }

    #[test]
    fn shares() {
        assert!(value_shares(&[]).is_empty());
        assert!(value_shares(&[(Game::Mtg, 0.0)]).is_empty());
        assert_eq!(value_shares(&[(Game::Mtg, 12.5)]), vec![(Game::Mtg, 1.0)]);
        // Old settings files have no chart choice.
        let s: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(s.home_chart, HomeChart::Bar);
    }

    #[test]
    fn money_parsing_and_formatting() {
        assert_eq!(parse_money(""), Ok(None));
        assert_eq!(parse_money("S$1,234.5"), Ok(Some(1234.5)));
        assert_eq!(parse_money("$3"), Ok(Some(3.0)));
        assert!(parse_money("abc").is_err());
        assert_eq!(money(1234567.891), "1,234,567.89");
        assert_eq!(money(0.5), "0.50");
    }
}
