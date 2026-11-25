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
	"testing/fstest"

	"github.com/egoist/mygo/ui"
)

// The catalogs hold the listed keys and every key the source names.
func TestCatalogKeys(t *testing.T) {
	const modulesDir = "../../../modules"
	paths, err := filepath.Glob(filepath.Join(modulesDir, "*", "desktop", "locales", "en.yml"))
	if err != nil {
		t.Fatal(err)
	}
	localPaths, err := filepath.Glob("../*/locales/en.yml")
	if err != nil {
		t.Fatal(err)
	}
	for _, path := range localPaths {
		if filepath.Base(filepath.Dir(filepath.Dir(path))) != "i18n" {
			paths = append(paths, path)
		}
	}
	ownerCatalogs := make(map[string]*Catalog, len(paths))
	var sources []fs.FS
	for _, path := range paths {
		desktop := filepath.Dir(filepath.Dir(path))
		source := os.DirFS(desktop)
		sources = append(sources, source)
		owner, err := filepath.Abs(desktop)
		if err != nil {
			t.Fatal(err)
		}
		ownerCatalogs[owner] = New(source)
	}
	all := New(sources...)

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
		if err := s.Err(); err != nil {
			t.Fatal(err)
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
	// The copy of controls removed from the accepted UI stays out
	// (AGENTS.md, Removed UI): the root catalogs still carry it for the Rust
	// application, and listing it here is the first step of bringing such a
	// control back.
	t.Run("RemovedControls", func(t *testing.T) {
		for _, key := range []string{
			"composer_host_hint", "location_project", // the composer's host choice and location dialog
			"reference_preview",                                                 // the @ and / picker's hint
			"approval_deny_reason", "approval_reason", "approval_reason_cancel", // Deny with reason
			"question_pending", "question_accepted", "question_unconsumed", "question_declined", "question_interrupted", // question row state text
			"telemetry_welcome",       // the welcome guide's diagnostics note
			"provider_editor_preview", // the provider editor's preview note
			// the queue rows' wait reasons and the held background results
			"queue_wait_held", "queue_wait_paused", "queue_wait_stopped", "queue_wait_interrupted", "queue_wait_unsent", "queue_wait_limit", "queue_wait_failed",
			// the text size's value beside its slider, and the stepped sizes the slider replaced
			"message_text_size_value", "message_text_small", "message_text_standard", "message_text_large", "message_text_extra_large",
			"settings_language_restart", // the note that a language change needs a restart
			// trust management in the project menu, and its dialog
			"project_trust", "project_trust_preview",
			"project_grant", "project_grant_description", "project_grant_title", "project_revoke", "project_revoke_description", "project_revoke_title",
			"workspace_records", // the project menu's Records item and its dialog
			// the host page's runtime section: its title, notes and the labels of its grid
			"workspace_runtime", "workspace_runtime_preview", "host_runtime_preview", "live_host_runtime_loading",
			"workspace_host_type", "host_active_preview", "host_system_version", "host_timezone",
			"project_empty",                               // the empty state of the host page's projects section
			"settings_sessions", "settings_session_order", // the session order row of Settings › Conversation
			"cli_launch", // the Launch AI CLI dialog of the project menu and page
			// developer-only demo and acceptance controls
			"settings_demo_mode", "settings_demo_mode_description", "settings_test_mode", "settings_test_mode_description",
			"acceptance_title", "acceptance_description", "acceptance_suite", "acceptance_quick", "acceptance_backend", "acceptance_ui", "acceptance_full",
			"acceptance_model", "acceptance_run", "acceptance_stop", "acceptance_report", "acceptance_results", "acceptance_running", "acceptance_passed",
			"acceptance_failed", "acceptance_blocked", "acceptance_skipped", "acceptance_canceled", "acceptance_finished", "acceptance_issues",
			"acceptance_run_failed", "acceptance_run_canceled",
		} {
			if _, listed := all.messages["en"][key]; listed {
				t.Errorf("%s is the copy of a removed control: see Removed UI in AGENTS.md", key)
			}
		}
	})
	// Every literal translation key is available with its owning module or in
	// the shared catalog. In without a window resolves shared keys only.
	t.Run("SourceKeys", func(t *testing.T) {
		fset := token.NewFileSet()
		for _, dir := range []string{"../..", "../../../cmd", modulesDir} {
			err := filepath.WalkDir(dir, func(path string, d fs.DirEntry, err error) error {
				if err != nil || d.IsDir() || !strings.HasSuffix(path, ".go") || strings.HasSuffix(path, "_test.go") {
					return err
				}
				f, err := parser.ParseFile(fset, path, nil, 0)
				if err != nil {
					return err
				}
				catalog := shared
				absolute, err := filepath.Abs(path)
				if err != nil {
					return err
				}
				for owner := filepath.Dir(absolute); ; owner = filepath.Dir(owner) {
					if local := ownerCatalogs[owner]; local != nil {
						catalog = local
						break
					}
					if filepath.Dir(owner) == owner {
						break
					}
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
					messages := catalog.messages["en"]
					if sel.Sel.Name == "In" {
						messages = shared.messages["en"]
					}
					if key, _ := strconv.Unquote(lit.Value); messages[key] == "" {
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
	if got := In("en", "usage_duration_ms", "value", 41); got != "41 ms" {
		t.Errorf("shared duration: %q", got)
	}
	providers := New(os.DirFS("../../../modules/providers/desktop"))
	if got := providers.In("en", "provider_model_count", "count", 3); got != "3 models" {
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
		// Desktop-only keys are translated too, not left to English.
		if got := In(lang, "appearance_blur"); got == In("en", "appearance_blur") {
			t.Errorf("%s desktop key is untranslated: %q", lang, got)
		}
	}
}

func TestBindUsesWindowAssembly(t *testing.T) {
	source := fstest.MapFS{
		"locales/en.yml":    {Data: []byte("module_title: 'Module for %{name}'\nmodule_status: 'Ready'\n")},
		"locales/zh-CN.yml": {Data: []byte("module_title: '%{name}的模块'\n")},
	}
	assembled := New(source)
	bound := assembled
	var first, second, fallback, sharedCopy string
	a := ui.NewTester(func(c *ui.Context) {
		Bind(c, bound)
		first = T(c, "module_title", "name", "Sailry")
		fallback = T(c, "module_status")
		sharedCopy = T(c, "settings")
	}, 100, 100)
	a.SetLocale("zh-CN")
	b := ui.NewTester(func(c *ui.Context) {
		second = T(c, "module_title", "name", "Sailry")
	}, 100, 100)
	a.Frame()
	b.Frame()
	if first != "Sailry的模块" || fallback != "Ready" || sharedCopy != In("zh-CN", "settings") {
		t.Fatalf("assembled window: title=%q, fallback=%q, shared=%q", first, fallback, sharedCopy)
	}
	if value := In("zh-CN", "module_title"); second != "module_title" || value != "module_title" {
		t.Fatalf("unassembled window: %q, shared lookup: %q", second, value)
	}
	bound = nil
	a.Frame()
	if first != "module_title" {
		t.Fatalf("clearing the assembly retained %q", first)
	}
}
