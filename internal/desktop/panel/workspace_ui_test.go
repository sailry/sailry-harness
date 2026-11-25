package panel

import "github.com/sailry/sailry-harness/internal/desktop/i18n"

func en(key string, args ...any) string { return i18n.In("en", key, args...) }
