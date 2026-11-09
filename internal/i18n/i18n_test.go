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

// Every key listed is in every catalog, and the catalogs hold nothing else.
func TestCatalogsComplete(t *testing.T) {
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
}

// Every literal key the source passes to i18n.T or i18n.In is in the
// English catalog, so no view shows a raw key.
func TestSourceKeys(t *testing.T) {
	fset := token.NewFileSet()
	for _, dir := range []string{"../../internal", "../../cmd"} {
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
}

func TestPlaceholdersAndMatch(t *testing.T) {
	if got := In("en", "provider_model_count", "count", 3); got != "3 models" {
		t.Errorf("en count: %q", got)
	}
	if got := In("zh-CN", "sidebar_age_minutes", "count", 5); got != "5m" {
		t.Errorf("zh-CN age: %q", got)
	}
	for tag, want := range map[string]string{"zh-Hans-CN": "zh-CN", "zh-CN": "zh-CN", "zh-TW": "en", "en-GB": "en", "de": "en"} {
		if got := Match(tag); got != want {
			t.Errorf("Match(%q) = %q, want %q", tag, got, want)
		}
	}
}
