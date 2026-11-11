package i18n

import (
	"bufio"
	"go/ast"
	"go/parser"
	"go/token"
	"io/fs"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"testing"
)

// The catalogs hold the listed keys and every key the source names.
func TestCatalogKeys(t *testing.T) {
	// Every key listed is in every catalog, and the catalogs hold nothing else.
	t.Run("CatalogsComplete", func(t *testing.T) {
		f, err := os.Open("keys.txt")
		if err != nil {
			t.Fatal(err)
		}
		defer f.Close()
		want := map[string]bool{}
		s := bufio.NewScanner(f)
		for s.Scan() {
			if k := strings.TrimSpace(s.Text()); k != "" && !strings.HasPrefix(k, "#") {
				want[k] = true
			}
		}
		for _, lang := range Languages {
			keys := Keys(lang)
			if len(keys) != len(want) {
				t.Errorf("%s has %d keys, keys.txt %d", lang, len(keys), len(want))
			}
			for _, k := range keys {
				if !want[k] {
					t.Errorf("%s: %s is not in keys.txt", lang, k)
				}
			}
		}
	})
	// Every literal key the source passes to i18n.T or i18n.In is in the
	// English catalog, so no view shows a raw key.
	t.Run("SourceKeys", func(t *testing.T) {
		fset := token.NewFileSet()
		for _, dir := range []string{"../..", "../../../cmd"} {
			err := filepath.WalkDir(dir, func(path string, d fs.DirEntry, err error) error {
				if err != nil || d.IsDir() || !strings.HasSuffix(path, ".go") || strings.HasSuffix(path, "_test.go") {
					return err
				}
				f, err := parser.ParseFile(fset, path, nil, 0)
				if err != nil {
					return err
				}
				ast.Inspect(f, func(n ast.Node) bool {
					call, ok := n.(*ast.CallExpr)
					if !ok || len(call.Args) < 2 {
						return true
					}
					sel, ok := call.Fun.(*ast.SelectorExpr)
					if !ok || (sel.Sel.Name != "T" && sel.Sel.Name != "In") {
						return true
					}
					if pkg, ok := sel.X.(*ast.Ident); !ok || pkg.Name != "i18n" {
						return true
					}
					lit, ok := call.Args[1].(*ast.BasicLit)
					if !ok || lit.Kind != token.STRING {
						return true
					}
					if key, _ := strconv.Unquote(lit.Value); catalogs["en"][key] == "" {
						t.Errorf("%s: %s is not in the catalog", fset.Position(lit.Pos()), key)
					}
					return true
				})
				return nil
			})
			if err != nil && !os.IsNotExist(err) {
				t.Fatal(err)
			}
		}
	})
}

func TestPlaceholdersAndMatch(t *testing.T) {
	if got := In("en", "provider_model_count", "count", 3); got != "3 models" {
		t.Errorf("en count: %q", got)
	}
	if got := In("zh-CN", "sidebar_age_minutes", "count", 5); got != "5m" {
		t.Errorf("zh-CN age: %q", got)
	}
	for tag, want := range map[string]string{
		"zh-Hans-CN": "zh-CN", "zh-CN": "zh-CN", "zh_CN.UTF-8": "zh-CN", "zh-Hans-TW": "zh-CN",
		"zh-TW": "zh-TW", "zh_HK.UTF-8": "zh-TW", "zh-MO": "zh-TW", "zh-Hant": "zh-TW",
		"en-GB": "en", "de": "de", "de-DE": "de", "fr-CA": "fr", "es-MX": "es",
		"ja-JP": "ja", "ko-KR": "ko", "pt_BR.UTF-8": "pt-BR", "pt-PT": "en", "ru-RU": "ru", "ar-SA": "en",
	} {
		if got := Match(tag); got != want {
			t.Errorf("Match(%q) = %q, want %q", tag, got, want)
		}
	}
	for _, fixture := range []struct {
		tags []string
		want string
	}{
		{[]string{"ar-SA", "es-ES"}, "es"}, {[]string{"fr-CA", "zh-TW"}, "fr"},
		{[]string{"en-US", "zh-CN"}, "en"}, {[]string{"pt-PT", "de-DE"}, "de"}, {nil, "en"},
	} {
		if got := MatchPreferred(fixture.tags...); got != fixture.want {
			t.Errorf("MatchPreferred(%q) = %q, want %q", fixture.tags, got, fixture.want)
		}
	}
	for lang, want := range map[string]string{
		"zh-TW": "設定", "ja": "設定", "ko": "설정", "fr": "Paramètres", "de": "Einstellungen",
		"es": "Configuración", "pt-BR": "Configurações", "ru": "Настройки",
	} {
		if got := In(lang, "settings"); got != want {
			t.Errorf("%s settings: %q, want %q", lang, got, want)
		}
		if got, want := In(lang, "appearance_blur"), In("en", "appearance_blur"); got != want {
			t.Errorf("%s untranslated desktop key: %q, want English %q", lang, got, want)
		}
	}
}
