//! Importing card lists from deck links (Archidekt, Moxfield) or pasted text, and
//! resolving them to Scryfall printings.

use std::collections::HashSet;

use serde_json::{json, Value};

use crate::model::{parse_money, Condition, Finish};
use crate::scryfall::{printed_number_candidates, Card, Client};

pub const MOXFIELD_HELP: &str = "Moxfield doesn't allow apps to read decks directly. \
On the Moxfield deck page click Export → \"Copy for Moxfield\" (or \"Copy for MTGA\"), \
then switch to Text here and paste it in.";

/// One card line from a deck or pasted list, before it's matched to a printing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ImportLine {
    pub qty: u32,
    pub name: String,
    pub set: Option<String>,
    pub number: Option<String>,
    pub scryfall_id: Option<String>,
    /// Finish given by the source (e.g. "*F*" or Archidekt's Foil modifier).
    pub finish: Option<Finish>,
    /// "Main deck", "Sideboard", "Maybeboard", …
    pub board: String,
    /// The original text line (empty for link imports).
    pub raw: String,
    /// Per-card details carried over from another install's spreadsheet.
    pub extras: Option<SheetExtras>,
}

/// Columns from an exported spreadsheet that the collection keeps per card.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SheetExtras {
    pub condition: Option<Condition>,
    pub manual_price: Option<f64>,
    pub notes: String,
    pub checked: bool,
    /// As shown in the sheet (an override or the auto-detected label).
    pub treatment: String,
    /// Currency the sheet's manual prices are in ("SGD" for sheets from before currencies).
    pub currency: String,
    /// The sheet's USD -> `currency` rate (Summary!B4), used to convert manual prices.
    pub usd_rate: Option<f64>,
}

#[derive(Clone, Debug)]
pub struct ImportRow {
    pub include: bool,
    pub qty: u32,
    pub finish: Finish,
    pub card: Option<Card>,
    pub line: ImportLine,
    /// Something worth checking about this row (shown with a warning icon).
    pub note: Option<Note>,
}

/// A short label for the table plus the full explanation (shown on hover).
#[derive(Clone, Debug, PartialEq)]
pub struct Note {
    pub short: &'static str,
    pub detail: String,
}

fn note(short: &'static str, detail: impl Into<String>) -> Option<Note> {
    Some(Note {
        short,
        detail: detail.into(),
    })
}

#[derive(Clone, Debug)]
pub struct ImportResult {
    pub source: String,
    pub rows: Vec<ImportRow>,
}

#[derive(Clone, Debug)]
pub enum ImportSource {
    Link(String),
    Text(String),
    /// A spreadsheet exported by Cardboard Capital (or MTG Stock Sheet).
    Sheet(std::path::PathBuf),
}

#[derive(Clone, Copy, Debug)]
pub struct ImportOptions {
    /// Finish for lines that don't say.
    pub default_finish: Finish,
    /// Also take sideboard / maybeboard cards.
    pub include_side: bool,
}

pub fn run(
    client: &Client,
    source: ImportSource,
    opts: ImportOptions,
) -> Result<ImportResult, String> {
    let (name, lines) = match source {
        ImportSource::Link(url) => match parse_link(&url)? {
            DeckLink::Archidekt(id) => archidekt(client, &id, opts.include_side)?,
            DeckLink::Moxfield(id) => moxfield(client, &id, opts.include_side)?,
        },
        ImportSource::Text(text) => {
            let lines: Vec<ImportLine> = parse_text(&text)
                .into_iter()
                .filter(|l| opts.include_side || !is_side_board(&l.board))
                .collect();
            ("Pasted list".to_string(), lines)
        }
        ImportSource::Sheet(path) => {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "Spreadsheet".into());
            (name, parse_sheet(&path)?)
        }
    };
    if lines.is_empty() {
        return Err("No cards found to import.".into());
    }
    Ok(ImportResult {
        source: name,
        rows: resolve(client, lines, opts.default_finish)?,
    })
}

// ---------------------------------------------------------------- links

#[derive(Debug, PartialEq)]
pub enum DeckLink {
    Archidekt(String),
    Moxfield(String),
}

