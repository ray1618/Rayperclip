# 📋 Rayperclip

Rayperclip is een klembordgeschiedenis-app voor de macOS-menubalk, geschreven in Rust. Het is een eigen implementatie op basis van een analyse van **CopyClip 2** (met Ghidra): alles wat je kopieert wordt bewaard, je kunt het terugzoeken, vastzetten, bewerken en met één toets weer plakken.

---

## ✨ Functies

- 🕒 **Klembordgeschiedenis** – het klembord wordt elke 0,5 s gecontroleerd; nieuwe tekst (plus RTF/HTML tot 2 MB) wordt opgeslagen samen met de app waaruit je kopieerde.
- 🔐 **Wachtwoorden worden overgeslagen** – inhoud die gemarkeerd is als *concealed* of *transient* (zoals uit 1Password of Bitwarden) wordt standaard niet bewaard.
- 🔍 **Snel zoeken** – full-text search (SQLite FTS5) op tekst en titels, met prefix-matching.
- 📌 **Vastzetten** – vastgezette clips staan altijd bovenaan, worden nooit automatisch verwijderd en zijn te herordenen.
- ✏️ **Bewerken** – pas de tekst aan of geef een clip een eigen titel.
- ⚡ **Direct plakken** – kies een clip en Rayperclip plakt hem meteen in de app waar je was (Cmd+V via een gesimuleerde toetsaanslag).
- ⌨️ **Globale sneltoetsen** – één voor het zoekvenster, één voor een clipmenu bij de muisaanwijzer.
- 🎨 **10 thema's** – Light, Dark, Midnight Blue, Forest, Sunset, Lavender, Graphite, Ocean, Rose en Slate.
- 🚀 **Starten bij inloggen** – via een LaunchAgent.
- 🧹 **Automatisch opruimen** – boven het maximum (standaard 80) verdwijnen de oudste niet-vastgezette clips.

---

## 🧰 Vereisten

- 🍎 macOS (Apple Silicon of Intel)
- 🦀 Rust **1.92** of nieuwer (`rustup update`)
- 🛠️ Xcode Command Line Tools (`xcode-select --install`), nodig voor het linken met de macOS-frameworks en het compileren van SQLite

---

## 🚀 Snel starten

```sh
# debug-build draaien
cargo run

# geoptimaliseerde build draaien
cargo run --release
```

Rayperclip verschijnt als 📋-icoon in de menubalk; er is geen Dock-icoon. Afsluiten doe je via **Quit Rayperclip** in het menubalkmenu.

### 🔓 Toegankelijkheid (voor direct plakken)

Om Cmd+V naar andere apps te sturen heeft Rayperclip **Accessibility-toestemming** nodig:

1. Kies een clip of klik in het venster op **Accessibility…**
2. Open **Systeeminstellingen → Privacy en beveiliging → Toegankelijkheid**
3. Zet Rayperclip (of de terminal waaruit je hem start) aan

Zonder toestemming wordt de clip alleen naar het klembord gekopieerd en plak je zelf met ⌘V.

> 💡 macOS koppelt de toestemming aan het exacte binaire bestand. Na een nieuwe `cargo build` kan het nodig zijn de oude regel te verwijderen en opnieuw toe te voegen. Start je via een terminal, dan erft Rayperclip de toestemming van die terminal (bijvoorbeeld Terminal of Ghostty).

---

## 🖱️ Gebruik

### Menubalkmenu
Klik op het icoon voor:
- 📌 vastgezette en 🕒 recente clips (klik = plakken)
- **Open Rayperclip…** (het zoekvenster)
- vinkjes voor **Record Clipboard**, **Paste Directly** en **Start at Login**
- **Theme** ▸ themakeuze
- **Delete All Unpinned Clips**
- **Edit Settings File…** / **Reload Settings**
- **Quit Rayperclip**

### ⌨️ Sneltoetsen

| Sneltoets | Actie |
|---|---|
| `⇧⌘V` | Zoekvenster openen/sluiten (globaal) |
| `⌥⌘V` | Clipmenu bij de muisaanwijzer (globaal) |
| `↑` / `↓` | Selectie verplaatsen |
| `Enter` / dubbelklik | Geselecteerde clip plakken |
| `⌘1` – `⌘9` | Clip 1 t/m 9 plakken |
| `⌘P` | Vastzetten / losmaken |
| `⌘E` | Bewerken |
| `⌘⌫` | Verwijderen |
| `Esc` | Venster sluiten (of bewerken annuleren) |

---

## ⚙️ Instellingen

Instellingen staan in een TOML-bestand dat bij de eerste start wordt aangemaakt:

```
~/Library/Application Support/nl.ray.Rayperclip/settings.toml
```

```toml
recording = true            # klembord opnemen
max_clips = 80              # max. niet-vastgezette clips (1–10000)
menu_clip_count = 20        # aantal recente clips in het menubalkmenu (1–100)
ignore_concealed = true     # wachtwoorden/transient data overslaan
paste_directly = true       # na kiezen direct plakken (Cmd+V)
window_hotkey = "cmd+shift+v"
menu_hotkey = "cmd+alt+v"
theme = "Light"
start_at_login = false
title_length = 50           # max. tekens per titel (10–200)
```

