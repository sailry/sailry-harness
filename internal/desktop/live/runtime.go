// Package live adapts the shared Client to the accepted desktop contracts.
package live

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"path/filepath"
	"strings"
	"sync"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/sidebar"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type Runtime struct {
	admittedSessions      map[string]protocol.Session
	composerRevisions     map[string]uint64
	composerSaving        map[string]bool
	composerWaiters       map[string][]func(bool)
	composerBases         map[*composer.State]protocol.SessionConfig
	App                   *shell.App
	Preferences           *preferences.Store
	controller            *client.Client
	ctx                   context.Context
	cancel                context.CancelFunc
	unsubscribe           func()
	unsubscribePrefs      func()
	mu                    sync.Mutex
	dirty                 bool
	closed                bool
	queue                 []func(*ui.Context)
	invalidate            func()
	update                func(func())
	workers               sync.WaitGroup
	openWindow            func(string)
	pickDirectory         func(func(string))
	preferencesChanged    func()
	busy                  map[string]bool
	uncertain             map[string]protocol.Request
	drafts                uint64
	lastPreferences       string
	lastMaterials         [3]float32
	lastFeatures          string
	lastKeys              string
	powerObservedRevision uint64
	prefsRevision         uint64
	conversationWatches   map[protocol.SessionID]func()
	conversationSeen      map[protocol.SessionID]bool
	composerInitialized   map[string]bool
	composerObserved      map[string]string
	readAloud             func(string, string, string)
	exportConversation    func(string, string, conversation.ExportFormat)
	conversationEdits     map[string]conversationEdit
	conversationCreations map[string]conversationCreation
	conversationCreating  map[string]bool
	providerPresets       map[string]protocol.ConversationPresetsEntry
	providerModels        map[string]map[string]protocol.ConversationModel
	providerDraftBases    map[string]protocol.ConversationProvider
	modelScores           map[string][]protocol.ConversationCatalogScoresEffortScore
	modelScoreKey         string
}

// New composes a window over an already started Client. Startup code should use
// NewWithPreferences with a Store opened before Node.Start for onboarding.
func New(ctx context.Context, controller *client.Client, profilePath string) (*Runtime, error) {
	return NewWithPreferences(ctx, controller, preferences.Open(filepath.Join(profilePath, "desktop", "preferences.json")))
}

func NewWithPreferences(ctx context.Context, controller *client.Client, prefs *preferences.Store) (*Runtime, error) {
	if controller == nil || prefs == nil {
		return nil, errors.New("desktop requires a Client and profile preferences")
	}
	ctx, cancel := context.WithCancel(ctx)
	r := &Runtime{controller: controller, ctx: ctx, cancel: cancel, Preferences: prefs, busy: map[string]bool{}, uncertain: map[string]protocol.Request{}}
	snapshot := controller.Snapshot()
	d := Sidebar(snapshot, prefs.Data)
	d.Notices = Notices(controller.Notices())
	r.App = shell.NewLive(d)
	r.App.SetLocalHost(localHost(snapshot, d))
	r.applyPreferences()
	r.App.Hooks = shell.Hooks{BeforeFrame: r.drain, AfterFrame: r.afterFrame, Refresh: r.Refresh, OpenFolder: r.openFolder,
		ReadNotice: r.markNotice, ReadNotices: r.readNotices, DismissRead: r.dismissRead, DismissNotices: r.dismissNotices}
	r.App.Sidebar.Actions = &sidebar.Actions{Rename: r.rename, Archive: r.archive, Read: r.read, Delete: r.removeSession,
		Pin: r.pin, MovePin: r.movePin, NewDraft: r.newDraft, RemoveProject: r.removeProject, SaveProject: r.saveProject, Browse: r.browse, ForkHere: r.forkHere, Export: r.exportSession, OpenWindow: func(id string) {
			if r.openWindow != nil {
				r.openWindow(id)
			}
		}}
	r.App.Settings.SavePower = r.savePower
	r.App.Settings.SaveTelemetry = r.saveTelemetry
	r.App.Hooks.Conversation = r.conversation
	r.conversationWatches = map[protocol.SessionID]func(){}
	r.conversationSeen = map[protocol.SessionID]bool{}
	r.composerInitialized = map[string]bool{}
	r.composerObserved = map[string]string{}
	r.composerRevisions = map[string]uint64{}
	r.composerSaving = map[string]bool{}
	r.composerWaiters = map[string][]func(bool){}
	r.composerBases = map[*composer.State]protocol.SessionConfig{}
	r.admittedSessions = map[string]protocol.Session{}
	r.conversationEdits = map[string]conversationEdit{}
	r.conversationCreations = map[string]conversationCreation{}
	r.conversationCreating = map[string]bool{}
	if ids := r.App.Sidebar.Order(); len(ids) > 0 {
		r.App.Sidebar.Selected = ids[0]
	} else {
		r.newDraft(nil)
	}
	r.unsubscribe = controller.OnChange(r.changed)
	r.unsubscribePrefs = prefs.OnChange(r.wake)
	if err := prefs.FinishStartup(); err != nil {
		r.enqueue(func(c *ui.Context) { kit.Toast(c, kit.Failed, i18n.T(c, "preferences_read_failed")) })
	}
	r.prefsRevision = prefs.Revision()
	r.readSettings()
	r.projectConversations()
	r.initProviders()
	r.initConversationSearch()
	return r, nil
}

