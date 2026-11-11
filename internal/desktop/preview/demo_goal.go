package preview

import (
	"time"

	"github.com/sailry/sailry-harness/internal/desktop/composer"
	cv "github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
)

// The demo's goals (the goals plugin): one in pursuit, whose turns after the
// user's are continuations under the package's marker, and one the turn
// limit paused after its wrap-up turn.

// goalMarker is the package's continuation marker in both languages.
func (b *demo) goalMarker(key string) *cv.Marker {
	label := b.both(key)
	return &cv.Marker{Label: label.Label, Locales: label.Locales}
}

// goalBadge is goalStatus for the objective written under a key of the
// demo's catalogs.
func (b *demo) goalBadge(state string, tone kit.Tone, tail composer.Text, objective, turns, spent, tokens string, buttons ...string) composer.StatusBadge {
	return goalStatus(state, tone, tail, b.both(objective), b.t(objective), turns, spent, tokens, buttons...)
}

// goalStatus is the plugin's Status badge: Goal, the state and what follows
// it (the time while it runs, the reason once paused), the counters as its
// tooltip, and the panel with the objective and its buttons. shown is the
// objective as the tooltip reads it.
func goalStatus(state string, tone kit.Tone, tail, objective composer.Text, shown, turns, spent, tokens string, buttons ...string) composer.StatusBadge {
	label := (&demo{}).both // the package's labels, which carry their translations
	badge := composer.StatusBadge{ID: "goals/status", Label: label("goal.label"), Icon: icons.Target,
		Segment: []composer.Segment{{Text: label("goal.label"), Tone: kit.Neutral}, {Text: label(state), Tone: tone}, {Text: tail, Tone: kit.Neutral}},
		Details: []composer.Detail{
			{Label: label("goal.label"), Value: shown},
			{Label: label("pursuit.turns"), Value: turns},
			{Label: label("pursuit.time"), Value: spent},
			{Label: label("pursuit.tokens"), Value: tokens},
		},
		Panel: &composer.Panel{Body: objective, Empty: label("goal.empty")}}
	for index, key := range buttons {
		badge.Panel.Buttons = append(badge.Panel.Buttons, composer.PanelButton{ID: "goal-" + key, Label: label("pursuit." + key), Ghost: index > 0})
	}
	return badge
}

// pursuit is a goal in progress: the user's turn set it, a continuation
// finished a step and the next one is running.
func (b *demo) pursuit() *Conversation {
	const id = "lumen-web/pursuit"
	set := b.user(id+"/0", "pursuit.0.user")
	inventory := b.call(set.ID, "search", "search", `"--chart-" in src/charts/`)
	inventory.Output = paths("src/charts/area.tsx", "src/charts/bar.tsx", "src/charts/donut.tsx", "src/charts/sparkline.tsx")
	set.Reply = done(cv.Completed, 64)
	set.Reply.Blocks = []cv.Block{tools(inventory), text(b.t("pursuit.0.answer"))}

	first := cv.Turn{ID: id + "/1", Marker: b.goalMarker("pursuit.marker")}
	area := b.call(first.ID, "edit", "pencil", "src/charts/area.tsx")
	bar := b.call(first.ID, "edit", "pencil", "src/charts/bar.tsx")
	visual := b.call(first.ID, "command", "command", "npm run test:visual -- charts/area charts/bar")
	visual.Output = &cv.Output{Text: "charts/area  12 passed\ncharts/bar   9 passed", Command: true}
	first.Reply = done(cv.Completed, 212)
	first.Reply.Blocks = []cv.Block{tools(area, bar, visual), text(b.t("pursuit.1.answer"))}

	second := cv.Turn{ID: id + "/2", Marker: b.goalMarker("pursuit.marker")}
	donut := b.call(second.ID, "edit", "pencil", "src/charts/donut.tsx")
	second.Reply = cv.Reply{Status: cv.Running, Elapsed: secs(38), Phase: cv.Generating}
	second.Reply.Blocks = []cv.Block{reasoning(b.t("pursuit.2.think")), tools(donut)}

	c := b.conv(id, "pursuit.title", Running, time.Minute, cv.Usage{Tokens: 184000, Speed: 71.4, Cost: 0.0932, Cached: 121000, Context: 0.21}, set, first, second)
	c.Badges = []composer.StatusBadge{b.goalBadge("goal.running", kit.Info, composer.Text{Label: "12m"},
		"pursuit.text", "2 / 25", "12m", "184.0k", "pause", "edit", "clear")}
	return c
}

// pursuitLimit is a goal the turn limit paused: its last continuation wrapped
// up, and Resume opens a new window of turns.
func (b *demo) pursuitLimit() *Conversation {
	const id = "lumen-web/pursuit-limit"
	set := b.user(id+"/0", "limited.0.user")
	set.Reply = done(cv.Completed, 48)
	set.Reply.Blocks = []cv.Block{text(b.t("limited.0.answer"))}

	last := cv.Turn{ID: id + "/1", Marker: b.goalMarker("pursuit.marker")}
	suite := b.call(last.ID, "command", "command", "npm run test:e2e -- --grep checkout")
	suite.Output = &cv.Output{Text: "checkout  31 passed, 4 failed\n  ✗ applies a gift card after a coupon\n  ✗ keeps the cart across sign-in", Command: true}
	last.Reply = done(cv.Completed, 187)
	last.Reply.Blocks = []cv.Block{tools(suite), text(b.t("limited.1.answer"))}

	c := b.conv(id, "limited.title", Unread, 26*time.Minute, cv.Usage{Tokens: 2410000, Speed: 66.9, Cost: 1.2140, Cached: 1930000, Context: 0.58}, set, last)
	c.Badges = []composer.StatusBadge{b.goalBadge("pursuit.paused", kit.Caution, b.both("pursuit.reason.turns"),
		"limited.text", "25 / 25", "1h 12m", "2.4M", "resume", "edit", "clear")}
	return c
}
