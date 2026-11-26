# Calculator & Converter Plugin (`com.aios.calculator`)

**Calculator & Converter** is a Pure Rust first-party AIOS plugin (`crates/aios-plugins`) that runs **100% offline** with zero network requests. It combines arithmetic and scientific math evaluation, programmer base conversions and bitwise operators, unit and CSS conversions, date and timezone math, offline currency conversion, and persistent SQLite calculation history.

---

## 1. Trigger Modes

Calculator supports three entry modes so it stays fast without interfering with normal application search:

1. **Zero-Prefix Live Inline (Root Search)**
   Type an expression directly in Root Search (for example `10*30`, `20% of 450`, `0xff to dec`, `100 cm to in`, `100 usd to idr`, `today + 14d`, or `now in tokyo`).
   - The host runs a **sub-microsecond fast-gate** so standard app queries (`firefox`, `code`, `settings`) are skipped immediately without invoking the parser.
   - Bare numbers without operators (such as `42` or `2026`) intentionally do not trigger an inline card so they do not clutter application search.
2. **Exclusive `=` Prefix (Root Search)**
   Prefix any query with `=` (for example `=10*30`, `=pi`, `=42`) to hide other Root Search sections and focus exclusively on Calculator results.
3. **Interactive View (`Calculator & Converter`)**
   - Press `Ctrl+Enter` on an inline Root Search result to open the full Calculator view pre-filled with the active expression.
   - Or search for `Calculator` / `calc` in Root Search (or switch the filter dropdown to `Plugins`) and press `Enter`.
   - Renders a `List + Split Detail` layout with live per-keystroke evaluation, structured breakdown metadata in the right pane, and **Recent Calculations**.

---

## 2. Features & Syntax Reference

### 2.1 Arithmetic & Scientific Math

| Category                           | Syntax / Symbols                                 | Examples                               | Results            |
| ---------------------------------- | ------------------------------------------------ | -------------------------------------- | ------------------ |
| Basic operations                   | `+`, `-` (`−`), `*` (`×`), `/` (`÷`)             | `(45 + 5) * 2 / 4`                     | `25`               |
| Exponentiation (right-associative) | `^`, `**`                                        | `2 ^ 3 ^ 2`, `2 ** 10`                 | `512`, `1024`      |
| Modulo & trailing percent          | `a % b` (modulo), `n%` (`n / 100`)               | `17 % 5`, `50%`                        | `2`, `0.5`         |
| Factorial                          | `n!` (integers `0..=170`)                        | `5!`, `0!`                             | `120`, `1`         |
| Grouping & implicit multiply       | `(...)`, `[...]`, `2(3+4)`, `2pi`                | `2(3 + 4)`, `3sqrt(9)`                 | `14`, `9`          |
| Number formatting                  | `_` separators, thousand `,`, scientific `e`/`E` | `1_000 + 2,500`, `2.5e3`               | `3500`, `2500`     |
| Percentage phrases                 | `X% of Y`, `Y + X%`, `Y - X%`                    | `20% of 450`, `100 + 15%`, `200 - 10%` | `90`, `115`, `180` |

**Built-in constants:**

- `pi` or `π` (`3.141592653589793`)
- `e` (`2.718281828459045`)
- `tau` (`6.283185307179586`)
- `phi` (golden ratio, `1.618033988749895`)

**Built-in math functions (trigonometric functions use radians):**

- Roots & magnitude: `sqrt(x)`, `cbrt(x)`, `abs(x)`
- Trigonometry: `sin(x)`, `cos(x)`, `tan(x)`, `asin(x)`, `acos(x)`, `atan(x)`
- Logarithms & exponentials: `ln(x)`, `log(x)` / `log10(x)`, `log2(x)`, `exp(x)`
- Rounding: `floor(x)`, `ceil(x)`, `round(x)`
- Multi-argument: `pow(base, exp)`, `min(a, b, ...)`, `max(a, b, ...)`

**Additional behavior:**

