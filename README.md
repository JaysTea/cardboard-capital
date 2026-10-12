# Cardboard Capital

**Know what your cardboard is worth, on paper.**

Like a stock portfolio, you don't actually have the money; you just know that's where it's at. Cardboard Capital tracks your trading card collection, checks each card against live market prices, converts them to your currency (SGD, USD, EUR, GBP, JPY and 21 more), and keeps a tidy spreadsheet you can open in Google Sheets, Excel or LibreOffice.

It currently supports **Magic: The Gathering** (card data and prices from [Scryfall](https://scryfall.com)). More TCGs are planned.

## Download

| | |
|---|---|
| ⭐ **Stable** (recommended) | [**Latest release**](https://github.com/JaysTea/cardboard-capital/releases/latest) |
| 🧪 **Beta** (new features being tested) | [All releases](https://github.com/JaysTea/cardboard-capital/releases): betas are marked *Pre-release* |
| 📦 **Older versions** | [All releases](https://github.com/JaysTea/cardboard-capital/releases): scroll down; every past version stays available |

The Releases page always lists the newest version first. Download **`Cardboard-Capital.exe`** from the release's *Assets*.

### Getting started
1. Make a folder for the app (e.g. `Documents\Cardboard Capital`) and put `Cardboard-Capital.exe` in it.
2. Double-click it. On first run Windows may say **"Windows protected your PC"**. This happens because the app isn't code-signed by a big publisher. Click **More info → Run anyway**.
3. The app creates two folders next to itself:

```
Cardboard Capital\
  Cardboard-Capital.exe
  data\       your collection, settings and image cache
  exports\    Cardboard Capital.xlsx (your spreadsheet, kept up to date automatically)
```

To move the app or back it up, copy the whole folder. **Back up `data\`**: it holds your collection.

**Upgrading from MTG Stock Sheet** (the old name): put the new exe in your old folder and run it. Your collection, settings and spreadsheet are moved into `data\` and `exports\` automatically.

Needs Windows 10/11 (64-bit) and an internet connection for prices and images.

## Using it

### Adding cards
1. Type the **card name** (autocomplete helps; typos are fuzzy-matched). Or switch to **By set + number** and copy the card's bottom-left corner: the number (`0105`, `M 0105` or `105/281`) and the set code under it (`SOS`).
2. Choose **Finish** (Non-foil / Foil / Etched), **Condition** and **Quantity**. In name lookups, **Rarity** narrows down cards with many printings.
3. Press **Enter**. The best-matching printing appears with its image, set, number, rarity, treatment and price. Warnings show if something doesn't add up.
4. Wrong version? Click **Not this one? Show printings** and pick the right one from the image grid.
5. Click **Add to collection** or press **Ctrl+Enter**. Adding a printing you already have, in the same finish and condition, just increases its quantity.

**Fast entry, no mouse:** type → **Enter** → glance at the card → **Ctrl+Enter** → type the next one. The cursor stays in the box, and in set + number mode the set code is kept between cards.

**Full art, borderless, showcase…** are detected automatically from the printing you pick. If a label is ever wrong, set your own in the **Treatment** field, or right-click the card in the Collection tab.

### Importing lists and decks (📥 Import tab)
- **From a link:** paste a public **Archidekt** deck link. Every card comes in as its exact printing and finish.
- **Moxfield** doesn't let apps read its decks. On the Moxfield deck page use **Export → Copy for Moxfield** and paste it into **From text**.
- **From text:** one card per line, as exported by Moxfield, Archidekt or MTG Arena:
  ```
  19 Swamp (UND) 91
  1 Thalia's Lancers (PLST) EMN-47
  1x Sol Ring (cmm) 410 *F* [Ramp]
  4 Lightning Bolt
  ```
  `*F*` = foil and `*E*` = etched. The set and number are optional; without them you get Scryfall's default printing.

You review everything before it's added: untick rows, change quantity or finish, and hover a name to see the card. Rows worth a second look are flagged:
- *Different card*: the set and number point at another card. These rows are unticked.
- *Default printing*: the line had no set.
- *Not found*: use **Look up** to find it in the Add card tab.

### Moving a collection between installs
Use **📥 Import → From a spreadsheet** and pick a spreadsheet exported by Cardboard Capital (or the older MTG Stock Sheet), e.g. from a friend or another PC. Quantity, finish, condition, manual prices, notes and checked marks all come across, and prices are refreshed. Importing the same sheet twice adds the quantities twice.

### Updates
The app checks GitHub for new versions when it starts. Pick your channel in **Settings → Updates**:
- **Stable** (default): tested releases only.
- **Beta**: also get betas, with new features early.
- **Off**: never check; stay on your version.

When an update is available, a bar appears along the bottom of the window with an **Update** button at the right. **Install & restart** swaps in the new version; your `data` and `exports` are left alone. Switching from Beta back to Stable doesn't downgrade you: you move to the next stable release once it's newer. For an older version, download it from [Releases](https://github.com/JaysTea/cardboard-capital/releases).

### Prices
- Prices are Scryfall's **USD (TCGplayer market)** price for each card's finish, converted to **your currency** with a **live exchange rate** (Frankfurter/ECB, falling back to ExchangeRate-API). Pick the currency in **Settings → Currency**; switching converts your manual prices too. A manual rate can also be set there.
- If a price looks wrong, type your own in the **Manual** column in the Collection tab. Tick ✔ once you've cross-checked a row.
- **Refresh prices** re-fetches everything.

### The spreadsheet
`exports\Cardboard Capital.xlsx` is rewritten after every change. On first run you're asked where it should live; change it any time in **Settings → Spreadsheet**. **Save copy as…** in the Collection tab saves a one-off copy elsewhere.

- **Collection sheet:** filters on every column; colour-coded rarities; foil rows tinted; valuable rows highlighted; Scryfall and TCGplayer links; dropdowns for Condition, Finish and Checked.
- **Summary sheet:** the exchange rate in `B4`, plus totals and breakdowns by rarity, finish, condition and set.

The price columns (labelled with your currency, e.g. "Unit (SGD)") are formulas, so changing `Summary!B4` or a Manual value reprices the whole sheet. In Google Sheets, use *File → Import → Upload*.

> The sheet is regenerated from the app's data, so edits made in the spreadsheet aren't read back. Make lasting changes in the app.

### Command line
```
Cardboard-Capital.exe "Lightning Bolt" --printings   # start with a lookup, printings grid open
Cardboard-Capital.exe --collection                   # open on the Collection tab
Cardboard-Capital.exe --import <deck link | list.txt | sheet.xlsx>  # open Import and read it
Cardboard-Capital.exe --pretend-version 1.1.0        # act as an older version (to test updates)
Cardboard-Capital.exe --pretend-version 1.0.0 --install-update-now  # test: install whatever update is found
```

## Versioning

[Semantic Versioning](https://semver.org): **MAJOR.MINOR.PATCH**, shown in the title bar, the header and Settings → About.

| Change | Bump | Example |
|---|---|---|
| Bug fix or small UI tweak | PATCH | 1.1.0 → 1.1.1 |
| New feature | MINOR | 1.1.3 → 1.2.0 |
| Big overhaul or incompatible data change | MAJOR | 1.4.2 → 2.0.0 |
| Beta of an upcoming version | pre-release suffix | 1.2.0-beta.1 → 1.2.0-beta.2 → 1.2.0 |

**History**
- **v1.2.0:** stable release of everything in the 1.2.0 betas: in-app updates with Stable / Beta / Off channels, choose your currency (26 supported), import another install's spreadsheet, choose the spreadsheet folder, Save copy as….
- v1.2.0-beta.3: the update button moved to a bar at the bottom right (it overlapped the Settings tab).
- v1.2.0-beta.2: choose your currency (26 supported); manual prices and imported sheets are converted automatically.
- v1.2.0-beta.1: in-app update checks with Stable / Beta / Off channels; import another install's spreadsheet; choose the spreadsheet folder; Save copy as….
- **v1.1.0:** first public release as Cardboard Capital. Includes the tidy `data\`/`exports\` folder layout, import from Archidekt links and pasted lists, and fast keyboard entry.
- v1.0.x: private builds as "MTG Stock Sheet".

## Development

Requires Rust (stable, MSVC toolchain) and the Visual Studio C++ Build Tools.

```
cargo test                      # unit tests
cargo test -- --ignored live    # live Scryfall / Archidekt / exchange-rate checks
cargo build --release           # target/release/cardboard_capital.exe
```

### Branches and releases
- `main` = stable. `beta` = features being tested.
- Releases are built by GitHub Actions ([`.github/workflows/release.yml`](.github/workflows/release.yml)) when a version tag is pushed. The workflow checks that the tag matches `Cargo.toml`, runs the tests, builds the exe and publishes the release.
  - A tag with a suffix (`v1.2.0-beta.1`) becomes a **pre-release**.
  - A plain tag (`v1.2.0`) becomes the **latest stable** release.

**Ship a beta** (on `beta`): set `version = "1.2.0-beta.1"` in `Cargo.toml`, commit, then:
```
git tag v1.2.0-beta.1
git push origin beta v1.2.0-beta.1
```

**Promote to stable**: merge `beta` into `main`, set `version = "1.2.0"`, commit, then:
```
git tag v1.2.0
git push origin main v1.2.0
```

### Code layout
| File | Purpose |
|---|---|
| `src/scryfall.rs` | API client (rate-limited), card model, treatment detection, price per finish |
| `src/import.rs` | Deck link (Archidekt/Moxfield) and text list parsing, Scryfall matching |
| `src/fx.rs` | USD → your currency exchange rate |
| `src/model.rs` | Collection entries, merging, settings |
| `src/storage.rs` | Folder layout, migration, JSON persistence |
| `src/update.rs` | GitHub release checks per channel, self-update |
| `src/export.rs` | XLSX writer |
| `src/worker.rs` | Background threads for API calls and image downloads |
| `src/ui/` | egui screens: `add.rs`, `import.rs`, `collection.rs`, `settings.rs`, plus shared `widgets.rs` |

## License

[MIT](LICENSE). Card data and images © Scryfall / Wizards of the Coast. Cardboard Capital isn't affiliated with Wizards of the Coast or Scryfall.
