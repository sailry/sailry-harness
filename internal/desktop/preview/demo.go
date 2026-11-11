package preview

import (
	"embed"
	"fmt"
	"strings"
	"sync"
	"time"

	"github.com/sailry/sailry-harness/internal/desktop/composer"
	cv "github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"go.yaml.in/yaml/v3"
)

// The demo is the dataset of Demo mode (Settings, Experimental): a few
// projects whose conversations between them show every kind of content the
// transcript and the composer draw, written as a product's own work for
// acceptance and for screenshots. Like the rest of the package it is made
// up and in memory: no tool ran, no model answered, nothing is read or
// written. Its prose is in demo/<locale>.yml, one catalog a language; the
// code here is its structure.

//go:embed demo/*.yml
var demoFiles embed.FS

// DemoLocales are the languages the demo is written in, the source first;
// another language reads the source.
var DemoLocales = []string{"en", "zh-CN"}

var demoCatalogs = sync.OnceValue(func() map[string]map[string]string {
	out := map[string]map[string]string{}
	for _, locale := range DemoLocales {
		data, err := demoFiles.ReadFile("demo/" + locale + ".yml")
		if err != nil {
			panic(err)
		}
		m := map[string]string{}
		if err := yaml.Unmarshal(data, &m); err != nil {
			panic(fmt.Sprintf("preview: demo/%s.yml: %v", locale, err))
		}
		for k, v := range m {
			m[k] = strings.TrimRight(v, "\n")
		}
		out[locale] = m
	}
	return out
})

// DemoCatalog is the demo's prose in locale by key, nil for a language it is
// not written in.
func DemoCatalog(locale string) map[string]string { return demoCatalogs()[locale] }

// demo builds the dataset in one language.
type demo struct {
	locale string
	host   string
}

// t is the prose under key: the locale's, or the source's.
func (b *demo) t(key string) string {
	if s, ok := demoCatalogs()[b.locale][key]; ok {
		return s
	}
	s, ok := demoCatalogs()[DemoLocales[0]][key]
	if !ok {
		panic("preview: demo text " + key + " is not written")
	}
	return s
}

// both is a plugin's label, which carries its translations.
func (b *demo) both(key string) composer.Text {
	text := composer.Text{Label: demoCatalogs()[DemoLocales[0]][key], Locales: map[string]string{}}
	for _, locale := range DemoLocales[1:] {
		text.Locales[locale] = demoCatalogs()[locale][key]
	}
	return text
}

// call is a tool call labeled in the demo's language.
func (b *demo) call(id, name, icon, detail string) cv.Call {
	return cv.Call{ID: id + "/" + name + "/" + detail, Name: name, Label: b.t("tool." + name), Icon: icon, Detail: detail, Code: true}
}

func (b *demo) user(id, key string) cv.Turn { return cv.Turn{ID: id, User: b.t(key)} }

func question(id string, st cv.QuestionState, prompt string, options []string, answers ...string) cv.Block {
	return cv.Block{Kind: cv.QuestionBlock, Question: &cv.Question{CallID: id, Prompt: prompt, Options: options, State: st, Answers: answers}}
}

// The demo's models: one provider, DeepSeek.
const (
	DemoProvider = "DeepSeek"
	DemoModel    = "DeepSeek V4.1 Pro"
	demoFast     = "DeepSeek V4.1 Flash"
	demoFamily   = "deepseek"
)

// DemoFamilies are the demo's model picker rail.
var DemoFamilies = []composer.Family{{ID: demoFamily, Label: "provider_deepseek", Icon: icons.Brand(demoFamily)}}

// DemoModels are the demo's models, the selected one first.
func DemoModels() []Model {
	capability := func(ids ...string) []composer.Capability {
		var out []composer.Capability
		for _, id := range ids {
			out = append(out, composer.Capability{ID: id, Label: map[string]string{"vision": "provider_cap_vision", "image": "provider_cap_image"}[id], Icon: icons.Capabilities[id]})
		}
		return out
	}
	score := func(overall float64, tested string, axes ...float64) *composer.Score {
		s := &composer.Score{Overall: overall, Tested: tested}
		copy(s.Axes[:], axes)
		return s
	}
	fast := []composer.Feature{{ID: "speed", Name: "Speed", Options: []composer.FeatureOption{{ID: "fast", Name: "Fast"}}}}
	return []Model{
		{ID: "deepseek-v4.1-pro", Name: DemoModel, Provider: DemoProvider, Reasoning: true, Family: demoFamily,
			Score: score(9.2, "high", 9.4, 8.9, 9.5, 8.7, 9.1), Features: fast, Capabilities: capability("vision", "image")},
		{ID: "deepseek-v4.1", Name: "DeepSeek V4.1", Provider: DemoProvider, Reasoning: true, Family: demoFamily,
			Score: score(8.7, "medium", 8.9, 8.5, 8.8, 8.3, 8.6), Capabilities: capability("vision")},
		{ID: "deepseek-v4.1-flash", Name: demoFast, Provider: DemoProvider, Family: demoFamily},
	}
}

// DemoComposer is what the composer of demo conversation c is offered: the
// preview's context over the demo's data, with the demo's provider on the
// model picker's rail.
func DemoComposer(d *Data, c *Conversation, worktree string) *composer.Context {
	ctx := Composer(d, c, worktree)
	ctx.Families = DemoFamilies
	return ctx
}