- Small floating-point noise is cleaned automatically (for example `0.1 + 0.2` formats as `0.3` instead of `0.30000000000000004`).
- For non-negative integer results, the Split Detail pane automatically displays `Hex`, `Binary`, and `Octal` breakdowns.

---

### 2.2 Programmer Bases & Bitwise Operations

Supported conversion connectors across all converters: `to`, `in`, `->`, `=>`.

| Feature                | Syntax                                                          | Examples                       | Results                                      |
| ---------------------- | --------------------------------------------------------------- | ------------------------------ | -------------------------------------------- |
| Base literals          | `0x`/`0X` (hex), `0b`/`0B` (bin), `0o`/`0O` (oct), supports `_` | `0xff`, `0b1010_0101`, `0o755` | `255`, `165`, `493`                          |
| Convert to Hex         | `<expr> to hex` (`hexadecimal`, `base16`)                       | `255 to hex`                   | `0xFF`                                       |
| Convert to Binary      | `<expr> to bin` (`binary`, `base2`)                             | `42 in bin`                    | `0b101010`                                   |
| Convert to Octal       | `<expr> to oct` (`octal`, `base8`)                              | `493 -> oct`                   | `0o755`                                      |
| Convert to Decimal     | `<expr> to dec` (`decimal`, `base10`)                           | `0xff to dec`                  | `255`                                        |
| Cross-base expressions | Mix literals and operators                                      | `(0x10 + 0b1010) to hex`       | `0x1A`                                       |
| Bitwise operators      | `&` (AND), `                                                    | `(OR),`xor`(XOR),`<<`, `>>`    | `0xff & 0x0f`, `1 << 8`, `0b1100 xor 0b1010` | `15`, `256`, `6` |

---

### 2.3 Unit & CSS Converter

Format: `<amount> <source_unit> <to|in|->|=> <target_unit>`.
The amount and unit may be space-separated (`100 cm to in`), attached (`24px to rem`, `72f to c`, `1.5gib to mb`), or an evaluated expression (`(10 + 5) kg to lb`).

| Dimension           | Supported Units                                                                                                                                                                     | Examples                                                                             |
| ------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------ |
| **Length**          | `mm`, `cm`, `m` (`meter`/`metre`), `km`, `in` (`inch`), `ft` (`foot`/`feet`), `yd` (`yard`), `mi` (`mile`), `nmi`                                                                   | `100 cm to in` -> `39.370079 in`                                                     |
| **Weight / Mass**   | `mg`, `g`, `kg`, `t` (`tonne`), `oz` (`ounce`), `lb`/`lbs` (`pound`)                                                                                                                | `70 kg to lb` -> `154.323584 lb`                                                     |
| **Temperature**     | `c`/`°c`/`celsius`, `f`/`°f`/`fahrenheit`, `k`/`kelvin`                                                                                                                             | `72 f to c` -> `22.222222 °C`                                                        |
| **Data (SI & IEC)** | `b`/`bit` (bit), `B`/`byte` (byte, case-sensitive), `KB`, `MB`, `GB`, `TB`, `PB`, `KiB`, `MiB`, `GiB`, `TiB`                                                                        | `1 GiB to MB` -> `1073.741824 MB`                                                    |
| **Time**            | `ms`, `s`/`sec`, `min`, `h`/`hr`, `d`/`day`, `wk`/`week`, `yr`/`year`                                                                                                               | `2.5 h to min` -> `150 min`                                                          |
| **Speed**           | `m/s`/`mps`, `km/h`/`kph`/`kmh`, `mph`, `kn`/`knot`                                                                                                                                 | `100 km/h to mph` -> `62.137119 mph`                                                 |
| **Area**            | `mm2`/`mm²`, `cm2`/`cm²`, `m2`/`m²`/`sqm`, `km2`/`km²`, `ha`/`hectare`, `sqft`/`ft2`/`ft²`, `acre`                                                                                  | `1 ha to m2` -> `10000 m²`                                                           |
| **Volume**          | `ml`/`cc`/`cm3`/`cm³`, `cl`, `dl`, `l`/`liter`/`dm3`/`dm³`, `m3`/`m³`/`cbm`, `tsp`, `tbsp`, `fl oz`/`floz`, `cup`, `pint`, `qt`/`quart`, `gal`/`gallon`, `in3`/`cuin`, `ft3`/`cuft` | `1500 ml to l` -> `1.5 L`, `1 gal to l` -> `3.785412 L`, `8 fl oz to cup` -> `1 cup` |
| **CSS**             | `px`, `rem`, `em`, `pt` (respects `base_font_px` preference, default `16px`)                                                                                                        | `24 px to rem` -> `1.5 rem`                                                          |