/// https://archidekt.com/decks/7031486/buffs_by_hans, https://moxfield.com/decks/AbC-123
pub fn parse_link(url: &str) -> Result<DeckLink, String> {
    let u = url.trim();
    let lower = u.to_lowercase();
    let id_after_decks = || -> Option<String> {
        let start = lower.find("/decks/")? + "/decks/".len();
        let id: String = u[start..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
            .collect();
        (!id.is_empty()).then_some(id)
    };
    if lower.contains("archidekt.com") {
        let id = id_after_decks().filter(|id| id.chars().all(|c| c.is_ascii_digit()));
        id.map(DeckLink::Archidekt)
            .ok_or_else(|| "That Archidekt link doesn't look like a deck link (archidekt.com/decks/<number>/…).".into())
    } else if lower.contains("moxfield.com") {
        id_after_decks().map(DeckLink::Moxfield).ok_or_else(|| {
            "That Moxfield link doesn't look like a deck link (moxfield.com/decks/<id>).".into()
        })
    } else {
        Err("Only archidekt.com and moxfield.com deck links are supported. For anything else, use Text.".into())
    }
}

fn archidekt(
    client: &Client,
    id: &str,
    include_side: bool,
) -> Result<(String, Vec<ImportLine>), String> {
    let v = client
        .get_value(&format!("https://archidekt.com/api/decks/{id}/"))
        .map_err(|e| {
            format!("Couldn't load the Archidekt deck ({e}). Private decks can't be imported.")
        })?;
    Ok(parse_archidekt(&v, include_side))
}

pub fn parse_archidekt(v: &Value, include_side: bool) -> (String, Vec<ImportLine>) {
    let name = v["name"].as_str().unwrap_or("Archidekt deck").to_string();
    // Categories like Maybeboard/Sideboard are marked as not part of the deck.
    let excluded: HashSet<String> = v["categories"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|c| c["includedInDeck"].as_bool() == Some(false))
        .filter_map(|c| c["name"].as_str().map(str::to_string))
        .collect();

    let mut lines = Vec::new();
    for c in v["cards"].as_array().into_iter().flatten() {
        let card = &c["card"];
        let primary = c["categories"]
            .as_array()
            .and_then(|a| a.first())
            .and_then(|s| s.as_str())
            .unwrap_or("");
        let board = if excluded.contains(primary) {
            primary.to_string()
        } else {
            "Main deck".to_string()
        };
        if !include_side && board != "Main deck" {
            continue;
        }
        let finish = match c["modifier"].as_str().unwrap_or("") {
            "Foil" => Finish::Foil,
            "Etched" => Finish::Etched,
            _ => Finish::NonFoil,
        };
        lines.push(ImportLine {
            qty: c["quantity"].as_u64().unwrap_or(1).max(1) as u32,
            name: card["oracleCard"]["name"]
                .as_str()
                .unwrap_or("")
                .to_string(),
            set: card["edition"]["editioncode"].as_str().map(str::to_string),
            number: card["collectorNumber"].as_str().map(str::to_string),
            scryfall_id: card["uid"].as_str().map(str::to_string),
            finish: Some(finish),
            board,
            raw: String::new(),
            extras: None,
        });
    }
    (name, lines)
}

fn moxfield(
    client: &Client,
    id: &str,
    include_side: bool,
) -> Result<(String, Vec<ImportLine>), String> {
    let v = client
        .get_value(&format!("https://api2.moxfield.com/v3/decks/all/{id}"))
        .map_err(|_| MOXFIELD_HELP.to_string())?;
    let parsed = parse_moxfield(&v, include_side);
    if parsed.1.is_empty() {
        return Err(MOXFIELD_HELP.to_string());
    }
    Ok(parsed)
}

pub fn parse_moxfield(v: &Value, include_side: bool) -> (String, Vec<ImportLine>) {
    let name = v["name"].as_str().unwrap_or("Moxfield deck").to_string();
    let mut lines = Vec::new();
    for (board_key, board) in v["boards"].as_object().into_iter().flatten() {
        let main = matches!(
            board_key.as_str(),
            "mainboard" | "commanders" | "companions"
        );
        if board_key == "tokens" || (!main && !include_side) {
            continue;
        }
        let board_name = if main {
            "Main deck".to_string()
        } else {
            title_case(board_key)
        };
        for entry in board["cards"]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(_, e)| e)
        {
            let card = &entry["card"];
            let finish = match entry["finish"].as_str().unwrap_or("") {
                "foil" => Finish::Foil,
                "etched" => Finish::Etched,
                _ => Finish::NonFoil,
            };
            lines.push(ImportLine {
                qty: entry["quantity"].as_u64().unwrap_or(1).max(1) as u32,
                name: card["name"].as_str().unwrap_or("").to_string(),
                set: card["set"].as_str().map(str::to_string),
                number: card["cn"].as_str().map(str::to_string),
                scryfall_id: card["scryfall_id"].as_str().map(str::to_string),
                finish: Some(finish),
                board: board_name.clone(),
                raw: String::new(),
                extras: None,
            });
        }
    }
    (name, lines)
}

