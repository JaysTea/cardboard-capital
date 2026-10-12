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
    /// Manual SGD unit price; overrides the converted Scryfall price.
    pub manual_sgd: Option<f64>,
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

    pub fn unit_sgd(&self, rate: f64) -> Option<f64> {
        self.manual_sgd.or(self.usd.map(|u| u * rate))
    }

    pub fn total_sgd(&self, rate: f64) -> Option<f64> {
        self.unit_sgd(rate).map(|u| u * self.qty as f64)
    }

    pub fn price_source(&self) -> &'static str {
        if self.manual_sgd.is_some() {
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
        if entry.manual_sgd.is_some() {
            existing.manual_sgd = entry.manual_sgd;
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
    /// Folder the spreadsheet is saved in. Empty = the app's `exports` folder.
    pub export_dir: String,
    /// The "where should the spreadsheet go?" question has been answered.
    pub export_prompted: bool,
    pub auto_export: bool,
    pub fx_rate: f64,
    pub fx_updated: String,
    pub fx_source: String,
    pub fx_manual: bool,
    pub fx_manual_rate: f64,
    /// Rows with Total S$ at or above this are highlighted in the sheet.
    pub highlight_sgd: f64,
    pub dark_mode: bool,
    /// None = follow the build: beta builds check the Beta channel, others Stable.
    pub update_channel: Option<UpdateChannel>,
    /// Tag of an update the user chose to skip (e.g. "v1.2.0").
    pub skipped_version: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            export_path: String::new(),
            export_dir: String::new(),
            export_prompted: false,
            auto_export: true,
            fx_rate: 1.30,
            fx_updated: String::new(),
            fx_source: "default (not fetched yet)".into(),
            fx_manual: false,
            fx_manual_rate: 1.30,
            highlight_sgd: 20.0,
            dark_mode: true,
            update_channel: None,
            skipped_version: String::new(),
        }
    }
}

impl Settings {
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
        .trim_start_matches("S$")
        .trim_start_matches('$')
        .chars()
        .filter(|c| *c != ',' && !c.is_whitespace())
        .collect();
    if cleaned.is_empty() {
        return Ok(None);
    }
    match cleaned.parse::<f64>() {
        Ok(v) if v.is_finite() && v >= 0.0 => Ok(Some(v)),
        _ => Err(()),
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
        second.manual_sgd = Some(12.0);
        second.notes = "from trade".into();
        add_or_merge(&mut c, second);
        assert_eq!(c[0].manual_sgd, Some(12.0));
        assert_eq!(c[0].notes, "binder 1; from trade");
    }

    #[test]
    fn pricing() {
        let mut e = entry("a", Finish::Foil, Condition::NM, 3);
        e.usd = Some(10.0);
        assert_eq!(e.unit_sgd(1.3), Some(13.0));
        assert_eq!(e.total_sgd(1.3), Some(39.0));
        assert_eq!(e.price_source(), "Scryfall");
        e.manual_sgd = Some(5.0);
        assert_eq!(e.total_sgd(1.3), Some(15.0));
        assert_eq!(e.price_source(), "Manual");
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
