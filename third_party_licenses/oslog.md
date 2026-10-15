# Native macOS logging

Sailry uses the unmodified `oslog 0.2.1` source at
[steven-joruk/oslog commit 8afad459855d8a77b2c0618fe29016d87674d6a3](https://github.com/steven-joruk/oslog/tree/8afad459855d8a77b2c0618fe29016d87674d6a3),
with its optional global logger disabled. This revision includes the upstream
fix retaining subsystem and category strings for the native log object's
lifetime. The macOS adapter uses its safe API around Apple's public `os/log.h`
macros; Sailry does not implement another native logging ABI.

The original MIT notice is retained in [oslog-LICENSE](oslog-LICENSE).