fn title_case(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
        .unwrap_or_default()
}

// ---------------------------------------------------------------- spreadsheets

fn cell_text(d: &calamine::Data) -> String {
    use calamine::Data;
    match d {
        Data::String(s) => s.trim().to_string(),
        Data::Float(f) if f.fract() == 0.0 => format!("{}", *f as i64),
        Data::Float(f) => f.to_string(),
        Data::Int(i) => i.to_string(),
        Data::Bool(b) => b.to_string(),
        Data::Empty => String::new(),
        other => other.to_string().trim().to_string(),
    }
}

fn cell_number(d: &calamine::Data) -> Option<f64> {
    use calamine::Data;
    match d {
        Data::Float(f) => Some(*f),
        Data::Int(i) => Some(*i as f64),
        Data::String(s) => parse_money(s).ok().flatten(),
        _ => None,
    }
}

/// Reads the "Collection" sheet of a spreadsheet exported by this app (any version, including
/// "MTG Stock Sheet"). Columns are found by their header, so the column order doesn't matter.
pub fn parse_sheet(path: &std::path::Path) -> Result<Vec<ImportLine>, String> {
    use calamine::{open_workbook_auto, Reader};
    let mut wb = open_workbook_auto(path).map_err(|e| format!("Couldn't open {}: {e}", path.display()))?;
    let names = wb.sheet_names();
    let sheet = names
        .iter()
        .find(|n| n.eq_ignore_ascii_case("Collection"))
        .or_else(|| names.first())
        .cloned()
        .ok_or("The spreadsheet has no sheets.")?;
    let range = wb
        .worksheet_range(&sheet)
        .map_err(|e| format!("Couldn't read sheet \"{sheet}\": {e}"))?;
    let mut rows = range.rows();
    let header: Vec<String> = rows.next().ok_or("The spreadsheet is empty.")?.iter().map(cell_text).collect();
    let col = |title: &str| header.iter().position(|h| h.eq_ignore_ascii_case(title));
    let (c_id, c_name, c_set, c_num) = (col("Scryfall ID"), col("Name"), col("Set Code"), col("#"));
    if c_id.is_none() && (c_name.is_none() || c_set.is_none()) {
        return Err("This doesn't look like a Cardboard Capital spreadsheet: it needs a \"Scryfall ID\" column, \
                    or \"Name\" and \"Set Code\" columns, on a sheet called Collection."
            .into());
    }
    let (c_qty, c_finish, c_cond) = (col("Qty"), col("Finish"), col("Condition"));
    // "Manual (EUR)" since currencies were added; "Manual S$" (SGD) before that.
    let c_manual = header.iter().position(|h| h.to_lowercase().starts_with("manual"));
    let sheet_currency = c_manual
        .map(|i| header[i].as_str())
        .and_then(|h| h.split_once('(').map(|(_, rest)| rest.trim_end_matches(')').trim().to_uppercase()))
        .filter(|code| code.len() == 3)
        .unwrap_or_else(|| "SGD".into());
    let usd_rate = wb
        .worksheet_range("Summary")
        .ok()
        .and_then(|r| r.get_value((3, 1)).and_then(cell_number))
        .filter(|r| *r > 0.0);
    let (c_notes, c_checked, c_treat) = (col("Notes"), col("Checked"), col("Treatment"));

    let mut lines = Vec::new();
    for row in rows {
        let text = |c: Option<usize>| c.and_then(|i| row.get(i)).map(cell_text).unwrap_or_default();
        let id = text(c_id);
        let name = text(c_name);
        if id.is_empty() && name.is_empty() {
            continue;
        }
        let qty = c_qty
            .and_then(|i| row.get(i))
            .and_then(cell_number)
            .map(|q| q.round().max(1.0) as u32)
            .unwrap_or(1);
        let finish_text = text(c_finish);
        let cond_text = text(c_cond);
        let non_empty = |s: String| (!s.is_empty()).then_some(s);
        lines.push(ImportLine {
            qty,
            name,
            set: non_empty(text(c_set)),
            number: non_empty(text(c_num)),
            scryfall_id: non_empty(id),
            finish: Finish::ALL.into_iter().find(|f| f.label().eq_ignore_ascii_case(&finish_text)),
            board: "Main deck".into(),
            raw: String::new(),
            extras: Some(SheetExtras {
                condition: Condition::ALL.into_iter().find(|c| c.label().eq_ignore_ascii_case(&cond_text)),
                manual_price: c_manual.and_then(|i| row.get(i)).and_then(cell_number),
                notes: text(c_notes),
                checked: text(c_checked).eq_ignore_ascii_case("yes"),
                treatment: text(c_treat),
                currency: sheet_currency.clone(),
                usd_rate,
            }),
        });
    }
    Ok(lines)
}

