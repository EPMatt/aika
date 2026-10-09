# Repository relocation and history

On October 9, 2026, the user requested moving the repository from `/home/srvadmin/Documents` into `/home/srvadmin/Documents/project-cal`, renaming `master` to `main`, rewriting history so app files are directly at the repository root, and force-pushing `main` to `origin` (`git@github.com:EPMatt/aika.git`).

The implementation first committed phase 4, renamed the local branch, and used git-filter-repo 2.47.0 with `--subdirectory-filter project-cal` in an independent local clone. All seven retained commits were checked: each rewritten root tree exactly equals its original `project-cal` subtree. No source content was changed by filtering. The rewritten Git metadata was then physically moved into `project-cal/.git`; the parent `.git` was moved to a temporary backup. Working app files, the pre-existing `.gitignore` edit, and sibling project directories were preserved.

The origin URL is unchanged. `main` is the publication branch. A force-with-lease push uses the observed prior origin/main tip to prevent overwriting an intervening remote change. No sibling projects were ever committed or pushed.

Historical handoffs keep their original source-revision references. Use this mapping to locate those commits in the rewritten repository:

| Before rewrite | App-root revision | Commit                                                         |
| -------------- | ----------------- | -------------------------------------------------------------- |
| `22f14fa`      | `9bfd1dc`         | Build aika desktop calendar prototype with Linux CI            |
| `c1b7fdf`      | `f72077f`         | Finish phase-one design copy and refresh review artifacts      |
| `1ce59e4`      | `9b9f877`         | Implement phase-two persistent local calendar domain           |
| `433172a`      | `467df3a`         | Complete phase-three calendar gestures and all-day spans       |
| `79d6b8f`      | `c726425`         | Document phase-three architectural decisions and rationale     |
| `634ce36`      | `3d33606`         | Document phase-two design decisions and limitations            |
| `7dc2fcb`      | `491f076`         | Implement phase-four provider proof and desktop quality checks |

The original metadata and an all-refs Git bundle were retained at `/tmp/aika-repository-backup-ed8oyvmq/original.git` and `/tmp/aika-repository-backup-ed8oyvmq/original.bundle`. These are temporary local recovery artifacts, not committed files or permanent backups. The full mapping also lives in `.git/filter-repo/commit-map`.

With app files at the root, the existing GitHub workflow paths now match the repository layout. Relocation and push do not establish hosted CI success or other-platform execution; inspect the workflow run separately.
