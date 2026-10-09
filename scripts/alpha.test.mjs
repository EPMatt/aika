import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdtemp, mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { collect } from './alpha.mjs';

test('candidate includes exact installer bytes, provenance, notes, and verifiable hashes', async () => {
  const directory = await mkdtemp(join(tmpdir(), 'aika-package-'));
  try {
    const bundle = join(directory, 'bundle');
    await mkdir(bundle);
    await writeFile(join(bundle, 'aika.deb'), 'deb fixture');
    await writeFile(join(bundle, 'aika.AppImage'), 'appimage fixture');
    const notes = join(directory, 'notes.md');
    await writeFile(notes, 'Private alpha; acceptance pending.');
    const options = {
      bundle,
      output: join(directory, 'candidate'),
      notes,
      version: '0.1.0-alpha.1',
      target: 'x86_64-unknown-linux-gnu',
      revision: 'fixture-dirty',
    };
    const manifest = await collect(options);
    assert.equal(manifest.revision, 'fixture-dirty');
    assert.equal(manifest.acceptance, 'pending');
    assert.equal(manifest.artifacts.length, 2);
    for (const artifact of manifest.artifacts) {
      assert.equal(
        await readFile(join(options.output, artifact.file), 'utf8'),
        artifact.file.endsWith('.deb') ? 'deb fixture' : 'appimage fixture',
      );
    }
    for (const line of (await readFile(join(options.output, 'SHA256SUMS'), 'utf8'))
      .trim()
      .split('\n')) {
      const [hash, file] = line.split('  ');
      assert.equal(
        createHash('sha256')
          .update(await readFile(join(options.output, file)))
          .digest('hex'),
        hash,
      );
    }
    await assert.rejects(collect(options), { code: 'EEXIST' });
    await rm(join(bundle, 'aika.deb'));
    await assert.rejects(
      collect({ ...options, output: join(directory, 'missing') }),
      /exactly one .deb/,
    );
    await writeFile(join(bundle, 'aika.deb'), 'deb fixture');
    await writeFile(join(bundle, 'stale.deb'), 'stale');
    await assert.rejects(
      collect({ ...options, output: join(directory, 'stale') }),
      /exactly one .deb/,
    );
    await assert.rejects(collect({ ...options, target: 'unknown' }), /Unsupported/);
    await rm(join(bundle, 'stale.deb'));
    await writeFile(join(bundle, 'aika.deb'), '');
    await assert.rejects(
      collect({ ...options, output: join(directory, 'empty') }),
      /Empty installer/,
    );
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

for (const [target, extension] of [
  ['x86_64-pc-windows-msvc', '.exe'],
  ['aarch64-apple-darwin', '.dmg'],
  ['x86_64-apple-darwin', '.dmg'],
]) {
  test(`collection names and signing status for ${target}`, async () => {
    const directory = await mkdtemp(join(tmpdir(), 'aika-package-'));
    try {
      const bundle = join(directory, 'bundle');
      await mkdir(bundle);
      await writeFile(join(bundle, `installer${extension}`), 'native fixture');
      const notes = join(directory, 'notes.md');
      await writeFile(notes, 'Candidate notes');
      const manifest = await collect({
        bundle,
        output: join(directory, 'candidate'),
        notes,
        version: '0.1.0-alpha.1',
        target,
        revision: 'fixture',
      });
      assert.equal(manifest.artifacts[0].file, `aika-0.1.0-alpha.1-${target}${extension}`);
      assert.equal(manifest.signing, extension === '.dmg' ? 'ad-hoc; not notarized' : 'unsigned');
    } finally {
      await rm(directory, { recursive: true, force: true });
    }
  });
}
