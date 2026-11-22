package desktop

import "embed"

// Locales contains the translations contributed by the module.
//
//go:embed locales/*.yml
var Locales embed.FS
