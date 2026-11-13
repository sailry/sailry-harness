// Package acceptance drives opt-in conversation checks with MyGo's in-memory
// Tester, not a native window. Run uses fixtures only; RunLive reaches an already
// started Node through Client. Only the desktop acceptance subprocess imports
// this package, keeping test drivers out of production UI and live actions.
package acceptance

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"runtime/debug"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	core "github.com/sailry/sailry-harness/internal/acceptance"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
)

type scenario struct {
	id, title string
	run       func(*window) error
}

// Run performs the manually requested headless conversation interaction suite.
// Each scenario gets a new shell and fixtures; an assertion or panic fails only
// that scenario. Callers run this in the dedicated acceptance subprocess because
// the shell also applies process-wide theme settings while building frames.
func Run(ctx context.Context, emit func(core.CaseResult)) core.Report {
	report := core.Report{Version: 1, Suite: "ui", Started: time.Now().UTC()}
	cases := []scenario{
		{"ui.send", "Send a message with the button", sendMessage},
		{"ui.stream", "Send with Enter and follow a streamed reply", streamReply},
		{"ui.stop", "Stop a reply without adding more output", stopReply},
		{"ui.tool-output", "Expand and copy a tool's original result", toolOutput},
		{"ui.file-references", "Keep file references as links and publications as cards", fileReferences},
		{"ui.approval", "Page through and resolve an approval", approveTool},
		{"ui.question", "Choose and submit a question answer", answerQuestion},
		{"ui.queue", "Queue and edit a message", queueMessage},
		{"ui.sessions", "Retain drafts and search for a conversation", switchSessions},
		{"ui.history", "Scroll to and load an older history page", loadHistory},
		{"ui.references", "Choose a reference and close its picker first", chooseReference},
		{"ui.width", "Send and retain the conversation across widths", resizeConversation},
	}
	for _, scene := range cases {
		result := runCase(ctx, scene)
		report.Cases = append(report.Cases, result)
		if emit != nil {
			emit(result)
		}
	}
	report.Finished = time.Now().UTC()
	return report
}

func runCase(ctx context.Context, scene scenario) (result core.CaseResult) {
	started := time.Now()
	result = core.CaseResult{ID: scene.id, Group: "ui", Title: scene.title,
		Status: "failed", Evidence: []core.Evidence{{Kind: "scope", Value: "Headless MyGo interaction; fixture data; no native window or model calls"}}}
	w := &window{ctx: ctx, result: &result}
	defer func() {
		if problem := recover(); problem != nil {
			result.Status, result.Detail = "failed", fmt.Sprintf("UI panic: %v", problem)
			w.evidence("stack", string(debug.Stack()))
		}
		result.Duration = time.Since(started)
		if result.Status == "failed" && w.tester != nil {
			w.evidence("rendered_text", strings.Join(w.tester.Texts(), "\n"))
			if tree, err := json.Marshal(w.tester.AccessAll()); err == nil {
				w.evidence("accessibility", string(tree))
			}
		}
	}()
	if err := ctx.Err(); err != nil {
		result.Status, result.Detail = "canceled", err.Error()
		return result
	}
	if err := scene.run(w); err != nil {
		result.Detail = err.Error()
		if errors.Is(err, context.Canceled) || errors.Is(err, context.DeadlineExceeded) {
			result.Status = "canceled"
		}
		return result
	}
	result.Status = "passed"
	return result
}

type window struct {
	ctx    context.Context
	result *core.CaseResult
	app    *shell.App
	tester *ui.Tester
}

func (w *window) open(prepare func(*preview.Data)) error {
	if err := w.ctx.Err(); err != nil {
		return err
	}
	data := preview.Load()
	if prepare != nil {
		prepare(data)
	}
	w.app = shell.New(data)
	w.app.Effects = false
	w.tester = ui.NewTesterWith(w.app.View, 1280, 820, ui.TesterOptions{
		Preferences: ui.Preferences{ReduceTransparency: true, ReduceMotion: true},
	})
	w.tester.SetLocale("en")
	w.tester.Frame()
	return nil
}

func (w *window) selectConversation(id string) error {
	cv := w.app.Data.Find(id)
	if cv == nil {
		return fmt.Errorf("fixture conversation %q does not exist", id)
	}
	if err := w.click(cv.Title); err != nil {
		return err
	}
	if w.app.Sidebar.Selected != id {
		return fmt.Errorf("selecting %q selected %q", id, w.app.Sidebar.Selected)
	}
	w.result.Session = id
	return nil
}

func (w *window) click(label string) error {
	if err := w.ctx.Err(); err != nil {
		return err
	}
	if err := w.tester.Click(label); err != nil {
		return liveFail(fmt.Sprintf("click %q: %v", label, err))
	}
	w.tester.Frame()
	return nil
}

func (w *window) control(role ui.Role, name string) (ui.AccessInfo, error) {
	info, ok := w.tester.AccessByRole(role, name)
	if !ok {
		return info, liveFail(fmt.Sprintf("control %q with role %v is missing", name, role))
	}
	if info.Bounds.W <= 0 || info.Bounds.H <= 0 {
		return info, liveFail(fmt.Sprintf("control %q has no visible bounds: %v", name, info.Bounds))
	}
	return info, nil
}

func (w *window) button(name string) error {
	if err := w.ctx.Err(); err != nil {
		return err
	}
	info, err := w.control(ui.RoleButton, name)
	if err != nil {
		return err
	}
	if info.Disabled {
		return liveFail(fmt.Sprintf("button %q is disabled", name))
	}
	w.tester.ClickAt(info.Bounds.X+info.Bounds.W/2, info.Bounds.Y+info.Bounds.H/2)
	w.tester.Frame()
	return nil
}

func (w *window) typeMessage(text string) error {
	if err := w.click(label("composer")); err != nil {
		return err
	}
	w.tester.Type(text)
	w.tester.Frame()
	return nil
}

func (w *window) advance(duration time.Duration) error {
	if err := w.ctx.Err(); err != nil {
		return err
	}
	w.tester.Advance(duration)
	w.tester.Frame()
	return nil
}

func (w *window) evidence(kind, value string) {
	w.result.Evidence = append(w.result.Evidence, core.Evidence{Kind: kind, Value: value})
}

func label(key string, args ...any) string { return i18n.In("en", key, args...) }
