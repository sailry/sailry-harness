# Node-owned Python

`internal/node/python` owns bundled Python 3.12.13 and plugin environments. It
verifies the bundled receipt and file inventory; there is no system interpreter
fallback. Lazy verification keeps ordinary Node startup from executing Python.

Packages capture root `requirements.txt` bytes with their exact reference.
Requirements cannot add installer options, includes or source dependencies.
Preparation requires hashed prebuilt packages, uses the captured index/cache,
seals the result and publishes from staging. Failure preserves the prior
environment and installed package. Progress contains identities and stages
without private paths.

Environment identity combines runtime and requirements. Installed packages retain
requirements, while active skills and MCP processes hold explicit leases.
Collection cannot remove a leased environment. Access rechecks sealed files using
stat-bound hash caching, without filesystem watchers.

`load_skill` prepares required dependencies and retains readiness for the turn.
The latest loaded ready skill selects `SAILRY_PYTHON`. Standalone skills use the
bundled interpreter when available; otherwise they return `python: null`.
Required preparation and cancellation failures remain errors.

Command execution refreshes captured access and supplies `SAILRY_PYTHON`,
`SAILRY_SESSION_DIR`, `PIP_CACHE_DIR`, `PYTHONDONTWRITEBYTECODE` and optional
`PIP_INDEX_URL`. Completion releases its lease independently of turn cleanup.
Declared `python`/`python3` MCP launches retain the same manager's interpreter
until their connection closes.

Preparation uses direct argv, an owned Unix process group or Windows Job, one
root wait, a twenty-minute budget, bounded stderr and two-second reap handling.
Unconfirmed cleanup reports uncertainty. Unit fixtures inject the subprocess
boundary; real runtime installation and cross-OS execution are integration checks.
