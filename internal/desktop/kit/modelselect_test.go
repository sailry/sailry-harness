package kit

import (
	"fmt"
	"image/png"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// modelSelectFixture is three providers' models: OpenAI's take efforts and
// have scores, Anthropic's take efforts, the gateway's take none and its
// provider has no brand mark.
func modelSelectFixture() []ModelOption {
	levels := []string{"low", "medium", "high", "xhigh"}
	openai, anthropic := icons.Brand("openai"), icons.Brand("anthropic")
	return []ModelOption{
		{ID: "e5d94674-2f9e-4b43-a92b-8674da01268c/gpt-5.5", Name: "GPT-5.5", Provider: "OpenAI", Mark: openai, Efforts: levels, DefaultEffort: "medium", Score: 71.2},
		{ID: "e5d94674-2f9e-4b43-a92b-8674da01268c/gpt-6-luna", Name: "GPT-6 Luna", Provider: "OpenAI", Mark: openai, Efforts: levels, DefaultEffort: "medium", Score: 47.9},
		{ID: "e5d94674-2f9e-4b43-a92b-8674da01268c/gpt-6-sol", Name: "GPT-6 Sol", Provider: "OpenAI", Mark: openai, Efforts: levels, DefaultEffort: "medium", Score: 69.2},
		{ID: "0b1c/claude-opus-5-5", Name: "Claude Opus 5.5", Provider: "Anthropic", Mark: anthropic, Efforts: []string{"low", "high"}, DefaultEffort: "high"},
		{ID: "0b1c/claude-haiku-5-5", Name: "Claude Haiku 5.5", Provider: "Anthropic", Mark: anthropic},
		{ID: "77aa/qwen-max", Name: "Qwen Max", Provider: "Team gateway"},
		{ID: "77aa/glm-5", Name: "GLM-5", Provider: "Team gateway"},
	}
}

// modelSelectTester draws a ModelSelect 360 wide over props, which a choice
// updates as a call site does.
func modelSelectTester(props *ModelSelectProps, mode theme.Mode, changes *[]string) *ui.Tester {
	tt := ui.NewTesterWith(func(c *ui.Context) {
		theme.Apply(c, "", mode)
		ui.Column(c).Fill().Padding(20).Background(theme.Of(c).Background).Children(func() {
			o := *props
			o.OnChange = func(model, effort string) {
				props.Model, props.Effort = model, effort
				if changes != nil {
					*changes = append(*changes, model+"@"+effort)
				}
			}
			ModelSelect(c.Key("select"), o).Width(360)
		})
	}, 640, 520, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
	tt.SetLocale("en")
	tt.Frame()
	return tt
}

// The model select names models by their display names, groups them under
// their providers, tells the chosen one by its fill, shows the efforts of a
// model that has any beside the list, and reports what is chosen.
func TestModelSelect(t *testing.T) {
	efforts := i18n.In("en", "composer_effort")
	rest := func(tt *ui.Tester) {
		tt.Frame()
		tt.Advance(300 * time.Millisecond)
		tt.Frame()
	}
	open := func(t *testing.T, tt *ui.Tester) {
		t.Helper()
		trigger, ok := tt.Access("Model")
		if !ok {
			t.Fatalf("no trigger: %q", tt.Texts())
		}
		tt.ClickAt(trigger.Bounds.X+trigger.Bounds.W/2, trigger.Bounds.Y+trigger.Bounds.H/2)
		rest(tt)
	}
	options := func(tt *ui.Tester, list string) (found []ui.AccessInfo) {
		box, ok := tt.AccessByRole(ui.RoleListBox, list)
		if !ok {
			return nil
		}
		for _, x := range tt.AccessAll() {
			if x.Role == ui.RoleOption && x.Bounds.X >= box.Bounds.X && x.Bounds.X+x.Bounds.W <= box.Bounds.X+box.Bounds.W+1 {
				found = append(found, x)
			}
		}
		return found
	}
	names := func(list []ui.AccessInfo) (out []string) {
		for _, x := range list {
			out = append(out, x.Name)
		}
		return out
	}
	click := func(t *testing.T, tt *ui.Tester, list, name string) {
		t.Helper()
		for _, x := range options(tt, list) {
			if x.Name == name {
				tt.ClickAt(x.Bounds.X+x.Bounds.W/2, x.Bounds.Y+x.Bounds.H/2)
				rest(tt)
				return
			}
		}
		t.Fatalf("no option %q in %q: %q", name, list, tt.Texts())
	}
	raw := func(t *testing.T, tt *ui.Tester) {
		t.Helper()
		for _, text := range tt.Texts() {
			if strings.Contains(text, "/") || strings.Contains(text, "e5d94674") {
				t.Errorf("a raw model ID shows: %q", text)
			}
		}
		for _, x := range tt.AccessAll() {
			if strings.Contains(x.Name+x.Value, "e5d94674") {
				t.Errorf("assistive technology reads a raw model ID: %+v", x)
			}
		}
	}

	t.Run("trigger", func(t *testing.T) {
		models := modelSelectFixture()
		props := &ModelSelectProps{Label: "Model", Placeholder: "Select model", Models: models}
		tt := modelSelectTester(props, theme.Dark, nil)
		if !tt.HasText("Select model") {
			t.Errorf("no placeholder: %q", tt.Texts())
		}
		props.Model, props.Effort = models[0].ID, "high"
		tt.Frame()
		trigger, _ := tt.Access("Model")
		if !tt.HasText("GPT-5.5") || !tt.HasText("high") || trigger.Value != "GPT-5.5 · high" || trigger.HasPopup != ui.PopupListBox || trigger.Expanded {
			t.Errorf("the trigger shows %q, reads %+v", tt.Texts(), trigger)
		}
		raw(t, tt)
		// A model without efforts shows none, whatever effort is kept.
		props.Model = models[5].ID
		tt.Frame()
		if tt.HasText("high") || !tt.HasText("Qwen Max") {
			t.Errorf("a model without efforts shows %q", tt.Texts())
		}
		// The choice of no model is the trigger's text, not a placeholder.
		props.None, props.Model = "Follow conversation model", ""
		tt.Frame()
		if !tt.HasText("Follow conversation model") || tt.HasText("Select model") {
			t.Errorf("no model shows %q", tt.Texts())
		}
	})

	t.Run("groups and efforts", func(t *testing.T) {
		models := modelSelectFixture()
		var changes []string
		props := &ModelSelectProps{Label: "Model", Models: models, Model: models[5].ID}
		tt := modelSelectTester(props, theme.Dark, &changes)
		open(t, tt)
		raw(t, tt)
		want := []string{"GPT-5.5", "GPT-6 Luna", "GPT-6 Sol", "Claude Opus 5.5", "Claude Haiku 5.5", "Qwen Max", "GLM-5"}
		list := options(tt, "Model")
		if !slices.Equal(names(list), want) {
			t.Fatalf("the list offers %q, want %q", names(list), want)
		}
		// Each provider heads its models, once, above the first of them.
		previous := float32(0)
		for provider, firstModel := range map[string]string{"OpenAI": "GPT-5.5", "Anthropic": "Claude Opus 5.5", "Team gateway": "Qwen Max"} {
			heading, ok := tt.Find(provider)
			row := list[slices.Index(want, firstModel)].Bounds
			if !ok || heading.Y+heading.H > row.Y+1 || row.Y-heading.Y > 40 {
				t.Errorf("the heading %q is at %v, its first model at %v", provider, heading, row)
			}
			previous = max(previous, heading.Y)
		}
		// The chosen row is selected and filled; none is checked; a row is
		// 32 high and ends with its score alone.
		var fills = map[string]ui.Color{}
		for _, x := range list {
			if x.Checked || x.Selected != (x.Name == "Qwen Max") || x.Bounds.H != modelRowHeight {
				t.Errorf("option %+v", x)
			}
			px := tt.Image().RGBAAt(int(x.Bounds.X+3), int(x.Bounds.Y+x.Bounds.H/2))
			fills[x.Name] = ui.RGBA(px.R, px.G, px.B, 1)
		}
		if fills["Qwen Max"] == fills["GLM-5"] || fills["GLM-5"] != fills["GPT-6 Sol"] {
			t.Errorf("fills: chosen %v, others %v and %v", fills["Qwen Max"], fills["GLM-5"], fills["GPT-6 Sol"])
		}
		if !tt.HasText("71.2") || !tt.HasText("47.9") {
			t.Errorf("no scores: %q", tt.Texts())
		}
		// The chosen model takes no effort: no second column.
		if _, ok := tt.AccessByRole(ui.RoleListBox, efforts); ok {
			t.Fatal("a model without efforts shows the effort column")
		}
		narrow, _ := tt.AccessByRole(ui.RoleListBox, "Model")
		// A model with efforts is chosen at its default and leaves the
		// popover open on them; an effort closes it.
		click(t, tt, "Model", "GPT-6 Luna")
		if !slices.Equal(changes, []string{models[1].ID + "@medium"}) {
			t.Fatalf("choosing a model reported %q", changes)
		}
		column := options(tt, efforts)
		if !slices.Equal(names(column), []string{"low", "medium", "high", "xhigh"}) {
			t.Fatalf("the efforts are %q: %q", names(column), tt.Texts())
		}
		for _, x := range column {
			if x.Selected != (x.Name == "medium") || x.Checked {
				t.Errorf("effort %+v", x)
			}
		}
		if wide, _ := tt.AccessByRole(ui.RoleListBox, "Model"); wide.Bounds.W != narrow.Bounds.W || wide.Bounds.X != narrow.Bounds.X {
			t.Errorf("the models were %v and are %v beside the efforts", narrow.Bounds, wide.Bounds)
		}
		click(t, tt, efforts, "xhigh")
		if !slices.Equal(changes, []string{models[1].ID + "@medium", models[1].ID + "@xhigh"}) {
			t.Fatalf("choosing an effort reported %q", changes)
		}
		if _, ok := tt.AccessByRole(ui.RoleListBox, "Model"); ok {
			t.Error("the popover stays open after an effort")
		}
		if trigger, _ := tt.Access("Model"); trigger.Value != "GPT-6 Luna · xhigh" {
			t.Errorf("the trigger reads %q", trigger.Value)
		}
		// Another model with that effort keeps it; one without takes its
		// default; one without efforts closes at once.
		open(t, tt)
		click(t, tt, "Model", "GPT-5.5")
		click(t, tt, "Model", "Claude Opus 5.5")
		click(t, tt, "Model", "Claude Haiku 5.5")
		if got := changes[2:]; !slices.Equal(got, []string{models[0].ID + "@xhigh", models[3].ID + "@high", models[4].ID + "@"}) {
			t.Errorf("later choices reported %q", got)
		}
		if _, ok := tt.AccessByRole(ui.RoleListBox, "Model"); ok {
			t.Error("the popover stays open after a model without efforts")
		}
	})

	t.Run("none and auto", func(t *testing.T) {
		models := modelSelectFixture()
		var changes []string
		props := &ModelSelectProps{Label: "Model", None: "Inherit", AutoEffort: "Auto", Models: models, Model: models[0].ID, Effort: "high"}
		tt := modelSelectTester(props, theme.Light, &changes)
		open(t, tt)
		if got := names(options(tt, "Model")); len(got) != 8 || got[0] != "Inherit" {
			t.Fatalf("the list offers %q", got)
		}
		if got := names(options(tt, efforts)); !slices.Equal(got, []string{"Auto", "low", "medium", "high", "xhigh"}) {
			t.Fatalf("the efforts are %q", got)
		}
		click(t, tt, efforts, "Auto")
		open(t, tt)
		click(t, tt, "Model", "Inherit")
		open(t, tt)
		if want := []string{models[0].ID + "@", "@"}; !slices.Equal(changes, want) {
			t.Errorf("reported %q, want %q", changes, want)
		}
		for _, x := range options(tt, "Model") {
			if x.Selected != (x.Name == "Inherit") {
				t.Errorf("option %+v", x)
			}
		}
		if _, ok := tt.AccessByRole(ui.RoleListBox, efforts); ok {
			t.Error("no model shows the effort column")
		}
	})

	t.Run("keyboard", func(t *testing.T) {
		models := modelSelectFixture()
		var changes []string
		props := &ModelSelectProps{Label: "Model", Models: models, Model: models[4].ID}
		tt := modelSelectTester(props, theme.Dark, &changes)
		open(t, tt)
		key := func(k ui.Key) {
			tt.Key(0, k)
			rest(tt)
		}
		// The focus opens on the chosen row; Down moves, Enter chooses.
		key(ui.KeyDown)
		key(ui.KeyEnter)
		if !slices.Equal(changes, []string{models[5].ID + "@"}) {
			t.Fatalf("Down and Enter reported %q", changes)
		}
		open(t, tt)
		key(ui.KeyUp)
		key(ui.KeyUp)
		// Claude Opus 5.5 has efforts, which show as the keys reach it.
		if got := names(options(tt, efforts)); !slices.Equal(got, []string{"low", "high"}) {
			t.Fatalf("the efforts of the focused model are %q", got)
		}
		key(ui.KeyRight)
		key(ui.KeyUp)
		key(ui.KeyEnter)
		if got := changes[1:]; !slices.Equal(got, []string{models[3].ID + "@low"}) {
			t.Fatalf("Right, Up and Enter reported %q", got)
		}
		open(t, tt)
		key(ui.KeyEscape)
		if _, ok := tt.AccessByRole(ui.RoleListBox, "Model"); ok {
			t.Error("Escape leaves the popover open")
		}
	})

	t.Run("search", func(t *testing.T) {
		models := modelSelectFixture()
		for i := range 3 {
			models = append(models, ModelOption{ID: fmt.Sprintf("77aa/extra-%d", i), Name: fmt.Sprintf("Extra %d", i), Provider: "Team gateway"})
		}
		var changes []string
		props := &ModelSelectProps{Label: "Model", Models: models, Model: models[0].ID, Effort: "low"}
		tt := modelSelectTester(props, theme.Dark, &changes)
		open(t, tt)
		tt.Type("claude")
		rest(tt)
		if got := names(options(tt, "Model")); !slices.Equal(got, []string{"Claude Opus 5.5", "Claude Haiku 5.5"}) {
			t.Fatalf("the query leaves %q", got)
		}
		if tt.HasText("OpenAI") || !tt.HasText("Anthropic") {
			t.Errorf("headings under the query: %q", tt.Texts())
		}
		tt.Key(0, ui.KeyDown)
		rest(tt)
		tt.Key(0, ui.KeyDown)
		rest(tt)
		tt.Key(0, ui.KeyEnter)
		rest(tt)
		if !slices.Equal(changes, []string{models[4].ID + "@"}) {
			t.Fatalf("the keys from the query reported %q", changes)
		}
		open(t, tt)
		tt.Type("gateway")
		rest(tt)
		tt.Key(0, ui.KeyEnter)
		rest(tt)
		if got := changes[1:]; !slices.Equal(got, []string{models[5].ID + "@"}) {
			t.Fatalf("Enter in the query reported %q", got)
		}
		open(t, tt)
		tt.Type("no such model")
		rest(tt)
		if !tt.HasText(i18n.In("en", "composer_model_empty")) || len(options(tt, "Model")) != 0 {
			t.Errorf("a query without matches shows %q", tt.Texts())
		}
		// Seven models have no query.
		props.Models = models[:7]
		tt.Key(0, ui.KeyEscape)
		rest(tt)
		open(t, tt)
		for _, x := range tt.AccessAll() {
			if x.Search {
				t.Error("a short list has a query")
			}
		}
	})

	t.Run("states", func(t *testing.T) {
		models := modelSelectFixture()
		var changes []string
		props := &ModelSelectProps{Label: "Model", Models: models, Model: "9f00/gone-model", Unavailable: "Model unavailable"}
		tt := modelSelectTester(props, theme.Dark, &changes)
		// A model that is not offered shows its ID after the provider's.
		if !tt.HasText("gone-model") || tt.HasText("9f00/gone-model") {
			t.Errorf("an unknown model shows %q", tt.Texts())
		}
		props.Stored = "Gone Model"
		tt.Frame()
		if trigger, _ := tt.Access("Model"); !tt.HasText("Gone Model") || trigger.Value != "Gone Model" || trigger.Description != "Model unavailable" {
			t.Errorf("the unavailable model shows %q, trigger %+v", tt.Texts(), trigger)
		}
		// Its name is muted: less apart from the surface than a chosen
		// model's.
		contrast := func(name string) int {
			r, _ := tt.Find(name)
			most := 0
			base := tt.Image().RGBAAt(int(r.X-2), int(r.Y+r.H/2))
			for y := int(r.Y); y < int(r.Y+r.H); y++ {
				for x := int(r.X); x < int(r.X+r.W); x++ {
					px := tt.Image().RGBAAt(x, y)
					most = max(most, max(int(px.R)-int(base.R), int(base.R)-int(px.R)))
				}
			}
			return most
		}
		muted := contrast("Gone Model")
		open(t, tt)
		for _, x := range options(tt, "Model") {
			if x.Selected {
				t.Errorf("an unavailable choice selects %q", x.Name)
			}
		}
		click(t, tt, "Model", "GLM-5")
		props.Unavailable = ""
		tt.Frame()
		if full := contrast("GLM-5"); !slices.Equal(changes, []string{models[6].ID + "@"}) || muted >= full {
			t.Errorf("chose %q; the unavailable name is %d apart, a chosen one %d", changes, muted, full)
		}
		if trigger, _ := tt.Access("Model"); trigger.Description != "" {
			t.Errorf("an available model keeps the warning %q", trigger.Description)
		}
		// Disabled and read-only controls do not open.
		for _, set := range []func(on bool){func(on bool) { props.Disabled = on }, func(on bool) { props.ReadOnly = on }} {
			set(true)
			tt.Frame()
			open(t, tt)
			if _, ok := tt.AccessByRole(ui.RoleListBox, "Model"); ok {
				t.Error("a disabled or read-only control opens")
			}
			set(false)
		}
		if trigger, _ := tt.Access("Model"); trigger.Disabled {
			t.Error("the control stays disabled")
		}
	})

	if os.Getenv("SAILRY_SNAPSHOTS") != "" {
		modelSelectSnapshots(t, open, rest)
	}
}

// modelSelectSnapshots saves the control, dark and light, under
// .runtime/ui-shots/model-select: closed, open on one provider, open on
// three with efforts, with the query, and with an unavailable choice.
func modelSelectSnapshots(t *testing.T, open func(*testing.T, *ui.Tester), rest func(*ui.Tester)) {
	shots := filepath.Join("..", "..", "..", ".runtime", "ui-shots", "model-select")
	if err := os.MkdirAll(shots, 0o755); err != nil {
		t.Fatal(err)
	}
	all := modelSelectFixture()
	many := slices.Clone(all)
	for _, name := range []string{"DeepSeek V4", "Kimi K3", "Doubao Pro 2"} {
		many = append(many, ModelOption{ID: "77aa/" + name, Name: name, Provider: "Team gateway"})
	}
	cases := []struct {
		name   string
		props  ModelSelectProps
		opened bool
		query  string
	}{
		{"closed", ModelSelectProps{Label: "Model", Models: all, Model: all[0].ID, Effort: "high"}, false, ""},
		{"closed-placeholder", ModelSelectProps{Label: "Model", Placeholder: "Select model", Models: all}, false, ""},
		{"open-one-provider", ModelSelectProps{Label: "Model", None: "Follow conversation model", Models: all[5:], Model: all[5].ID}, true, ""},
		{"open-three-providers-efforts", ModelSelectProps{Label: "Model", Models: all, Model: all[1].ID, Effort: "high"}, true, ""},
		{"open-search", ModelSelectProps{Label: "Model", Models: many, Model: all[3].ID, Effort: "high"}, true, "gpt"},
		{"unavailable", ModelSelectProps{Label: "Model", Models: all, Model: "9f00/gpt-4-legacy", Stored: "GPT-4 Legacy", Unavailable: "Model unavailable"}, false, ""},
	}
	for _, mode := range []theme.Mode{theme.Dark, theme.Light} {
		for _, shot := range cases {
			props := shot.props
			tt := modelSelectTester(&props, mode, nil)
			tt.SetScale(2)
			rest(tt)
			if shot.opened {
				open(t, tt)
				if shot.query != "" {
					tt.Type(shot.query)
					rest(tt)
				}
			}
			suffix := "dark"
			if mode == theme.Light {
				suffix = "light"
			}
			file, err := os.Create(filepath.Join(shots, shot.name+"-"+suffix+".png"))
			if err != nil {
				t.Fatal(err)
			}
			err = png.Encode(file, tt.Image())
			file.Close()
			if err != nil {
				t.Fatal(err)
			}
		}
	}
}
