package scheduledtasksdesktop

import "embed"

// Locales contains the translations contributed by the scheduled tasks module.
//
//go:embed locales/*.yml
var Locales embed.FS
