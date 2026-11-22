// Package i18n looks up shared and assembled module copy in the window's language.
package i18n

//go:generate go run ./gen -src ../../../locales -keys keys.txt -out locales

import (
	"embed"
	"errors"
	"fmt"
	"io/fs"
	"maps"
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"
	"go.yaml.in/yaml/v3"
)

// Languages are the catalogs shipped, the source language first.
var Languages = []string{"en", "zh-CN", "zh-TW", "ja", "ko", "fr", "de", "es", "pt-BR", "ru"}

//go:embed locales/*.yml
var files embed.FS

// Catalog contains the shared translations and the translations for an assembly.
type Catalog struct {
	messages map[string]map[string]string
}

var shared = &Catalog{messages: load(files, false)}

// New adds the supplied module catalogs to the shared catalog. Each source
// embeds locales/<language>.yml and must define its keys in English.
func New(sources ...fs.FS) *Catalog {
	if len(sources) == 0 {
		return shared
	}
	messages := make(map[string]map[string]string, len(Languages))
	for lang, keys := range shared.messages {
		messages[lang] = maps.Clone(keys)
	}
	for i, source := range sources {
		module := load(source, true)
		for lang, keys := range module {
			for key, value := range keys {
				if _, exists := messages[lang][key]; exists {
					panic(fmt.Sprintf("i18n: module catalog %d (%s) redefines %q", i+1, lang, key))
				}
				messages[lang][key] = value
			}
		}
	}
	return &Catalog{messages: messages}
}

func load(source fs.FS, module bool) map[string]map[string]string {
	out := map[string]map[string]string{}
	for _, lang := range Languages {
		path := "locales/" + lang + ".yml"
		data, err := fs.ReadFile(source, path)
		if module && lang != "en" && errors.Is(err, fs.ErrNotExist) {
			continue
		}
		if err != nil {
			panic(err)
		}
		m := map[string]string{}
		if err := yaml.Unmarshal(data, &m); err != nil {
			panic(fmt.Sprintf("i18n: %s: %v", path, err))
		}
		if module && lang != "en" {
			for key := range m {
				if _, exists := out["en"][key]; !exists {
					panic(fmt.Sprintf("i18n: %s: %q has no English definition", path, key))
				}
			}
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

type catalogKey struct{}

func windowCatalog(c *ui.Context) **Catalog {
	return ui.Local(c.Root(), catalogKey{}, func() *Catalog { return shared })
}

// Bind sets the assembly catalog for this window, including its overlays.
// Call it before building the view. A nil catalog selects the shared copy.
func Bind(c *ui.Context, catalog *Catalog) {
	if bound := windowCatalog(c); bound != nil {
		if catalog == nil {
			catalog = shared
		}
		*bound = catalog
	}
}

// T says key in the frame's language; args are name, value pairs filling
// %{name}. A key missing from a translation falls back to English, and one
// missing everywhere shows as itself (Keys and the tests catch that).
func T(c *ui.Context, key string, args ...any) string {
	if bound := windowCatalog(c); bound != nil {
		return (*bound).In(c.Locale(), key, args...)
	}
	return In(c.Locale(), key, args...)
}

// In looks up shared copy without a window.
func In(lang, key string, args ...any) string {
	return shared.In(lang, key, args...)
}

// In looks up copy in this assembly without a window.
func (c *Catalog) In(lang, key string, args ...any) string {
	lang = Match(lang)
	s, ok := c.messages[lang][key]
	if !ok {
		if s, ok = c.messages["en"][key]; !ok {
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

// Keys returns the keys of a shared catalog.
func Keys(lang string) []string {
	return slices.Collect(maps.Keys(shared.messages[lang]))
}
