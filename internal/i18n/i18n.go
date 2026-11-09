// Package i18n says Sailry's copy in the window's language. Messages are a
// subset of sailry-public's locales/*.yml, copied under the same keys by
// `go generate` (see gen/main.go), with the same %{name} placeholders.
package i18n

//go:generate go run ./gen -src ../../../sailry-public/locales -keys keys.txt -out locales

import (
	"embed"
	"fmt"
	"strings"

	"github.com/egoist/mygo/ui"
	"gopkg.in/yaml.v3"
)

// Languages are the catalogs shipped, the source language first.
var Languages = []string{"en", "zh-CN"}

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

// Match returns the catalog for a BCP 47 tag: any Chinese but the
// traditional scripts reads zh-CN, everything else English.
func Match(tag string) string {
	t := strings.ToLower(tag)
	if strings.HasPrefix(t, "zh") && !strings.Contains(t, "hant") && !strings.Contains(t, "-tw") && !strings.Contains(t, "-hk") {
		return "zh-CN"
	}
	return "en"
}

// T says key in the frame's language; args are name, value pairs filling
// %{name}. A key missing from a translation falls back to English, and one
// missing everywhere shows as itself (Keys and the tests catch that).
func T(c *ui.Context, key string, args ...any) string {
	return In(Match(c.Locale()), key, args...)
}

// In says key in the catalog lang.
func In(lang, key string, args ...any) string {
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
