package desktop

import "embed"

// Locales contains the translations contributed by the browser settings module.
//
//go:embed locales/*.yml
var Locales embed.FS