func (r *Runtime) SetInvalidate(f func())                { r.mu.Lock(); r.invalidate = f; r.mu.Unlock(); r.wake() }
func (r *Runtime) SetUpdate(f func(func()))              { r.mu.Lock(); r.update = f; r.mu.Unlock(); r.wake() }
func (r *Runtime) SetOpenWindow(f func(string))          { r.openWindow = f }
func (r *Runtime) SetPickDirectory(f func(func(string))) { r.pickDirectory = f }
func (r *Runtime) SetPreferencesChanged(f func())        { r.preferencesChanged = f }

// Report presents a native callback failure without changing page content.
func (r *Runtime) Report(err error) {
	r.enqueue(func(c *ui.Context) { r.report(c, err, "session_changed") })
}

func (r *Runtime) Close() error {
	r.mu.Lock()
	if r.closed {
		r.mu.Unlock()
		return nil
	}
	r.closed = true
	r.mu.Unlock()
	r.unsubscribe()
	r.unsubscribePrefs()
	for _, release := range r.conversationWatches {
		release()
	}
	r.cancel()
	r.workers.Wait()
	return nil
}

func (r *Runtime) wake() {
	r.mu.Lock()
	update, invalidate, closed := r.update, r.invalidate, r.closed
	r.mu.Unlock()
	if closed {
		return
	}
	if update != nil {
		update(func() {})
	} else if invalidate != nil {
		invalidate()
	}
}
func (r *Runtime) changed() { r.mu.Lock(); r.dirty = true; r.mu.Unlock(); r.wake() }
func (r *Runtime) enqueue(f func(*ui.Context)) {
	r.mu.Lock()
	if !r.closed {
		r.queue = append(r.queue, f)
	}
	r.mu.Unlock()
	r.wake()
}

// drain is called only by App.View on MyGo's UI thread. Background callbacks
// touch the mutex-protected queue, never presentation fields.
func (r *Runtime) drain(c *ui.Context) {
	r.conversationSeen = map[protocol.SessionID]bool{}
	if revision := r.Preferences.Revision(); revision != r.prefsRevision {
		r.applyPreferences()
		r.prefsRevision = revision
		r.project()
	}
	kit.SetPreferences(c, kit.Preferences{ToastSeconds: r.Preferences.Data.ToastSeconds, MessageTextSize: uint8(r.App.Settings.TextSize)})
	r.App.Messages.CompactDisplay = r.App.Settings.MessageDisplay == 1
	r.mu.Lock()
	dirty, queue := r.dirty, r.queue
	r.dirty = false
	r.queue = nil
	r.mu.Unlock()
	if dirty {
		r.project()
	}
	for _, f := range queue {
		f(c)
	}
	snapshot := r.controller.Snapshot()
	if snapshot.PowerSettingsRevision != r.powerObservedRevision && !r.App.Settings.Power.Pending {
		r.readPower()
	}
}

