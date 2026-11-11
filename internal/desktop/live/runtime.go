// Package live adapts the shared Client to the accepted desktop contracts.
package live

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"maps"
	"path/filepath"
	"slices"
	"strings"
	"sync"
	"sync/atomic"
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
	pluginViews              *pluginViewState
	pluginStatus             map[string]*pluginStatusFeed
	pluginControls           map[string]*pluginControlPresentation
	pluginCards              *pluginCardState
	terminals                *terminalState
	browser                  *browserState
	authorizations           *authorizationState
	packages                 *packageState
	parent                   *Runtime
	network                  *Network
	bindings                 map[string]*Runtime
	unsubscribeNetwork       func()
	nodeModels               []composer.Model
	hosts                    map[string]*liveHost
	connections              *connectionState
	connectionReferences     map[string]*referenceCatalog
	sshTransfers             *sshTransferStore
	ports                    *livePorts
	pickKeyFile              func(func(string))
	teams                    *teamState
	commands                 *liveCommands
	openService              func(string)
	attachmentSessionWaiters map[string][]attachmentSessionWaiter
	attachments              *attachmentState
	workspace                *workspaceState
	workspaceSettings        workspaceSettingsState
	workspaceSettingsOwner   *Runtime
	terminalSettings         terminalSettingsState
	backupSettings           backupSettingsState
	localModels              localModelState
	media                    mediaState
	dictation                dictationState
	permissions              permissionState
	knowledgeWanted          string // the citation the knowledge viewer was last asked for
	pendingSession           string
	transferDialogOwner      *Runtime
	admittedSessions         map[string]protocol.Session
	composerRevisions        map[string]uint64
	composerSaving           map[string]bool
	composerWaiters          map[string][]func(bool)
	composerBases            map[*composer.State]protocol.SessionConfig
	sessionBases             map[*composer.State]*sessionBaseStart
	App                      *shell.App
	Preferences              *preferences.Store
	controller               *client.Client
	ctx                      context.Context
	cancel                   context.CancelFunc
	unsubscribe              func()
	unsubscribePrefs         func()
	mu                       sync.Mutex
	dirty                    bool
	closed                   bool
	queue                    []func(*ui.Context)
	invalidate               func()
	update                   func(func())
	workers                  sync.WaitGroup
	observers                sync.WaitGroup
	openWindow               func(string)
	pickDirectory            func(func(string))
	preferencesChanged       func()
	busy                     map[string]bool
	uncertain                map[string]protocol.Request
	drafts                   uint64
	lastPreferences          string
	lastMaterials            [3]float32
	lastFeatures             string
	lastKeys                 string
	powerObservedRevision    uint64
	updates                  *Updates
	updatesFocused           func() bool
	prefsRevision            uint64
	conversationWatches      map[protocol.SessionID]func()
	conversationSeen         map[protocol.SessionID]bool
	composerInitialized      map[string]bool
	composerObserved         map[string]string
	readAloud                func(string, string, string)
	exportConversation       func(string, string, conversation.ExportFormat)
	conversationEdits        map[string]conversationEdit
	turnRetries              map[string]turnRetry
	conversationCreations    map[string]conversationCreation
	conversationCreating     map[string]bool
	providerPresets          map[string]protocol.ConversationPresetsEntry
	providerModels           map[string]map[string]protocol.ConversationModel
	providerDraftBases       map[string]protocol.ConversationProvider
	modelScores              map[string][]protocol.ConversationCatalogScoresEffortScore
	modelScoreKey            string
	snapshotMu               sync.Mutex
	snapshotCopy             protocol.Snapshot
	snapshotRevision         uint64
	snapshotRead             bool
	conversationCopies       map[protocol.SessionID]conversationCopy
	conversationsStale       atomic.Bool
}

type conversationCopy struct {
	state client.ConversationState
	ok    bool
}

// snapshot returns the Client's Node snapshot, copied once per Client
// revision. Every reader shares the value and must not modify it; code that
// edits a snapshot reads its own from the Client.
func (r *Runtime) snapshot() protocol.Snapshot {
	snapshot, _ := r.snapshotAt()
	return snapshot
}

