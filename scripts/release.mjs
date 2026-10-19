import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import {
  appendFile,
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
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { installerArtifacts, targets } from './alpha.mjs';
import { projectEnvironment } from './environment.mjs';

const root = fileURLToPath(new URL('..', import.meta.url));
const sha256 = (bytes) => createHash('sha256').update(bytes).digest('hex');

function run(command, args, cwd = root, capture = false, env = process.env) {
  const result = spawnSync(command, args, {
    cwd,
    env,
    encoding: 'utf8',
    stdio: capture ? 'pipe' : 'inherit',
  });
  if (result.error || result.status !== 0)
    throw new Error(result.error?.message || result.stderr || `${command} failed`);
  return result.stdout?.trim();
}
const git = (args, cwd = root) => run('git', args, cwd, true);
const tool = (env, ...args) =>
  run(process.execPath, [join(root, 'scripts/tool.mjs'), ...args], root, false, env);

export function tagVersion(tag) {
  if (!/^v(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(tag))
    throw new Error('Release tag must be vMAJOR.MINOR.PATCH without leading zeros or suffixes');
  return tag.slice(1);
}

export async function validate(tag, cwd = root) {
  const version = tagVersion(tag);
  for (const path of [
    'package.json',
    'src-tauri/tauri.conf.json',
    'src-tauri/Cargo.toml',
    'src-tauri/core/Cargo.toml',
    'src-tauri/domain/Cargo.toml',
    'src-tauri/cli/Cargo.toml',
  ]) {
    const contents = await readFile(join(cwd, path), 'utf8');
    const actual = path.endsWith('.json')
      ? JSON.parse(contents).version
      : contents.match(/\[package\][\s\S]*?^version\s*=\s*"([^"]+)"/m)?.[1];
    if (actual !== version) throw new Error(`${path}: version ${actual} does not match ${tag}`);
  }
  const revision = git(['rev-parse', `refs/tags/${tag}^{commit}`], cwd);
  if (git(['rev-parse', 'HEAD'], cwd) !== revision)
    throw new Error('Checkout does not match release tag');
  try {
    git(['merge-base', '--is-ancestor', revision, 'refs/remotes/origin/main'], cwd);
  } catch {
    throw new Error('Release tag commit must be contained in origin/main');
  }
  return { tag, version, revision };
}

export async function collectRelease({ bundle, output, target, version, revision }) {
  const artifacts = await installerArtifacts({ bundle, target, version });
  const manifest = {
    product: 'aika',
    version,
    target,
    revision,
    builtAt: new Date().toISOString(),
    signing: targets[target].platform === 'darwin' ? 'ad-hoc; not notarized' : 'unsigned',
    cli: { bundled: true, version },
    artifacts: artifacts.map(({ file, bytes }) => ({
      file,
      bytes: bytes.length,
      sha256: sha256(bytes),
    })),
  };
  await mkdir(output, { recursive: false });
  for (const { file, bytes } of artifacts)
    await writeFile(join(output, file), bytes, {
      mode: file.endsWith('.AppImage') ? 0o755 : 0o644,
    });
  await writeFile(
    join(output, `aika-${version}-${target}.manifest.json`),
    `${JSON.stringify(manifest, null, 2)}\n`,
  );
  return manifest;
}

export async function verifyAssets(directory, { version, revision }) {
  const expected = [];
  for (const [target, spec] of Object.entries(targets)) {
    const name = `aika-${version}-${target}.manifest.json`;
    const manifest = JSON.parse(await readFile(join(directory, name), 'utf8'));
    const installers = spec.extensions.map((ext) => `aika-${version}-${target}${ext}`).sort();
    if (
      manifest.product !== 'aika' ||
      manifest.version !== version ||
      manifest.target !== target ||
      manifest.revision !== revision ||
      manifest.cli?.bundled !== true ||
      manifest.cli?.version !== version ||
      JSON.stringify(manifest.artifacts.map(({ file }) => file).sort()) !==
        JSON.stringify(installers)
    )
      throw new Error(`Invalid release manifest: ${name}`);
    expected.push(name, ...installers);
    for (const artifact of manifest.artifacts) {
      const bytes = await readFile(join(directory, artifact.file));
      if (!bytes.length || bytes.length !== artifact.bytes || sha256(bytes) !== artifact.sha256)
        throw new Error(`Artifact size/checksum mismatch: ${artifact.file}`);
    }
  }
  const actual = (await readdir(directory)).filter((name) => name !== 'SHA256SUMS').sort();
  if (JSON.stringify(actual) !== JSON.stringify(expected.sort()))
    throw new Error('Release asset set is incomplete or contains unexpected files');
  return (
    await Promise.all(
      expected.map(async (file) => `${sha256(await readFile(join(directory, file)))}  ${file}\n`),
    )
  ).join('');
}

async function build(tag, target) {
  const env = projectEnvironment(root);
  const release = await validate(tag);
  if (git(['status', '--porcelain', '--untracked-files=no']))
    throw new Error('Release builds require a clean tracked checkout');
  const spec = targets[target];
  if (!spec || spec.platform !== process.platform)
    throw new Error('Release builds require a supported native OS target');
  if (env.CARGO_TARGET_DIR || env.CARGO_BUILD_TARGET)
    throw new Error('Unset Cargo target directory/target overrides for release builds');
  const output = join(root, 'artifacts', `release-${target}`);
  try {
    await readdir(output);
    throw new Error(`Release output already exists: ${output}`);
  } catch (error) {
    if (error.code !== 'ENOENT') throw error;
  }
  tool(
    env,
    'cargo',
    'build',
    '--manifest-path',
    'src-tauri/Cargo.toml',
    '-p',
    'aika-cli',
    '--release',
    '--locked',
    '--target',
    target,
  );
  const extension = spec.platform === 'win32' ? '.exe' : '';
  const sidecars = join(root, 'src-tauri/target/release-sidecars');
  await mkdir(sidecars, { recursive: true });
  const staged = join(sidecars, `aika-cli-${target}${extension}`);
  await copyFile(join(root, `src-tauri/target/${target}/release/aika-cli${extension}`), staged);
  await chmod(staged, 0o755);
  const bundle = join(root, `src-tauri/target/${target}/release/bundle`);
  await rm(bundle, { recursive: true, force: true });
  tool(
    env,
    'tauri',
    'build',
    '--ci',
    '--target',
    target,
    '--bundles',
    spec.bundles.join(','),
    '--config',
    JSON.stringify({ bundle: { externalBin: ['target/release-sidecars/aika-cli'] } }),
    '--',
    '--locked',
  );
  await mkdir(join(root, 'artifacts'), { recursive: true });
  await collectRelease({ ...release, target, bundle, output });
}

async function publish(tag, directory) {
  // Recheck the remote tag and main before granting a release public visibility.
  tagVersion(tag);
  git(['fetch', 'origin', '+refs/heads/main:refs/remotes/origin/main']);
  git(['fetch', 'origin', `refs/tags/${tag}`]);
  const remoteRevision = git(['rev-parse', 'FETCH_HEAD^{commit}']);
  const release = await validate(tag);
  if (remoteRevision !== release.revision)
    throw new Error('Remote release tag changed during the build');
  const sums = await verifyAssets(directory, release);
  await writeFile(join(directory, 'SHA256SUMS'), sums);
  const gh = (...args) => run('gh', args, root, true);
  const lookup = spawnSync(
    'gh',
    ['api', `repos/${process.env.GITHUB_REPOSITORY}/releases/tags/${tag}`],
    { encoding: 'utf8' },
  );
  if (lookup.error) throw lookup.error;
  if (lookup.status === 0) {
    if (!JSON.parse(lookup.stdout).draft)
      throw new Error('Refusing to overwrite an already published release');
  } else if (/\(HTTP 404\)/.test(lookup.stderr)) {
    gh(
      'release',
      'create',
      tag,
      '--verify-tag',
      '--generate-notes',
      '--draft',
      '--title',
      `aika ${tag}`,
    );
  } else {
    throw new Error(lookup.stderr || 'Could not inspect existing release');
  }
  const names = (await readdir(directory)).sort();
  // Remove obsolete draft assets before uploading the complete, freshly verified set.
  const existing = JSON.parse(gh('release', 'view', tag, '--json', 'assets')).assets;
  for (const asset of existing)
    if (!names.includes(asset.name)) gh('release', 'delete-asset', tag, asset.name, '--yes');
  gh('release', 'upload', tag, ...names.map((name) => join(directory, name)), '--clobber');
  const remoteNames = JSON.parse(gh('release', 'view', tag, '--json', 'assets'))
    .assets.map(({ name }) => name)
    .sort();
  if (JSON.stringify(remoteNames) !== JSON.stringify(names))
    throw new Error('Uploaded release asset set differs');
  const downloaded = await mkdtemp(join(tmpdir(), 'aika-release-'));
  try {
    gh('release', 'download', tag, '--dir', downloaded);
    const remoteSums = await verifyAssets(downloaded, release);
    if (remoteSums !== sums || (await readFile(join(downloaded, 'SHA256SUMS'), 'utf8')) !== sums)
      throw new Error('Uploaded release checksum verification failed');
  } finally {
    await rm(downloaded, { recursive: true, force: true });
  }
  gh('release', 'edit', tag, '--draft=false');
}

async function main() {
  const [command, tag, argument, ...extra] = process.argv.slice(2);
  if (extra.length) throw new Error('Unexpected release arguments');
  if (command === 'validate' && !argument) {
    const release = await validate(tag);
    if (process.env.GITHUB_OUTPUT)
      await appendFile(
        process.env.GITHUB_OUTPUT,
        `version=${release.version}\nrevision=${release.revision}\n`,
      );
    console.log(JSON.stringify(release));
  } else if (command === 'build' && argument) await build(tag, argument);
  else if (command === 'publish' && argument) await publish(tag, resolve(argument));
  else
    throw new Error(
      'Usage: node scripts/release.mjs validate <tag> | build <tag> <target> | publish <tag> <assets-directory>',
    );
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url))
  main().catch((error) => {
    console.error(error.message);
    process.exitCode = 1;
  });