func (r *Runtime) afterFrame() {
	for id, release := range r.conversationWatches {
		if !r.conversationSeen[id] {
			release()
			delete(r.conversationWatches, id)
		}
	}
	if session, err := protocol.ParseSessionID(r.App.Messages.SearchSession()); err == nil && !r.conversationSeen[session] {
		r.App.Messages.CloseSearch()
	}
	r.savePreferences()
}

func (r *Runtime) project() {
	snapshot := r.controller.Snapshot()
	for id, session := range r.admittedSessions {
		present := false
		for i, current := range snapshot.Sessions {
			if current.ID == session.ID {
				if current.Revision >= session.Revision {
					delete(r.admittedSessions, id)
				} else {
					snapshot.Sessions[i] = session
				}
				present = true
				break
			}
		}
		if !present {
			snapshot.Sessions = append(snapshot.Sessions, session)
		}
	}
	d := Sidebar(snapshot, r.Preferences.Data)
	d.Notices = Notices(r.controller.Notices())
	r.App.ApplyData(d)
	r.projectConversations()
	r.projectProviderSnapshot()
	r.App.SetLocalHost(localHost(snapshot, d))
	// Removed or archived resources lose their controller pins for this Node.
	if pins := r.Preferences.Data.Pins; pins != nil {
		keep := map[string]bool{}
		for _, s := range snapshot.Sessions {
			if !s.Archived {
				keep[preferences.PinKey(snapshot.Node, s.ID.String())] = true
			}
		}
		for _, t := range snapshot.Terminals {
			if t.Status.Kind != "closed" {
				keep[preferences.PinKey(snapshot.Node, "terminal/"+t.ID.String())] = true
			}
		}
		prefix := preferences.PinKey(snapshot.Node, "")
		for key := range *pins {
			if strings.HasPrefix(key, prefix) && !keep[key] {
				delete(*pins, key)
			}
		}
	}
}

func (r *Runtime) async(work func()) bool {
	r.mu.Lock()
	if r.closed {
		r.mu.Unlock()
		return false
	}
	r.workers.Add(1)
	r.mu.Unlock()
	go func() { defer r.workers.Done(); work() }()
	return true
}

func (r *Runtime) Refresh() {
	if r.busy["refresh"] {
		return
	}
	r.busy["refresh"] = true
	r.async(func() {
		err := r.controller.Refresh(r.ctx)
		r.enqueue(func(c *ui.Context) {
			delete(r.busy, "refresh")
			if err != nil {
				r.report(c, err, "session_changed")
			}
			r.readSettings()
		})
	})
}

// execute captures immutable input and retains its request ID after an unknown
// completion. All outcome delivery and draft changes run on the UI thread.
func (r *Runtime) execute(command protocol.Command, finished func(*ui.Context, protocol.Output, error)) {
	r.executeScoped("", command, finished)
}

func (r *Runtime) executeScoped(scope string, command protocol.Command, finished func(*ui.Context, protocol.Output, error)) {
	b, err := json.Marshal(command)
	if err != nil {
		r.enqueue(func(c *ui.Context) { finished(c, protocol.Output{}, err) })
		return
	}
	key := scope + "\x00" + string(b)
	if r.busy[key] {
		return
	}
	r.busy[key] = true
	request, exists := r.uncertain[key]
	if !exists {
		request = r.controller.Prepare(command)
	}
	r.async(func() {
		output, err := r.controller.Execute(r.ctx, request)
		r.enqueue(func(c *ui.Context) {
			delete(r.busy, key)
			if faultCode(err) == protocol.ErrorCodeOutcomeUnknown {
				r.uncertain[key] = request
			} else {
				delete(r.uncertain, key)
			}
			finished(c, output, err)
		})
	})
}

func (r *Runtime) command(command protocol.Command, conflict string, done func(protocol.Output, bool)) {
	r.execute(command, func(c *ui.Context, output protocol.Output, err error) {
		if err != nil {
			r.report(c, err, conflict)
		}
		if done != nil {
			done(output, err == nil)
		}
	})
}