// snapshotAt also returns the Client revision the copy was taken for, which
// identifies it to whatever is derived from it.
func (r *Runtime) snapshotAt() (protocol.Snapshot, uint64) {
	revision := r.controller.Revision()
	r.snapshotMu.Lock()
	defer r.snapshotMu.Unlock()
	if !r.snapshotRead || revision != r.snapshotRevision {
		r.snapshotCopy, r.snapshotRevision, r.snapshotRead = r.controller.Snapshot(), revision, true
	}
	return r.snapshotCopy, revision
}

// conversationState returns one shared, read-only copy of a conversation per
// frame, taken again after an observed conversation changes. It is for the UI
// thread; outside a frame it reads the Client.
func (r *Runtime) conversationState(id protocol.SessionID) (client.ConversationState, bool) {
	if r.conversationCopies == nil {
		return r.controller.Conversation(id)
	}
	if r.conversationsStale.Swap(false) {
		clear(r.conversationCopies)
	}
	copied, ok := r.conversationCopies[id]
	if !ok {
		copied.state, copied.ok = r.controller.Conversation(id)
		r.conversationCopies[id] = copied
	}
	return copied.state, copied.ok
}

// conversationChanged redraws after an observed conversation changed. The frame reads that state itself, so the Node snapshot is not
// projected again.
func (r *Runtime) conversationChanged() {
	r.conversationsStale.Store(true)
	r.localRuntime().wake()
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
		ReadNotice: r.markNotice, ReadNotices: r.readNotices, DismissRead: r.dismissRead, DismissNotices: r.dismissNotices, OpenKnowledge: r.openKnowledge,
		CaptureSelection: func() { r.CaptureSelection(nil) }}
	r.App.Messages.OpenLink = r.openChatLink
	r.App.Sidebar.Actions = &sidebar.Actions{Rename: r.rename, Archive: r.archive, Read: r.read, Delete: r.removeSession,
		Pin: r.pin, MovePin: r.movePin, NewDraft: r.newDraft, RemoveProject: r.removeProject, Worktrees: r.sidebarWorkspaceAction, Records: r.projectRecords, SaveProject: r.saveProject, Browse: r.browse, ForkHere: r.forkHere, ForkWorktree: r.forkWorkspaceConversation, Export: r.exportSession, OpenWindow: func(id string) {
			if r.openWindow != nil {
				r.openWindow(id)
			}
		}}
	r.initCommands()
	r.initAttachments()
	r.App.Hooks.DownloadDialog = r.transferDialogs
	r.initWorkspace()
	r.App.Hooks.WorkspaceProject = r.workspaceProject
	r.bindWorkspaceSettings()
	r.App.Settings.SavePower = r.savePower
	r.App.Settings.SaveTelemetry = r.saveTelemetry
	r.App.Settings.ResetTelemetry = r.resetTelemetry
	r.App.Settings.SendDiagnostics = r.sendDiagnostics
	r.App.Hooks.Conversation = r.conversation
	r.App.Hooks.RenderChild = r.renderLiveChild
	r.conversationWatches = map[protocol.SessionID]func(){}
	r.conversationSeen = map[protocol.SessionID]bool{}
	r.composerInitialized = map[string]bool{}
	r.composerObserved = map[string]string{}
	r.composerRevisions = map[string]uint64{}
	r.composerSaving = map[string]bool{}
	r.composerWaiters = map[string][]func(bool){}
	r.composerBases = map[*composer.State]protocol.SessionConfig{}
	r.sessionBases = map[*composer.State]*sessionBaseStart{}
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
	r.initTeams()
	r.initConversationSearch()
	r.initNetworkUI()
	r.initB8Packages()
	r.initPluginViews()
	r.App.Panel.SetCreateResource(r.createPanelResource)
	r.App.Hooks.PluginOverlays = r.renderPluginOverlays
	r.App.Hooks.PluginHeader = r.renderPluginHeader
	r.App.Hooks.PluginTitle = r.renderPluginTitle
	return r, nil
}

func (r *Runtime) SetInvalidate(f func())         { r.mu.Lock(); r.invalidate = f; r.mu.Unlock(); r.wake() }
func (r *Runtime) SetUpdate(f func(func()))       { r.mu.Lock(); r.update = f; r.mu.Unlock(); r.wake() }
func (r *Runtime) SetPreferencesChanged(f func()) { r.preferencesChanged = f }

