# Go module dependencies

These are the Go modules linked into the shipped binaries, taken from
`go list -deps` for `cmd/sailry-desktop` and `cmd/sailry-host` (macOS, Linux and
Windows) and `cmd/sailry-mobile` (Android and iOS) at the versions pinned in
`go.mod`. Modules that `go.mod` lists only for tests or tools are not linked and
are not listed.

Every module keeps its own license. The license column names the license
texts found in the notice files at the root of each module in the Go module
cache; it is a summary, and those files are authoritative. License and notice
texts are not copied into this repository. Release packaging copies them from
the module cache into the package (`internal/node/host/deployment/licenses.sh`
for the Host archive).

In the **Linked into** column, D is Desktop, H is Host and M is Mobile.

| Module | Version | License | Notice files | Linked into |
| --- | --- | --- | --- | --- |
| `charm.land/fantasy` | v0.46.0 | Apache-2.0 | `LICENSE`, `NOTICE` | D H |
| `cloud.google.com/go` | v0.123.0 | Apache-2.0 | `LICENSE` | D H |
| `cloud.google.com/go/auth` | v0.24.2 | Apache-2.0 | `LICENSE` | D H |
| `cloud.google.com/go/auth/oauth2adapt` | v0.3.0 | Apache-2.0 | `LICENSE` | D H |
| `cloud.google.com/go/compute/metadata` | v0.10.0 | Apache-2.0 | `LICENSE` | D H |
| `dario.cat/mergo` | v1.0.0 | BSD-3-Clause | `LICENSE` | D H |
| `filippo.io/age` | v1.3.2 | BSD-3-Clause | `LICENSE` | D H |
| `filippo.io/edwards25519` | v1.2.0 | BSD-3-Clause | `LICENSE` | D H |
| `filippo.io/hpke` | v0.4.0 | BSD-3-Clause | `LICENSE` | D H |
| `github.com/anthropics/anthropic-sdk-go` | v1.84.0 | MIT | `LICENSE` | D H |
| `github.com/aws/aws-sdk-go-v2` | v1.47.5 | Apache-2.0 | `LICENSE.txt`, `NOTICE.txt` | D H |
| `github.com/aws/aws-sdk-go-v2/aws/protocol/eventstream` | v1.7.25 | Apache-2.0 | `LICENSE.txt` | D H |
| `github.com/aws/aws-sdk-go-v2/config` | v1.33.12 | Apache-2.0 | `LICENSE.txt` | D H |
| `github.com/aws/aws-sdk-go-v2/credentials` | v1.20.12 | Apache-2.0 | `LICENSE.txt` | D H |
| `github.com/aws/aws-sdk-go-v2/feature/ec2/imds` | v1.20.5 | Apache-2.0 | `LICENSE.txt` | D H |
| `github.com/aws/aws-sdk-go-v2/internal/configsources` | v1.5.8 | Apache-2.0 | `LICENSE.txt` | D H |
| `github.com/aws/aws-sdk-go-v2/internal/endpoints/v2` | v2.8.8 | Apache-2.0 | `LICENSE.txt` | D H |
| `github.com/aws/aws-sdk-go-v2/internal/v4a` | v1.5.8 | Apache-2.0 | `LICENSE.txt` | D H |
| `github.com/aws/aws-sdk-go-v2/service/internal/accept-encoding` | v1.13.23 | Apache-2.0 | `LICENSE.txt` | D H |
| `github.com/aws/aws-sdk-go-v2/service/internal/presigned-url` | v1.14.8 | Apache-2.0 | `LICENSE.txt` | D H |
| `github.com/aws/aws-sdk-go-v2/service/signin` | v1.12.0 | Apache-2.0 | `LICENSE.txt` | D H |
| `github.com/aws/aws-sdk-go-v2/service/sso` | v1.39.0 | Apache-2.0 | `LICENSE.txt` | D H |
| `github.com/aws/aws-sdk-go-v2/service/ssooidc` | v1.44.0 | Apache-2.0 | `LICENSE.txt` | D H |
| `github.com/aws/aws-sdk-go-v2/service/sts` | v1.51.6 | Apache-2.0 | `LICENSE.txt` | D H |
| `github.com/aws/smithy-go` | v1.28.7 | Apache-2.0 | `LICENSE`, `NOTICE` | D H |
| `github.com/aymerick/douceur` | v0.2.0 | MIT | `LICENSE` | D H |
| `github.com/bahlo/generic-list-go` | v0.2.0 | BSD-3-Clause | `LICENSE` | D H |
| `github.com/Bios-Marcel/wastebasket/v2` | v2.0.3 | MPL-2.0 | `LICENSE` | D H |
| `github.com/bmatcuk/doublestar/v4` | v4.10.4 | MIT | `LICENSE` | D H |
| `github.com/Broderick-Westrope/pele-go` | v0.2.0 | MIT | `LICENSE` | D |
| `github.com/buger/jsonparser` | v1.1.2 | MIT | `LICENSE` | D H |
| `github.com/caelyreth/untie` | v0.5.0 | Apache-2.0, MIT | `LICENSE-APACHE`, `LICENSE-MIT`, `NOTICE` | D |
| `github.com/caelyreth/untie/contrib/mygo` | v0.5.0 | No notice file in the module | none | D |
| `github.com/cenkalti/backoff/v5` | v5.0.3 | MIT | `LICENSE` | D H |
| `github.com/cespare/xxhash/v2` | v2.3.0 | MIT | `LICENSE.txt` | D H |
| `github.com/charmbracelet/openai-go` | v0.0.0-20260921175203-216db9e71b83 | Apache-2.0 | `LICENSE` | D H |
| `github.com/charmbracelet/x/exp/slice` | v0.1.0 | MIT | `LICENSE` | D H |
| `github.com/chromedp/cdproto` | v0.159.0 | MIT | `LICENSE` | D H |
| `github.com/chromedp/chromedp` | v0.23.0 | MIT | `LICENSE` | D H |
| `github.com/cloudflare/circl` | v1.6.3 | BSD-3-Clause | `LICENSE` | D H |
| `github.com/coder/acp-go-sdk` | v0.13.5 | Apache-2.0 | `LICENSE` | D H |
| `github.com/coder/websocket` | v1.8.15 | ISC | `LICENSE.txt` | D H |
| `github.com/creack/pty/v2` | v2.0.1 | MIT | `LICENSE` | D H |
| `github.com/cyphar/filepath-securejoin` | v0.6.1 | MPL-2.0, BSD-3-Clause | `COPYING.md`, `LICENSE.BSD`, `LICENSE.MPL-2.0` | D H |
| `github.com/dicebear/dicebear-go/v10` | v10.7.0 | MIT | `LICENSE` | D |
| `github.com/dicebear/schema` | v1.5.1 | MIT | `LICENSE` | D |
| `github.com/dlclark/regexp2` | v1.11.0 | MIT | `LICENSE` | D H |
| `github.com/dustin/go-humanize` | v1.1.0 | MIT | `LICENSE` | D H |
| `github.com/ebitengine/purego` | v0.11.3 | Apache-2.0 | `LICENSE` | D H M |
| `github.com/egoist/mygo` | v0.8.4-0.20261109104234-eadf1fce6af3 | MIT | `LICENSE` | D H |
| `github.com/ELY-Open-Source/CrychicUI` | v0.0.0-20261110123803-1c55e6a884bb | MIT; bundled Lucide icons ISC | `LICENSE`, `THIRD_PARTY_NOTICES.md` | D |
| `github.com/emirpasic/gods` | v1.18.1 | BSD-2-Clause, ISC | `LICENSE` | D H |
| `github.com/felixge/httpsnoop` | v1.1.0 | MIT | `LICENSE.txt` | D H |
| `github.com/firede/limu` | v0.5.0 | MIT | `LICENSE` | D |
| `github.com/fsnotify/fsnotify` | v1.10.1 | BSD-3-Clause | `LICENSE` | D H |
| `github.com/gabriel-vasile/mimetype` | v1.4.16 | MIT | `LICENSE` | D H |
| `github.com/getsentry/sentry-go` | v0.50.0 | MIT | `LICENSE` | D H |
| `github.com/go-git/gcfg` | v1.5.1-0.20230307220236-3a3c6141e376 | BSD-3-Clause | `LICENSE` | D H |
| `github.com/go-git/go-billy/v5` | v5.9.2 | Apache-2.0 | `LICENSE` | D H |
| `github.com/go-git/go-git/v5` | v5.19.3 | Apache-2.0 | `LICENSE` | D H |
| `github.com/go-logr/logr` | v1.4.4 | Apache-2.0 | `LICENSE` | D H |
| `github.com/go-logr/stdr` | v1.2.2 | Apache-2.0 | `LICENSE` | D H |
| `github.com/go-ole/go-ole` | v1.2.6 | MIT | `LICENSE` | D H |
| `github.com/go-sql-driver/mysql` | v1.10.1 | MPL-2.0 | `LICENSE` | D H |
| `github.com/go-text/typesetting` | v0.3.5 | BSD-3-Clause, Unlicense | `LICENSE` | D H |
| `github.com/go-typeset/bidi` | v0.3.1 | BSD-3-Clause | `LICENSE` | D |
| `github.com/go-viper/mapstructure/v2` | v2.5.0 | MIT | `LICENSE` | D H |
| `github.com/gobwas/glob` | v0.2.3 | MIT | `LICENSE` | D H |
| `github.com/goccy/go-yaml` | v1.19.2 | MIT | `LICENSE` | D H |
| `github.com/golang-jwt/jwt/v5` | v5.3.1 | MIT | `LICENSE` | D H |
| `github.com/golang/groupcache` | v0.0.0-20241129210726-2c02b8208cf8 | Apache-2.0 | `LICENSE` | D H |
| `github.com/google/go-cmp` | v0.7.0 | BSD-3-Clause | `LICENSE` | D H |
| `github.com/google/jsonschema-go` | v0.4.3 | MIT | `LICENSE` | D H |
| `github.com/google/s2a-go` | v0.1.11 | Apache-2.0 | `LICENSE.md` | D H |
| `github.com/google/uuid` | v1.6.0 | BSD-3-Clause | `LICENSE` | D H M |
| `github.com/googleapis/enterprise-certificate-proxy` | v0.3.24 | Apache-2.0 | `LICENSE` | D H |
| `github.com/googleapis/gax-go/v2` | v2.27.0 | BSD-3-Clause | `LICENSE` | D H |
| `github.com/gorilla/css` | v1.0.1 | BSD-3-Clause | `LICENSE` | D H |
| `github.com/gorilla/websocket` | v1.5.3 | BSD-2-Clause | `LICENSE` | D H |
| `github.com/invopop/jsonschema` | v0.14.0 | MIT | `COPYING` | D H |
| `github.com/jackc/pgpassfile` | v1.0.0 | MIT | `LICENSE` | D H |
| `github.com/jackc/pgservicefile` | v0.0.0-20240606120523-5a60cdf6a761 | MIT | `LICENSE` | D H |
| `github.com/jackc/pgx/v5` | v5.12.0 | MIT | `LICENSE` | D H |
| `github.com/jbenet/go-context` | v0.0.0-20150711004518-d14ea06fba99 | MIT | `LICENSE` | D H |
| `github.com/JohannesKaufmann/dom` | v0.3.1 | MIT | `LICENSE` | D H |
| `github.com/JohannesKaufmann/html-to-markdown/v2` | v2.5.2 | MIT | `LICENSE` | D H |
| `github.com/jolestar/go-commons-pool/v2` | v2.1.2 | Apache-2.0 | `LICENSE` | D H |
| `github.com/jonboulle/clockwork` | v0.5.0 | Apache-2.0 | `LICENSE` | D H |
| `github.com/json-iterator/go` | v1.1.12 | MIT | `LICENSE` | D H |
| `github.com/kaptinlin/jsonpointer` | v0.4.28 | MIT | `LICENSE` | D H |
| `github.com/kaptinlin/jsonschema` | v0.9.10 | MIT | `LICENSE` | D H |
| `github.com/kevinburke/ssh_config` | v1.2.0 | MIT | `LICENSE` | D H |
| `github.com/klauspost/compress` | v1.20.4 | Apache-2.0, MIT, BSD-3-Clause | `LICENSE` | D H M |
| `github.com/klauspost/cpuid/v2` | v2.4.0 | MIT | `LICENSE` | D H M |
| `github.com/klauspost/crc32` | v1.3.0 | BSD-3-Clause | `LICENSE` | D H |
| `github.com/klippa-app/go-pdfium` | v1.21.4 | MIT | `LICENSE` | D H |
| `github.com/kr/fs` | v0.1.0 | BSD-3-Clause | `LICENSE` | D H |
| `github.com/ledongthuc/pdf` | v0.0.0-20261020125853-5e46805f718e | BSD-3-Clause | `LICENSE` | D H |
| `github.com/Masterminds/semver/v3` | v3.5.0 | MIT | `LICENSE.txt` | D H |
| `github.com/mattn/go-isatty` | v0.0.24 | MIT | `LICENSE` | D H |
| `github.com/microcosm-cc/bluemonday` | v1.0.27 | BSD-3-Clause | `LICENSE.md` | D H |
| `github.com/Microsoft/go-winio` | v0.6.2 | MIT | `LICENSE` | D H |
| `github.com/minio/crc64nvme` | v1.1.1 | Apache-2.0 | `LICENSE` | D H |
| `github.com/minio/md5-simd` | v1.1.2 | Apache-2.0, BSD-3-Clause | `LICENSE`, `LICENSE.Golang` | D H |
| `github.com/minio/minio-go/v7` | v7.4.1 | Apache-2.0 | `LICENSE`, `NOTICE` | D H |
| `github.com/modelcontextprotocol/go-sdk` | v1.8.0 | Apache-2.0, MIT | `LICENSE`, `copyright_test.go` | D H |
| `github.com/modern-go/concurrent` | v0.0.0-20180306012644-bacd9c7ef1dd | Apache-2.0 | `LICENSE` | D H |
| `github.com/modern-go/reflect2` | v1.0.2 | Apache-2.0 | `LICENSE` | D H |
| `github.com/MoeForge/moego` | v0.1.0-alpha.1 | MIT | `LICENSE` | D H |
| `github.com/MoeForge/moejs` | v0.1.0 | Apache-2.0 | `LICENSE` | D H |
| `github.com/ncruces/go-strftime` | v1.1.0 | MIT | `LICENSE` | D H |
| `github.com/nikolalohinski/gonja/v2` | v2.5.1 | MIT | `LICENSE` | D H |
| `github.com/odvcencio/gotreesitter` | v0.57.0 | MIT | `LICENSE` | D H |
| `github.com/openai/openai-go/v3` | v3.98.0 | Apache-2.0 | `LICENSE` | D H |
| `github.com/pb33f/ordered-map/v2` | v2.3.1 | Apache-2.0 | `LICENSE` | D H |
| `github.com/philhofer/fwd` | v1.2.0 | MIT | `LICENSE.md` | D H |
| `github.com/pjbgf/sha1cd` | v0.7.0 | Apache-2.0 | `LICENSE` | D H |
| `github.com/pkg/errors` | v0.9.1 | BSD-2-Clause | `LICENSE` | D H |
| `github.com/pkg/sftp` | v1.13.12 | BSD-2-Clause | `LICENSE` | D H |
| `github.com/pkoukk/tiktoken-go` | v0.1.8 | MIT | `LICENSE` | D H |
| `github.com/pkoukk/tiktoken-go-loader` | v0.0.2 | MIT | `LICENSE` | D H |
| `github.com/ProtonMail/go-crypto` | v1.1.6 | BSD-3-Clause | `LICENSE`, `PATENTS` | D H |
| `github.com/remyoudompheng/bigfft` | v0.0.0-20230129092748-24d4a6f8daec | BSD-3-Clause | `LICENSE` | D H |
| `github.com/richardlehane/mscfb` | v1.0.9 | Apache-2.0 | `LICENSE.txt` | D H |
| `github.com/richardlehane/msoleps` | v1.0.6 | Apache-2.0 | `LICENSE.txt` | D H |
| `github.com/rs/xid` | v1.6.0 | MIT | `LICENSE` | D H |
| `github.com/santhosh-tekuri/jsonschema/v6` | v6.0.3 | Apache-2.0 | `LICENSE` | D H |
| `github.com/segmentio/asm` | v1.1.3 | MIT | `LICENSE` | D H |
| `github.com/segmentio/encoding` | v0.5.4 | MIT | `LICENSE` | D H |
| `github.com/sergi/go-diff` | v1.4.0 | MIT | `LICENSE` | D H |
| `github.com/shirou/gopsutil/v4` | v4.26.10 | BSD-3-Clause | `LICENSE` | D H |
| `github.com/sirupsen/logrus` | v1.10.2 | MIT | `LICENSE` | D H |
| `github.com/skeema/knownhosts` | v1.3.1 | Apache-2.0 | `LICENSE`, `NOTICE` | D H |
| `github.com/standard-webhooks/standard-webhooks/libraries` | v0.0.1 | MIT | `LICENSE` | D H |
| `github.com/studio-b12/gowebdav` | v0.13.0 | BSD-3-Clause | `LICENSE` | D H |
| `github.com/tetratelabs/wazero` | v1.12.0 | Apache-2.0 | `LICENSE`, `NOTICE` | D H |
| `github.com/tidwall/gjson` | v1.20.0 | MIT | `LICENSE` | D H |
| `github.com/tidwall/match` | v1.1.1 | MIT | `LICENSE` | D H |
| `github.com/tidwall/pretty` | v1.2.1 | MIT | `LICENSE` | D H |
| `github.com/tidwall/sjson` | v1.2.5 | MIT | `LICENSE` | D H |
| `github.com/tiendc/go-deepcopy` | v1.7.2 | MIT | `LICENSE` | D H |
| `github.com/tinylib/msgp` | v1.6.4 | MIT | `LICENSE` | D H |
| `github.com/tklauser/go-sysconf` | v0.4.0 | BSD-3-Clause | `LICENSE` | D H |
| `github.com/tklauser/numcpus` | v0.12.0 | Apache-2.0 | `LICENSE` | D H |
| `github.com/xanzy/ssh-agent` | v0.3.3 | Apache-2.0 | `LICENSE` | D H |
| `github.com/xuri/efp` | v0.0.3-0.20260924042702-217b81952dec | BSD-3-Clause | `LICENSE` | D H |
| `github.com/xuri/excelize/v2` | v2.11.1 | BSD-3-Clause | `LICENSE` | D H |
| `github.com/xuri/nfp` | v0.0.2-0.20250530014748-2ddeb826f9a9 | BSD-3-Clause | `LICENSE` | D H |
| `github.com/yldm-tech/ai-logo/mygo` | v1.20.0 | MIT | `LICENSE`, `THIRD_PARTY_NOTICES.md` | D |
| `github.com/yosida95/uritemplate/v3` | v3.0.2 | BSD-3-Clause | `LICENSE` | D H |
| `github.com/yuin/goldmark` | v1.8.7 | MIT | `LICENSE` | D H M |
| `github.com/yusufpapurcu/wmi` | v1.2.4 | MIT | `LICENSE` | D H |
| `github.com/zeebo/xxh3` | v1.1.0 | BSD-2-Clause | `LICENSE` | D H |
| `go.opentelemetry.io/auto/sdk` | v1.2.1 | Apache-2.0 | `LICENSE` | D H |
| `go.opentelemetry.io/contrib/instrumentation/google.golang.org/grpc/otelgrpc` | v0.72.0 | Apache-2.0, BSD-3-Clause | `LICENSE` | D H |
| `go.opentelemetry.io/contrib/instrumentation/net/http/otelhttp` | v0.72.0 | Apache-2.0, BSD-3-Clause | `LICENSE` | D H |
| `go.opentelemetry.io/otel` | v1.47.0 | Apache-2.0, BSD-3-Clause | `LICENSE` | D H |
| `go.opentelemetry.io/otel/log` | v1.47.0 | Apache-2.0, BSD-3-Clause | `LICENSE` | D H |
| `go.opentelemetry.io/otel/metric` | v1.47.0 | Apache-2.0, BSD-3-Clause | `LICENSE` | D H |
| `go.opentelemetry.io/otel/trace` | v1.47.0 | Apache-2.0, BSD-3-Clause | `LICENSE` | D H |
| `go.yaml.in/yaml/v3` | v3.0.6 | Apache-2.0, MIT | `LICENSE`, `NOTICE` | D H |
| `go.yaml.in/yaml/v4` | v4.0.0-rc.3 | Apache-2.0 | `LICENSE`, `NOTICE` | D H |
| `golang.org/x/crypto` | v0.58.0 | BSD-3-Clause | `LICENSE`, `PATENTS` | D H |
| `golang.org/x/exp` | v0.0.0-20260410095643-746e56fc9e2f | BSD-3-Clause | `LICENSE`, `PATENTS` | D H |
| `golang.org/x/image` | v0.47.0 | BSD-3-Clause | `LICENSE`, `PATENTS` | D H |
| `golang.org/x/mod` | v0.43.0 | BSD-3-Clause | `LICENSE`, `PATENTS` | D H |
| `golang.org/x/net` | v0.62.0 | BSD-3-Clause | `LICENSE`, `PATENTS` | D H M |
| `golang.org/x/oauth2` | v0.37.0 | BSD-3-Clause | `LICENSE` | D H |
| `golang.org/x/sync` | v0.25.0 | BSD-3-Clause | `LICENSE`, `PATENTS` | D H |
| `golang.org/x/sys` | v0.50.0 | BSD-3-Clause | `LICENSE`, `PATENTS` | D H M |
| `golang.org/x/text` | v0.43.0 | BSD-3-Clause | `LICENSE`, `PATENTS` | D H M |
| `golang.org/x/time` | v0.16.0 | BSD-3-Clause | `LICENSE`, `PATENTS` | D H |
| `google.golang.org/api` | v0.309.0 | BSD-3-Clause | `LICENSE` | D H |
| `google.golang.org/genai` | v1.76.0 | Apache-2.0, BSD-3-Clause | `LICENSE` | D H |
| `google.golang.org/genproto/googleapis/rpc` | v0.0.0-20261026165753-2ffab32bce7e | Apache-2.0 | `LICENSE` | D H |
| `google.golang.org/grpc` | v1.85.0 | Apache-2.0 | `LICENSE`, `NOTICE.txt` | D H |
| `google.golang.org/protobuf` | v1.36.12 | BSD-3-Clause | `LICENSE`, `PATENTS` | D H |
| `gopkg.in/ini.v1` | v1.67.3 | Apache-2.0 | `LICENSE` | D H |
| `gopkg.in/natefinch/lumberjack.v2` | v2.2.1 | MIT | `LICENSE` | D H |
| `gopkg.in/warnings.v0` | v0.1.2 | BSD-2-Clause | `LICENSE` | D H |
| `lukechampine.com/blake3` | v1.4.1 | MIT | `LICENSE` | D H M |
| `modernc.org/libc` | v1.80.1 | BSD-3-Clause, MIT | `LICENSE`, `LICENSE-3RD-PARTY.md` | D H |
| `modernc.org/mathutil` | v1.7.1 | BSD-3-Clause | `LICENSE` | D H |
| `modernc.org/memory` | v1.12.1 | BSD-3-Clause | `LICENSE`, `LICENSE-GO`, `LICENSE-LOGO`, `LICENSE-MMAP-GO` | D H |
| `modernc.org/sqlite` | v1.62.0 | BSD-3-Clause, Apache-2.0, MPL-2.0, MIT | `LICENSE`, `LICENSE-3RD-PARTY.md`, `LICENSE-SQLITE`, `LICENSE-SQLITE_VEC` | D H |
| `rsc.io/qr` | v0.2.0 | BSD-3-Clause | `LICENSE` | D |