- ✏️ Pas het bestand aan via **Edit Settings File…** en kies daarna **Reload Settings**.
- 🔢 Waarden buiten het bereik worden automatisch begrensd; ontbrekende sleutels krijgen hun standaardwaarde.
- ⌨️ **Sneltoetsformaat:** modifiers `cmd`/`command`, `ctrl`/`control`, `alt`/`option`, `shift`, gevolgd door één toets, bijvoorbeeld `ctrl+alt+space` of `cmd+shift+1`. Er is minstens één modifier nodig (alleen `shift` mag niet). Met `""` of `"none"` zet je een sneltoets uit.
- ⚠️ Kan een sneltoets niet geregistreerd worden (bijvoorbeeld omdat een andere app hem al gebruikt), dan zie je een melding onderin het venster.

---

## 🏗️ Hoe het werkt

```
 NSPasteboard ──(elke 0,5 s)──▶ Poller ──▶ capture::evaluate ──▶ Store (SQLite + FTS5)
                                   │            (regels)              │
                     changeCount ──┘                                  ▼
                                                     Menubalkmenu  /  zoekvenster (egui)
                                                                      │
                                                              paste::plan + execute
                                                                      │
                                                   klembord schrijven → focus terug → ⌘V
```

1. 🔁 Een run-loop-timer op de hoofdthread roept elke 0,5 s de **poller** aan.
2. 🔢 De poller doet alleen iets als de `changeCount` van het klembord veranderd is.
3. 🧪 `capture::evaluate` beslist of de inhoud bewaard wordt (opname aan? wachtwoord? te groot? leeg? dubbel? zelf geplakt? uit Rayperclip zelf?).
4. 💾 De **store** slaat de clip op. Bestaat dezelfde tekst al, dan schuift die naar boven; daarna wordt de lijst ingekort tot `max_clips`.
5. 🖼️ Het menubalkmenu en het egui-venster tonen de clips.
6. 📤 Bij plakken: clip naar het klembord, focus terug naar de vorige app en na 200 ms een Cmd+V via CoreGraphics.

Alle native callbacks (timer, sneltoetsen, menukliks, UI-acties) lopen via één `dispatch`-wachtrij in `src/app.rs`. Zo wordt de staat nooit dubbel geleend, ook niet tijdens geneste event-loops zoals een open popupmenu.

### 📁 Projectstructuur

```
src/
├── lib.rs           # bibliotheek: alle platformonafhankelijke logica
├── model.rs         # Clip, titels afleiden, tokenizen
├── capture.rs       # opnameregels (wat wordt wel/niet bewaard)
├── clipboard.rs     # Clipboard-trait + MemoryClipboard voor tests
├── poller.rs        # klembord-polling → store
├── store.rs         # SQLite + FTS5: opslaan, zoeken, vastzetten, inkorten
├── paste.rs         # plakstappen (plan/execute) + PasteBackend-trait
├── settings.rs      # TOML-instellingen en bestandslocaties
├── theme.rs         # de 10 thema's + contrastberekening
├── hotkeys.rs       # sneltoetsen parsen en tonen (⇧⌘V)
├── login.rs         # LaunchAgent-plist voor starten bij inloggen
├── presentation.rs  # menulabels, relatieve tijd, menubalk-icoon
├── main.rs          # opstarten (eframe, menubalk-app zonder focus)
├── app.rs           # kern: events, menubalkmenu, sneltoetsen, plakken
├── macos.rs         # NSPasteboard, CGEvent, AX-toestemming, vensters
└── ui.rs            # zoekvenster (egui)
tests/
└── pipeline.rs      # end-to-end: kopiëren → opslaan → zoeken → plakken → herstart
```

🧩 **Ontwerpkeuze:** alles wat macOS nodig heeft zit achter traits (`Clipboard`, `PasteBackend`) of in `macos.rs`/`app.rs`/`ui.rs`. De rest is gewone Rust-code die je zonder macOS-rechten kunt testen.

### 📦 Belangrijkste crates

| Crate | Waarvoor |
|---|---|
| `eframe` / `egui` | zoekvenster |
| `tray-icon` (`muda`) | menubalkicoon en menu's |
| `global-hotkey` | globale sneltoetsen |
| `rusqlite` (bundled) | SQLite met FTS5 |
| `objc2-app-kit`, `objc2-foundation` | NSPasteboard, NSWorkspace, NSApplication |
| `core-graphics`, `core-foundation` | Cmd+V-events en run-loop-timer |
| `serde` + `toml` | instellingen |
| `directories` | standaardmappen |

---

## 🧪 Testen

```sh
cargo test                       # alle tests (96)
cargo test --lib                 # alleen unittests van de bibliotheek
cargo test --test pipeline       # alleen de integratietests
cargo test store::               # filteren op module/naam
cargo test -- --nocapture        # println!-uitvoer tonen
cargo test -- --list             # alle tests opsommen
```