func (r *Runtime) SetOpenWindow(f func(string)) {
	r.openWindow = f
	for _, bound := range r.bindings {
		bound.SetOpenWindow(f)
	}
}
func (r *Runtime) SetPickDirectory(f func(func(string))) {
	r.pickDirectory = f
	for _, bound := range r.bindings {
		bound.SetPickDirectory(f)
	}
}

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
	if r.unsubscribe != nil {
		r.unsubscribe()
	}
	if r.unsubscribePrefs != nil {
		r.unsubscribePrefs()
	}
	if r.unsubscribeNetwork != nil {
		r.unsubscribeNetwork()
	}
	for _, bound := range r.bindings {
		_ = bound.Close()
	}
	r.closeNetworkUI()
	for _, release := range r.conversationWatches {
		release()
	}
	r.closePluginViews()
	r.closePluginStatus()
	r.closePluginCards()
	r.closeCommands()
	r.closeAttachments()
	r.closeWorkspace()
	r.cancel()
	r.workers.Wait()
	r.observers.Wait()
	if r.dictation.manager != nil {
		// A speech download ends with the context and leaves no partial
		// write behind the closed window.
		r.dictation.manager.Wait()
	}
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
func (r *Runtime) changed() {
	if r.parent != nil {
		r.parent.changed()
		return
	}
	r.mu.Lock()
	r.dirty = true
	r.mu.Unlock()
	r.wake()
}
func (r *Runtime) enqueue(f func(*ui.Context)) {
	if r.parent != nil {
		r.parent.enqueue(f)
		return
	}
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
	r.conversationSeen, r.conversationCopies = map[protocol.SessionID]bool{}, map[protocol.SessionID]conversationCopy{}
	r.syncNetwork()
	for _, bound := range r.bindings {
		bound.conversationSeen, bound.conversationCopies = map[protocol.SessionID]bool{}, map[protocol.SessionID]conversationCopy{}
	}
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
	owner := r.selectedRuntime()
	if owner != nil {
		owner.workspaceFrame(c)
	} else {
		if r.workspace != nil {
			r.releaseSelectedWorkspace()
			r.workspace.selected = protocol.WorktreeID{}
		}
		r.App.Panel.Actions = nil
		r.App.Panel.Data = &viewmodel.Workspace{}
	}
	r.workspaceSettingsFrame()
	r.terminalSettingsFrame()
	r.backupSettingsFrame(c)
	r.localModelsFrame()
	r.dictationFrame(c)
	r.permissionsFrame()
	r.teamsFrame()
	r.packagesFrame(c)
	r.pluginViewsFrame(c)
	r.terminalsFrame(c)
	r.pluginCardsFrame(c)
	r.networkFrame(c)
	r.browserFrame()
	r.aboutFrame(c)
	snapshot := r.snapshot()
	if snapshot.PowerSettingsRevision != r.powerObservedRevision && !r.App.Settings.Power.Pending {
		r.readPower()
	}
}

func (r *Runtime) afterFrame() {
	for _, bound := range r.bindings {
		bound.finishFrame()
		bound.conversationCopies = nil
	}
	r.finishFrame()
	r.conversationCopies = nil
	r.pluginViewsAfterFrame()
	if session, err := protocol.ParseSessionID(r.App.Messages.SearchSession()); err == nil && !r.seenConversation(session) {
		r.App.Messages.CloseSearch()
	}
	r.savePreferences()
}

func (r *Runtime) finishFrame() {
	r.pluginStatusAfterFrame()
	r.pluginControlsAfterFrame()
	r.pluginCardsAfterFrame()
	r.pruneAttachmentImages()
	r.pruneCommands()
	for id, release := range r.conversationWatches {
		if !r.conversationSeen[id] {
			release()
			delete(r.conversationWatches, id)
		}
	}
	// Leaving a conversation abandons its edit.
	for id := range r.conversationEdits {
		if session, err := protocol.ParseSessionID(id); err != nil || !r.conversationSeen[session] {
			delete(r.conversationEdits, id)
		}
	}
}

