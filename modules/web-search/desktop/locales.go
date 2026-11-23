package desktop

import "embed"

// Locales contains the translations contributed by the web search settings module.
//
//go:embed locales/*.yml
var Locales embed.FS
