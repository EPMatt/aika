import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { access, mkdtemp, mkdir, readFile, rm, writeFile } from 'node:fs/promises';
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
    await mkdir(join(bundle, 'rpm'));
    await writeFile(join(bundle, 'rpm', 'aika-0.1.0-alpha.1-1.x86_64.rpm'), 'rpm fixture');
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
    assert.equal(manifest.artifacts.length, 3);
    const fixtures = {
      '.deb': 'deb fixture',
      '.rpm': 'rpm fixture',
      '.AppImage': 'appimage fixture',
    };
    for (const artifact of manifest.artifacts) {
      const extension = Object.keys(fixtures).find((ext) => artifact.file.endsWith(ext));
      assert.ok(extension);
      assert.equal(artifact.file, `aika-${options.version}-${options.target}${extension}`);
      assert.equal(artifact.bytes, Buffer.byteLength(fixtures[extension]));
      assert.equal(
        await readFile(join(options.output, artifact.file), 'utf8'),
        fixtures[extension],
      );
      assert.equal(artifact.sha256, createHash('sha256').update(fixtures[extension]).digest('hex'));
    }
    assert.deepEqual(
      JSON.parse(await readFile(join(options.output, 'manifest.json'), 'utf8')),
      manifest,
    );
    const hashes = (await readFile(join(options.output, 'SHA256SUMS'), 'utf8')).trim().split('\n');
    assert.deepEqual(
      hashes.map((line) => line.split('  ')[1]).sort(),
      [...manifest.artifacts.map(({ file }) => file), 'manifest.json', 'RELEASE-NOTES.md'].sort(),
    );
    for (const line of hashes) {
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

for (const scenario of ['missing', 'duplicate', 'empty']) {
  test(`rejects ${scenario} RPM before creating a candidate`, async () => {
    const directory = await mkdtemp(join(tmpdir(), 'aika-package-'));
    try {
      const bundle = join(directory, 'bundle');
      await mkdir(bundle);
      await writeFile(join(bundle, 'aika.deb'), 'deb fixture');
      await writeFile(join(bundle, 'aika.AppImage'), 'appimage fixture');
      if (scenario !== 'missing') {
        await writeFile(join(bundle, 'aika.rpm'), scenario === 'empty' ? '' : 'rpm fixture');
      }
      if (scenario === 'duplicate') {
        await mkdir(join(bundle, 'stale'));
        await writeFile(join(bundle, 'stale', 'aika.rpm'), 'stale rpm fixture');
      }
      const notes = join(directory, 'notes.md');
      await writeFile(notes, 'Candidate notes');
      const output = join(directory, 'candidate');
      await assert.rejects(
        collect({
          bundle,
          output,
          notes,
          version: '0.1.0-alpha.1',
          target: 'x86_64-unknown-linux-gnu',
          revision: 'fixture',
        }),
        scenario === 'empty' ? /Empty installer/ : /exactly one .rpm/,
      );
      await assert.rejects(access(output), { code: 'ENOENT' });
    } finally {
      await rm(directory, { recursive: true, force: true });
    }
  });
}

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
