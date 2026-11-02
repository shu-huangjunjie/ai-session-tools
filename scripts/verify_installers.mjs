// Extract both NSIS installers and verify the real payload, not the setup stub's
// architecture (NSIS itself is x86 for both x64 and ARM64 packages).
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const version = JSON.parse(fs.readFileSync(path.join(root, 'package.json'), 'utf8')).version;
const sevenZip = process.argv[2] ?? path.join(root, 'releases/build-tools/7zip/full/7z.exe');
assert(fs.existsSync(sevenZip), 'Pass the path to a full 7-Zip executable as the first argument.');
const output = path.join(root, 'releases/installers', version);
const qaRoot = path.join(root, 'releases/qa');
fs.mkdirSync(qaRoot, { recursive: true });
const stage = fs.mkdtempSync(path.join(qaRoot, `installer-${version}-check-`));
const sha256 = (bytes) => createHash('sha256').update(bytes).digest('hex');
const sums = new Map(fs.readFileSync(path.join(output, `SHA256SUMS-${version}.txt`), 'utf8')
  .trim().split(/\r?\n/).map((line) => line.split(/\s{2}/))
  .map(([hash, name]) => [name, hash]));
function filesIn(directory) {
  return fs.readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const file = path.join(directory, entry.name);
    return entry.isDirectory() ? filesIn(file) : [file];
  });
}
function machine(bytes) {
  assert.equal(bytes.readUInt16LE(0), 0x5a4d);
  const pe = bytes.readUInt32LE(0x3c);
  assert.equal(bytes.readUInt32LE(pe), 0x4550);
  return bytes.readUInt16LE(pe + 4);
}
function normalizedBundleTag(bytes) {
  const normalized = Buffer.from(bytes);
  const marker = Buffer.from('__TAURI_BUNDLE_TYPE_VAR_');
  const offset = normalized.indexOf(marker);
  assert(offset >= 0, 'Missing Tauri bundle type marker');
  const tag = normalized.subarray(offset + marker.length, offset + marker.length + 3).toString();
  assert(['UNK', 'NSS'].includes(tag), `Unexpected bundle type: ${tag}`);
  normalized.write('UNK', offset + marker.length, 'ascii');
  return normalized;
}
const results = [];
for (const [architecture, target, expectedMachine] of [
  ['x64', 'x86_64-pc-windows-msvc', 0x8664],
  ['arm64', 'aarch64-pc-windows-msvc', 0xaa64],
]) {
  const name = `AI-Session-Tools-${version}-Windows-${architecture}-setup.exe`;
  const installer = path.join(output, name);
  const hash = sha256(fs.readFileSync(installer));
  assert.equal(hash, sums.get(name), `Installer checksum mismatch: ${name}`);
  const extracted = path.join(stage, architecture);
  const result = spawnSync(sevenZip, ['x', installer, `-o${extracted}`, '-y'], { encoding: 'utf8' });
  assert.equal(result.status, 0, `Extraction failed: ${result.stderr || result.stdout || result.error}`);
  const files = filesIn(extracted);
  assert(fs.existsSync(path.join(extracted, 'uninstall.exe')), 'Missing uninstaller');
  for (const file of files) {
    const relative = path.relative(extracted, file).replaceAll('\\', '/');
    assert(!/(^|\/)(data|backups|accounts)(\/|$)|(^|\/)auth\.json$|\.(db|sqlite|sqlite3)(-wal|-shm)?$/i.test(relative),
      `Runtime data found inside installer: ${relative}`);
  }
  const payload = fs.readFileSync(path.join(extracted, 'context-vault.exe'));
  assert.equal(machine(payload), expectedMachine, `Wrong ${architecture} application architecture`);
  const targetRoot = process.env.CARGO_TARGET_DIR ? path.resolve(process.env.CARGO_TARGET_DIR) : path.join(root, 'src-tauri/target');
  const built = fs.readFileSync(path.join(targetRoot, target, 'release/context-vault.exe'));
  // Tauri temporarily patches UNK -> NSS while packaging and restores the build
  // output afterwards. Every other byte must match the compiled executable.
  assert(normalizedBundleTag(payload).equals(normalizedBundleTag(built)), 'Unexpected payload modification');
  results.push({ architecture, applicationMachine: `0x${expectedMachine.toString(16)}`,
    installer: name, installerSha256: hash, payloadSha256: sha256(payload), files: files.length });
  process.stdout.write(`PASS ${architecture}: checksum, extraction, native payload and clean package contents.\n`);
}
fs.writeFileSync(path.join(output, 'verification.json'), JSON.stringify({ version, results,
  verificationScope: 'Archive integrity, native payload architecture and package contents. Runtime checks are separate.' }, null, 2) + '\n');
process.stdout.write(`Verified extracted files retained at: ${stage}\n`);
