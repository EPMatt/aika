// Reuse the same Cargo build directory to check environment invalidation.
// Only fictional public desktop-client settings are compiled by this check.
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { join } from 'node:path';

const root = fileURLToPath(new URL('..', import.meta.url));
const tool = join(root, 'scripts/tool.mjs');
const cleanEnvironment = { ...process.env };
for (const key of [
  'AIKA_GOOGLE_CLIENT_ID',
  'AIKA_GOOGLE_CLIENT_SECRET',
  'AIKA_TEST_EXPECT_GOOGLE_ID',
  'AIKA_TEST_EXPECT_GOOGLE_SECRET',
])
  delete cleanEnvironment[key];

function build(configuration) {
  const output = execFileSync(
    process.execPath,
    [
      tool,
      '--no-env-file',
      'cargo',
      'test',
      '--manifest-path',
      'src-tauri/Cargo.toml',
      '-p',
      'aika-core',
      '--no-run',
      '--message-format=json',
    ],
    {
      cwd: root,
      env: { ...cleanEnvironment, ...configuration },
      encoding: 'utf8',
      maxBuffer: 16 * 1024 * 1024,
      stdio: ['ignore', 'pipe', 'inherit'],
    },
  );
  const artifacts = output
    .split('\n')
    .filter((line) => line.startsWith('{'))
    .map((line) => JSON.parse(line));
  const executable = artifacts.find(
    (artifact) =>
      artifact.reason === 'compiler-artifact' &&
      artifact.target.name === 'aika_core' &&
      artifact.profile.test &&
      artifact.executable,
  )?.executable;
  assert.ok(executable, 'Cargo did not produce the core test executable');
  return executable;
}
function verify(executable, runtime, id, secret = '') {
  execFileSync(
    executable,
    ['--exact', 'google::tests::embedded_google_configuration_smoke', '--ignored'],
    {
      cwd: root,
      env: {
        ...cleanEnvironment,
        ...runtime,
        AIKA_TEST_EXPECT_GOOGLE_ID: id,
        AIKA_TEST_EXPECT_GOOGLE_SECRET: secret,
      },
      stdio: 'inherit',
    },
  );
}

const first = build({
  AIKA_GOOGLE_CLIENT_ID: 'fictional-build-a',
  AIKA_GOOGLE_CLIENT_SECRET: 'fictional-build-secret-a',
});
verify(first, {}, 'fictional-build-a', 'fictional-build-secret-a');
verify(
  first,
  {
    AIKA_GOOGLE_CLIENT_ID: 'fictional-runtime',
    AIKA_GOOGLE_CLIENT_SECRET: 'fictional-runtime-secret',
  },
  'fictional-runtime',
  'fictional-runtime-secret',
);
verify(first, { AIKA_GOOGLE_CLIENT_ID: 'fictional-runtime' }, 'fictional-runtime');
verify(
  first,
  { AIKA_GOOGLE_CLIENT_SECRET: 'fictional-orphan' },
  'fictional-build-a',
  'fictional-build-secret-a',
);
verify(
  first,
  { AIKA_GOOGLE_CLIENT_ID: ' ', AIKA_GOOGLE_CLIENT_SECRET: 'fictional-orphan' },
  'fictional-build-a',
  'fictional-build-secret-a',
);
const changed = build({ AIKA_GOOGLE_CLIENT_ID: 'fictional-build-b' });
verify(changed, {}, 'fictional-build-b');
// Finish with unconfigured artifacts so later default checks stay local-only.
const unconfigured = build({});
verify(unconfigured, {}, '');
verify(unconfigured, { AIKA_GOOGLE_CLIENT_SECRET: 'fictional-orphan' }, '');
verify(
  unconfigured,
  {
    AIKA_GOOGLE_CLIENT_ID: 'fictional-runtime',
    AIKA_GOOGLE_CLIENT_SECRET: 'fictional-runtime-secret',
  },
  'fictional-runtime',
  'fictional-runtime-secret',
);
console.log('Google build configuration: nine smoke launches across three builds passed.');
