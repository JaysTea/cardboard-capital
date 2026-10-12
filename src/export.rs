//! Writes the collection as a formatted .xlsx that opens cleanly in Excel,
//! Google Sheets and LibreOffice.

use std::collections::BTreeMap;
use std::path::Path;

use rust_xlsxwriter::{
    Color, ConditionalFormatFormula, DataValidation, Format, FormatAlign, FormatBorder, Formula,
    Url, Workbook, Worksheet, XlsxError,
};

use crate::model::{rarity_label, Condition, Entry, Finish, Settings};

/// Summary!B4 holds the USD->SGD rate; every S$ formula points at it.
const FX_CELL: &str = "Summary!$B$4";

const HEADERS: [(&str, f64); 22] = [
    ("Name", 32.0),
    ("Set", 26.0),
    ("Set Code", 9.0),
    ("#", 7.0),
    ("Rarity", 10.0),
    ("Finish", 10.0),
    ("Treatment", 24.0),
    ("Condition", 10.0),
    ("Colors", 9.0),
    ("Type", 30.0),
    ("Qty", 6.0),
    ("USD (Scryfall)", 13.0),
    ("Manual S$", 11.0),
    ("Unit S$", 11.0),
    ("Total S$", 12.0),
    ("Price Source", 12.0),
    ("Checked", 9.0),
    ("Price Updated", 16.0),
    ("Scryfall", 10.0),
    ("TCGplayer", 11.0),
    ("Notes", 30.0),
    ("Scryfall ID", 38.0),
];

// Column indexes
const C_RARITY: u16 = 4;
const C_FINISH: u16 = 5;
const C_COND: u16 = 7;
const C_QTY: u16 = 10;
const C_USD: u16 = 11;
const C_MANUAL: u16 = 12;
const C_UNIT: u16 = 13;
const C_TOTAL: u16 = 14;
const C_CHECKED: u16 = 16;
const C_NOTES: u16 = 20;
const C_ID: u16 = 21;

const USD_FMT: &str = "\"$\"#,##0.00";
const SGD_FMT: &str = "\"S$\"#,##0.00";

pub fn export(path: &Path, entries: &[Entry], settings: &Settings) -> Result<(), String> {
    build(path, entries, settings).map_err(|e| match e {
        XlsxError::IoError(io) => format!(
            "Couldn't write {}: {io}. If the sheet is open in Excel/LibreOffice, close it and export again.",
            path.display()
        ),
        other => format!("Export failed: {other}"),
    })
}

fn rarity_colors(rarity: &str) -> (u32, u32) {
    match rarity {
        "common" => (0xE5E7EB, 0x374151),
        "uncommon" => (0xD6E4F0, 0x1E3A5F),
        "rare" => (0xF6E7B0, 0x6B4E00),
        "mythic" => (0xF9C9A8, 0x8A2E00),
        _ => (0xE9D5FF, 0x5B21B6),
    }
}

fn base() -> Format {
    Format::new()
        .set_align(FormatAlign::VerticalCenter)
        .set_border_bottom(FormatBorder::Thin)
        .set_border_bottom_color(Color::RGB(0xE2E8F0))
}