func faultCode(err error) protocol.ErrorCode {
	var fault *link.FaultError
	if errors.As(err, &fault) {
		return fault.Fault.Code
	}
	var direct protocol.Fault
	if errors.As(err, &direct) {
		return direct.Code
	}
	return ""
}
func (r *Runtime) report(c *ui.Context, err error, conflict string) {
	if err == nil {
		return
	}
	o, key := kit.Failed, "chat_action_failed"
	switch faultCode(err) {
	case protocol.ErrorCodeConflict, protocol.ErrorCodeRevisionConflict:
		o, key = kit.Conflict, conflict
	case protocol.ErrorCodeOutcomeUnknown:
		o, key = kit.Uncertain, "chat_action_unknown"
	}
	kit.Toast(c, o, i18n.T(c, key))
}

func (r *Runtime) rename(cv *viewmodel.Conversation, title string, done func(bool)) {
	if strings.HasPrefix(cv.ID, "draft/") {
		cv.Title = title
		done(true)
		return
	}
	id, err := protocol.ParseSessionID(cv.ID)
	if err != nil {
		done(false)
		return
	}
	r.command(protocol.Command{Kind: "rename_session", Data: protocol.CommandRenameSessionPayload{Session: id, ExpectedRevision: cv.Revision, Title: title}}, "session_changed", func(_ protocol.Output, ok bool) { done(ok) })
}
func (r *Runtime) archive(cv *viewmodel.Conversation) {
	if strings.HasPrefix(cv.ID, "draft/") {
		r.removeDraft(cv.ID)
		return
	}
	id, err := protocol.ParseSessionID(cv.ID)
	if err != nil {
		return
	}
	r.command(protocol.Command{Kind: "set_session_archived", Data: protocol.CommandSetSessionArchivedPayload{Session: id, ExpectedRevision: cv.Revision, Archived: true}}, "session_changed", nil)
}
func (r *Runtime) read(cv *viewmodel.Conversation, read bool) {
	id, err := protocol.ParseSessionID(cv.ID)
	if err != nil {
		return
	}
	r.command(protocol.Command{Kind: "set_session_read", Data: protocol.CommandSetSessionReadPayload{Session: id, ExpectedRevision: cv.AttentionRevision, Read: read}}, "session_changed", nil)
}
func (r *Runtime) removeSession(cv *viewmodel.Conversation) {
	id, err := protocol.ParseSessionID(cv.ID)
	if err != nil {
		r.removeDraft(cv.ID)
		return
	}
	r.command(protocol.Command{Kind: "remove_session", Data: protocol.CommandRemoveSessionPayload{Session: id, ExpectedRevision: cv.Revision}}, "session_changed", nil)
}

func expectedProject(p *viewmodel.Project) (protocol.Project, error) {
	id, err := protocol.ParseProjectID(p.ID)
	return protocol.Project{ID: id, Name: p.Name, Path: p.Path, Appearance: protocol.ProjectsAppearance{Icon: p.Icon, Color: p.Color}}, err
}
func (r *Runtime) removeProject(p *viewmodel.Project) {
	expected, err := expectedProject(p)
	if err != nil {
		return
	}
	r.command(protocol.Command{Kind: "remove_project", Data: protocol.CommandRemoveProjectPayload{Expected: expected}}, "project_changed", nil)
}
func (r *Runtime) saveProject(e sidebar.Editor, host string, done func(bool)) {
	if e.Clone != 0 || len(r.App.Data.Hosts) == 0 || host != r.App.Data.Hosts[0].ID {
		r.enqueue(func(c *ui.Context) { kit.Toast(c, kit.Failed, i18n.T(c, "unavailable")); done(false) })
		return
	}
	command := protocol.Command{Kind: "create_project", Data: protocol.ProjectsDraft{Name: e.Name, Path: e.Path, Appearance: protocol.DefaultProjectsAppearance(), Source: protocol.ProjectsSource{Kind: "local"}}}
	if e.Project != nil {
		expected, err := expectedProject(e.Project)
		if err != nil {
			done(false)
			return
		}
		command = protocol.Command{Kind: "update_project", Data: protocol.CommandUpdateProjectPayload{Expected: expected, Name: e.Name, Path: e.Path, Appearance: expected.Appearance}}
	}
	r.command(command, "project_changed", func(_ protocol.Output, ok bool) { done(ok) })
}

