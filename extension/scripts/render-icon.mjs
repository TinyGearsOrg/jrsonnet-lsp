// Draws the extension icon: a gear (for the TinyGears publisher) between curly braces (for
// Jsonnet's JSON heritage). Writes images/icon.svg, the editable source, and images/icon.png,
// which is what VS Code and the Marketplace use (they don't accept SVG icons). It also writes
// .github/social-preview.png (1280x640), the image to upload as the repository's social preview
// in the GitHub settings; GitHub has no API for that.
//
//   npm run icon
//
// Everything is drawn as shapes, with no text, so the result doesn't depend on installed fonts.

import { Resvg } from '@resvg/resvg-js';
import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const SIZE = 256;
const C = SIZE / 2;

const fmt = (n) => Number(n.toFixed(2));
const point = (radius, angle) => `${fmt(C + radius * Math.sin(angle))} ${fmt(C - radius * Math.cos(angle))}`;

/** A gear outline: `teeth` trapezoidal teeth joined by arcs of the root circle, plus a hole. */
function gearPath({ teeth, tip, root, rootHalf, tipHalf, hole }) {
  const pitch = (2 * Math.PI) / teeth;
  const parts = [];
  for (let i = 0; i < teeth; i++) {
    const a = i * pitch;
    parts.push(
      `${i === 0 ? 'M' : `A ${root} ${root} 0 0 1`} ${point(root, a - rootHalf)}`,
      `L ${point(tip, a - tipHalf)}`,
      `L ${point(tip, a + tipHalf)}`,
      `L ${point(root, a + rootHalf)}`,
    );
  }
  // Close the loop along the root circle, then cut the hole (even-odd fill).
  parts.push(`A ${root} ${root} 0 0 1 ${point(root, -rootHalf)} Z`);
  parts.push(`M ${C - hole} ${C} a ${hole} ${hole} 0 1 0 ${2 * hole} 0 a ${hole} ${hole} 0 1 0 ${-2 * hole} 0 Z`);
  return parts.join(' ');
}

/** `{` as an open path; `mirror` gives `}`. */
function bracePath(mirror) {
  const x = (v) => (mirror ? SIZE - v : v);
  return [
    `M ${x(80)} 60`,
    `C ${x(62)} 60 ${x(60)} 72 ${x(60)} 90`,
    `L ${x(60)} 112`,
    `C ${x(60)} 122 ${x(54)} 128 ${x(42)} 128`,
    `C ${x(54)} 128 ${x(60)} 134 ${x(60)} 144`,
    `L ${x(60)} 166`,
    `C ${x(60)} 184 ${x(62)} 196 ${x(80)} 196`,
  ].join(' ');
}

const gear = gearPath({ teeth: 10, tip: 53, root: 41, rootHalf: 0.155, tipHalf: 0.085, hole: 17 });

const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="${SIZE}" height="${SIZE}" viewBox="0 0 ${SIZE} ${SIZE}">
  <defs>
    <linearGradient id="bg" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#27365c"/>
      <stop offset="1" stop-color="#121829"/>
    </linearGradient>
    <linearGradient id="gear" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#ffc857"/>
      <stop offset="1" stop-color="#ef8a1f"/>
    </linearGradient>
  </defs>
  <rect width="${SIZE}" height="${SIZE}" rx="56" fill="url(#bg)"/>
  <g fill="none" stroke="#e8eef9" stroke-width="13" stroke-linecap="round" stroke-linejoin="round">
    <path d="${bracePath(false)}"/>
    <path d="${bracePath(true)}"/>
  </g>
  <path d="${gear}" fill="url(#gear)" fill-rule="evenodd"/>
</svg>
`;

mkdirSync(join(root, 'images'), { recursive: true });
writeFileSync(join(root, 'images', 'icon.svg'), svg);
const png = new Resvg(svg, { fitTo: { mode: 'width', value: SIZE } }).render().asPng();
writeFileSync(join(root, 'images', 'icon.png'), png);
console.log(`wrote images/icon.svg and images/icon.png (${SIZE}x${SIZE}, ${png.length} bytes)`);

// The social preview: the icon, centered on a canvas in the icon's colors.
const WIDTH = 1280;
const HEIGHT = 640;
const ICON = 400;
const iconInner = svg.replace(/^<svg[^>]*>/, '').replace(/<\/svg>\s*$/, '');
const social = `<svg xmlns="http://www.w3.org/2000/svg" width="${WIDTH}" height="${HEIGHT}" viewBox="0 0 ${WIDTH} ${HEIGHT}">
  <defs>
    <linearGradient id="social-bg" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0" stop-color="#101728"/>
      <stop offset="1" stop-color="#05070d"/>
    </linearGradient>
  </defs>
  <rect width="${WIDTH}" height="${HEIGHT}" fill="url(#social-bg)"/>
  <svg x="${(WIDTH - ICON) / 2}" y="${(HEIGHT - ICON) / 2}" width="${ICON}" height="${ICON}" viewBox="0 0 ${SIZE} ${SIZE}">${iconInner}</svg>
</svg>
`;
const githubDir = join(root, '..', '.github');
mkdirSync(githubDir, { recursive: true });
const socialPng = new Resvg(social, { fitTo: { mode: 'width', value: WIDTH } }).render().asPng();
writeFileSync(join(githubDir, 'social-preview.png'), socialPng);
console.log(`wrote .github/social-preview.png (${WIDTH}x${HEIGHT}, ${socialPng.length} bytes)`);

// Small previews, to check legibility at the sizes it is shown at; not part of the package.
if (process.argv.includes('--previews')) {
  for (const size of [128, 48, 32, 16]) {
    const small = new Resvg(svg, { fitTo: { mode: 'width', value: size } }).render().asPng();
    writeFileSync(join(process.argv[process.argv.indexOf('--previews') + 1] ?? '.', `icon-${size}.png`), small);
  }
}
