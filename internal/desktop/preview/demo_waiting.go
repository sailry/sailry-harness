package preview

import (
	"strings"
	"time"

	"github.com/sailry/sailry-harness/internal/desktop/composer"
	cv "github.com/sailry/sailry-harness/internal/desktop/conversation"
)

// The demo's runs that wait on the user, one for each thing a run can wait
// on: a release whose approvals are in every state, a plan awaiting its
// review, a connector's form, a connector's authorization link, and a
// subagent's own session, which is read and guided but not written to.

// deploy is approvals in every state: an SSH host key to trust and one that
// replaces a known key, details still being read and a read that failed,
// a decision on its way, one whose outcome is unknown and a receipt; and a
// question whose answer is sent again.
func (b *demo) deploy() *Conversation {
	const id = "harbor-api/deploy"
	const where = "~/Web/harbor-api"
	ship := b.user(id+"/0", "deploy.0.user")
	// A rewind of the message is under way.
	ship.Controls = cv.Controls{NoEdit: true, NoFork: true, NoRewind: true, Busy: true}
	build := b.call(ship.ID, "command", "command", "make image TAG=2.4.0-rc.3")
	build.Output = &cv.Output{Text: "#12 exporting to image\n#12 naming to registry.harbor.example/harbor-api:2.4.0-rc.3 done\n#12 DONE 4.1s", Command: true}
	pending := func(name, icon, detail string) cv.Call {
		call := b.call(ship.ID, name, icon, detail)
		call.Pending, call.State = true, cv.Pending
		return call
	}
	staging := pending("ssh", "command", "deploy@staging-2.harbor.example")
	bastion := pending("ssh", "command", "deploy@bastion.harbor.example:2222")
	values := pending("write", "writing", "deploy/staging/values.yaml")
	secrets := pending("edit", "pencil", "deploy/staging/secrets.enc.yaml")
	scale := pending("command", "command", "kubectl scale deploy/harbor-api --replicas=4 -n staging")
	tag := pending("command", "command", "git tag -a v2.4.0-rc.3 -m \"Release candidate 3\"")
	job := pending("command", "command", "kubectl apply -f deploy/staging/migrate-job.yaml")
	ship.Reply = cv.Reply{Status: cv.Running, Elapsed: secs(141), Phase: cv.AwaitingResponse}
	ship.Reply.Blocks = []cv.Block{
		reasoning(b.t("deploy.0.think")),
		tools(build),
		tools(staging, bastion, values, secrets, scale, tag, job),
		question(ship.ID+"/ask", cv.QuestionPending, b.t("deploy.ask.prompt"), nil),
	}

	c := b.conv(id, "deploy.title", Waiting, 3*time.Minute, cv.Usage{Tokens: 27400, Speed: 68.0, Cost: 0.0171, Cached: 19800, Context: 0.14}, ship)
	// each applies change to the approval id; false takes it off the list.
	each := func(id string, change func(*composer.Approval) bool) {
		var rest []composer.Approval
		for _, a := range c.Waiting.Approvals {
			if a.ID != id || change(&a) {
				rest = append(rest, a)
			}
		}
		c.Waiting.Approvals = rest
	}
	// A decision sent again arrives: the request is decided.
	again := func(id string) func() { return func() { Decide(c, id, composer.Allow) } }
	c.Waiting = composer.Interactions{
		Approvals: []composer.Approval{
			{ID: staging.ID, Title: b.t("tool.ssh"), Context: "staging-2.harbor.example",
				Trust: &composer.Trust{Host: "staging-2.harbor.example", Port: 22, Algorithm: "ED25519", Fingerprint: "SHA256:Qm8r1c0T5v0n3xJtYp2uKfLw9aZdE4hGs7BiNo6RkXc"}},
			{ID: bastion.ID, Title: b.t("tool.ssh"), Context: "bastion.harbor.example", Notices: []string{b.t("deploy.approval.replaced")},
				Trust: &composer.Trust{Host: "bastion.harbor.example", Port: 2222, Algorithm: "ECDSA", Fingerprint: "SHA256:f3Vd0aW8mN1qLx7TzUo5JcYb2PpGk6RrHs9eCiA4wDE", Changed: true}},
			{ID: values.ID, Title: b.t("tool.write"), Context: where, Remember: true, Loading: true},
			{ID: secrets.ID, Title: b.t("tool.edit"), Context: where, Remember: true, Failed: true, Reload: func() {
				each(secrets.ID, func(a *composer.Approval) bool {
					a.Failed = false
					a.Changes = []composer.Change{{Path: "deploy/staging/secrets.enc.yaml", Kind: composer.Modified, Added: 3, Removed: 3}}
					return true
				})
			}},
			{ID: scale.ID, Title: b.t("tool.command"), Context: where, Description: "kubectl scale deploy/harbor-api --replicas=4 -n staging", Code: true,
				Remember: true, Prefix: "kubectl scale", Disabled: true, Busy: true},
			{ID: tag.ID, Title: b.t("tool.command"), Context: where, Description: "git tag -a v2.4.0-rc.3 -m \"Release candidate 3\"", Code: true,
				Again: again(tag.ID)},
			{ID: job.ID, Title: b.t("tool.command"), Context: where, Description: "kubectl apply -f deploy/staging/migrate-job.yaml", Code: true,
				Receipt: true, Again: again(job.ID)},
		},
		Question: &composer.Ask{ID: ship.ID + "/ask", Prompt: b.t("deploy.ask.prompt"),
			Options: strings.Split(b.t("deploy.ask.options"), "\n"), Retry: true},
	}
	return c
}

