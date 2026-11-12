package preview

import (
	"go/ast"
	"go/parser"
	"go/token"
	"os"
	"path"
	"path/filepath"
	"reflect"
	"slices"
	"sort"
	"strings"
	"testing"
	"unicode"

	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
)

// viewPackages are the packages whose types make a conversation's view
// model, beside this one in the source tree.
var viewPackages = []string{"viewmodel", "conversation", "composer"}

// unshown are the fields no demo conversation sets, each with its reason.
// Everything else the view model declares has to show somewhere.
var unshown = map[string]string{
	"viewmodel.Conversation.Node":              "the live controller's identity of a session",
	"viewmodel.Conversation.Revision":          "the live controller's revision of a session",
	"viewmodel.Conversation.AttentionRevision": "the live controller's revision of a session",
	"viewmodel.Conversation.Archived":          "an archived session is not listed",
	"viewmodel.Conversation.Animating":         "set while a sent reply plays",
	"conversation.Session.Loading":             "a moment before any content, not content",
	"conversation.Session.LoadingOlder":        "a moment, which would keep the window redrawing",
	"conversation.Call.Load":                   "a live disclosure's request",
	"composer.Attachment.Chip":                 "set by a pick in the live browser, which binds the file to its chip",
	"composer.Queued.Load":                     "a live queue's request",
	"composer.Queued.Save":                     "a live queue's request",
	"composer.Queued.Revision":                 "a live queue's revision",
	"composer.Queued.Marker":                   "a continuation is queued only for the moment before it starts",
	"composer.Ask.Busy":                        "a moment while an answer is on its way",
	"conversation.Editing.Pending":             "a moment while a message's replacement is on its way",
}

// typeKey names a type or a field as package.Type.
func typeKey(t reflect.Type) string { return path.Base(t.PkgPath()) + "." + t.Name() }

// own reports a named type of the view model's packages.
func own(t reflect.Type) bool {
	return t.Name() != "" && slices.Contains(viewPackages, path.Base(t.PkgPath())) &&
		strings.HasSuffix(t.PkgPath(), "internal/desktop/"+path.Base(t.PkgPath()))
}

func integer(k reflect.Kind) bool { return k >= reflect.Int && k <= reflect.Uint64 }

// declared walks the types reachable from t: the enumerations and the
// exported fields of the view model's own types.
func declared(t reflect.Type, enums, fields map[string]bool, seen map[reflect.Type]bool) {
	if seen[t] {
		return
	}
	seen[t] = true
	switch t.Kind() {
	case reflect.Pointer, reflect.Slice, reflect.Array:
		declared(t.Elem(), enums, fields, seen)
	case reflect.Map:
		declared(t.Key(), enums, fields, seen)
		declared(t.Elem(), enums, fields, seen)
	case reflect.Struct:
		if !own(t) {
			return
		}
		for i := range t.NumField() {
			if f := t.Field(i); f.IsExported() {
				fields[typeKey(t)+"."+f.Name] = true
				declared(f.Type, enums, fields, seen)
			}
		}
	default:
		if integer(t.Kind()) && own(t) {
			enums[typeKey(t)] = true
		}
	}
}

// shown walks a value of the view model: the enumeration values it holds,
// the fields it sets and every string in it.
type shown struct {
	values map[string]map[int64]bool
	fields map[string]bool
	texts  []string
}

func (s *shown) see(v reflect.Value) {
	t := v.Type()
	switch v.Kind() {
	case reflect.Pointer:
		if !v.IsNil() {
			s.see(v.Elem())
		}
	case reflect.Slice, reflect.Array:
		for i := range v.Len() {
			s.see(v.Index(i))
		}
	case reflect.Map:
		for it := v.MapRange(); it.Next(); {
			s.see(it.Key())
			s.see(it.Value())
		}
	case reflect.String:
		s.texts = append(s.texts, v.String())
	case reflect.Struct:
		if !own(t) {
			return
		}
		for i := range t.NumField() {
			f := t.Field(i)
			if !f.IsExported() {
				continue
			}
			if !v.Field(i).IsZero() {
				s.fields[typeKey(t)+"."+f.Name] = true
			}
			s.see(v.Field(i))
		}
	default:
		if integer(v.Kind()) && own(t) {
			key := typeKey(t)
			if s.values[key] == nil {
				s.values[key] = map[int64]bool{}
			}
			if v.CanInt() {
				s.values[key][v.Int()] = true
			} else {
				s.values[key][int64(v.Uint())] = true
			}
		}
	}
}

