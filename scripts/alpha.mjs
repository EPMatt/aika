import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { mkdir, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { projectEnvironment } from './environment.mjs';

export const targets = {
  'x86_64-unknown-linux-gnu': {
    platform: 'linux',
    bundles: ['deb', 'rpm', 'appimage'],
    extensions: ['.deb', '.rpm', '.AppImage'],
  },
  'x86_64-pc-windows-msvc': { platform: 'win32', bundles: ['nsis'], extensions: ['.exe'] },
  'aarch64-apple-darwin': { platform: 'darwin', bundles: ['dmg'], extensions: ['.dmg'] },
  'x86_64-apple-darwin': { platform: 'darwin', bundles: ['dmg'], extensions: ['.dmg'] },
};

async function files(directory) {
  const entries = await readdir(directory, { withFileTypes: true });
  const nested = await Promise.all(
    entries.map((entry) => {
      const path = join(directory, entry.name);
      return entry.isDirectory() ? files(path) : entry.isFile() ? [path] : [];
    }),
  );
  return nested.flat();
}

export async function installerArtifacts({ bundle, target, version }) {
  const spec = targets[target];
  if (!spec) throw new Error('Unsupported packaging target');
  const installers = (await files(bundle))
    .filter((path) => spec.extensions.some((ext) => path.endsWith(ext)))
    .sort();
  for (const ext of spec.extensions) {
    if (installers.filter((path) => path.endsWith(ext)).length !== 1) {
      throw new Error(`Expected exactly one ${ext} installer; clean/rebuild the bundle directory`);
    }
  }
  const artifacts = [];
  // Read and validate before creating a deliverable directory.
  for (const path of installers) {
    const bytes = await readFile(path);
    if (!bytes.length) throw new Error('Empty installer');
    artifacts.push({
      file: `aika-${version}-${target}${path.endsWith('.AppImage') ? '.AppImage' : path.slice(path.lastIndexOf('.'))}`,
      bytes,
    });
  }
  return artifacts;
}

export async function collect({ bundle, output, target, version, revision, notes }) {
  const spec = targets[target];
  const artifacts = await installerArtifacts({ bundle, target, version });
  const releaseNotes = await readFile(notes);
  await mkdir(output, { recursive: false });
  const manifest = {
    product: 'aika',
    version,
    target,
    revision,
    builtAt: new Date().toISOString(),
    signing: spec.platform === 'darwin' ? 'ad-hoc; not notarized' : 'unsigned',
    acceptance: 'pending',
    artifacts: [],
  };
  for (const artifact of artifacts) {
    await writeFile(join(output, artifact.file), artifact.bytes, {
      mode: artifact.file.endsWith('.AppImage') ? 0o755 : 0o644,
    });
    manifest.artifacts.push({
      file: artifact.file,
      bytes: artifact.bytes.length,
      sha256: createHash('sha256').update(artifact.bytes).digest('hex'),
    });
  }
  await writeFile(join(output, 'RELEASE-NOTES.md'), releaseNotes);
  await writeFile(join(output, 'manifest.json'), `${JSON.stringify(manifest, null, 2)}\n`);
  const hashes = [...manifest.artifacts];
  for (const file of ['manifest.json', 'RELEASE-NOTES.md']) {
    hashes.push({
      file,
      sha256: createHash('sha256')
        .update(await readFile(join(output, file)))
        .digest('hex'),
    });
  }
  await writeFile(
    join(output, 'SHA256SUMS'),
    hashes.map(({ file, sha256 }) => `${sha256}  ${file}\n`).join(''),
  );
  return manifest;
}

async function main() {
  const root = fileURLToPath(new URL('..', import.meta.url));
  const env = projectEnvironment(root);
  const [target, ...extra] = process.argv.slice(2);
  const spec = targets[target];
  if (!spec || extra.length || spec.platform !== process.platform)
    throw new Error('Usage: pnpm alpha:build <native target triple>');
  if (env.CARGO_TARGET_DIR || env.CARGO_BUILD_TARGET)
    throw new Error('Unset Cargo target directory/target overrides for alpha builds');
  const pkg = JSON.parse(await readFile(join(root, 'package.json'), 'utf8'));
  const config = JSON.parse(await readFile(join(root, 'src-tauri/tauri.conf.json'), 'utf8'));
  const cargo = await readFile(join(root, 'src-tauri/Cargo.toml'), 'utf8');
  if (
    pkg.version !== config.version ||
    cargo.match(/\[package\][\s\S]*?^version\s*=\s*"([^"]+)"/m)?.[1] !== pkg.version
  )
    throw new Error('Package, Tauri, and Cargo versions must match');
  const output = join(root, 'artifacts', `aika-${pkg.version}-${target}`);
  await mkdir(join(root, 'artifacts'), { recursive: true });
  // Refuse overwriting a previous candidate, including its recorded checksums.
  try {
    await readdir(output);
    throw new Error(`Candidate already exists: ${output}`);
  } catch (error) {
    if (error.code !== 'ENOENT') throw error;
  }
  const git = (args) => {
    const result = spawnSync('git', args, { cwd: root, env, encoding: 'utf8' });
    if (result.status !== 0) throw new Error('Could not record source revision');
    return result.stdout.trim();
  };
  const revision =
    git(['rev-parse', 'HEAD']) + (git(['status', '--porcelain']).length ? '-dirty' : '');
  const bundle = join(root, 'src-tauri/target', target, 'release/bundle');
  await rm(bundle, { recursive: true, force: true });
  const build = spawnSync(
    process.execPath,
    [
      join(root, 'scripts/tool.mjs'),
      'tauri',
      'build',
      '--ci',
      '--target',
      target,
      '--bundles',
      spec.bundles.join(','),
    ],
    { cwd: root, env, stdio: 'inherit' },
  );
  if (build.error || build.status !== 0)
    throw new Error('Alpha release build failed; no candidate collected');
  const manifest = await collect({
    bundle,
    output,
    target,
    version: pkg.version,
    revision,
    notes: join(root, 'docs/alpha/RELEASE-NOTES.md'),
  });
  console.log(
    `Private candidate: ${output}\n${manifest.artifacts.map((artifact) => artifact.file).join('\n')}`,
  );
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch((error) => {
    console.error(error.message);
    process.exitCode = 1;
  });
}
