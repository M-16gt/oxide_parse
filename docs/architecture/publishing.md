# Publishing to PyPI

Workflow `.github/workflows/publish.yml` builds release/full wheels and the source distribution (`sdist`). Pushing a `v<version>` tag to the private source repository triggers builds, tests, and publishing to PyPI. The tag must match `[project].version` in `pyproject.toml`. Running the workflow manually from a branch builds and tests artifacts without publishing.

## Platforms

| OS | Architectures | Wheel Tag |
|---|---|---|
| Linux glibc 2.17+ | x86_64, aarch64 | manylinux2014 |
| Linux musl 1.2+ / Alpine | x86_64, aarch64 | musllinux_1_2 |
| macOS | Intel x86_64, Apple Silicon arm64 | per-architecture wheel |
| Windows | x86_64, ARM64 | MSVC wheel |

Wheels target PyO3 `abi3-py310` for CPython 3.10+ portability (CPython 3.9 remains supported via source builds or the optional `abi3-py39` feature). Maturin embeds the minimum macOS deployment target (ARM64 requires macOS 11+). Every target installs the built wheel and runs the Python test suite; `musllinux` is verified in Alpine, Rust tests and `sdist` wheel builds run before publishing, and Linux wheels are tested on CPython 3.10 and 3.14 (source builds on CPython 3.9 are verified in CI). PyPy and free-threaded CPython require separate support.

## API Token

Create a PyPI API token in your PyPI account settings scoped to `oxide-parse` (an account-wide token may be needed for a first publish). Store only the `pypi-...` token in `.secrets/pypi-token.txt` (gitignored and excluded from maturin archives). GitHub Actions reads the repository secret `PYPI_API_TOKEN`.

Install and authenticate the GitHub CLI, then run from the repository root:

```powershell
winget install GitHub.cli
gh auth login
./scripts/configure_pypi_secret.ps1
```

The script feeds the token to GitHub CLI over stdin without echoing it. Alternatively, configure `PYPI_API_TOKEN` under **GitHub -> Settings -> Secrets and variables -> Actions**.

## Releasing a Version

Update the version in `pyproject.toml`, `python/oxide_parse/__init__.py`, Cargo manifests, lockfiles, and this document, then commit and tag:

```powershell
git push origin main
git tag v0.9.1
git push origin v0.9.1
```

Publishing runs after all platform builds and test jobs pass.

References: [maturin distribution](https://www.maturin.rs/distribution), [PyPA GitHub Actions publishing guide](https://packaging.python.org/en/latest/guides/publishing-package-distribution-releases-using-github-actions-ci-cd-workflows/).
