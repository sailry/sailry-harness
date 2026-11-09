// Package i18n says Sailry's copy in the window's language. Messages are a
// subset of the repository's locales/*.yml, copied under the same keys by
// `go generate` (see gen/main.go), with the same %{name} placeholders.
package i18n

//go:generate go run ./gen -src ../../../locales -keys keys.txt -out locales

import (
	"embed"
	"fmt"
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"
	"go.yaml.in/yaml/v3"
)

// Languages are the catalogs shipped, the source language first.
var Languages = []string{"en", "zh-CN", "zh-TW", "ja", "ko", "fr", "de", "es", "pt-BR", "ru"}

//go:embed locales/*.yml
var files embed.FS

var catalogs = load()

func load() map[string]map[string]string {
	out := map[string]map[string]string{}
	for _, lang := range Languages {
		data, err := files.ReadFile("locales/" + lang + ".yml")
		if err != nil {
			panic(err)
		}
		m := map[string]string{}
		if err := yaml.Unmarshal(data, &m); err != nil {
			panic(fmt.Sprintf("i18n: %s: %v", lang, err))
		}
		out[lang] = m
	}
	return out
}

// Match returns the supported catalog for a BCP 47 or system locale tag.
func Match(tag string) string {
	return MatchPreferred(tag)
}

// MatchPreferred selects the first supported system preference, then English.
func MatchPreferred(tags ...string) string {
	for _, tag := range tags {
		if catalog := preferred(tag); catalog != "" {
			return catalog
		}
	}
	return "en"
}

func preferred(tag string) string {
	normalized := strings.ToLower(strings.ReplaceAll(tag, "_", "-"))
	normalized, _, _ = strings.Cut(normalized, ".")
	parts := strings.Split(normalized, "-")
	switch parts[0] {
	case "zh":
		if slices.Contains(parts, "hant") || !slices.Contains(parts, "hans") && slices.ContainsFunc(parts, func(part string) bool { return part == "tw" || part == "hk" || part == "mo" }) {
			return "zh-TW"
		}
		return "zh-CN"
	case "pt":
		if slices.Contains(parts, "br") {
			return "pt-BR"
		}
	case "en", "ja", "ko", "fr", "de", "es", "ru":
		return parts[0]
	}
	return ""
}

// T says key in the frame's language; args are name, value pairs filling
// %{name}. A key missing from a translation falls back to English, and one
// missing everywhere shows as itself (Keys and the tests catch that).
func T(c *ui.Context, key string, args ...any) string {
	return In(Match(c.Locale()), key, args...)
}

// In says key in the catalog lang.
func In(lang, key string, args ...any) string {
	lang = Match(lang)
	s, ok := catalogs[lang][key]
	if !ok {
		if s, ok = catalogs["en"][key]; !ok {
			return key
		}
	}
	for i := 0; i+1 < len(args); i += 2 {
		// A few reference strings name their values {name}, without %.
		name, v := fmt.Sprint(args[i]), fmt.Sprint(args[i+1])
		s = strings.ReplaceAll(strings.ReplaceAll(s, "%{"+name+"}", v), "{"+name+"}", v)
	}
	return s
}

// Keys returns the keys of a catalog.
func Keys(lang string) []string {
	keys := make([]string, 0, len(catalogs[lang]))
	for k := range catalogs[lang] {
		keys = append(keys, k)
	}
	return keys
}
