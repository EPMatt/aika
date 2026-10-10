import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { mkdirSync } from 'node:fs';
const root = fileURLToPath(new URL('..', import.meta.url));
function run(tool, args) {
  const result = spawnSync(process.execPath, ['scripts/tool.mjs', tool, ...args], {
    cwd: root,
    stdio: 'inherit',
  });
  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status ?? 1);
}
run('cargo', [
  'build',
  '--manifest-path',
  'src-tauri/Cargo.toml',
  '-p',
  'aika-domain',
  '--target',
  'wasm32-unknown-unknown',
  '--features',
  'wasm',
  '--release',
  '--locked',
]);
mkdirSync(new URL('../src/wasm/', import.meta.url), { recursive: true });
run('wasm-bindgen', [
  'src-tauri/target/wasm32-unknown-unknown/release/aika_domain.wasm',
  '--out-dir',
  'src/wasm',
  '--target',
  'web',
]);
