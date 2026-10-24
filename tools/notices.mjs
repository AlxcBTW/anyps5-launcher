// Merges cargo-about reports into one plain-text licence file (tools/notices.sh).
//
//   node notices.mjs <output file> <report.json>...
//
// A report is the output of `cargo about generate --format json`. Every distinct licence text is
// written once, after the crates it applies to: the same MIT text with another copyright line is
// its own entry. The output is sorted, so the same crates give the same file.

import { readFileSync, writeFileSync } from "node:fs";

const [out, ...reports] = process.argv.slice(2);
if (!out || reports.length === 0) {
  console.error("usage: node notices.mjs <output file> <report.json>...");
  process.exit(2);
}

const WIDTH = 100;
const byName = (a, b) => a.localeCompare(b, "en", { numeric: true });

const entries = new Map();
const cratesById = new Map();
for (const report of reports) {
  for (const licence of JSON.parse(readFileSync(report, "utf8")).licenses) {
    const text = licence.text.replace(/\r\n?/g, "\n").trim();
    const key = `${licence.id}\n${text}`;
    let entry = entries.get(key);
    if (!entry) {
      entry = { id: licence.id, name: licence.name, text, crates: new Set() };
      entries.set(key, entry);
    }
    for (const { crate } of licence.used_by) {
      const label = `${crate.name} ${crate.version}`;
      entry.crates.add(label);
      if (!cratesById.has(licence.id)) cratesById.set(licence.id, new Set());
      cratesById.get(licence.id).add(label);
    }
  }
}
if (entries.size === 0) {
  console.error("notices: the reports list no licences");
  process.exit(1);
}

// Licences used by more crates first; entries of one licence by their first crate.
const rank = (id) => -cratesById.get(id).size;
const sorted = [...entries.values()]
  .map((entry) => ({ ...entry, crates: [...entry.crates].sort(byName) }))
  .sort((a, b) => rank(a.id) - rank(b.id) || byName(a.id, b.id) || byName(a.crates[0], b.crates[0]));

// "<prefix>a, b, c", broken before WIDTH and continued under the first word.
function wrap(prefix, words) {
  const indent = " ".repeat(prefix.length);
  const lines = [];
  let line = prefix + words[0];
  for (const word of words.slice(1)) {
    // ", " before the word and the "," a broken line ends with.
    if (line.length + 2 + word.length + 1 > WIDTH) {
      lines.push(line + ",");
      line = indent + word;
    } else {
      line = `${line}, ${word}`;
    }
  }
  lines.push(line);
  return lines.join("\n");
}

const rule = "=".repeat(WIDTH);
const all = new Set([...cratesById.values()].flatMap((crates) => [...crates]));
const ids = [...cratesById.keys()].sort((a, b) => rank(a) - rank(b) || byName(a, b));
const width = Math.max(...ids.map((id) => id.length));

let text = `Third-party licences

The launcher and the AnyPlay net service it carries are built with ${all.size} crates by other
authors. Their licences follow, each after the crates it applies to. The launcher itself is
licensed under the GPL-2.0 (LICENSE).

`;
for (const id of ids) {
  const count = cratesById.get(id).size;
  text += `  ${id.padEnd(width)}  ${count} ${count === 1 ? "crate" : "crates"}\n`;
}
for (const entry of sorted) {
  text += `\n${rule}\n${entry.name} (${entry.id})\n`;
  text += `${wrap("Used by: ", entry.crates)}\n\n`;
  text += `${entry.text}\n`;
}
writeFileSync(out, text);
console.log(`notices: ${all.size} crates, ${ids.length} licences, ${sorted.length} texts -> ${out}`);
