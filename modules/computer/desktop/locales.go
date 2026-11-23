package desktop

import "embed"

// Locales contains the translations contributed by the computer use settings module.
//
//go:embed locales/*.yml
var Locales embed.FS