> **Case-sensitive Data units:** lowercase `b` represents **bit**, while uppercase `B` represents **Byte** (for example `8 b to B` -> `1 B`).

---

### 2.4 Offline Currency Converter

Supports **26 built-in currencies** backed by an offline USD reference rate table (100% local, no internet API calls):

`USD`, `IDR`, `EUR`, `GBP`, `JPY`, `SGD`, `MYR`, `AUD`, `CAD`, `CHF`, `CNY`, `KRW`, `HKD`, `INR`, `THB`, `VND`, `PHP`, `NZD`, `SEK`, `NOK`, `BRL`, `MXN`, `AED`, `SAR`, `BTC`, `ETH`.

| Mode                     | Examples                                                    | Behavior                                                     |
| ------------------------ | ----------------------------------------------------------- | ------------------------------------------------------------ |
| Explicit currency codes  | `100 usd to idr`, `50 sgd in myr`, `1 btc -> usd`           | Uses `to`, `in`, `->`, or `=>`                               |
| Symbol + explicit target | `$49.99 to idr`, `€20 in usd`, `£15 -> eur`, `¥5000 to idr` | Supports `$` (USD), `€` (EUR), `£` (GBP), `¥` (JPY)          |
| Bare currency symbol     | `$49.99`, `€20`, `£10`, `¥5000`                             | Automatically converts to `default_currency` (default `IDR`) |

> Display output includes thousand separators for readability (for example `1,620,000 IDR`), while the clipboard **Copy Value** is a clean unformatted number ready to paste into spreadsheets or forms. The Split Detail pane displays the reference rate used.

---

### 2.5 Date & Timezone Math

Implemented in Pure Rust using Howard Hinnant's exact civil Gregorian calendar algorithms.

| Feature                   | Syntax                                       | Examples                                        | Results                                    |
| ------------------------- | -------------------------------------------- | ----------------------------------------------- | ------------------------------------------ |
| Date literals             | `today`, `tomorrow`, `yesterday`, `now`      | `today`                                         | `YYYY-MM-DD (Weekday)`                     |
| Add / subtract duration   | `<date> + <dur>` or `<date> - <dur>`         | `today + 14d`, `2026-01-01 + 2w`, `today - 3m`  | `YYYY-MM-DD (Weekday)`                     |
| Date difference           | `<date1> - <date2>`                          | `2026-12-31 - today`, `2026-12-31 - 2026-01-01` | Total days + week breakdown                |
| Current time in timezone  | `now in <tz>`, `now to <tz>`, `time in <tz>` | `now in tokyo`, `now in utc+7`                  | `HH:MM (YYYY-MM-DD)`                       |
| Convert time across zones | `HH:MM <src_tz> to <dst_tz>`                 | `14:30 wib to utc`, `22:00 jakarta to tokyo`    | `07:30 UTC`, `00:00 JST/WIT (UTC+9) (+1d)` |

**Supported date duration units:**

- Days: `d`, `day`, `days`
- Weeks: `w`, `wk`, `wks`, `week`, `weeks` (7 days)
- Months: `m`, `mo`, `mos`, `month`, `months` (30 days)
- Years: `y`, `yr`, `yrs`, `year`, `years` (365 days)

**Built-in timezone aliases:**