// enumerations reads the view model's sources for the constant blocks that
// count a type's values from iota: the names of each type, by value.
func enumerations(t *testing.T) map[string][]string {
	t.Helper()
	out := map[string][]string{}
	for _, pkg := range viewPackages {
		dir := filepath.Join("..", pkg)
		files, err := os.ReadDir(dir)
		if err != nil {
			t.Fatal(err)
		}
		for _, entry := range files {
			name := entry.Name()
			if entry.IsDir() || !strings.HasSuffix(name, ".go") || strings.HasSuffix(name, "_test.go") {
				continue
			}
			file, err := parser.ParseFile(token.NewFileSet(), filepath.Join(dir, name), nil, parser.SkipObjectResolution)
			if err != nil {
				t.Fatal(err)
			}
			for _, decl := range file.Decls {
				block, ok := decl.(*ast.GenDecl)
				if !ok || block.Tok != token.CONST {
					continue
				}
				current := ""
				for i, spec := range block.Specs {
					value := spec.(*ast.ValueSpec)
					switch {
					case i == 0 && value.Type != nil && len(value.Values) == 1:
						kind, named := value.Type.(*ast.Ident)
						first, plain := value.Values[0].(*ast.Ident)
						if named && plain && first.Name == "iota" {
							current = pkg + "." + kind.Name
						}
					case value.Type != nil || len(value.Values) != 0:
						current = "" // the block stops counting
					}
					if current == "" {
						break
					}
					for _, id := range value.Names {
						out[current] = append(out[current], id.Name)
					}
				}
			}
		}
	}
	return out
}

// demoShown is everything the demo shows in English: its dataset and the
// drafts its conversations open with.
func demoShown() (*Data, *shown) {
	d := Demo("en", "local")
	s := &shown{values: map[string]map[int64]bool{}, fields: map[string]bool{}}
	s.see(reflect.ValueOf(d))
	for _, draft := range DemoDrafts("en") {
		s.see(reflect.ValueOf(draft.Text))
		s.see(reflect.ValueOf(draft.Attachments))
	}
	return d, s
}

