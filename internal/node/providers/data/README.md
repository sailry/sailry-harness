# Bundled model metadata

`model-catalog.json` is a deterministic snapshot of the public
[models.dev API](https://models.dev/api.json), distributed under its
[MIT license](LICENSE.models.dev). It preserves the upstream model identifiers,
limits, modalities, reasoning options, prices and other metadata. Each model is
kept on one line so generated changes can be reviewed.

The Node uses this snapshot when it has no successfully downloaded catalog.
Successful downloads take precedence; failed downloads do not erase available
metadata. Bundled data has no fabricated network-update timestamp. Benchmark
scores are a separate dataset and are not included in this snapshot.

From the repository root, regenerate it with:

```sh
unset GOROOT
export GOTOOLCHAIN=go1.27.3 CGO_ENABLED=0
go run ./cmd/update-model-catalog
```

For an isolated input, use `-input <file>` and optionally `-output <file>`.
The command validates the whole source with the production catalog parser before
replacing the snapshot. Invalid or empty downloads leave the existing file intact.
It preserves numeric precision and unknown fields, sorts object keys and does not
rewrite unchanged content or add fetch timestamps.

`.github/workflows/model-catalog.yml` checks every six hours and supports manual
dispatch on the default branch. It runs the catalog checks before committing a
changed snapshot. Releases embed the snapshot committed at their source revision;
release builds do not fetch mutable data or modify version tags. Runtime manual
updates remain available between releases.

This reference metadata is separate from the reviewed request-parameter policy
catalog under `internal/node/agent/model/data/`. Automated metadata refreshes do
not change that policy.