- `utc`, `gmt`, `z` (`UTC+0`)
- `wib`, `jakarta`, `bangkok`, `hanoi`, `ict` (`UTC+7`)
- `wita`, `singapore`, `sg`, `sgt`, `kl`, `hongkong`, `hk`, `beijing`, `shanghai`, `taipei`, `perth` (`UTC+8`)
- `wit`, `tokyo`, `jst`, `seoul`, `kst` (`UTC+9`)
- `sydney`, `aest` (`UTC+10`)
- `auckland`, `nzst` (`UTC+12`)
- `ist`, `india`, `mumbai`, `delhi` (`UTC+5:30`)
- `dubai`, `gst` (`UTC+4`)
- `london`, `bst` (`UTC+0`)
- `berlin`, `paris`, `cet`, `amsterdam`, `madrid`, `rome` (`UTC+1`)
- `nyc`, `newyork`, `est`, `et` (`UTC-5`), `edt` (`UTC-4`)
- `cst`, `chicago` (`UTC-6`)
- `mst`, `denver` (`UTC-7`)
- `pst`, `pt`, `sf`, `la` (`UTC-8`), `pdt` (`UTC-7`)
- **Custom offsets:** `UTC+H`, `UTC-H`, `UTC+H:MM`, `UTC-H:MM`, `GMT+H`, `GMT-H` (range `0..=14` hours).

---

## 3. Keyboard Shortcuts & Actions

### Root Search (Inline Result)

- `Enter`: Copy the **Copy Value** to the Wayland clipboard, save the entry to SQLite history, display a confirmation HUD, and close the launcher window.
- `Ctrl+Enter`: Open the full **Calculator & Converter** view seeded with the current expression.
- `Ctrl+K`: Open the Action Panel to inspect all available actions.

### Interactive View (`Calculator & Converter`)

- **Live Result item:**
  - `Enter`: **Copy `<result>`** (copies the clean value and records it in history).
  - `Ctrl+Enter`: **Copy Expression = Result** (copies `<expression> = <result>` and records it in history).
  - Via `Ctrl+K` Action Panel: **Save to History**, **Clear History**, **Back to Root** (`Esc`).
- **Recent Calculations (History) item:**
  - `Enter`: **Copy `<result>`**.
  - `Ctrl+Enter`: **Copy Expression = Result**.
  - Via `Ctrl+K` Action Panel: **Remove Entry**, **Clear All History**.
- **Parse Hint (incomplete or invalid expression):**
  - The Split Detail pane shows the parse error and supported syntax examples.
  - `Enter`: **Clear Input**.
- `Esc`: Clears the input when non-empty; pressing `Esc` again returns to Root Search.

---

## 4. Calculation History (Persistence)

- Stored locally in SQLite plugin KV storage (`plugin_id = "com.aios.calculator"`, key `history_v1`).
- Automatically deduplicates repeated entries (promoting the latest execution to the top) and retains up to **50 most recent entries**.

---

## 5. Plugin Preferences

Open **Settings & Plugins** (`aios://settings`) -> **First-Party Plugins** -> **Calculator & Converter** -> **Configure Preferences**:

| Key                | Title                   | Default | Range / Description                                                                                           |
| ------------------ | ----------------------- | ------- | ------------------------------------------------------------------------------------------------------------- |
| `base_font_px`     | CSS Base Font Size (px) | `16`    | Range `4` to `128`, used for `px <-> rem/em` conversions.                                                     |
| `decimal_places`   | Max Decimal Places      | `6`     | Range `0` to `12`, maximum fractional digits for floating-point results.                                      |
| `default_currency` | Default Currency        | `IDR`   | 3-letter target currency code used when entering bare symbol amounts such as `$50`, `€20`, `£10`, or `¥5000`. |

---

## 6. Build & Testing

This plugin is compiled automatically as part of the AIOS Cargo workspace:

```bash
# Run all Calculator unit & integration tests
cargo test -p aios-plugins

# Build the AIOS binary (includes Calculator)
cargo build -p aios-app
```

Source and test references:

- Implementation: [crates/aios-plugins/src/calculator.rs](../../crates/aios-plugins/src/calculator.rs)
- Test suite: [crates/aios-plugins/tests/calculator_plugin.rs](../../crates/aios-plugins/tests/calculator_plugin.rs)
