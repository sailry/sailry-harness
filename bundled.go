package sailry

import "embed"

// Themes is retained for source compatibility. Stock builds do not bundle theme packages.
var Themes embed.FS

// Plugins is retained for source compatibility. Stock builds do not bundle plugin packages.
var Plugins embed.FS