// The demo (Demo mode) between its conversations shows the whole view model
// in both languages, with DeepSeek's models and nothing that names it a
// sample.
func TestDemo(t *testing.T) {
	enums, fields := map[string]bool{}, map[string]bool{}
	seen := map[reflect.Type]bool{}
	declared(reflect.TypeFor[Conversation](), enums, fields, seen)
	declared(reflect.TypeFor[composer.Attachment](), enums, fields, seen)
	d, s := demoShown()

	// Every value of every enumeration the view model declares shows in some
	// conversation. The values are read from the types' sources, so a kind
	// added to the view model fails here until the demo shows it.
	t.Run("kinds", func(t *testing.T) {
		names := enumerations(t)
		if len(enums) < 12 {
			t.Fatalf("the view model declares %d enumerations: %v", len(enums), enums)
		}
		for _, key := range sorted(enums) {
			values := names[key]
			if len(values) == 0 {
				t.Errorf("%s has no constants counted from iota in its package", key)
				continue
			}
			for value, name := range values {
				if !s.values[key][int64(value)] {
					t.Errorf("no demo conversation shows %s %s", key, name)
				}
			}
		}
	})

	// Every field the view model declares is set somewhere, but the ones
	// listed with a reason.
	t.Run("fields", func(t *testing.T) {
		for key := range unshown {
			if !fields[key] {
				t.Errorf("unshown names %s, which the view model does not declare", key)
			}
		}
		for _, key := range sorted(fields) {
			if !s.fields[key] && unshown[key] == "" {
				t.Errorf("no demo conversation sets %s", key)
			}
		}
	})

	// Both catalogs have the same keys, and neither names the demo.
	t.Run("catalogs", func(t *testing.T) {
		source := DemoCatalog("en")
		if len(source) == 0 || DemoCatalog("fr") != nil {
			t.Fatalf("the English catalog has %d texts; French has %d", len(source), len(DemoCatalog("fr")))
		}
		for _, locale := range DemoLocales {
			catalog := DemoCatalog(locale)
			for key, text := range source {
				if strings.TrimSpace(catalog[key]) == "" {
					t.Errorf("%s has no text for %s", locale, key)
				}
				if locale == "en" && strings.ContainsFunc(text, func(r rune) bool { return unicode.Is(unicode.Han, r) }) {
					t.Errorf("the English text of %s holds Chinese", key)
				}
			}
			for key, text := range catalog {
				if _, ok := source[key]; !ok {
					t.Errorf("%s has %s, which English does not", locale, key)
				}
				lower := strings.ToLower(text)
				for _, word := range []string{"preview", "demo", "sample", "lorem", "placeholder", "预览", "演示", "示例", "样例", "占位"} {
					if strings.Contains(lower, word) {
						t.Errorf("%s text %s says %q", locale, key, word)
					}
				}
			}
		}
		if !strings.ContainsFunc(DemoCatalog("zh-CN")["limit.title"], func(r rune) bool { return unicode.Is(unicode.Han, r) }) {
			t.Error("the Chinese catalog is not Chinese")
		}
	})

	// The demo in Chinese is the same conversations in Chinese; a language
	// it is not written in reads the English one.
	t.Run("languages", func(t *testing.T) {
		zh, fr := Demo("zh-CN", "local"), Demo("fr", "local")
		for _, id := range []string{DemoStart, "lumen-web/metrics", "chat/hashing"} {
			en := d.Find(id)
			if en == nil || zh.Find(id) == nil || fr.Find(id) == nil {
				t.Fatalf("%s is not in every language", id)
			}
			if got := zh.Find(id); got.Title == en.Title || len(got.Turns) != len(en.Turns) || got.Turns[0].User == en.Turns[0].User {
				t.Errorf("%s in Chinese is %q with %d turns, English %q with %d", id, got.Title, len(got.Turns), en.Title, len(en.Turns))
			}
			if got := fr.Find(id); got.Title != en.Title {
				t.Errorf("%s in French is titled %q, want the English %q", id, got.Title, en.Title)
			}
		}
		if d.Find(DemoStart) == nil || d.ProjectOf(DemoStart) == nil {
			t.Errorf("the demo opens on %s, which is not in a project", DemoStart)
		}
		for id := range DemoDrafts("en") {
			if d.Find(id) == nil {
				t.Errorf("a draft waits in %s, which is not a conversation", id)
			}
		}
	})

	// What a run waits on and what a reply links to: a question of every
	// kind, a subagent's session with its queue alone, and the cards of the
	// links that stand alone in a reply.
	t.Run("cards", func(t *testing.T) {
		kinds := map[string]composer.AskKind{DemoStart: composer.AskChoice, "harbor-api/webhooks": composer.AskPlan,
			"lumen-web/tracker": composer.AskForm, "growth-notes/sign-in": composer.AskLink}
		for id, kind := range kinds {
			c := d.Find(id)
			if c == nil || c.Waiting.Question == nil || c.Waiting.Question.Kind != kind {
				t.Errorf("%s does not wait on a question of kind %d", id, kind)
				continue
			}
			if ctx := DemoComposer(d, c, c.Worktree); ctx.Child || ctx.Interactions.Question == nil {
				t.Errorf("the composer of %s is not offered its question", id)
			}
		}
		if q := d.Find("lumen-web/tracker").Waiting.Question; len(q.Fields) < 6 || !q.Declinable {
			t.Errorf("the form has %d fields, declinable %v", len(q.Fields), q.Declinable)
		}
		child := d.Find("lumen-web/audit")
		if ctx := DemoComposer(d, child, ""); !ctx.Child || !ctx.Readonly || len(ctx.Queue) != 2 || ctx.Interactions.Question != nil {
			t.Errorf("a subagent's session is offered child %v, read-only %v, %d queued", ctx.Child, ctx.Readonly, len(ctx.Queue))
		}
		c := d.Find("lumen-web/metrics")
		if e, ok := Embed(c, "reports/metrics-week-45.md", false); !ok || e.File == nil || !e.File.Document {
			t.Errorf("the report's link is %+v, %v, want its file card", e, ok)
		}
		if e, ok := Embed(c, "charts/retention.png", true); !ok || e.Image == nil {
			t.Errorf("the chart's image is %+v, %v, want its picture", e, ok)
		}
		if _, ok := Embed(c, "charts/retention.png", false); ok {
			t.Error("a link to a picture shows as a card")
		}
		if e, ok := Embed(c, "sailry-image:"+c.ID+"/2/shot/0", true); !ok || e.Image == nil || e.Image.Source == nil {
			t.Errorf("the tool's picture is %+v, %v", e, ok)
		}
		if e, ok := Embed(c, "sailry-image:"+c.ID+"/2/shot/1", true); !ok || !e.Missing {
			t.Errorf("the tool's picture that is gone is %+v, %v", e, ok)
		}
		if _, ok := Embed(c, "https://lumen.example/docs", false); ok {
			t.Error("a web link shows as a card")
		}
		if _, ok := Embed(nil, "README.md", false); ok {
			t.Error("a link of no conversation shows as a card")
		}
		if f, ok := Asset(c, c.ID+"/0/report"); !ok || f.Path != "reports/metrics-week-45.md" {
			t.Errorf("the overview's report is %+v, %v", f, ok)
		}
		o := Overview(c)
		if len(o.Citations) != 2 || len(o.Output) < 9 {
			t.Errorf("the overview lists %d cited pages and %d outputs", len(o.Citations), len(o.Output))
		}
		// A request sent again is decided; a failed read of details reads.
		ship := d.Find("harbor-api/deploy")
		before := len(ship.Waiting.Approvals)
		for _, a := range ship.Waiting.Approvals {
			if a.Failed {
				a.Reload()
			}
			if a.Receipt {
				a.Again()
			}
		}
		after := ship.Waiting.Approvals
		if before != 7 || len(after) != 6 || slices.ContainsFunc(after, func(a composer.Approval) bool { return a.Failed || a.Receipt }) {
			t.Errorf("the release waits on %d approvals, then %d", before, len(after))
		}
	})

	// One provider, DeepSeek, wherever a model shows: the composer's models
	// and rail, the /model page, and the subagents' attribution.
	t.Run("models", func(t *testing.T) {
		want := []string{"DeepSeek V4.1 Pro", "DeepSeek V4.1", "DeepSeek V4.1 Flash"}
		var got []string
		for _, m := range d.Models {
			got = append(got, m.Name)
			if m.Provider != "DeepSeek" || m.Family != "deepseek" {
				t.Errorf("%s is of provider %q, family %q", m.Name, m.Provider, m.Family)
			}
		}
		if !slices.Equal(got, want) || DemoModel != want[0] || DemoProvider != "DeepSeek" {
			t.Errorf("the demo's models are %q of %q, want %q", got, DemoProvider, want)
		}
		if len(d.Providers) != 1 || d.Providers[0].Name != "DeepSeek" {
			t.Errorf("the demo's providers are %+v", d.Providers)
		}
		cv := d.Find(DemoStart)
		ctx := DemoComposer(d, cv, cv.Worktree)
		if len(ctx.Models) != 3 || ctx.Models[0].Name != "DeepSeek V4.1 Pro" || ctx.Teams != nil {
			t.Errorf("the composer is offered %d models, the first %q, and %d teams", len(ctx.Models), ctx.Models[0].Name, len(ctx.Teams))
		}
		if len(ctx.Families) != 1 || i18n.In("en", ctx.Families[0].Label) != "DeepSeek" || ctx.Families[0].Icon == nil || icons.Brand("deepseek") == nil {
			t.Errorf("the model picker's rail is %+v", ctx.Families)
		}
		var listed []string
		for _, command := range ctx.Commands {
			if command.ID == "models" {
				for _, m := range command.Children {
					listed = append(listed, m.Label)
				}
			}
		}
		if !slices.Equal(listed, want) {
			t.Errorf("/model lists %q, want %q", listed, want)
		}
		children := 0
		for _, list := range [][]*Conversation{d.Projects[0].Conversations, d.Projects[1].Conversations, d.Projects[2].Conversations, d.Unassigned} {
			for _, c := range list {
				for _, turn := range c.Turns {
					for _, block := range turn.Reply.Blocks {
						for _, child := range block.Children {
							children++
							if !slices.Contains(want, child.Model) {
								t.Errorf("subagent %s runs on %q", child.ID, child.Model)
							}
						}
					}
				}
			}
		}
		if children == 0 {
			t.Error("the demo has no subagent")
		}
		// No text of the dataset names another provider's model or the demo.
		for _, text := range s.texts {
			lower := strings.ToLower(text)
			for _, word := range []string{"claude", "anthropic", "openai", "gpt-", "gemini", "preview", "lorem"} {
				if strings.Contains(lower, word) {
					t.Errorf("the demo says %q in %q", word, text)
				}
			}
		}
	})

	// The Usage page's figures in Demo mode.
	t.Run("usage", testDemoUsage)
}

func sorted(set map[string]bool) []string {
	keys := make([]string, 0, len(set))
	for key := range set {
		keys = append(keys, key)
	}
	sort.Strings(keys)
	return keys
}