fn build(path: &Path, entries: &[Entry], settings: &Settings) -> Result<(), XlsxError> {
    let rate = settings.rate();
    let mut workbook = Workbook::new();
    let mut ws = Worksheet::new();
    ws.set_name("Collection")?;

    let header = Format::new()
        .set_bold()
        .set_font_color(Color::White)
        .set_background_color(Color::RGB(0x1F2937))
        .set_align(FormatAlign::VerticalCenter)
        .set_text_wrap()
        .set_border(FormatBorder::Thin)
        .set_border_color(Color::RGB(0x374151));
    let text = base();
    let name_fmt = base().set_bold();
    let center = base().set_align(FormatAlign::Center);
    let usd = base().set_num_format(USD_FMT);
    let sgd = base().set_num_format(SGD_FMT);
    let sgd_bold = base().set_num_format(SGD_FMT).set_bold();
    let manual = base()
        .set_num_format(SGD_FMT)
        .set_font_color(Color::RGB(0x1D4ED8));
    let link = base()
        .set_font_color(Color::RGB(0x2563EB))
        .set_underline(rust_xlsxwriter::FormatUnderline::Single);
    let muted = base().set_font_color(Color::RGB(0x6B7280));

    for (col, (title, width)) in HEADERS.iter().enumerate() {
        ws.write_string_with_format(0, col as u16, *title, &header)?;
        ws.set_column_width(col as u16, *width)?;
    }
    ws.set_row_height(0, 24)?;
    ws.set_column_hidden(C_ID)?;

    let mut sorted: Vec<&Entry> = entries.iter().collect();
    sorted.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then(a.set_code.cmp(&b.set_code))
            .then(a.collector_number.cmp(&b.collector_number))
    });

    for (i, e) in sorted.iter().enumerate() {
        let r = (i + 1) as u32;
        let n = r + 1; // 1-based row number for formulas
        let (fill, font) = rarity_colors(&e.rarity);
        let rarity_fmt = base()
            .set_bold()
            .set_align(FormatAlign::Center)
            .set_background_color(Color::RGB(fill))
            .set_font_color(Color::RGB(font));

        ws.write_string_with_format(r, 0, &e.name, &name_fmt)?;
        ws.write_string_with_format(r, 1, &e.set_name, &text)?;
        ws.write_string_with_format(r, 2, &e.set_code, &center)?;
        ws.write_string_with_format(r, 3, &e.collector_number, &center)?;
        ws.write_string_with_format(r, C_RARITY, rarity_label(&e.rarity), &rarity_fmt)?;
        ws.write_string_with_format(r, C_FINISH, e.finish.label(), &center)?;
        ws.write_string_with_format(r, 6, e.treatment_display(), &text)?;
        ws.write_string_with_format(r, C_COND, e.condition.label(), &center)?;
        ws.write_string_with_format(r, 8, &e.colors, &center)?;
        ws.write_string_with_format(r, 9, &e.type_line, &muted)?;
        ws.write_number_with_format(r, C_QTY, e.qty as f64, &center)?;
        match e.usd {
            Some(v) => ws.write_number_with_format(r, C_USD, v, &usd)?,
            None => ws.write_blank(r, C_USD, &usd)?,
        };
        match e.manual_sgd {
            Some(v) => ws.write_number_with_format(r, C_MANUAL, v, &manual)?,
            None => ws.write_blank(r, C_MANUAL, &manual)?,
        };

        let unit = e.unit_sgd(rate);
        let unit_formula = Formula::new(format!(
            "=IF(M{n}<>\"\",M{n},IF(L{n}<>\"\",L{n}*{FX_CELL},\"\"))"
        ))
        .set_result(unit.map(|u| format!("{u}")).unwrap_or_default());
        ws.write_formula_with_format(r, C_UNIT, unit_formula, &sgd)?;
        let total_formula = Formula::new(format!("=IF(N{n}=\"\",\"\",N{n}*K{n})")).set_result(
            e.total_sgd(rate)
                .map(|t| format!("{t}"))
                .unwrap_or_default(),
        );
        ws.write_formula_with_format(r, C_TOTAL, total_formula, &sgd_bold)?;
        let source_formula = Formula::new(format!(
            "=IF(M{n}<>\"\",\"Manual\",IF(L{n}<>\"\",\"Scryfall\",\"No price\"))"
        ))
        .set_result(e.price_source());
        ws.write_formula_with_format(r, 15, source_formula, &center)?;

        ws.write_string_with_format(r, C_CHECKED, if e.checked { "Yes" } else { "No" }, &center)?;
        ws.write_string_with_format(r, 17, &e.price_updated, &muted)?;
        if e.scryfall_uri.is_empty() {
            ws.write_blank(r, 18, &link)?;
        } else {
            ws.write_url_with_format(
                r,
                18,
                Url::new(e.scryfall_uri.as_str()).set_text("Scryfall"),
                &link,
            )?;
        }
        if e.tcgplayer_uri.is_empty() {
            ws.write_blank(r, 19, &link)?;
        } else {
            ws.write_url_with_format(
                r,
                19,
                Url::new(e.tcgplayer_uri.as_str()).set_text("TCGplayer"),
                &link,
            )?;
        }
        ws.write_string_with_format(r, C_NOTES, &e.notes, &text)?;
        ws.write_string_with_format(r, C_ID, &e.scryfall_id, &muted)?;
        ws.set_row_height(r, 20)?;
    }

    let last = (sorted.len() as u32).max(1);
    let last_visible = C_NOTES;
    ws.set_freeze_panes(1, 1)?;
    ws.autofilter(0, 0, sorted.len() as u32, C_ID)?;

    // Row shading via conditional formats so it survives sorting/filtering.
    // Rarity column keeps its own colour, so it's excluded.
    let high = Format::new()
        .set_background_color(Color::RGB(0xDCFCE7))
        .set_bold();
    let foil = Format::new().set_background_color(Color::RGB(0xF3ECFF));
    let band = Format::new().set_background_color(Color::RGB(0xF7F9FC));
    let rules = [
        (
            format!("=AND(ISNUMBER($O2),$O2>={})", settings.highlight_sgd),
            &high,
        ),
        ("=$F2<>\"Non-foil\"".to_string(), &foil),
        ("=MOD(ROW(),2)=0".to_string(), &band),
    ];
    for (rule, fmt) in &rules {
        for (c0, c1) in [(0, C_RARITY - 1), (C_RARITY + 1, last_visible)] {
            let cf = ConditionalFormatFormula::new()
                .set_rule(rule.as_str())
                .set_format(*fmt);
            ws.add_conditional_format(1, c0, last, c1, &cf)?;
        }
    }

    // Dropdowns for the columns that have a fixed set of values.
    let yes_no = DataValidation::new().allow_list_strings(&["Yes", "No"])?;
    ws.add_data_validation(1, C_CHECKED, last + 500, C_CHECKED, &yes_no)?;
    let conds: Vec<&str> = Condition::ALL.iter().map(|c| c.label()).collect();
    let cond_dv = DataValidation::new().allow_list_strings(&conds)?;
    ws.add_data_validation(1, C_COND, last + 500, C_COND, &cond_dv)?;
    let finishes: Vec<&str> = Finish::ALL.iter().map(|f| f.label()).collect();
    let finish_dv = DataValidation::new().allow_list_strings(&finishes)?;
    ws.add_data_validation(1, C_FINISH, last + 500, C_FINISH, &finish_dv)?;

    workbook.push_worksheet(ws);
    workbook.push_worksheet(summary_sheet(&sorted, settings)?);
    workbook.save(path)?;
    Ok(())
}

