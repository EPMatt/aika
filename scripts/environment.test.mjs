import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { copyFile, mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { projectEnvironment } from './environment.mjs';

async function temporary(body) {
  const directory = await mkdtemp(join(tmpdir(), 'aika-env-'));
  try {
    await body(directory);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
}

test('without .env, inherited variables remain available without mutating the input', () =>
  temporary(async (directory) => {
    const inherited = { AIKA_GOOGLE_CLIENT_ID: 'shell-client', UNCHANGED: 'shell' };
    const actual = projectEnvironment(directory, inherited);
    assert.deepEqual(actual, inherited);
    assert.notEqual(actual, inherited);
  }));

test('.env values override shell values, including explicit empties, with omitted values inherited', () =>
  temporary(async (directory) => {
    await writeFile(
      join(directory, '.env'),
      'AIKA_GOOGLE_CLIENT_ID=file-client\nAIKA_GOOGLE_CLIENT_SECRET=\n',
    );
    const inherited = {
      AIKA_GOOGLE_CLIENT_ID: 'shell-client',
      AIKA_GOOGLE_CLIENT_SECRET: 'shell-secret',
      OTHER: 'preserved',
    };
    assert.deepEqual(projectEnvironment(directory, inherited), {
      AIKA_GOOGLE_CLIENT_ID: 'file-client',
      AIKA_GOOGLE_CLIENT_SECRET: '',
      OTHER: 'preserved',
    });
    assert.equal(inherited.AIKA_GOOGLE_CLIENT_ID, 'shell-client');
  }));

test('supports comments, export, quoting, multiline values, duplicate assignments, and literal dollar text', () =>
  temporary(async (directory) => {
    await writeFile(
      join(directory, '.env'),
      '# comment\r\nexport CLIENT="quoted # value" # trailing\r\nMULTILINE="first\nsecond"\nDUP=first\nDUP=last\nLITERAL=\'$OTHER/path\'\n',
    );
    assert.deepEqual(projectEnvironment(directory, {}), {
      CLIENT: 'quoted # value',
      MULTILINE: 'first\nsecond',
      DUP: 'last',
      LITERAL: '$OTHER/path',
    });
  }));

test('re-reads edits and removal on the next command; an empty file preserves inherited variables', () =>
  temporary(async (directory) => {
    const file = join(directory, '.env');
    await writeFile(file, 'VALUE=first');
    assert.equal(projectEnvironment(directory, {}).VALUE, 'first');
    await writeFile(file, 'VALUE=second');
    assert.equal(projectEnvironment(directory, {}).VALUE, 'second');
    await writeFile(file, '');
    assert.equal(projectEnvironment(directory, { VALUE: 'shell' }).VALUE, 'shell');
    await rm(file);
    assert.equal(projectEnvironment(directory, { VALUE: 'shell' }).VALUE, 'shell');
  }));

test('unreadable .env fails with a value-free explanation', () =>
  temporary(async (directory) => {
    await mkdir(join(directory, '.env'));
    assert.throws(() => projectEnvironment(directory, {}), {
      message: 'Could not read the project .env file. Check that it is a readable file.',
    });
  }));

test('tool commands load project-root .env from another working directory and allow an explicit smoke-test bypass', () =>
  temporary(async (directory) => {
    await mkdir(join(directory, 'scripts'));
    for (const file of ['tool.mjs', 'environment.mjs'])
      await copyFile(new URL(file, import.meta.url), join(directory, 'scripts', file));
    const cli = join(directory, 'node_modules/@tauri-apps/cli');
    await mkdir(cli, { recursive: true });
    await writeFile(
      join(cli, 'tauri.js'),
      'process.stdout.write(JSON.stringify({id:process.env.AIKA_GOOGLE_CLIENT_ID,secret:process.env.AIKA_GOOGLE_CLIENT_SECRET,other:process.env.OTHER}));',
    );
    const cwd = join(directory, 'elsewhere');
    await mkdir(cwd);
    await writeFile(join(cwd, '.env'), 'AIKA_GOOGLE_CLIENT_ID=wrong-directory');
    const invoke = (args = []) =>
      JSON.parse(
        execFileSync(process.execPath, [join(directory, 'scripts/tool.mjs'), ...args, 'tauri'], {
          cwd,
          env: {
            ...process.env,
            AIKA_GOOGLE_CLIENT_ID: 'shell-client',
            AIKA_GOOGLE_CLIENT_SECRET: 'shell-secret',
            OTHER: 'inherited',
          },
          encoding: 'utf8',
        }),
      );
    assert.deepEqual(invoke(), { id: 'shell-client', secret: 'shell-secret', other: 'inherited' });
    await writeFile(
      join(directory, '.env'),
      'AIKA_GOOGLE_CLIENT_ID=file-client\nAIKA_GOOGLE_CLIENT_SECRET="file-secret # literal"',
    );
    assert.deepEqual(invoke(), {
      id: 'file-client',
      secret: 'file-secret # literal',
      other: 'inherited',
    });
    assert.deepEqual(invoke(['--no-env-file']), {
      id: 'shell-client',
      secret: 'shell-secret',
      other: 'inherited',
    });
  }));

test('alpha packaging applies .env before checking build-directory overrides', () =>
  temporary(async (directory) => {
    const targets = {
      linux: 'x86_64-unknown-linux-gnu',
      darwin: 'aarch64-apple-darwin',
      win32: 'x86_64-pc-windows-msvc',
    };
    const target = targets[process.platform];
    assert.ok(target, 'unsupported fixture host');
    await mkdir(join(directory, 'scripts'));
    for (const file of ['alpha.mjs', 'environment.mjs'])
      await copyFile(new URL(file, import.meta.url), join(directory, 'scripts', file));
    await writeFile(join(directory, '.env'), 'CARGO_TARGET_DIR=fictional-override');
    assert.throws(
      () =>
        execFileSync(process.execPath, [join(directory, 'scripts/alpha.mjs'), target], {
          cwd: directory,
          env: { ...process.env, CARGO_TARGET_DIR: '', CARGO_BUILD_TARGET: '' },
          stdio: 'pipe',
        }),
      (error) =>
        error.status === 1 &&
        /Unset Cargo target directory\/target overrides for alpha builds/.test(
          error.stderr.toString(),
        ),
    );
  }));
