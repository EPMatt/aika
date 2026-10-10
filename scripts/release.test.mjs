import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import {
  chmod,
  copyFile,
  mkdir,
  mkdtemp,
  readFile,
  readdir,
  rm,
  writeFile,
} from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { delimiter, join } from 'node:path';
import { test } from 'node:test';
import { collectRelease, tagVersion, validate, verifyAssets } from './release.mjs';
import { targets } from './alpha.mjs';

async function temporary(body) {
  const directory = await mkdtemp(join(tmpdir(), 'aika-release-test-'));
  try {
    await body(directory);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
}
function git(directory, ...args) {
  const result = spawnSync('git', args, { cwd: directory, encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr);
  return result.stdout.trim();
}
async function repository(directory) {
  git(directory, 'init', '-b', 'main');
  git(directory, 'config', 'user.name', 'Release test');
  git(directory, 'config', 'user.email', 'release@example.invalid');
  for (const path of ['src-tauri/core', 'src-tauri/cli', 'scripts'])
    await mkdir(join(directory, path), { recursive: true });
  for (const path of ['package.json', 'src-tauri/tauri.conf.json'])
    await writeFile(join(directory, path), '{"version":"1.2.3"}\n');
  for (const path of [
    'src-tauri/Cargo.toml',
    'src-tauri/core/Cargo.toml',
    'src-tauri/cli/Cargo.toml',
  ])
    await writeFile(join(directory, path), '[package]\nname = "fixture"\nversion = "1.2.3"\n');
  git(directory, 'add', '.');
  git(directory, 'commit', '-m', 'Version 1.2.3');
  git(directory, 'tag', 'v1.2.3');
  git(directory, 'update-ref', 'refs/remotes/origin/main', 'HEAD');
  return git(directory, 'rev-parse', 'HEAD');
}
async function assets(directory, revision) {
  const output = join(directory, 'assets');
  await mkdir(output);
  for (const [target, spec] of Object.entries(targets)) {
    const bundle = join(directory, target);
    const collected = join(directory, `collected-${target}`);
    await mkdir(bundle);
    for (const ext of spec.extensions)
      await writeFile(join(bundle, `installer${ext}`), `fixture ${target} ${ext}`);
    const manifest = await collectRelease({
      bundle,
      output: collected,
      target,
      version: '1.2.3',
      revision,
    });
    assert.deepEqual(manifest.cli, { bundled: true, version: '1.2.3' });
    assert.equal(manifest.acceptance, undefined);
    assert.equal(
      manifest.signing,
      spec.platform === 'darwin' ? 'ad-hoc; not notarized' : 'unsigned',
    );
    for (const name of await readdir(collected))
      await copyFile(join(collected, name), join(output, name));
  }
  return output;
}

test('only strict numeric release tags qualify', () => {
  for (const tag of ['v0.0.0', 'v1.2.3', 'v12.345.678'])
    assert.equal(tagVersion(tag), tag.slice(1));
  for (const tag of [
    '1.2.3',
    'v1.2',
    'v01.2.3',
    'v1.02.3',
    'v1.2.03',
    'v1.2.3-alpha.1',
    'v1.2.3+build',
    'v1.2.3\n',
    'v1.a.3',
    'v1.2.3/branch',
  ])
    assert.throws(() => tagVersion(tag), /Release tag/);
});

test('accepts lightweight/annotated tags and older main commits; rejects other commits and checkouts', async () =>
  temporary(async (directory) => {
    const revision = await repository(directory);
    assert.equal((await validate('v1.2.3', directory)).revision, revision);
    git(directory, 'tag', '-d', 'v1.2.3');
    git(directory, 'tag', '-a', 'v1.2.3', '-m', 'Release');
    assert.equal((await validate('v1.2.3', directory)).revision, revision);
    await writeFile(join(directory, 'later'), 'later');
    git(directory, 'add', '.');
    git(directory, 'commit', '-m', 'Later main commit');
    git(directory, 'update-ref', 'refs/remotes/origin/main', 'HEAD');
    await assert.rejects(validate('v1.2.3', directory), /Checkout does not match/);
    git(directory, 'checkout', '--detach', 'v1.2.3');
    assert.equal((await validate('v1.2.3', directory)).revision, revision);
    git(directory, 'checkout', '-b', 'feature');
    await writeFile(join(directory, 'feature'), 'not on main');
    git(directory, 'add', '.');
    git(directory, 'commit', '-m', 'Feature');
    git(directory, 'tag', '-f', 'v1.2.3');
    await assert.rejects(validate('v1.2.3', directory), /must be contained in origin\/main/);
  }));

test('requires matching versions across JSON configs and all Rust packages', async () =>
  temporary(async (directory) => {
    await repository(directory);
    for (const path of [
      'package.json',
      'src-tauri/tauri.conf.json',
      'src-tauri/Cargo.toml',
      'src-tauri/core/Cargo.toml',
      'src-tauri/cli/Cargo.toml',
    ]) {
      const original = await readFile(join(directory, path), 'utf8');
      await writeFile(join(directory, path), original.replace('1.2.3', '1.2.4'));
      await assert.rejects(validate('v1.2.3', directory), /does not match/);
      await writeFile(join(directory, path), original);
    }
  }));

test('validates all target assets, exact filenames, provenance, and combined checksums', async () =>
  temporary(async (directory) => {
    const output = await assets(directory, 'revision');
    const release = { version: '1.2.3', revision: 'revision' };
    const sums = await verifyAssets(output, release);
    assert.equal(sums.trim().split('\n').length, 10);
    await writeFile(join(output, 'SHA256SUMS'), sums);
    assert.equal(await verifyAssets(output, release), sums);
    await assert.rejects(
      verifyAssets(output, { ...release, revision: 'different' }),
      /Invalid release manifest/,
    );
    const file = join(output, 'aika-1.2.3-x86_64-pc-windows-msvc.exe');
    const original = await readFile(file);
    await writeFile(file, 'corrupt');
    await assert.rejects(verifyAssets(output, release), /checksum mismatch/);
    await writeFile(file, original);
    await writeFile(join(output, 'unexpected.exe'), original);
    await assert.rejects(verifyAssets(output, release), /unexpected files/);
    await rm(join(output, 'unexpected.exe'));
    await rm(file);
    await assert.rejects(verifyAssets(output, release), { code: 'ENOENT' });
  }));

for (const problem of ['missing', 'duplicate', 'empty']) {
  test(`rejects ${problem} release installers before writing output`, async () =>
    temporary(async (directory) => {
      const bundle = join(directory, 'bundle');
      await mkdir(bundle);
      if (problem !== 'missing')
        await writeFile(join(bundle, 'installer.exe'), problem === 'empty' ? '' : 'fixture');
      if (problem === 'duplicate') await writeFile(join(bundle, 'stale.exe'), 'fixture');
      const output = join(directory, 'output');
      await assert.rejects(
        collectRelease({
          bundle,
          output,
          target: 'x86_64-pc-windows-msvc',
          version: '1.2.3',
          revision: 'revision',
        }),
        /exactly one|Empty/,
      );
      await assert.rejects(readdir(output), { code: 'ENOENT' });
    }));
}

// Exercise the publication process with real Git and a fake GitHub CLI. No network or releases are created.
for (const scenario of ['new', 'retry', 'published', 'corrupt-download']) {
  test(`publication: ${scenario}`, { skip: process.platform === 'win32' }, async () =>
    temporary(async (directory) => {
      const revision = await repository(directory);
      git(directory, 'remote', 'add', 'origin', directory);
      for (const file of ['release.mjs', 'alpha.mjs'])
        await copyFile(new URL(file, import.meta.url), join(directory, 'scripts', file));
      const output = await assets(directory, revision);
      const bin = join(directory, 'bin');
      await mkdir(bin);
      const statePath = join(directory, 'github.json');
      await writeFile(
        statePath,
        JSON.stringify({
          exists: scenario !== 'new' && scenario !== 'corrupt-download',
          draft: scenario !== 'published',
          assets: scenario === 'retry' ? { 'obsolete.txt': 'obsolete' } : {},
          calls: [],
        }),
      );
      const fake = join(bin, 'gh');
      await writeFile(
        fake,
        `#!/usr/bin/env node
const fs = require('node:fs');
const path = require('node:path');
const args = process.argv.slice(2);
const state = JSON.parse(fs.readFileSync(process.env.FAKE_GH_STATE));
state.calls.push(args.slice(0, 2).join(' '));
const save = () => fs.writeFileSync(process.env.FAKE_GH_STATE, JSON.stringify(state));
save();
if (args[0] === 'api') {
  if (!state.exists) { process.stderr.write('gh: Not Found (HTTP 404)'); process.exit(1); }
  process.stdout.write(JSON.stringify({ draft: state.draft }));
} else if (args[1] === 'create') { state.exists = true; state.draft = true; save(); }
else if (args[1] === 'view') process.stdout.write(JSON.stringify({ assets: Object.keys(state.assets).map(name => ({name})) }));
else if (args[1] === 'delete-asset') { delete state.assets[args[3]]; save(); }
else if (args[1] === 'upload') {
  for (const file of args.slice(3).filter(x => x !== '--clobber')) state.assets[path.basename(file)] = fs.readFileSync(file).toString('base64');
  save();
} else if (args[1] === 'download') {
  const dest = args[args.indexOf('--dir') + 1];
  for (const [name, bytes] of Object.entries(state.assets)) fs.writeFileSync(path.join(dest, name), process.env.FAKE_GH_CORRUPT && name.endsWith('.exe') ? 'corrupt' : Buffer.from(bytes, 'base64'));
} else if (args[1] === 'edit') { state.draft = false; save(); }
else process.exit(2);
`,
      );
      await chmod(fake, 0o755);
      const result = spawnSync(
        process.execPath,
        [join(directory, 'scripts/release.mjs'), 'publish', 'v1.2.3', output],
        {
          encoding: 'utf8',
          env: {
            ...process.env,
            PATH: `${bin}${delimiter}${process.env.PATH}`,
            FAKE_GH_STATE: statePath,
            FAKE_GH_CORRUPT: scenario === 'corrupt-download' ? '1' : '',
            GITHUB_REPOSITORY: 'fixture/aika',
          },
        },
      );
      const state = JSON.parse(await readFile(statePath, 'utf8'));
      if (scenario === 'published') {
        assert.notEqual(result.status, 0);
        assert.match(result.stderr, /already published/);
        assert.deepEqual(state.calls, ['api repos/fixture/aika/releases/tags/v1.2.3']);
      } else if (scenario === 'corrupt-download') {
        assert.notEqual(result.status, 0);
        assert.match(result.stderr, /checksum mismatch/);
        assert.equal(state.draft, true);
        assert.ok(!state.calls.includes('release edit'));
      } else {
        assert.equal(result.status, 0, result.stderr);
        assert.equal(state.draft, false);
        assert.equal(Object.keys(state.assets).length, 11);
        assert.ok(!state.assets['obsolete.txt']);
        assert.equal(state.calls.at(-1), 'release edit');
        assert.equal(state.calls.includes('release create'), scenario === 'new');
      }
    }),
  );
}