fn summary_sheet(entries: &[&Entry], settings: &Settings) -> Result<Worksheet, XlsxError> {
    let rate = settings.rate();
    let n = entries.len().max(1) + 1; // last data row (1-based) on Collection
    let mut ws = Worksheet::new();
    ws.set_name("Summary")?;
    ws.set_column_width(0, 30)?;
    ws.set_column_width(1, 16)?;
    ws.set_column_width(2, 16)?;
    ws.set_column_width(3, 40)?;

    let title = Format::new()
        .set_bold()
        .set_font_size(18)
        .set_font_color(Color::RGB(0x1F2937));
    let label = Format::new().set_bold();
    let note = Format::new()
        .set_italic()
        .set_font_color(Color::RGB(0x6B7280));
    let rate_fmt = Format::new()
        .set_num_format("0.0000")
        .set_bold()
        .set_background_color(Color::RGB(0xFEF3C7))
        .set_border(FormatBorder::Thin);
    let int = Format::new().set_num_format("#,##0");
    let sgd = Format::new().set_num_format(SGD_FMT);
    let sgd_big = Format::new()
        .set_num_format(SGD_FMT)
        .set_bold()
        .set_font_size(14);
    let usd = Format::new().set_num_format(USD_FMT);
    let head = Format::new()
        .set_bold()
        .set_font_color(Color::White)
        .set_background_color(Color::RGB(0x1F2937));

    ws.write_string_with_format(0, 0, "Cardboard Capital — Summary", &title)?;
    ws.write_string_with_format(
        1,
        0,
        format!("Exported {}", crate::model::now_string()),
        &note,
    )?;

    ws.write_string_with_format(3, 0, "USD → SGD rate", &label)?;
    ws.write_number_with_format(3, 1, rate, &rate_fmt)?;
    let src = if settings.fx_manual {
        "Manual rate".to_string()
    } else {
        format!("{} · {}", settings.fx_source, settings.fx_updated)
    };
    ws.write_string_with_format(3, 2, src, &note)?;
    ws.write_string_with_format(4, 0, "Change B4 to reprice every S$ cell.", &note)?;

    let total_qty: u32 = entries.iter().map(|e| e.qty).sum();
    let total_sgd: f64 = entries.iter().filter_map(|e| e.total_sgd(rate)).sum();
    let total_usd: f64 = entries
        .iter()
        .filter_map(|e| e.usd.map(|u| u * e.qty as f64))
        .sum();
    let missing = entries
        .iter()
        .filter(|e| e.unit_sgd(rate).is_none())
        .count();

    let rows: [(&str, String, String, &Format); 5] = [
        (
            "Unique entries",
            format!("=COUNTA(Collection!A2:A{n})"),
            entries.len().to_string(),
            &int,
        ),
        (
            "Total cards",
            format!("=SUM(Collection!K2:K{n})"),
            total_qty.to_string(),
            &int,
        ),
        (
            "Total value (S$)",
            format!("=SUM(Collection!O2:O{n})"),
            total_sgd.to_string(),
            &sgd_big,
        ),
        (
            "Scryfall value (US$)",
            format!("=SUMPRODUCT(Collection!K2:K{n},Collection!L2:L{n})"),
            total_usd.to_string(),
            &usd,
        ),
        (
            "Entries without a price",
            format!("=COUNTIF(Collection!P2:P{n},\"No price\")"),
            missing.to_string(),
            &int,
        ),
    ];
    for (i, (name, formula, result, fmt)) in rows.iter().enumerate() {
        let r = 6 + i as u32;
        ws.write_string_with_format(r, 0, *name, &label)?;
        ws.write_formula_with_format(
            r,
            1,
            Formula::new(formula.as_str()).set_result(result.as_str()),
            *fmt,
        )?;
    }

    // Breakdown tables
    let mut row = 13u32;
    let mut section = |ws: &mut Worksheet,
                       heading: &str,
                       col_letter: &str,
                       groups: Vec<(String, u32, f64)>|
     -> Result<(), XlsxError> {
        ws.write_string_with_format(row, 0, heading, &head)?;
        ws.write_string_with_format(row, 1, "Cards", &head)?;
        ws.write_string_with_format(row, 2, "Value S$", &head)?;
        row += 1;
        for (key, qty, value) in groups {
            ws.write_string(row, 0, &key)?;
            let crit = format!("Collection!{col_letter}2:{col_letter}{n}");
            ws.write_formula_with_format(
                row,
                1,
                Formula::new(format!("=SUMIF({crit},A{},Collection!K2:K{n})", row + 1))
                    .set_result(qty.to_string()),
                &int,
            )?;
            ws.write_formula_with_format(
                row,
                2,
                Formula::new(format!("=SUMIF({crit},A{},Collection!O2:O{n})", row + 1))
                    .set_result(value.to_string()),
                &sgd,
            )?;
            row += 1;
        }
        row += 1;
        Ok(())
    };

    let group = |key_of: &dyn Fn(&Entry) -> String| -> Vec<(String, u32, f64)> {
        let mut map: BTreeMap<String, (u32, f64)> = BTreeMap::new();
        for e in entries {
            let slot = map.entry(key_of(e)).or_default();
            slot.0 += e.qty;
            slot.1 += e.total_sgd(rate).unwrap_or(0.0);
        }
        let mut v: Vec<_> = map.into_iter().map(|(k, (q, s))| (k, q, s)).collect();
        v.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
        v
    };

    section(
        &mut ws,
        "By rarity",
        "E",
        group(&|e| rarity_label(&e.rarity)),
    )?;
    section(
        &mut ws,
        "By finish",
        "F",
        group(&|e| e.finish.label().to_string()),
    )?;
    section(
        &mut ws,
        "By condition",
        "H",
        group(&|e| e.condition.label().to_string()),
    )?;
    section(&mut ws, "By set", "B", group(&|e| e.set_name.clone()))?;

    Ok(ws)
}

