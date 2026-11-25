package testassembly

import "embed"

//go:embed locales/*.yml
var Locales embed.FS
