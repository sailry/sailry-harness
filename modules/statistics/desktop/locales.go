package statisticsdesktop

import "embed"

// Locales contains the translations contributed by the statistics desktop module.
//
//go:embed locales/*.yml
var Locales embed.FS