func (r *Runtime) project() {
	if r.parent != nil {
		r.parent.project()
		return
	}
	snapshot := r.snapshot()
	r.projectAdmission(&snapshot)
	d := Sidebar(snapshot, r.Preferences.Data)
	d.Notices = Notices(r.controller.Notices())
	for _, bound := range r.bindings {
		peerSnapshot := bound.snapshot()
		bound.projectAdmission(&peerSnapshot)
		peerData := Sidebar(peerSnapshot, r.Preferences.Data)
		peerData.Notices = Notices(bound.controller.Notices())
		d.Projects = append(d.Projects, peerData.Projects...)
		d.Unassigned = append(d.Unassigned, peerData.Unassigned...)
		d.Notices = append(d.Notices, peerData.Notices...)
		bound.projectModels()
		bound.prunePins(peerSnapshot)
		bound.pruneSessions(peerSnapshot)
	}
	r.projectHosts(d)
	r.App.ApplyData(d)
	r.selectPendingSession()
	r.projectConversations()
	r.projectProviderSnapshot()
	r.prunePins(snapshot)
	r.pruneSessions(snapshot)
}

func (r *Runtime) projectAdmission(snapshot *protocol.Snapshot) {
	if len(r.admittedSessions) == 0 {
		return
	}
	// The snapshot is shared, so admitted sessions go into this projection's own list.
	snapshot.Sessions = slices.Clone(snapshot.Sessions)
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
}

// pruneSessions forgets what was kept for sessions the Node no longer has. A
// draft is not a session yet and keeps its state.
func (r *Runtime) pruneSessions(snapshot protocol.Snapshot) {
	known := make(map[protocol.SessionID]bool, len(snapshot.Sessions))
	for _, session := range snapshot.Sessions {
		known[session.ID] = true
	}
	gone := func(id string) bool {
		session, err := protocol.ParseSessionID(id)
		return err == nil && !known[session]
	}
	maps.DeleteFunc(r.composerInitialized, func(id string, _ bool) bool { return gone(id) })
	maps.DeleteFunc(r.composerObserved, func(id string, _ string) bool { return gone(id) })
	maps.DeleteFunc(r.composerRevisions, func(id string, _ uint64) bool { return gone(id) })
	maps.DeleteFunc(r.composerSaving, func(id string, saving bool) bool { return !saving && gone(id) })
	maps.DeleteFunc(r.conversationEdits, func(id string, _ conversationEdit) bool { return gone(id) })
	for id, retry := range r.turnRetries {
		if !r.busy[retry.key] && gone(id) {
			r.forgetTurnRetry(id)
		}
	}
	if r.teams != nil {
		maps.DeleteFunc(r.teams.childParents, func(id string, _ protocol.SessionID) bool { return gone(id) })
	}
}