// ---------------------------------------------------------------- text

fn is_side_board(board: &str) -> bool {
    let b = board.to_lowercase();
    b.contains("maybe") || b.contains("side") || b.contains("consider")
}

/// Section headers used by Moxfield, Archidekt and MTG Arena exports.
fn header_name(line: &str) -> Option<String> {
    let t = line.trim().trim_end_matches(':').trim();
    // "Sideboard (15)" -> "Sideboard"
    let t = match t.find('(') {
        Some(i)
            if t.ends_with(')') && t[i + 1..t.len() - 1].chars().all(|c| c.is_ascii_digit()) =>
        {
            t[..i].trim()
        }
        _ => t,
    };
    let known = [
        "deck",
        "main",
        "mainboard",
        "main deck",
        "sideboard",
        "side",
        "maybeboard",
        "maybe",
        "considering",
        "commander",
        "commanders",
        "companion",
        "about",
        "tokens",
    ];
    known
        .contains(&t.to_lowercase().as_str())
        .then(|| t.to_string())
}

/// Parses lines like:
///   `19 Swamp (UND) 91`, `1x Sol Ring (cmm) 400 *F* [Ramp]`, `1 Thalia's Lancers (PLST) EMN-47`,
///   `2 Lightning Bolt`, `Lightning Bolt`
pub fn parse_text(text: &str) -> Vec<ImportLine> {
    let mut board = "Main deck".to_string();
    let mut out = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with("//") || line.starts_with('#') {
            continue;
        }
        if let Some(h) = header_name(line) {
            board = match h.to_lowercase().as_str() {
                "deck" | "main" | "mainboard" | "main deck" | "commander" | "commanders"
                | "companion" => "Main deck".to_string(),
                _ => title_case(&h),
            };
            continue;
        }
        let b = board.to_lowercase();
        if b == "about" || b == "tokens" {
            continue;
        }
        if let Some(mut l) = parse_line(line) {
            l.board = board.clone();
            out.push(l);
        }
    }
    out
}

fn parse_line(line: &str) -> Option<ImportLine> {
    let mut s = line.trim().to_string();

    // Quantity: "4 ", "4x ", "4X "
    let digits: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
    let mut qty = 1;
    if !digits.is_empty() {
        let rest = &s[digits.len()..];
        let rest = rest.strip_prefix(['x', 'X']).unwrap_or(rest);
        if rest.starts_with(char::is_whitespace) {
            qty = digits.parse().unwrap_or(1);
            s = rest.trim().to_string();
        }
    }

    // Trailing markers: "*F*" foil, "*E*" etched, other "*X*" flags, "[Category]", "^Tag^".
    let mut finish = None;
    let mut kept = Vec::new();
    let mut in_bracket = false;
    let mut in_caret = false;
    for tok in s.split_whitespace() {
        if in_bracket {
            in_bracket = !tok.ends_with(']');
            continue;
        }
        if in_caret {
            in_caret = !tok.ends_with('^');
            continue;
        }
        if tok.starts_with('[') {
            in_bracket = !tok.ends_with(']');
            continue;
        }
        if tok.starts_with('^') && tok.len() > 1 {
            in_caret = !tok[1..].ends_with('^');
            continue;
        }
        if tok.len() >= 3 && tok.starts_with('*') && tok.ends_with('*') {
            match tok.to_uppercase().as_str() {
                "*F*" => finish = Some(Finish::Foil),
                "*E*" => finish = Some(Finish::Etched),
                _ => {}
            }
            continue;
        }
        kept.push(tok);
    }
    let s = kept.join(" ");

    // "Name (SET) 123": the last parenthesised group is the set, the token after it the number.
    let (name, set, number) = match (s.rfind('('), s.rfind(')')) {
        (Some(open), Some(close))
            if close > open && !s[open + 1..close].contains(' ') && close - open > 1 =>
        {
            let set = s[open + 1..close].trim().to_string();
            let after = s[close + 1..].trim();
            let number = after.split_whitespace().next().map(str::to_string);
            (s[..open].trim().to_string(), Some(set), number)
        }
        _ => (s.trim().to_string(), None, None),
    };
    if name.is_empty() {
        return None;
    }
    Some(ImportLine {
        qty: qty.max(1),
        name,
        set,
        number,
        scryfall_id: None,
        finish,
        board: String::new(),
        raw: line.to_string(),
        extras: None,
    })
}