// webhooks is a plan awaiting its review: Continue keeps planning, Execute
// starts the work.
func (b *demo) webhooks() *Conversation {
	const id = "harbor-api/webhooks"
	ask := b.user(id+"/0", "hooks.0.user")
	ask.Controls = cv.Controls{NoEdit: true, NoFork: true, NoRewind: true}
	sender := b.call(ask.ID, "read", "book-open", "internal/webhook/sender.go")
	sender.Output = output("func (s *Sender) Deliver(ctx context.Context, e Event) error {\n\tres, err := s.client.Do(s.request(ctx, e))\n\tif err != nil {\n\t\treturn err\n\t}\n\tdefer res.Body.Close()\n\tif res.StatusCode >= 300 {\n\t\treturn fmt.Errorf(\"webhook: %s answered %d\", e.Endpoint, res.StatusCode)\n\t}\n\treturn nil\n}")
	callers := b.call(ask.ID, "search", "search", `"Deliver(" in internal/`)
	callers.Output = paths("internal/booking/events.go", "internal/payment/events.go", "internal/webhook/sender_test.go")
	review := cv.Call{ID: ask.ID + "/plan", Name: "plan_review", Label: b.t("tool.plan"), Icon: "tasks", Detail: b.t("hooks.plan.detail"),
		Terminal: true, Output: output(b.t("hooks.plan.text"))}
	ask.Reply = cv.Reply{Status: cv.Running, Elapsed: secs(58), Phase: cv.AwaitingResponse}
	ask.Reply.Blocks = []cv.Block{
		reasoning(b.t("hooks.0.think")),
		tools(sender, callers),
		text(b.t("hooks.0.answer")),
		tools(review),
		question(ask.ID+"/ask", cv.QuestionPending, b.t("hooks.plan.detail"), nil),
	}
	c := b.conv(id, "hooks.title", Waiting, 6*time.Minute, cv.Usage{Tokens: 18900, Speed: 70.3, Cost: 0.0117, Cached: 12600, Context: 0.09}, ask)
	c.Waiting = composer.Interactions{Question: &composer.Ask{ID: ask.ID + "/ask", Kind: composer.AskPlan, Prompt: b.t("hooks.plan.detail")}}
	return c
}

// tracker is a connector's form: a field of every kind, required and
// optional, with the bounds each kind takes.
func (b *demo) tracker() *Conversation {
	const id = "lumen-web/tracker"
	options := func(key string) []string { return strings.Split(b.t(key), "\n") }
	bound := func(n uint64) *uint64 { return &n }
	limit := func(n float64) *float64 { return &n }

	file := b.user(id+"/0", "tracker.0.user")
	file.Controls = cv.Controls{NoEdit: true, NoFork: true, NoRewind: true}
	notes := b.call(file.ID, "read", "book-open", "docs/onboarding-review.md")
	notes.Output = output(b.t("tracker.file"))
	form := cv.Call{ID: file.ID + "/form", Name: "mcp_elicitation", Label: "Trackboard", Icon: "tasks", Detail: b.t("tracker.form.detail"),
		Standalone: true, Running: true}
	file.Reply = cv.Reply{Status: cv.Running, Elapsed: secs(26), Phase: cv.AwaitingResponse}
	file.Reply.Blocks = []cv.Block{
		tools(notes),
		text(b.t("tracker.0.answer")),
		tools(form),
		question(file.ID+"/ask", cv.QuestionPending, b.t("tracker.form.prompt"), nil),
	}
	c := b.conv(id, "tracker.title", Waiting, 8*time.Minute, cv.Usage{Tokens: 11300, Speed: 72.6, Cost: 0.0069, Cached: 7400, Context: 0.06}, file)
	c.Waiting = composer.Interactions{Question: &composer.Ask{ID: file.ID + "/ask", Kind: composer.AskForm, Prompt: b.t("tracker.form.prompt"), Declinable: true,
		Fields: []composer.Field{
			{Name: "project", Title: b.t("tracker.field.project"), Description: b.t("tracker.field.project.help"), Required: true,
				Kind: composer.FieldText, MinLength: bound(2), MaxLength: bound(10)},
			{Name: "priority", Title: b.t("tracker.field.priority"), Required: true, Kind: composer.FieldChoice, Options: options("tracker.field.priority.options")},
			{Name: "estimate", Title: b.t("tracker.field.estimate"), Description: b.t("tracker.field.estimate.help"), Required: true,
				Kind: composer.FieldNumber, Integer: true, Minimum: limit(1), Maximum: limit(13)},
			{Name: "notify", Title: b.t("tracker.field.notify"), Required: true, Kind: composer.FieldBoolean},
			{Name: "labels", Title: b.t("tracker.field.labels"), Description: b.t("tracker.field.labels.help"),
				Kind: composer.FieldChoice, Options: options("tracker.field.labels.options"), Multi: true, MinItems: bound(1), MaxItems: bound(3)},
			{Name: "context", Title: b.t("tracker.field.context"), Description: b.t("tracker.field.context.help"),
				Kind: composer.FieldText, Multiline: true, MaxLength: bound(500)},
		}}}
	return c
}

