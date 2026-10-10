import { existsSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { delimiter, join } from 'node:path';

const root = fileURLToPath(new URL('..', import.meta.url));
const localCargo = join(root, '.tools', 'cargo');
const localRustup = join(root, '.tools', 'rustup');
const env = { ...process.env };
if (existsSync(localCargo) && existsSync(localRustup)) {
  env.CARGO_HOME = localCargo;
  env.RUSTUP_HOME = localRustup;
  env.PATH = `${join(localCargo, 'bin')}${delimiter}${env.PATH ?? ''}`;
}

const [tool, ...args] = process.argv.slice(2);
if (!['cargo', 'tauri', 'wasm-bindgen', 'rustup'].includes(tool))
  throw new Error('Expected cargo, tauri, wasm-bindgen or rustup');
const result =
  tool !== 'tauri'
    ? spawnSync(tool, args, { cwd: root, env, stdio: 'inherit' })
    : spawnSync(
        process.execPath,
        [join(root, 'node_modules', '@tauri-apps', 'cli', 'tauri.js'), ...args],
        { cwd: root, env, stdio: 'inherit' },
      );
if (result.error) {
  console.error(result.error.message);
  process.exit(1);
}
process.exit(result.status ?? 1);