// ---------------------------------------------------------------- resolving

fn name_matches(card: &Card, name: &str) -> bool {
    let n = name.trim().to_lowercase();
    let full = card.name.to_lowercase();
    full == n
        || full.split(" // ").any(|f| f == n)
        || card
            .card_faces
            .iter()
            .flatten()
            .any(|f| f.name.to_lowercase() == n)
}

/// "0105" -> "105". Case is kept: Scryfall's collector numbers are case-sensitive ("EMN-47").
fn norm_number(n: &str) -> String {
    printed_number_candidates(n)
        .into_iter()
        .next()
        .unwrap_or_default()
}

fn identifier(l: &ImportLine) -> Value {
    match (&l.scryfall_id, &l.set, &l.number) {
        (Some(id), _, _) => json!({ "id": id }),
        (None, Some(set), Some(num)) => {
            json!({ "set": set.to_lowercase(), "collector_number": norm_number(num) })
        }
        (None, Some(set), None) => json!({ "name": l.name, "set": set.to_lowercase() }),
        _ => json!({ "name": l.name }),
    }
}

fn matches_line(c: &Card, l: &ImportLine) -> bool {
    match (&l.scryfall_id, &l.set, &l.number) {
        (Some(id), _, _) => &c.id == id,
        (None, Some(set), Some(num)) => {
            c.set.eq_ignore_ascii_case(set)
                && c.collector_number.eq_ignore_ascii_case(&norm_number(num))
        }
        (None, Some(set), None) => c.set.eq_ignore_ascii_case(set) && name_matches(c, &l.name),
        _ => name_matches(c, &l.name),
    }
}