// DemoDraft is an unsent message a demo conversation opens with.
type DemoDraft struct {
	Text        string
	Attachments []*composer.Attachment
}

// DemoStart is the conversation the demo opens on.
const DemoStart = "harbor-api/rate-limit"

// Demo builds the demo in locale, its projects on the host with ID host.
func Demo(locale, host string) *Data {
	b := &demo{locale: locale, host: host}
	d := &Data{Models: DemoModels(), Workspace: b.workspace()}
	d.Providers = []Provider{{ID: demoFamily, Name: DemoProvider, Description: b.t("provider.description"), Badge: "provider_auth_key", Enabled: true}}
	d.Plugins = []Plugin{
		{ID: "git", Name: "Git", Description: b.t("plugin.git"), Version: "0.3.2", Official: true, Enabled: true},
		{ID: "knowledge", Name: "Knowledge", Description: b.t("plugin.knowledge"), Version: "0.1.1", Official: true, Enabled: true},
		{ID: "goals", Name: "Goals", Description: b.t("plugin.goals"), Version: "0.2.0", Official: true, Enabled: true},
	}
	d.Projects = []*Project{
		{ID: "harbor-api", Name: "harbor-api", Path: "~/Web/harbor-api", Branch: "main", Host: host, Trusted: true,
			Worktrees:     []Worktree{{Branch: "main", Path: "~/Web/harbor-api", Main: true}, {Branch: "feature/rate-limit", Path: "~/Web/harbor-api-rate-limit"}},
			Conversations: []*Conversation{b.rateLimit(), b.flakyTest(), b.invoices()}},
		{ID: "lumen-web", Name: "lumen-web", Path: "~/Web/lumen-web", Branch: "main", Host: host, Trusted: true,
			Worktrees:     []Worktree{{Branch: "main", Path: "~/Web/lumen-web", Main: true}},
			Conversations: []*Conversation{b.review(), b.onboarding(), b.metrics()}},
	}
	d.Unassigned = []*Conversation{b.handbook(), b.hashing(), b.staging(), b.releaseNotes(), b.translate()}
	DemoReplies(d, locale)
	return d
}

// DemoReplies gives every conversation of d, also the ones made since, the
// reply the demo plays after a send, in locale.
func DemoReplies(d *Data, locale string) {
	b := &demo{locale: locale}
	play := &script{label: b.t("tool.read"), file: "README.md", output: b.t("reply.file"), answer: b.t("reply.answer")}
	each := func(list []*Conversation) {
		for _, c := range list {
			if s := simulation(c); s.script == nil {
				s.script = play
			}
		}
	}
	for _, p := range d.Projects {
		each(p.Conversations)
	}
	each(d.Unassigned)
}

// DemoDrafts are the unsent messages of the demo's conversations, by ID.
func DemoDrafts(locale string) map[string]DemoDraft {
	b := &demo{locale: locale}
	pics := pictures()
	return map[string]DemoDraft{
		"lumen-web/metrics": {Text: b.t("metrics.draft"), Attachments: []*composer.Attachment{
			{ID: "draft-chart", Name: "signup-funnel.png", Mime: "image/png", Size: 184 << 10, Image: pics[1].Source, Path: "~/Desktop/signup-funnel.png", Status: composer.Ready, Progress: 1},
			{ID: "draft-events", Name: "events-week-45.csv", Mime: "text/csv", Size: 2 << 20, Path: "~/Downloads/events-week-45.csv", Data: []byte("day,signups\n"), Status: composer.Ready, Progress: 1},
			{ID: "draft-cohorts", Name: "cohorts.xlsx", Mime: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet", Size: 640 << 10, Status: composer.Sending, Progress: 0.62},
			{ID: "draft-notes", Name: "interview-notes.md", Mime: "text/markdown", Size: 12 << 10, Status: composer.Preparing},
			{ID: "draft-survey", Name: "survey-results.pdf", Mime: "application/pdf", Size: 3 << 20, Status: composer.Publishing, Progress: 1},
			{ID: "draft-trace", Name: "trace.har", Mime: "application/json", Size: 48 << 20, Status: composer.Failed, Error: "chat_attachment_unknown"},
			{ID: "draft-board", Name: "roadmap-board.png", Mime: "image/png", Size: 420 << 10, Status: composer.Uncertain},
			{ID: "draft-old", Name: "metrics-week-44.pdf", Mime: "application/pdf", Size: 2 << 20, Status: composer.Removing},
		}},
	}
}

// conv is a demo conversation last updated ago before the dataset's clock,
// its turns sent up to then.
func (b *demo) conv(id, title string, st Status, ago time.Duration, usage cv.Usage, turns ...cv.Turn) *Conversation {
	c := &Conversation{ID: id, Title: b.t(title), Status: st, Updated: Now.Add(-ago), Created: 1000 - int(ago/time.Minute), Turns: turns}
	for i := range c.Turns {
		t := &c.Turns[i]
		t.Sent = c.Updated.Add(-time.Duration(len(c.Turns)-i) * 6 * time.Minute)
		if !t.Reply.Status.Active() {
			t.Reply.Finished = t.Sent.Add(t.Reply.Elapsed + 2*time.Second)
		}
	}
	usage.Turns = len(turns)
	c.Session.Usage = &usage
	return c
}
