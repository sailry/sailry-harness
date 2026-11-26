package providerstatusdesktop

import "embed"

// Locales contains the translations contributed by the provider-status desktop module.
//
//go:embed locales/*.yml
var Locales embed.FS
