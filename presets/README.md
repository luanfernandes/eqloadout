# Game presets

Starter EQ presets for EQ Loadout. Each one is a plain Equalizer APO file, so you can also copy it into your Equalizer APO config folder by hand and pick it in a rule.

**These presets go on top of your headphone correction.** They don't fix your headphones. They only shift the balance toward what matters in a game, like footsteps. Keep your headphone EQ (for example an [AutoEq](https://github.com/jaakkopasanen/AutoEq) profile) in `config.txt` before `Include: game.txt`.

They are starting points built from common advice (see Sources), not measured or tested curves. Tune them by ear: have a friend walk around you in a custom match, or replay a demo.

## Presets

| File | Game | Focus |
|---|---|---|
| `eql-cs2-footsteps.txt` | Counter-Strike 2 | Footsteps |
| `eql-valorant-footsteps.txt` | VALORANT | Footsteps (keeps the low thud) |
| `eql-r6siege-footsteps.txt` | Rainbow Six Siege | Footsteps and vertical cues |
| `eql-apex-footsteps.txt` | Apex Legends | Footsteps |
| `eql-pubg-footsteps.txt` | PUBG: Battlegrounds | Footsteps |
| `eql-tarkov-footsteps.txt` | Escape from Tarkov | Footsteps |
| `eql-hunt-footsteps.txt` | Hunt: Showdown 1896 | Footsteps |
| `eql-overwatch2-cues.txt` | Overwatch 2 | Footsteps and ability cues |
| `eql-cod-footsteps.txt` | Call of Duty (Black Ops 6 / Warzone) | Footsteps |
| `eql-fortnite-footsteps.txt` | Fortnite | Footsteps |
| `eql-thefinals-footsteps.txt` | THE FINALS | Footsteps |
| `eql-bf6-footsteps.txt` | Battlefield 6 | Footsteps |
| `eql-marvelrivals-cues.txt` | Marvel Rivals | Footsteps and ability cues |
| `eql-deadlock-cues.txt` | Deadlock | Footsteps and ability cues |
| `eql-abi-footsteps.txt` | Arena Breakout: Infinite | Footsteps |
| `eql-genre-competitive.txt` | Any shooter | Footsteps |
| `eql-genre-cinematic.txt` | Single-player, open world | Weight and air |
| `eql-genre-dialogue.txt` | Story games, RPGs | Voice clarity |
| `eql-genre-racing.txt` | Racing | Engine body |
| `eql-genre-moba.txt` | MOBA, strategy | Ability cues, low fatigue |
| `eql-genre-horror.txt` | Horror | Quiet details |

## Format

```
# eqloadout-preset: 1
# name: Footsteps
# game: Counter-Strike 2
# exe: cs2.exe
# steam_appid: 730
# notes: Trims low boom and mud, lifts footstep scuffs at 2-4 kHz.
Preamp: -2.0 dB
Filter: ON LS Fc 160 Hz Gain -2.5 dB Q 0.70
Filter: ON PK Fc 4000 Hz Gain 2.0 dB Q 1.20
```

- The first line must be `# eqloadout-preset: 1`. `name` is required. `game` is required except for `eql-genre-*` presets. `exe` is a comma-separated list of exe file names. Equalizer APO ignores these comment lines.
- File names look like `eql-<game>-<focus>.txt`: lowercase letters, digits and dashes. The `eql-` prefix keeps them from colliding with files already in your config folder.

## Safety rules

Equalizer APO config lines can do more than EQ. `VSTPlugin` loads a DLL into the Windows audio engine, and `Include` pulls in other files. So a preset may only contain:

- one `Preamp:` line, between -30 and 0 dB
- `Filter: ON PK|LS|HS Fc <Hz> Hz Gain <dB> dB Q <q>` lines: 20-20000 Hz, at most ±12 dB, Q 0.1-10
- `GraphicEQ: <Hz> <dB>; ...` lines with the same ranges
- comments and blank lines

Every other line is rejected. That includes `VSTPlugin`, `Include`, `Device`, `Eval`, `If`, `Copy`, `Channel`, and `LP`/`HP` filters. There are also limits: at most 32 filters, 8 KB and ASCII only. The `Preamp` must cover the largest boost, so a preset can't make your audio clip.

`cargo test` checks every file in this folder against these rules. EQ Loadout runs the same check on anything it downloads.

## Sources

The starter values follow advice that recurs across these sources, then are kept conservative (boosts of +3 dB or less):

- Steam discussions: [Hunt: Showdown EQ](https://steamcommunity.com/app/594650/discussions/8/3844431603856400627), [Rainbow Six Siege footstep frequencies](https://steamcommunity.com/app/359550/discussions/0/1634166237654286859)
- EA forums: [Apex sound EQ](https://forums.ea.com/discussions/apex-legends-general-discussion-en/improve-in-game-sounds---how-to/5676306/replies/5676363), [Battlefield 6 footsteps](https://forums.ea.com/discussions/battlefield-6-general-discussion-en/i-found-a-way-to-fix-the-footstep-sounds/12831794/replies/13365640)
- Guides: [Evetech on footstep frequencies](https://evezone.evetech.co.za/quick-bytes/eq-for-footsteps-the-frequencies-that-actually-matter), [Elgato EQ guide](https://marketplace.elgato.com/learn/inspiration/elgato-equalizer-step-up-your-game), [SteelSeries BO6/Warzone EQ](https://steelseries.com/blog/best-sonar-eq-settings-to-hear-footsteps-in-black-ops-6-warzone-1174), [FxSound PUBG thread](https://forum.fxsound.com/t/hi-need-help-with-a-eq-setting-pubg/2304)