func (r *Runtime) prunePins(snapshot protocol.Snapshot) {
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

func (r *Runtime) observe(work func()) bool {
	r.mu.Lock()
	if r.closed {
		r.mu.Unlock()
		return false
	}
	r.observers.Add(1)
	r.mu.Unlock()
	go func() { defer r.observers.Done(); work() }()
	return true
}

func (r *Runtime) Refresh() {
	if r.parent == nil {
		owner := r.selectedRuntime()
		if r.App.Page == shell.HostPage {
			owner = r.runtimeFor(r.App.Host)
		}
		if owner == nil {
			r.Report(fileActionFault(protocol.ErrorCodeUnavailable, "the execution host is unavailable"))
			return
		}
		if owner != r {
			owner.Refresh()
			return
		}
	}
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
			if r.parent == nil {
				r.readSettings()
			} else {
				r.readSandbox()
			}
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
			code := faultCode(err)
			if code == protocol.ErrorCodeOutcomeUnknown || code == protocol.ErrorCodeUnavailable && (strings.HasPrefix(scope, "teams/") || strings.HasPrefix(scope, "session-team/")) {
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
	var pointer *protocol.Fault
	if errors.As(err, &pointer) {
		return pointer.Code
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
	if owner := r.conversationRuntime(cv, nil); owner == nil {
		r.Report(fileActionFault(protocol.ErrorCodeUnavailable, "the execution host is unavailable"))
		done(false)
		return
	} else if owner != r {
		owner.rename(cv, title, done)
		return
	}
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
	if owner := r.conversationRuntime(cv, nil); owner == nil {
		r.Report(fileActionFault(protocol.ErrorCodeUnavailable, "the execution host is unavailable"))
		return
	} else if owner != r {
		owner.archive(cv)
		return
	}
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
	if owner := r.conversationRuntime(cv, nil); owner == nil {
		r.Report(fileActionFault(protocol.ErrorCodeUnavailable, "the execution host is unavailable"))
		return
	} else if owner != r {
		owner.read(cv, read)
		return
	}
	id, err := protocol.ParseSessionID(cv.ID)
	if err != nil {
		return
	}
	r.command(protocol.Command{Kind: "set_session_read", Data: protocol.CommandSetSessionReadPayload{Session: id, ExpectedRevision: cv.AttentionRevision, Read: read}}, "session_changed", nil)
}
func (r *Runtime) removeSession(cv *viewmodel.Conversation) {
	if owner := r.conversationRuntime(cv, nil); owner == nil {
		r.Report(fileActionFault(protocol.ErrorCodeUnavailable, "the execution host is unavailable"))
		return
	} else if owner != r {
		owner.removeSession(cv)
		return
	}
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
	if owner := r.runtimeFor(p.Host); owner == nil {
		r.Report(fileActionFault(protocol.ErrorCodeUnavailable, "the execution host is unavailable"))
		return
	} else if owner != r {
		owner.removeProject(p)
		return
	}
	expected, err := expectedProject(p)
	if err != nil {
		return
	}
	r.command(protocol.Command{Kind: "remove_project", Data: protocol.CommandRemoveProjectPayload{Expected: expected}}, "project_changed", nil)
}
func (r *Runtime) saveProject(e sidebar.Editor, host string, done func(bool)) {
	if owner := r.runtimeFor(host); owner == nil {
		r.Report(fileActionFault(protocol.ErrorCodeUnavailable, "the execution host is unavailable"))
		done(false)
		return
	} else if owner != r {
		owner.saveProject(e, host, done)
		return
	}
	if host != "" && host != r.controller.Target().String() {
		done(false)
		return
	}
	source := protocol.ProjectsSource{Kind: "local"}
	if e.Clone == 1 && e.Project == nil {
		var branch *string
		if strings.TrimSpace(e.Branch) != "" {
			value := strings.TrimSpace(e.Branch)
			branch = &value
		}
		source = protocol.ProjectsSource{Kind: "clone", Data: protocol.ProjectsSourceClonePayload{Url: e.Repo, Branch: branch}}
	}
	command := protocol.Command{Kind: "create_project", Data: protocol.ProjectsDraft{Name: e.Name, Path: e.Path, Appearance: protocol.DefaultProjectsAppearance(), Source: source}}
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
	if owner != "" && owner != r.localRuntime().controller.Target().String() {
		r.enqueue(func(c *ui.Context) { kit.Toast(c, kit.Failed, i18n.T(c, "unavailable")) })
		return
	}
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
	cv := &viewmodel.Conversation{Node: r.controller.Target().String(), ID: fmt.Sprintf("draft/%d", r.drafts), Updated: time.Now(), Created: int(^uint(0) >> 1)}
	if p != nil {
		cv.Node = p.Host
	}
	if p == nil {
		r.App.Data.Unassigned = append([]*viewmodel.Conversation{cv}, r.App.Data.Unassigned...)
	} else {
		for _, current := range r.App.Data.Projects {
			if current.ID == p.ID && current.Host == p.Host {
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
	if owner := r.conversationRuntime(cv, nil); owner == nil {
		r.Report(fileActionFault(protocol.ErrorCodeUnavailable, "the execution host is unavailable"))
		return
	} else if owner != r {
		owner.pin(cv)
		return
	}
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
		owner := r.conversationRuntime(r.App.Data.Find(id), nil)
		if owner != nil {
			r.Preferences.Data.SetPin(owner.controller.Target(), id, preferences.Pin{Pinned: true, Order: &order})
		}
	}
	r.project()
}

// OpenSession and OpenNotice are called by window and notification callbacks on
// the UI thread; asynchronous work still returns through the same queue.
func (r *Runtime) OpenSession(id string) {
	r.pendingSession = id
	r.selectPendingSession()
}
func (r *Runtime) selectPendingSession() {
	if r.pendingSession == "" || r.App.Data.Find(r.pendingSession) == nil {
		return
	}
	r.App.Page = shell.ConversationPage
	r.App.Sidebar.Selected = r.pendingSession
	r.pendingSession = ""
}
func (r *Runtime) markNotice(id string) {
	if r.parent == nil {
		for _, bound := range r.bindings {
			bound.markNotice(id)
		}
	}
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

func (r *Runtime) SetOpenService(open func(string)) {
	r.openService = open
	for _, bound := range r.bindings {
		bound.SetOpenService(open)
	}
}