#[cfg(test)]
mod tests {
    use super::*;
    use calamine::{open_workbook, Data, Reader, Xlsx};

    /// Builds a small real collection from Scryfall into $MTG_SEED_DIR (collection.json + xlsx).
    /// `MTG_SEED_DIR=... cargo test -- --ignored seed_demo`
    #[test]
    #[ignore]
    fn seed_demo() {
        use crate::model::{add_or_merge, Condition};
        use crate::scryfall::Client;
        let dir =
            std::path::PathBuf::from(std::env::var("MTG_SEED_DIR").expect("set MTG_SEED_DIR"));
        let client = Client::new();
        let picks = [
            ("dmu", "107", Finish::NonFoil, Condition::NM, 2),
            ("dmu", "435", Finish::Foil, Condition::NM, 1),
            ("2xm", "129", Finish::NonFoil, Condition::LP, 3),
            ("unf", "235", Finish::Foil, Condition::NM, 4),
            ("mh3", "1", Finish::NonFoil, Condition::NM, 1),
            ("isd", "51", Finish::NonFoil, Condition::MP, 1),
        ];
        let mut entries = Vec::new();
        for (i, (set, num, finish, cond, qty)) in picks.into_iter().enumerate() {
            let card = client.card_by_set_number(set, num).unwrap();
            let mut e = Entry::from_card(&card, finish, cond, qty);
            e.uid = i as u64 + 1;
            if i == 3 {
                e.manual_sgd = Some(6.5);
            }
            add_or_merge(&mut entries, e);
        }
        crate::storage::save_collection(&dir, &entries).unwrap();
        let settings = Settings {
            fx_rate: 1.2804,
            ..Default::default()
        };
        export(&dir.join("Cardboard Capital.xlsx"), &entries, &settings).unwrap();
    }

