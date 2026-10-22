# Community layouts

Key layouts for AnyPS5 games, shared by players. The launcher lists the ones for a game in its
Controls panel, under Layout, as "Community: <name>". They are read from this folder on GitHub when
the panel opens, so a new layout reaches everyone without a launcher release.

A layout is a plain `anyps5-input.ini`, the file described in upstream's
[`INPUT_MAPPING.md`](https://github.com/boykopovar/AnyPS5/releases/latest) (a release asset). It only
changes keyboard and mouse; a controller keeps its own buttons.

## Add one

1. Set up the keys in the launcher (Game Settings, Controls) and press **Export Layout**. That
   writes a complete layout file. Or write the file by hand, as in the example below.
2. Put the file in `community/layouts/<TITLEID>/<name>.ini`. The title ID is the one on the game's
   page in the launcher (`PPSA16064`). The name uses letters, digits, `.`, `_` and `-`.
3. Add an entry to `index.json`:

   ```json
   { "titleId": "PPSA16064", "name": "Mouse look", "author": "your-github-name",
     "description": "One sentence on what it is for.", "file": "PPSA16064/mouse-look.ini" }
   ```

   `name` is what the launcher shows (up to 40 characters, no two the same for one game).
   `description` and `author` are optional.
4. Open a pull request. `cargo test -p anyps5-core community` checks the index and that every file
   in it can be read.

## The file

```ini
# AnyPS5 controls layout: Minecraft Dungeons II (PPSA16064)
Cross = KEY:Space
Circle = KEY:C
R2 = MOUSE:Left
Up = WHEEL:Up
StickModifier = KEY:Left Alt
LeftStickDeadzone = 12
```

- One line per key, `Action = Type:Value`. Types are `KEY` (SDL key names such as `Return`,
  `Left Shift`), `MOUSE` (`Left`, `Middle`, `Right`, `X1`, `X2`) and `WHEEL` (`Up`, `Down`).
  Lines starting with `#` or `;` are comments.
- The first line of an action replaces its keys, further lines add keys.
- **List every action the layout moves, and better all of them.** An action the file leaves out keeps
  the engine's own keys, which are not the launcher's default keys.
- `KEY:F24` alone means "no key".
- Tuning lines (`LeftStickDeadzone`, `MouseSensitivity`, and the others of `INPUT_MAPPING.md`) are
  optional. Choosing a layout with tuning lines also sets the game's tuning.
- `StickModifier`, `KeyboardRampMs` and `TriggerRampMs` need toolkit v0.1.3 or newer. Leave them
  out if the layout should work with older ones.
- A line the engine would refuse makes the launcher skip the whole file. Try it with **Import
  Layout** first: it names the line.

The data format is in [`docs/DESIGN.md`](../../docs/DESIGN.md), section 5.18.
