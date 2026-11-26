package remindersdesktop

import "embed"

// Locales contains the translations contributed by the reminders module.
//
//go:embed locales/*.yml
var Locales embed.FS
