// Collect notices for a specific built GUI without launching an app or altering dist.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const gui = path.join(root, 'gui');
const output = process.argv[2];
if (!output) throw Error('Usage: node scripts/collect-frontend-notices.mjs <new-output-directory>');
const destination = path.resolve(output);
if (fs.existsSync(destination)) throw Error('Output already exists; preserve the previous build record');
const hash = bytes => crypto.createHash('sha256').update(bytes).digest('hex');
const read = file => fs.readFileSync(file);
const dist = path.join(gui, 'dist');
const identityBytes = read(path.join(dist, 'unvac-ui-build.json'));
const identity = JSON.parse(identityBytes);
const lockBytes = read(path.join(gui, 'package-lock.json'));
const lock = JSON.parse(lockBytes);
if (identity.design_preview !== false) throw Error('A normal GUI build is required');
for (const [name, digest] of Object.entries(identity.files)) {
  const file = path.resolve(dist, name);
  if (!file.startsWith(dist + path.sep) || hash(read(file)) !== digest) throw Error(`Build mismatch: ${name}`);
}
const packages = new Map();
function add(specifier, reason) {
  const pieces = specifier.split('/');
  const name = pieces[0].startsWith('@') ? pieces.slice(0, 2).join('/') : pieces[0];
  if (!/^(@[a-z0-9._-]+\/)?[a-z0-9._-]+$/i.test(name) || name === '..') throw Error(`Invalid package: ${name}`);
  if (!packages.has(name)) packages.set(name, new Set());
  packages.get(name).add(reason);
}
const maps = [];
function visit(dir) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    if (entry.isSymbolicLink()) throw Error('Unexpected link in build output');
    const file = path.join(dir, entry.name);
    if (entry.isDirectory()) visit(file);
    else if (entry.name.endsWith('.map')) {
      const bytes = read(file);
      maps.push({ file: path.relative(dist, file).replaceAll('\\', '/'), sha256: hash(bytes) });
      for (const source of JSON.parse(bytes).sources ?? []) {
        const normalized = source.replaceAll('\\', '/');
        const marker = normalized.lastIndexOf('node_modules/');
        if (marker >= 0) add(normalized.slice(marker + 13), 'generated-source-map');
      }
    }
  }
}
visit(dist);
if (!maps.length || !packages.size) throw Error('No mapped dependency evidence');
const cssBytes = read(path.join(gui, 'src/app.css'));
for (const match of cssBytes.toString().matchAll(/@(?:import|source)\s+"([^"]+)"/g)) {
  const specifier = match[1];
  if (specifier.includes('node_modules/')) add(specifier.split('node_modules/').at(-1), 'stylesheet-source');
  else if (!specifier.startsWith('.')) add(specifier, 'stylesheet-import');
}
const records = [];
const sections = [];
for (const [name, reasons] of [...packages].sort(([a], [b]) => a.localeCompare(b))) {
  const directory = path.join(gui, 'node_modules', name);
  const meta = JSON.parse(read(path.join(directory, 'package.json')));
  if (meta.version !== lock.packages[`node_modules/${name}`]?.version) throw Error(`Installed version differs from lock: ${name}`);
  const notices = fs.readdirSync(directory).filter(n => /^(license|licence|copying|notice)([._-]|$)/i.test(n) && fs.statSync(path.join(directory, n)).isFile());
  if (!notices.length) throw Error(`Missing upstream notice: ${name}`);
  const record = { name, version: meta.version, reasons: [...reasons], notices: [] };
  for (const filename of notices.sort()) {
    const bytes = read(path.join(directory, filename));
    record.notices.push({ file: filename, sha256: hash(bytes) });
    sections.push(`===== ${name} ${meta.version} / ${filename} =====\n\n${bytes.toString('utf8')}\n`);
  }
  records.push(record);
}
const text = sections.join('\n');
const manifest = {
  schema_version: 1,
  scope: 'Generated source-map packages and direct app.css imports/sources; assets and unmapped embedded code require separate review.',
  ui_identity_sha256: hash(identityBytes), package_lock_sha256: hash(lockBytes),
  app_css_sha256: hash(cssBytes), maps, packages: records,
  notice_sha256: hash(text),
};
fs.mkdirSync(destination, { recursive: false });
fs.writeFileSync(path.join(destination, 'THIRD-PARTY-FRONTEND.txt'), text);
fs.writeFileSync(path.join(destination, 'frontend-notices.json'), JSON.stringify(manifest, null, 2) + '\n');
console.log(`Collected ${records.length} package notices for UI ${manifest.ui_identity_sha256}; not full release acceptance`);