/// Matches lines to printings: exact identifiers first, then by name for anything left over.
pub fn resolve(
    client: &Client,
    lines: Vec<ImportLine>,
    default_finish: Finish,
) -> Result<Vec<ImportRow>, String> {
    let ids: Vec<Value> = lines.iter().map(identifier).collect();
    let cards = client.cards_by_identifiers(&ids)?;

    let mut rows: Vec<ImportRow> = lines
        .into_iter()
        .map(|line| {
            let card = cards.iter().find(|c| matches_line(c, &line)).cloned();
            let mut note = None;
            let mut include = true;
            if let Some(c) = &card {
                if !line.name.is_empty() && !name_matches(c, &line.name) {
                    // The set/number points at a different card: don't add it unless the user says so.
                    include = false;
                    note = self::note(
                        "Different card",
                        format!(
                        "The list says \"{}\" but {} #{} is {}. Unticked; tick it if that's right.",
                        line.name,
                        c.set.to_uppercase(),
                        c.collector_number,
                        c.name
                    ),
                    );
                }
            }
            ImportRow {
                include,
                qty: line.qty,
                finish: line.finish.unwrap_or(default_finish),
                card,
                line,
                note,
            }
        })
        .collect();

    // Second pass: printings that weren't found get the default printing for their name.
    let missing: Vec<usize> = (0..rows.len())
        .filter(|&i| rows[i].card.is_none() && !rows[i].line.name.is_empty())
        .collect();
    if !missing.is_empty() {
        let ids: Vec<Value> = missing
            .iter()
            .map(|&i| json!({ "name": rows[i].line.name }))
            .collect();
        let cards = client.cards_by_identifiers(&ids)?;
        for &i in &missing {
            let row = &mut rows[i];
            if let Some(c) = cards.iter().find(|c| name_matches(c, &row.line.name)) {
                if row.line.set.is_some() || row.line.scryfall_id.is_some() {
                    row.note = note(
                        "Printing not found",
                        format!(
                        "{} {} wasn't found on Scryfall, so this is the default printing. Check it.",
                        row.line.set.as_deref().unwrap_or("").to_uppercase(),
                        row.line.number.as_deref().unwrap_or("")
                        ),
                    );
                }
                row.card = Some(c.clone());
            }
        }
    }

    for row in &mut rows {
        match &row.card {
            None => {
                row.include = false;
                row.note = note(
                    "Not found",
                    "Not found on Scryfall. Use Look up to find it manually.",
                );
            }
            Some(_)
                if row.line.set.is_none()
                    && row.line.scryfall_id.is_none()
                    && row.note.is_none() =>
            {
                row.note = note(
                    "Default printing",
                    "No set in the list, so this is Scryfall's default printing. Check it.",
                );
            }
            Some(c) if !c.has_finish(row.finish) => {
                if let Some(f) = Finish::ALL.into_iter().find(|f| c.has_finish(*f)) {
                    let wanted = row.finish;
                    row.finish = f;
                    if row.note.is_none() {
                        row.note = note(
                            "Finish changed",
                            format!(
                                "This printing has no {} version, so it's set to {}.",
                                wanted.label(),
                                f.label()
                            ),
                        );
                    }
                }
            }
            _ => {}
        }
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(s: &str) -> ImportLine {
        parse_line(s).unwrap()
    }

    #[test]
    fn moxfield_style_lines() {
        let l = line("19 Swamp (UND) 91");
        assert_eq!(
            (
                l.qty,
                l.name.as_str(),
                l.set.as_deref(),
                l.number.as_deref()
            ),
            (19, "Swamp", Some("UND"), Some("91"))
        );
        let l = line("1 Thalia's Lancers (PLST) EMN-47");
        assert_eq!(
            (l.name.as_str(), l.set.as_deref(), l.number.as_deref()),
            ("Thalia's Lancers", Some("PLST"), Some("EMN-47"))
        );
        let l = line("1 Zahur, Glory's Past (DFT) 229");
        assert_eq!(l.name, "Zahur, Glory's Past");
        let l = line("1 Sol Ring (CMM) 400 *F*");
        assert_eq!(l.finish, Some(Finish::Foil));
        assert_eq!(l.number.as_deref(), Some("400"));
    }

    #[test]
    fn archidekt_and_plain_lines() {
        let l = line("2x Sol Ring (cmm) 400 *E* [Ramp,Artifacts]");
        assert_eq!(
            (l.qty, l.name.as_str(), l.set.as_deref(), l.finish),
            (2, "Sol Ring", Some("cmm"), Some(Finish::Etched))
        );
        let l = line("1x Arcane Signet (cmm) 390 [Ramp] ^Have,#37d67a^");
        assert_eq!(
            (l.name.as_str(), l.number.as_deref()),
            ("Arcane Signet", Some("390"))
        );
        let l = line("4 Lightning Bolt");
        assert_eq!(
            (l.qty, l.name.as_str(), l.set.clone()),
            (4, "Lightning Bolt", None)
        );
        let l = line("Opt");
        assert_eq!((l.qty, l.name.as_str()), (1, "Opt"));
        let l = line("1 Delver of Secrets // Insectile Aberration (ISD) 51");
        assert_eq!(l.name, "Delver of Secrets // Insectile Aberration");
    }

    #[test]
    fn sections_and_comments() {
        let text = "Commander\n1 Teysa, Orzhov Scion (RVR) 386\n\nDeck\n1 Victimize (EMA) 113\n// comment\nSideboard (1)\n1 Duress (M19) 94\nMaybeboard:\n1 Opt\n";
        let lines = parse_text(text);
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0].board, "Main deck");
        assert_eq!(lines[1].board, "Main deck");
        assert_eq!(lines[2].board, "Sideboard");
        assert_eq!(lines[3].board, "Maybeboard");
        assert!(is_side_board(&lines[2].board));
    }

    #[test]
    fn links() {
        assert_eq!(
            parse_link("https://archidekt.com/decks/7031486/buffs_by_hans"),
            Ok(DeckLink::Archidekt("7031486".into()))
        );
        assert_eq!(
            parse_link("archidekt.com/decks/123"),
            Ok(DeckLink::Archidekt("123".into()))
        );
        assert_eq!(
            parse_link("https://www.moxfield.com/decks/aB3-xY_9z?tab=stats"),
            Ok(DeckLink::Moxfield("aB3-xY_9z".into()))
        );
        assert!(parse_link("https://tappedout.net/mtg-decks/foo").is_err());
        assert!(parse_link("https://archidekt.com/folders/5").is_err());
    }

    #[test]
    fn archidekt_json() {
        let v: Value = serde_json::from_str(r#"{
            "name": "Test Deck",
            "categories": [{"name": "Ramp", "includedInDeck": true}, {"name": "Maybeboard", "includedInDeck": false}],
            "cards": [
                {"quantity": 2, "modifier": "Foil", "categories": ["Ramp"],
                 "card": {"uid": "abc", "collectorNumber": "400", "edition": {"editioncode": "cmm"}, "oracleCard": {"name": "Sol Ring"}}},
                {"quantity": 1, "modifier": "Normal", "categories": ["Maybeboard"],
                 "card": {"uid": "def", "collectorNumber": "1", "edition": {"editioncode": "lea"}, "oracleCard": {"name": "Opt"}}}
            ]}"#).unwrap();
        let (name, lines) = parse_archidekt(&v, false);
        assert_eq!(name, "Test Deck");
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].scryfall_id.as_deref(), Some("abc"));
        assert_eq!(lines[0].finish, Some(Finish::Foil));
        assert_eq!(lines[0].qty, 2);
        let (_, all) = parse_archidekt(&v, true);
        assert_eq!(all.len(), 2);
        assert_eq!(all[1].board, "Maybeboard");
    }

    #[test]
    fn sheet_roundtrip() {
        use crate::model::{Entry, Settings};
        let entries = vec![
            Entry {
                uid: 1,
                scryfall_id: "id-sheol".into(),
                name: "Sheoldred, the Apocalypse".into(),
                set_code: "DMU".into(),
                collector_number: "107".into(),
                rarity: "mythic".into(),
                finish: Finish::Foil,
                condition: Condition::LP,
                qty: 3,
                usd: Some(10.0),
                manual_price: Some(99.5),
                notes: "binder 2".into(),
                checked: true,
                treatment: "Normal".into(),
                treatment_override: "Signed".into(),
                ..Default::default()
            },
            Entry {
                uid: 2,
                scryfall_id: "id-opt".into(),
                name: "Opt".into(),
                set_code: "XLN".into(),
                collector_number: "65".into(),
                rarity: "common".into(),
                qty: 1,
                ..Default::default()
            },
        ];
        let path = std::env::temp_dir().join(format!("cc_sheet_roundtrip_{}.xlsx", std::process::id()));
        crate::export::export(&path, &entries, &Settings::default()).unwrap();

        let lines = parse_sheet(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(lines.len(), 2);
        // Export sorts by name: Opt first.
        let opt = &lines[0];
        assert_eq!((opt.scryfall_id.as_deref(), opt.qty, opt.finish), (Some("id-opt"), 1, Some(Finish::NonFoil)));
        assert_eq!(opt.extras.as_ref().unwrap().manual_price, None);
        let s = &lines[1];
        assert_eq!(s.scryfall_id.as_deref(), Some("id-sheol"));
        assert_eq!((s.qty, s.finish, s.set.as_deref(), s.number.as_deref()), (3, Some(Finish::Foil), Some("DMU"), Some("107")));
        let x = s.extras.as_ref().unwrap();
        assert_eq!(x.condition, Some(Condition::LP));
        assert_eq!(x.manual_price, Some(99.5));
        assert_eq!(x.notes, "binder 2");
        assert!(x.checked);
        assert_eq!(x.treatment, "Signed");
    }

    #[test]
    fn sheet_remembers_its_currency() {
        use crate::model::{Entry, Settings};
        let entries = vec![Entry {
            uid: 1,
            scryfall_id: "id".into(),
            name: "Opt".into(),
            qty: 1,
            manual_price: Some(4.5),
            ..Default::default()
        }];
        let settings = Settings { currency: "EUR".into(), fx_rate: 0.9, ..Default::default() };
        let path = std::env::temp_dir().join(format!("cc_sheet_eur_{}.xlsx", std::process::id()));
        crate::export::export(&path, &entries, &settings).unwrap();
        let lines = parse_sheet(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        let x = lines[0].extras.as_ref().unwrap();
        assert_eq!((x.currency.as_str(), x.usd_rate, x.manual_price), ("EUR", Some(0.9), Some(4.5)));
    }

    #[test]
    fn sheet_rejects_other_spreadsheets() {
        let path = std::env::temp_dir().join(format!("cc_not_ours_{}.xlsx", std::process::id()));
        let mut wb = rust_xlsxwriter::Workbook::new();
        wb.add_worksheet().write_string(0, 0, "Hello").unwrap();
        wb.save(&path).unwrap();
        let err = parse_sheet(&path).unwrap_err();
        let _ = std::fs::remove_file(&path);
        assert!(err.contains("doesn't look like"), "{err}");
    }

    #[test]
    fn moxfield_json() {
        let v: Value = serde_json::from_str(r#"{
            "name": "Mox Deck",
            "boards": {
                "mainboard": {"cards": {"x": {"quantity": 3, "finish": "foil", "card": {"scryfall_id": "s1", "name": "Swamp", "set": "und", "cn": "91"}}}},
                "commanders": {"cards": {"y": {"quantity": 1, "finish": "nonFoil", "card": {"scryfall_id": "s2", "name": "Teysa", "set": "rvr", "cn": "386"}}}},
                "sideboard": {"cards": {"z": {"quantity": 1, "finish": "etched", "card": {"scryfall_id": "s3", "name": "Duress", "set": "m19", "cn": "94"}}}},
                "tokens": {"cards": {"t": {"quantity": 1, "card": {"scryfall_id": "s4", "name": "Spirit"}}}}
            }}"#).unwrap();
        let (_, main) = parse_moxfield(&v, false);
        assert_eq!(main.len(), 2);
        let (_, all) = parse_moxfield(&v, true);
        assert_eq!(all.len(), 3);
        let side = all.iter().find(|l| l.name == "Duress").unwrap();
        assert_eq!(
            (side.board.as_str(), side.finish),
            ("Sideboard", Some(Finish::Etched))
        );
    }

    /// `cargo test -- --ignored live`
    #[test]
    #[ignore]
    fn live_import_text_and_archidekt() {
        let client = Client::new();
        let text = "19 Swamp (UND) 91\n1 Teysa, Orzhov Scion (RVR) 386\n1 Thalia's Lancers (PLST) EMN-47\n\
                    1 Thalisse, Reverent Medium (CMR) 291\n1 The Sibsig Ceremony (TDM) 91\n1 Victimize (EMA) 113\n\
                    1 Village Rites (GN3) 64\n1 Vindictive Vampire (RNA) 90\n1 Yahenni, Undying Partisan (LCC) 214\n\
                    1 Zahur, Glory's Past (DFT) 229\n2 Lightning Bolt\n1 Delver of Secrets\n1 Notacard Atall\n";
        let opts = ImportOptions {
            default_finish: Finish::NonFoil,
            include_side: false,
        };
        let r = run(&client, ImportSource::Text(text.into()), opts).unwrap();
        for row in &r.rows {
            println!(
                "{:>2} {:<32} -> {:?} {:?}",
                row.qty,
                row.line.name,
                row.card
                    .as_ref()
                    .map(|c| format!("{} {} #{}", c.name, c.set, c.collector_number)),
                row.note
            );
        }
        assert_eq!(r.rows.len(), 13);
        assert_eq!(r.rows.iter().filter(|r| r.card.is_some()).count(), 12);
        let lancers = &r.rows[2];
        assert_eq!(lancers.card.as_ref().unwrap().collector_number, "EMN-47");
        assert!(r.rows[12].card.is_none() && !r.rows[12].include);

        let deck = run(
            &client,
            ImportSource::Link("https://archidekt.com/decks/7031486/buffs_by_hans".into()),
            opts,
        )
        .unwrap();
        println!("{}: {} rows", deck.source, deck.rows.len());
        assert!(deck.rows.len() > 50);
        assert!(deck.rows.iter().all(|r| r.card.is_some()));

        let mox = run(
            &client,
            ImportSource::Link("https://moxfield.com/decks/abc123".into()),
            opts,
        );
        println!("moxfield: {:?}", mox.as_ref().err());
    }
}