Wat wordt er getest:
- 🧾 **model** – titels, afkappen met `…`, Unicode, eigen titels
- 🚦 **capture** – elke overslaanregel apart, RTF/HTML-limieten
- 💾 **store** – opslaan, dubbele clips, zoeken, vastzetten/herordenen, inkorten, bewerken, verwijderen, opslag op schijf
- 🔁 **poller** – met een nep-klembord (`MemoryClipboard`)
- 📤 **paste** – stapvolgorde met een opnemende nep-backend
- ⚙️ **settings**, 🎨 **theme** (incl. WCAG-contrast ≥ 4,5), ⌨️ **hotkeys**, 🚀 **login**, 🖼️ **presentation** en de egui-thema-omzetting

### ✅ Kwaliteitscontroles

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

---

## 📦 Builden

```sh
cargo build              # debug  → target/debug/rayperclip
cargo build --release    # release → target/release/rayperclip
```

### 🍏 Als `.app` bundelen (optioneel)

Een echte app-bundle heeft een vaste identiteit, zodat de Accessibility-toestemming na een update niet steeds opnieuw nodig is. Een minimale handmatige bundle:

```sh
cargo build --release
APP=Rayperclip.app
mkdir -p "$APP/Contents/MacOS"
cp target/release/rayperclip "$APP/Contents/MacOS/Rayperclip"
cat > "$APP/Contents/Info.plist" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>Rayperclip</string>
  <key>CFBundleIdentifier</key><string>nl.ray.rayperclip</string>
  <key>CFBundleExecutable</key><string>Rayperclip</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>0.1.0</string>
  <key>LSUIElement</key><true/>
</dict></plist>
EOF
codesign --force --deep --sign - "$APP"   # ad-hoc ondertekening
```

Daarna kun je `Rayperclip.app` naar `/Applications` slepen. Zet **Start at Login** daarna opnieuw aan, zodat de LaunchAgent naar het nieuwe pad wijst.

> 💡 [`cargo-bundle`](https://github.com/burtonageo/cargo-bundle) kan dit ook automatisch doen.

---

## 🗂️ Bestanden en data

| Wat | Locatie |
|---|---|
| ⚙️ Instellingen | `~/Library/Application Support/nl.ray.Rayperclip/settings.toml` |
| 💾 Database | `~/Library/Application Support/nl.ray.Rayperclip/clips.sqlite` |
| 🚀 Login-item | `~/Library/LaunchAgents/nl.ray.rayperclip.plist` |

🔎 De database kun je direct bekijken:

```sh
sqlite3 ~/Library/Application\ Support/nl.ray.Rayperclip/clips.sqlite \
  "select id, substr(contents,1,40), source_app, pinned from clips order by recorded_at desc limit 10;"
```

🧽 **Alles resetten:** sluit Rayperclip af en verwijder de map `~/Library/Application Support/nl.ray.Rayperclip` (en eventueel de LaunchAgent-plist).

> 🔒 Je klembordgeschiedenis wordt **onversleuteld** op schijf opgeslagen. Laat `ignore_concealed` aan zodat wachtwoordmanagers niet worden vastgelegd.

---

## 🩺 Problemen oplossen

| Probleem | Oplossing |
|---|---|
| 🐞 Nieuwe kopieën verschijnen niet | Start met `RAYPERCLIP_DEBUG=1 cargo run` en kijk naar de `poll:`-regels. Staat **Record Clipboard** aan? |
| ⌨️ Plakken gebeurt niet automatisch | Geef Accessibility-toestemming (zie hierboven); na een rebuild opnieuw. Staat **Paste Directly** aan? |
| 🎹 Sneltoets werkt niet | Controleer de melding onderin het venster; mogelijk gebruikt een andere app dezelfde combinatie. Kies een andere in `settings.toml` en kies **Reload Settings**. |
| 📄 Instellingen worden niet geladen | Ongeldige TOML wordt gemeld en de standaardwaarden worden gebruikt. Controleer de syntax. |
| 🪟 Venster weg | Open het via het menubalkmenu of `⇧⌘V`; het rode sluitknopje verbergt het venster alleen. |

---

## 🚧 Bekende beperkingen

- 🖼️ Alleen tekst (met RTF/HTML erbij). Afbeeldingen en bestanden worden niet opgeslagen.
- 🚫 Geen uitsluitlijst per app en geen voorkeurenvenster; instellingen gaan via het TOML-bestand.
- 🍎 Alleen macOS.

---

## 🙏 Achtergrond

Het gedrag is afgeleid van een statische analyse van CopyClip 2 (FIPLAB) met Ghidra: polling-interval, groottelimieten, het overslaan van concealed data, het inkorten van de geschiedenis en de Cmd+V-simulatie (keycode 9, Command-flag). Er is geen code van CopyClip 2 overgenomen; licentiecontrole, updates en reviewprompts zijn bewust weggelaten.
