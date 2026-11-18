// Mechanical build of a portable HTML preview, with all local modules inlined.
// It has no external dependencies and does not modify Rusty Desk or its assets.
import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = dirname(fileURLToPath(import.meta.url));
const html = readFileSync(join(root, 'index.html'), 'utf8');
const motion = readFileSync(join(root, 'motion.mjs'), 'utf8').replace(/^export /gm, '');
const character = readFileSync(join(root, 'character.mjs'), 'utf8').replace(/^export /gm, '');
const prefix = `<script type="module">\n    import { RustyMotion } from './motion.mjs';\n    import { createRustyRenderer } from './character.mjs';`;
if (!html.includes(prefix)) throw new Error('Could not locate preview module imports');
const bundled = html.replace(prefix, `<script>\n${motion}\n${character}\n`);
if (/\b(?:import|export)\s+/.test(bundled.slice(bundled.indexOf('<script>')))) {
  throw new Error('Unexpected JS import/export in standalone build');
}
writeFileSync(join(root, 'preview-standalone.html'), bundled);
console.log('Generated standalone, zero-dependency Rusty Motion Lab preview');