// audit is a subagent's own session: its messages are read, not edited or
// forked, and it has no composer, only the tray of the guidance queued for
// it.
func (b *demo) audit() *Conversation {
	const id = "lumen-web/audit"
	brief := b.user(id+"/0", "audit.0.user")
	brief.Controls = cv.Controls{ReadOnly: true, NoEdit: true, NoFork: true, NoRewind: true, NoWorktree: true}
	page := b.call(brief.ID, "read", "book-open", "src/pages/Onboarding.tsx")
	page.Output = output("export function Onboarding() {\n  return (\n    <main className=\"onboarding\">\n      <img src={hero} />\n      <button className=\"ghost\" onClick={skip}>Skip</button>\n      <Checklist items={steps} />\n    </main>\n  );\n}")
	axe := b.call(brief.ID, "command", "command", "npx axe http://localhost:5173/onboarding --tags wcag2aa")
	axe.Output = &cv.Output{Text: "Violation of \"image-alt\" with 1 occurrence\nViolation of \"color-contrast\" with 3 occurrences\nViolation of \"button-name\" with 1 occurrence\n\n5 accessibility issues detected", Command: true}
	contrast := b.call(brief.ID, "search", "search", `"--muted-foreground" in src/styles/`)
	contrast.Running = true
	brief.Reply = cv.Reply{Status: cv.Running, Elapsed: secs(63), Phase: cv.Generating}
	brief.Reply.Blocks = []cv.Block{reasoning(b.t("audit.0.think")), tools(page, axe), text(b.t("audit.0.partial")), tools(contrast)}
	c := b.conv(id, "audit.title", Running, 4*time.Minute, cv.Usage{Tokens: 14200, Speed: 84.1, Cost: 0.0021, Cached: 9600, Context: 0.07}, brief)
	c.Session.ReadOnly = true
	c.Queue = []composer.Queued{
		{ID: id + "/queued-focus", Text: b.t("audit.queue.focus"), Guided: true},
		{ID: id + "/queued-report", Text: b.t("audit.queue.report")},
	}
	return c
}

// signIn is a connector's authorization link, in a project that is a plain
// folder: the page to open in the system's browser, which can be declined.
func (b *demo) signIn() *Conversation {
	const id = "growth-notes/sign-in"
	const page = "https://auth.pulse.example/device?code=LUMN-4821"
	pull := b.user(id+"/0", "pulse.0.user")
	pull.Controls = cv.Controls{NoEdit: true, NoFork: true, NoRewind: true, NoWorktree: true}
	link := cv.Call{ID: pull.ID + "/link", Name: "mcp_elicitation", Label: "Pulse Analytics", Icon: "globe", Detail: page, Standalone: true, Running: true}
	pull.Reply = cv.Reply{Status: cv.Running, Elapsed: secs(12), Phase: cv.AwaitingResponse}
	pull.Reply.Blocks = []cv.Block{
		text(b.t("pulse.0.answer")),
		tools(link),
		question(pull.ID+"/ask", cv.QuestionPending, b.t("pulse.link.prompt"), nil),
	}
	c := b.conv(id, "pulse.title", Waiting, 14*time.Minute, cv.Usage{Tokens: 3100, Speed: 76.9, Cost: 0.0018, Cached: 1500, Context: 0.01}, pull)
	c.Waiting = composer.Interactions{Question: &composer.Ask{ID: pull.ID + "/ask", Kind: composer.AskLink, Prompt: b.t("pulse.link.prompt"), URL: page, Declinable: true}}
	return c
}