func (r *Runtime) browse(owner string, selected func(string)) {
	if r.pickDirectory == nil {
		r.enqueue(func(c *ui.Context) { kit.Toast(c, kit.Failed, i18n.T(c, "unavailable")) })
		return
	}
	r.pickDirectory(func(path string) {
		if path != "" {
			r.enqueue(func(*ui.Context) { selected(path) })
		}
	})
}
func (r *Runtime) openFolder() { r.browse("", r.App.Sidebar.OpenFolder) }

func (r *Runtime) newDraft(p *viewmodel.Project) {
	r.drafts++
	cv := &viewmodel.Conversation{ID: fmt.Sprintf("draft/%d", r.drafts), Updated: time.Now(), Created: int(^uint(0) >> 1)}
	if p == nil {
		r.App.Data.Unassigned = append([]*viewmodel.Conversation{cv}, r.App.Data.Unassigned...)
	} else {
		for _, current := range r.App.Data.Projects {
			if current.ID == p.ID {
				current.Conversations = append([]*viewmodel.Conversation{cv}, current.Conversations...)
				r.App.Sidebar.Reveal(p.ID)
				break
			}
		}
	}
	r.App.Page = shell.ConversationPage
	r.App.Sidebar.Selected = cv.ID
}

func (r *Runtime) removeDraft(id string) {
	remove := func(list []*viewmodel.Conversation) []*viewmodel.Conversation {
		out := list[:0]
		for _, cv := range list {
			if cv.ID != id {
				out = append(out, cv)
			}
		}
		return out
	}
	r.App.Data.Unassigned = remove(r.App.Data.Unassigned)
	for _, p := range r.App.Data.Projects {
		p.Conversations = remove(p.Conversations)
	}
	r.App.ApplyData(r.App.Data)
}

func (r *Runtime) pin(cv *viewmodel.Conversation) {
	if strings.HasPrefix(cv.ID, "draft/") {
		r.enqueue(func(c *ui.Context) { kit.Toast(c, kit.Failed, i18n.T(c, "unavailable")) })
		return
	}
	node := r.controller.Target()
	r.Preferences.Data.SetPin(node, cv.ID, preferences.Pin{Pinned: !cv.Pinned})
	r.project()
}
func (r *Runtime) movePin(source, target string, after bool) {
	var pins []sidebar.Pin
	add := func(list []*viewmodel.Conversation) {
		for _, cv := range list {
			if cv.Pinned {
				pins = append(pins, sidebar.Pin{ID: cv.ID, Order: cv.PinOrder, LastRun: cv.Updated, Created: cv.Created})
			}
		}
	}
	add(r.App.Data.Unassigned)
	for _, p := range r.App.Data.Projects {
		add(p.Conversations)
	}
	var ids []string
	for _, p := range sidebar.OrderPins(pins) {
		ids = append(ids, p.ID)
	}
	moved, ok := sidebar.MovePin(ids, source, target, after)
	if !ok {
		return
	}
	for i, id := range moved {
		order := uint64(i)
		r.Preferences.Data.SetPin(r.controller.Target(), id, preferences.Pin{Pinned: true, Order: &order})
	}
	r.project()
}

// OpenSession and OpenNotice are called by window and notification callbacks on
// the UI thread; asynchronous work still returns through the same queue.
func (r *Runtime) OpenSession(id string) {
	if r.App.Data.Find(id) != nil {
		r.App.Page = shell.ConversationPage
		r.App.Sidebar.Selected = id
	}
}
func (r *Runtime) markNotice(id string) {
	for _, n := range r.controller.Notices() {
		if noticeID(n) != id {
			continue
		}
		if n.ID.Target.Kind == "notification" {
			r.command(protocol.Command{Kind: "mark_notification_read", Data: protocol.CommandMarkNotificationReadPayload{ID: n.ID.Target.Notification}}, "notification_update_failed", nil)
		} else {
			r.controller.MarkAttentionRead(n.ID)
		}
		return
	}
}
func (r *Runtime) OpenNotice(id string) {
	r.markNotice(id)
	for _, n := range r.App.Data.Notices {
		if n.ID == id && n.Session != "" {
			r.OpenSession(n.Session)
			return
		}
	}
}
