#!/usr/bin/env node
// Inject code from examples/ into slides.md so the slides never drift from
// the real, reviewable source files.
//
// In slides.md, mark a code block with the file + region it comes from:
//
//   <!-- snippet: examples/canonical/peer.js#create -->
//   ```js
//   ...generated, do not edit by hand...
//   ```
//
// The fenced block immediately after the marker is replaced with the named
//   // #region create ... // #endregion
// span from that file (comment markers stripped, block dedented).
//
// Run with `npm run snippets`. Every export hook (preview/html/pdf/pptx/lint)
// runs it first via `prepare:deck`, so the embedded code is always current.
// `--check` exits non-zero if slides.md would change (useful in CI).

import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..');
const SLIDES = join(ROOT, 'slides.md');
const CHECK = process.argv.includes('--check');

// marker, then (allowing a blank line) a fenced block whose body we replace
const MARKER = /(<!-- snippet: (\S+?)#(\S+?) -->)\s*```([a-zA-Z]*)\n[\s\S]*?\n```/g;

function dedent(lines) {
  const indents = lines
    .filter((line) => line.trim().length > 0)
    .map((line) => line.match(/^\s*/)[0].length);
  const trim = indents.length ? Math.min(...indents) : 0;
  return lines.map((line) => line.slice(trim)).join('\n');
}

const fileCache = new Map();
function regionOf(file, region) {
  if (!fileCache.has(file)) fileCache.set(file, readFileSync(join(ROOT, file), 'utf8'));
  const lines = fileCache.get(file).split('\n');
  const start = lines.findIndex((line) => line.trim() === `// #region ${region}`);
  if (start === -1) throw new Error(`${file}: region "${region}" not found`);
  const end = lines.findIndex((line, i) => i > start && /^\s*\/\/ #endregion\b/.test(line));
  if (end === -1) throw new Error(`${file}: region "${region}" has no // #endregion`);
  return dedent(lines.slice(start + 1, end)).replace(/^\n+|\n+$/g, '');
}

const before = readFileSync(SLIDES, 'utf8');
const after = before.replace(MARKER, (_match, marker, file, region, lang) => {
  return `${marker}\n\n\`\`\`${lang}\n${regionOf(file, region)}\n\`\`\``;
});

if (CHECK) {
  if (after !== before) {
    console.error('slides.md snippets are stale; run `npm run snippets`');
    process.exit(1);
  }
  console.log('Snippets OK');
} else {
  if (after !== before) writeFileSync(SLIDES, after);
  console.log('Injected code snippets into slides.md');
}
