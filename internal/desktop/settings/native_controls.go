package settings

import "github.com/egoist/mygo/ui"

func DialogWidth(c *ui.Context, size float32) float32 { return min(size, width(c)-48) }

func Labels(c *ui.Context, keys []string) []string { return labels(c, keys) }

func PreviewNotice(c *ui.Context) { notice(c) }

func TargetStatus(c *ui.Context, key string) { settingsTargetStatus(c, key) }
