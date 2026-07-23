#!/usr/bin/env node
import { readFileSync, writeFileSync, mkdirSync, existsSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..');
const SLIDES = join(ROOT, 'slides.md');
const ASSETS_DIR = join(ROOT, 'assets', 'generated', 'mermaid');

if (!existsSync(ASSETS_DIR)) {
  mkdirSync(ASSETS_DIR, { recursive: true });
}

const content = readFileSync(SLIDES, 'utf8');
// Capture mermaid blocks and ensure we ignore trailing spaces after ```mermaid
const mermaidBlocks = [...content.matchAll(/```mermaid\s*\n([\s\S]*?)\n```/g)];

let generatedCount = 0;

for (const match of mermaidBlocks) {
  const code = match[1].trim();
  const hash = createHash('sha256').update(code).digest('hex').substring(0, 12);
  const mmdPath = join(ASSETS_DIR, `${hash}.mmd`);
  const svgPath = join(ASSETS_DIR, `${hash}.svg`);
  
  if (!existsSync(svgPath)) {
    console.log(`Generating Mermaid SVG for ${hash}...`);
    writeFileSync(mmdPath, code, 'utf8');
    
    // Execute mermaid-cli
    execFileSync('npx', [
      'mmdc',
      '-i', mmdPath,
      '-o', svgPath,
      '-t', 'dark',
      '-c', 'mermaid.config.json',
      '-p', 'puppeteer-config.json',
      '-b', 'transparent'
    ], { stdio: 'inherit', cwd: ROOT });
    
    generatedCount++;
    console.log(`Generated ${svgPath}`);
  }
}

if (generatedCount === 0) {
  console.log(`Diagrams OK (${mermaidBlocks.length} mermaid blocks)`);
} else {
  console.log(`Generated ${generatedCount} new Mermaid diagrams.`);
}
