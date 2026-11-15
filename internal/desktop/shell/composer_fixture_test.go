package shell

import (
	"encoding/json"
	"maps"
	"reflect"
	"slices"
	"sync"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
)

type composerFixture struct {
	data    []byte
	models  []composer.Model
	badges  []composer.StatusBadge
	created time.Time
}

// Composer-only cases retain the preview's catalogs and last input without
// rebuilding its thousand-turn transcript. JSON owns the small mutable data;
// models and badges are copied separately because their SVG assets are shared
// immutable icons, rather than JSON values.
var composerFixtureSource = sync.OnceValues(func() (*composerFixture, error) {
	full := preview.Load()
	data := *full
	data.Models, data.Unassigned = nil, nil
	data.Workspace = &preview.Workspace{Files: full.Files, Changes: full.Changes, Repo: full.Repo}
	data.Projects = make([]*preview.Project, len(full.Projects))
	for i, project := range full.Projects {
		projectCopy := *project
		projectCopy.Conversations = make([]*preview.Conversation, len(project.Conversations))
		for j, session := range project.Conversations {
			copy := *session
			copy.Turns, copy.Queue, copy.Commands, copy.Badges = nil, nil, nil, nil
			copy.Waiting = composer.Interactions{}
			if session.ID == preview.Big {
				last := session.Turns[len(session.Turns)-1]
				turn := conversation.Turn{ID: last.ID, User: last.User, Sent: last.Sent, Images: last.Images, Files: last.Files,
					Reply: conversation.Reply{Status: last.Reply.Status}}
				for _, block := range last.Reply.Blocks {
					if block.Kind == conversation.ProgressBlock || block.Kind == conversation.ChildrenBlock {
						turn.Reply.Blocks = append(turn.Reply.Blocks, block)
					}
				}
				copy.Turns, copy.Commands = []conversation.Turn{turn}, session.Commands
			}
			projectCopy.Conversations[j] = &copy
		}
		data.Projects[i] = &projectCopy
	}
	encoded, err := json.Marshal(&data)
	if err != nil {
		return nil, err
	}
	return &composerFixture{data: encoded, models: full.Models, badges: full.Find(preview.Big).Badges, created: time.Now()}, nil
})

func composerFixtureData(t *testing.T) *preview.Data {
	t.Helper()
	source, err := composerFixtureSource()
	if err != nil {
		t.Fatal(err)
	}
	var data preview.Data
	if err := json.Unmarshal(source.data, &data); err != nil {
		t.Fatal(err)
	}
	cloneScore := func(score *composer.Score) *composer.Score {
		if score == nil {
			return nil
		}
		copy := *score
		return &copy
	}
	data.Models = slices.Clone(source.models)
	for i := range data.Models {
		model := &data.Models[i]
		model.Efforts, model.Score, model.Scores = slices.Clone(model.Efforts), cloneScore(model.Score), maps.Clone(model.Scores)
		for effort, score := range model.Scores {
			model.Scores[effort] = cloneScore(score)
		}
		model.Features = slices.Clone(model.Features)
		for j := range model.Features {
			model.Features[j].Options = slices.Clone(model.Features[j].Options)
		}
	}
	text := func(value composer.Text) composer.Text {
		value.Locales = maps.Clone(value.Locales)
		return value
	}
	session := data.Find(preview.Big)
	session.Badges = slices.Clone(source.badges)
	for i := range session.Badges {
		badge := &session.Badges[i]
		badge.Label, badge.Segment, badge.Details = text(badge.Label), slices.Clone(badge.Segment), slices.Clone(badge.Details)
		for j := range badge.Segment {
			badge.Segment[j].Text = text(badge.Segment[j].Text)
		}
		for j := range badge.Details {
			badge.Details[j].Label = text(badge.Details[j].Label)
		}
		if badge.Panel != nil {
			panel := *badge.Panel
			panel.Body, panel.Empty, panel.Buttons = text(panel.Body), text(panel.Empty), slices.Clone(panel.Buttons)
			for j := range panel.Buttons {
				panel.Buttons[j].Label = text(panel.Buttons[j].Label)
			}
			badge.Panel = &panel
		}
	}
	for i := range session.Commands {
		session.Commands[i].Started = session.Commands[i].Started.Add(time.Since(source.created))
	}
	return &data
}

func TestComposerFixtureKeepsContextAndIsolation(t *testing.T) {
	full, first, second := preview.Load(), composerFixtureData(t), composerFixtureData(t)
	context := func(data *preview.Data) *composer.Context {
		ctx := preview.Composer(data, data.Find(preview.Big), "main")
		// Both composer-only owners replace pending interactions. Command
		// ages are relative to fixture creation, rather than a shared clock.
		ctx.Interactions, ctx.Queue = composer.Interactions{}, nil
		for i := range ctx.Activity.Commands {
			ctx.Activity.Commands[i].Started = time.Time{}
		}
		return ctx
	}
	want := context(full)
	if !reflect.DeepEqual(context(first), want) || !reflect.DeepEqual(first.Find(preview.Big).Session.Usage, full.Find(preview.Big).Session.Usage) {
		t.Fatal("the compact fixture changed the composer's context or metrics")
	}
	first.Models[0].Name = "changed"
	first.Models[0].Score.Overall = 0
	first.Models[0].Features[0].Options[0].Name = "changed"
	first.Models[4].Efforts[0] = "changed"
	first.Providers[0].Name = "changed"
	first.Plugins[0].Enabled = false
	first.Projects[0].Worktrees[0].Branch = "changed"
	first.Projects[0].Conversations[1].Title = "changed"
	first.Files.Children[0].Name = "changed"
	first.Changes[0].Added++
	first.Changes[0].Hunks[0].Lines[0].Text = "changed"
	first.Repo.Branches[0].Name = "changed"
	session := first.Find(preview.Big)
	session.Turns[0].User = "changed"
	for i := range session.Turns[0].Reply.Blocks {
		block := &session.Turns[0].Reply.Blocks[i]
		if len(block.Children) > 0 {
			block.Children[0].Title = "changed"
		}
		if len(block.Steps) > 0 {
			block.Steps[0].Text = "changed"
		}
	}
	session.Badges[0].Label.Locales["zh-CN"] = "changed"
	session.Badges[0].Panel.Body.Label = "changed"
	session.Badges[0].Panel.Buttons[0].Label.Locales["zh-CN"] = "changed"
	session.Commands[1].Ports[0]++
	session.Session.Usage.Tokens++
	if !reflect.DeepEqual(context(second), want) || !reflect.DeepEqual(second.Providers, full.Providers) ||
		!reflect.DeepEqual(second.Repo, full.Repo) || !reflect.DeepEqual(second.Find(preview.Big).Session.Usage, full.Find(preview.Big).Session.Usage) {
		t.Fatal("one composer fixture changed another's mutable data")
	}
}