    #[test]
    fn export_roundtrip() {
        let entries = vec![
            Entry {
                uid: 1,
                scryfall_id: "id-1".into(),
                name: "Sheoldred, the Apocalypse".into(),
                set_code: "DMU".into(),
                set_name: "Dominaria United".into(),
                collector_number: "107".into(),
                rarity: "mythic".into(),
                finish: Finish::Foil,
                qty: 2,
                usd: Some(10.0),
                scryfall_uri: "https://scryfall.com/card/dmu/107".into(),
                ..Default::default()
            },
            Entry {
                uid: 2,
                scryfall_id: "id-2".into(),
                name: "Opt".into(),
                rarity: "common".into(),
                qty: 4,
                manual_sgd: Some(0.5),
                ..Default::default()
            },
        ];
        let settings = Settings {
            fx_rate: 1.3,
            ..Default::default()
        };
        let path =
            std::env::temp_dir().join(format!("mtg_export_test_{}.xlsx", std::process::id()));
        export(&path, &entries, &settings).unwrap();

        let mut wb: Xlsx<_> = open_workbook(&path).unwrap();
        let range = wb.worksheet_range("Collection").unwrap();
        assert_eq!(range.get_value((0, 0)), Some(&Data::String("Name".into())));
        // Sorted by name: Opt first.
        assert_eq!(range.get_value((1, 0)), Some(&Data::String("Opt".into())));
        assert_eq!(
            range.get_value((2, 4)),
            Some(&Data::String("Mythic".into()))
        );
        assert_eq!(range.get_value((2, C_QTY as u32)), Some(&Data::Float(2.0)));

        let formulas = wb.worksheet_formula("Collection").unwrap();
        let unit = formulas.get_value((2, C_UNIT as u32)).unwrap();
        assert!(unit.contains("Summary!$B$4"), "{unit}");

        let summary = wb.worksheet_range("Summary").unwrap();
        assert_eq!(summary.get_value((3, 1)), Some(&Data::Float(1.3)));
        let _ = std::fs::remove_file(&path);
    }
}
