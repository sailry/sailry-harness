// Package live adapts the shared Client to the accepted desktop contracts.
package live

import (
	"context"
	"encoding/json"
	"errors"
	"maps"
	"path/filepath"
	"runtime"
	"slices"
	"strings"
	"sync"
	"time"
	"weak"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/sidebar"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/memory"
	nodeplugins "github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type Runtime struct {
	nativeWindowResources []nativeWindowResource

	executionSettingFactories []settings.ExecutionSettingFactory
	executionSettings         []settings.ExecutionSetting
	worktreeActions           shell.ConversationWorktreeActions
	projectLocations          func(shell.ProjectLocationReader)
	workspaceActions          map[string]shell.WorkspaceAction

	pluginViews               *pluginViewState
	pluginControlReplacements []shell.PluginControlReplacement
	// demoModels is the model each demo conversation's composer is set to,
	// by conversation: what the statistics package reads as its model.
	demoModels           sync.Map
	pluginStatus         map[string]*pluginStatusFeed
	pluginControls       map[string]*pluginControlPresentation
	pluginCards          *pluginCardState
	terminals            *terminalState
	browser              *browserState
	browserPanel         bool
	browserSources       *browserSources // a test's Chrome profile, key and cookie store
	browserConfig        browserSettingsState
	searchSettings       searchSettingsState
	memorySettings       memorySettingsState
	computerSettings     computerSettingsState
	authorizations       *authorizationState
	packages             *packageState
	parent               *Runtime
	network              *Network
	bindings             map[string]*Runtime
	resourceCatalog      *protocol.ResourceCatalog
	unsubscribeNetwork   func()
	nodeModels           []composer.Model
	hosts                map[string]*liveHost
	hostsProjection      hostsProjectionKey
	localHostID          string
	localHostIDNode      protocol.NodeID
	connections          *connectionState
	connectionReferences map[string]*referenceCatalog
	sshTransfers         *sshTransferStore
	ports                *livePorts
	pickKeyFile          func(func(string))
	teams                *teamState
	commands             *liveCommands
	openService          func(string)
	attachments          *attachmentState
	workspace            *workspaceState
	resourcePage         resourcePageState
	workspaceSettings    workspaceSettingsState
	hookSettings         hookSettingsState
	personalInstructions personalInstructionsState
	// instructionsOwner is the Runtime whose custom prompt the settings page
	// edits, on the local Runtime.
	instructionsOwner   *Runtime
	settingsShown       *Runtime
	settingsGeneration  uint64
	settingsPages       []liveSettingsPage
	providerMoves       map[string]providerMove
	terminalSettings    terminalSettingsState
	backupSettings      backupSettingsState
	localModels         localModelState
	knowledge           knowledgeState
	media               mediaState
	dictation           dictationState
	usage               featureUsage
	permissions         permissionState
	knowledgeWanted     string // the citation the knowledge viewer was last asked for
	pendingSession      string
	pendingSelected     string // the selection pendingSession waits behind
	transferDialogOwner *Runtime
	admittedSessions    map[string]protocol.Session

	// acknowledged is the attention revision each shown session was marked
	// read at (attentionFrame).
	acknowledged map[string]uint64
	// keptUnread is the shown session the user marked unread: it stays so
	// until another one is shown.
	keptUnread           string
	composerRevisions    map[string]uint64
	composerSaving       map[string]bool
	composerWaiters      map[string][]func(bool)
	composerBases        map[*composer.State]protocol.SessionConfig
	composerBaseRevision uint64
	sessionBases         map[*composer.State]*sessionBaseStart
	App                  *shell.App
	Preferences          *preferences.Store

	// delivered counts the frames that projected a change or ran delivered
	// results, which is what frame-kept projections are invalidated by
	// besides their own keys.
	delivered uint64
	// composerCatalogs are the composer catalogs by conversation.
	composerCatalogs map[string]*composerCatalog
	// pluginCatalog is pluginViewInfos as of pluginCatalogKey.
	pluginCatalog    []protocol.PluginInfo
	pluginCatalogKey pluginCatalogKey

	controller *client.Client
	ctx        context.Context
	// filePreview is the window's preview dialog (file_preview.go).
	filePreview     *filePreview
	pendingFileLink *conversationFileLink
	// memoryNext is when Settings › Experimental samples memory again.
	memoryNext time.Time
	// memoryBase is the last sample of the Go classes, beside which the
	// plugins' group updates.
	memoryBase                []kit.TreemapGroup
	pluginMemory              *pluginMemoryState
	cancel                    context.CancelFunc
	stopMemory                func()
	unsubscribe               func()
	unsubscribePrefs          func()
	mu                        sync.Mutex
	dirty                     bool
	readsConnected            bool
	closed                    bool
	queue                     []func(*ui.Context)
	invalidate                func()
	update                    func(func())
	workers                   sync.WaitGroup
	observers                 sync.WaitGroup
	openWindow                func(string)
	pickDirectory             func(func(string))
	savedProject              string // the project the editor saved, opened once projected
	preferencesChanged        func()
	busy                      map[string]bool
	commandRevision           uint64
	uncertain                 map[string]protocol.Request
	sessionRemovals           map[removalKey]*sessionRemoval
	confirmedRemovals         map[protocol.NodeID]map[protocol.SessionID]bool
	drafts                    uint64
	openDrafts                map[string]bool // drafts New session opened, on the local Runtime
	draftsShown               map[string]bool // those a pane showed this frame
	lastPreferences           preferences.Data
	lastMaterials             [3]float32
	lastFeatureOrder          []string
	lastFeaturePins           map[string]bool
	lastKeys                  map[string]string
	powerObservedRevision     uint64
	powerSaving               map[protocol.PowerSettings]func() bool
	compactionObserved        uint64
	updates                   *Updates
	updatesFocused            func() bool
	updatesFlush              func() error
	prefsRevision             uint64
	conversationWatches       map[protocol.SessionID]func()
	conversationSeen          map[protocol.SessionID]bool
	removedRead               uint64 // how far the Client's removed sessions were read
	childStops                map[protocol.TurnID]bool
	composerInitialized       map[string]bool
	composerObserved          map[string]string
	composerObservedConfig    map[string]protocol.SessionConfig
	readAloud                 func(string, string, string)
	exportConversation        func(string, string, conversation.ExportFormat)
	conversationEdits         map[string]conversationEdit
	conversationPending       map[string]protocol.TurnID // the turn a fork or rewind in flight acts on
	turnRetries               map[string]turnRetry
	interactionFlows          map[string]*interactionFlow
	conversationCreations     map[string]*conversationCreation
	conversationCreating      map[string]bool
	providerPresets           map[string]protocol.ConversationPresetsEntry
	providerBrands            map[string]string // brand names by presetBrand, offered or not
	providerCatalogBrands     []viewmodel.Brand
	providerPresetsLoaded     bool
	providerPresetsFailed     bool
	providerPresetsLoading    bool
	providerPresetPackages    []protocol.PluginSummary
	providerModels            map[string]providerDiscovery
	providerDraftBases        map[string]protocol.ConversationProvider
	modelCatalogEntered       bool
	modelCatalogLoading       bool
	modelScores               modelScoreState
	modelProjection           modelProjection
	snapshotMu                sync.Mutex
	snapshotCopy              protocol.Snapshot
	snapshotRevision          uint64
	snapshotRead              bool
	conversationCopies        map[protocol.SessionID]conversationCopy
	conversationCache         map[protocol.SessionID]conversationCopy
	conversationFeeds         map[protocol.SessionID]*conversationFeed
	conversationLane          chan struct{}
	conversationVersion       uint64
	conversationPresentations map[*viewmodel.Conversation]conversationPresentation
	childConversations        map[protocol.SessionID]*viewmodel.Conversation
	conversationChanges       map[protocol.SessionID]struct{} // guarded by mu
	// The transcript and queue tray retain their own input until its Node projection arrives.
	composerOutgoing     map[string]*conversationOutgoing
	composerTrayOutgoing map[string]*conversationOutgoing

	queuedSends map[protocol.SessionID]map[protocol.TurnID]*queuedSend
	// guidanceSent is the guidance this desktop sent from the composer and
	// has not seen settled, by conversation and turn: a message that did not
	// join its recipient is reported once.
	guidanceSent map[string]map[protocol.TurnID]sentGuidance
	// queueHalts is the last halt read for each conversation's queue, by
	// conversation: a halt is reported once, when it is first read.
	queueHalts map[string]protocol.ConversationQueuePause
	// usageFailed is whether each conversation's statistics were last read
	// failed, by conversation: a failure is reported when it begins, once.
	usageFailed map[string]bool
	// overviews are the asset inventories the overview popover read.
	overviews map[protocol.SessionID]*overviewRead
}

type conversationCopy struct {
	stale      bool
	state      client.ConversationState
	ok         bool
	generation uint64
	composer   *composerHistory
	// usage outlives view changes so an unchanged usage keeps its identity.
	usage    *composerHistory
	prepared *preparedConversation
	controls *conversationControlFacts
	request  uint64
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

// conversationState shares a read-only copy until an observation changes.
// The persistent cache is attached only during UI frames; actions outside a
// frame read the Client directly.
func (r *Runtime) conversationState(id protocol.SessionID) (client.ConversationState, bool) {
	if feed := r.conversationFeeds[id]; feed != nil {
		// All consumers of a mounted transcript use the same publication in a
		// frame. Action guards read the Client directly, not this view.
		copied := r.conversationCache[id]
		return copied.state, copied.ok
	}
	if r.conversationCopies == nil {
		return r.controller.Conversation(id)
	}
	copied, ok := r.conversationCopies[id]
	if !ok && r.conversationWatches[id] == nil {
		// Workspace and composer hooks can ask before the new pane mounts.
		// Do not enter the shared Client reducer for a view this window does
		// not own (another window may be restoring a very large page).
		return client.ConversationState{}, false
	}
	if !ok || copied.stale {
		previous := copied.state
		copied.state, copied.ok = r.controller.ConversationView(id, previous)
		current := copied.state
		if !current.SameViewSnapshot(previous) {
			copied.composer = nil
		}
		if !sameTranscriptSource(current, previous) {
			r.conversationVersion++
			copied.generation = r.conversationVersion
		}
		copied.stale = false
		r.conversationCopies[id] = copied
	}
	return copied.state, copied.ok
}

// conversationChanged redraws the changed conversation without projecting the
// Node snapshot or dropping copies of other observed conversations.
func (r *Runtime) conversationChanged(id protocol.SessionID) {
	r.mu.Lock()
	if r.conversationChanges == nil {
		r.conversationChanges = map[protocol.SessionID]struct{}{}
	}
	r.conversationChanges[id] = struct{}{}
	r.mu.Unlock()
	r.localRuntime().wake()
}

// New composes a window over an already started Client. Startup code should use
// NewWithAssembly with a Store opened before Node.Start for onboarding.
func New(ctx context.Context, controller *client.Client, profilePath string, pages ...shell.NativePage) (*Runtime, error) {
	return NewWithProfile(ctx, controller, profilePath, shell.Assembly{WorkspacePages: pages})
}

func NewWithProfile(ctx context.Context, controller *client.Client, profilePath string, assembly shell.Assembly) (*Runtime, error) {
	return NewWithAssembly(ctx, controller, preferences.Open(filepath.Join(profilePath, "desktop", "preferences.json")), assembly)
}

func NewWithPreferences(ctx context.Context, controller *client.Client, prefs *preferences.Store, pages ...shell.NativePage) (*Runtime, error) {
	return NewWithAssembly(ctx, controller, prefs, shell.Assembly{WorkspacePages: pages})
}

func NewWithAssembly(ctx context.Context, controller *client.Client, prefs *preferences.Store, assembly shell.Assembly) (*Runtime, error) {
	if controller == nil || prefs == nil {
		return nil, errors.New("desktop requires a Client and profile preferences")
	}
	ctx, cancel := context.WithCancel(ctx)
	r := &Runtime{controller: controller, ctx: ctx, cancel: cancel, Preferences: prefs, busy: map[string]bool{}, uncertain: map[string]protocol.Request{}, readsConnected: controller.Connected()}
	snapshot := controller.Snapshot()
	d := Sidebar(snapshot, prefs.Data)
	d.Notices = Notices(controller.Notices())
	r.App = shell.NewLiveWithAssembly(d, assembly)
	bindErr := r.bindWorkspaceAppActions()
	if bindErr == nil {
		bindErr = r.bindConversationLinks()
	}
	if bindErr != nil {
		r.App.CloseWindowBindings()
		cancel()
		return nil, bindErr
	}
	controller.OnDispatch(countSaved(controller))
	r.pluginControlReplacements = slices.Clone(assembly.PluginControlReplacements)
	r.permissions.contributions = permissionContributions(assembly.Permissions)
	r.App.SetLocalHost(localHost(snapshot, d))
	for _, pkg := range snapshot.Plugins {
		if nodeplugins.IsLegacyDesktopPackage(pkg.Name, pkg.Digest) {
			r.App.InheritPackagePreferences(controller.Target().String(), pkg.Name)
		}
	}
	r.applyPreferences()
	r.App.Hooks = shell.Hooks{BeforeFrame: r.drain, AfterFrame: r.afterFrame, Refresh: r.Refresh, NewConversationDraft: r.newConversationDraft,
		ReadNotice: r.markNotice, ReadNotices: r.readNotices, DismissRead: r.dismissRead, DismissNotices: r.dismissNotices, OpenKnowledge: r.openKnowledge,
		CaptureSelection: func() { r.CaptureSelection(nil) }}
	r.App.Messages.SessionLink = func(link string) bool { return client.SessionLink(normalizeLink(link)) != nil }
	r.App.Sidebar.Actions = &sidebar.Actions{Rename: r.rename, Archive: r.archive, Unarchive: r.unarchive, Read: r.read, Delete: r.removeSessions,
		Pin: r.pin, MovePin: r.movePin, MoveSession: r.moveSession, NewDraft: r.newDraft, RemoveProject: r.removeProject, SaveProject: r.saveProject, Files: r.files, ForkHere: r.forkHere, Export: r.exportSession, OpenHost: r.App.OpenHost, OpenWindow: func(id string) {
			if r.openWindow != nil {
				r.openWindow(id)
			}
		}}
	r.bindPicker()
	r.initCommands()
	r.initAttachments()
	r.App.Hooks.DownloadDialog = r.transferDialogs
	r.App.Hooks.FilePreview = r.renderFilePreview
	r.initWorkspace()
	r.bindProjectWorkflows(assembly.ProjectWorkflows)
	r.App.Hooks.WorkspaceProject = r.workspaceProject
	r.App.Hooks.ResourceWorkspace = r.resourceWorkspace
	r.App.Settings.SaveTelemetry = r.saveTelemetry
	r.App.Settings.ResetTelemetry = r.resetTelemetry
	r.App.Settings.SendDiagnostics = r.sendDiagnostics
	r.App.Hooks.Conversation = r.conversation
	r.App.Hooks.DemoConversation = r.demoConversation
	r.App.Hooks.RenderChild = r.renderLiveChild
	r.App.Hooks.Overview = r.sessionOverview
	r.conversationWatches = map[protocol.SessionID]func(){}
	r.conversationSeen = map[protocol.SessionID]bool{}
	r.composerInitialized = map[string]bool{}
	r.composerObserved = map[string]string{}
	r.composerObservedConfig = map[string]protocol.SessionConfig{}
	r.composerRevisions = map[string]uint64{}
	r.composerSaving = map[string]bool{}
	r.composerWaiters = map[string][]func(bool){}
	r.composerBases = map[*composer.State]protocol.SessionConfig{}
	r.sessionBases = map[*composer.State]*sessionBaseStart{}
	r.admittedSessions = map[string]protocol.Session{}
	r.conversationEdits = map[string]conversationEdit{}
	r.conversationPending = map[string]protocol.TurnID{}
	r.conversationCreations = map[string]*conversationCreation{}
	r.conversationCreating = map[string]bool{}
	r.listSidebar()
	r.ensureConversationSelection()
	r.unsubscribe = controller.OnChange(r.changed)
	r.unsubscribePrefs = prefs.OnChange(r.wake)
	if err := prefs.FinishStartup(); err != nil {
		key := "preferences_save_failed"
		if !prefs.Readable {
			key = "preferences_read_failed"
		}
		r.enqueue(func(c *ui.Context) { kit.Fail(c, kit.Failed, key, err) })
	}
	r.prefsRevision = prefs.Revision()
	r.readTelemetry()
	r.readSandbox()
	r.projectModels()
	r.initTeams()
	r.executionSettingFactories = slices.Clone(assembly.ExecutionSettings)
	r.bindExecutionSettings()
	r.initNodeSettings()
	r.initConversationSearch()
	r.initNetworkUI()
	r.initB8Packages()
	r.App.BindPanelResources(assembly, shell.PanelResourceHost{
		BindBrowser: r.bindPanelBrowser,
		Report:      r.Report,
		BrowserNavigation: shell.BrowserNavigation{
			Available: r.panelBrowserAvailable,
			Navigate:  r.browserNavigate,
		},
		BindTerminal: func() sidebar.ProjectOpen {
			r.initLiveTerminals()
			return r.newProjectTerminal
		},
	})
	r.initPluginViews()
	r.App.Hooks.PluginOverlays = r.renderPluginOverlays
	r.App.Hooks.PluginTitle = r.renderPluginTitle
	r.bindNativeWindow()
	r.bindSettingsPages(assembly.SettingsPages)
	// A Client event may arrive between the first snapshot and subscription.
	// Project its latest state on the first frame even if no later event arrives.
	r.changed()
	r.observeResourceCatalog()
	weakRoot := weak.Make(r)
	stopMemory := memory.Default().Register("desktop.runtime", func(level memory.Level) {
		if level < memory.Pressure {
			return
		}
		if root := weakRoot.Value(); root != nil {
			root.enqueue(func(*ui.Context) { root.trimMemory() })
		}
	})
	r.stopMemory = stopMemory
	runtime.AddCleanup(r, func(stop func()) { stop() }, stopMemory)
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
	r.queue = nil
	r.mu.Unlock()
	if r.updates != nil {
		// A closing window no longer needs to acknowledge an installation's
		// read-only frame. The remaining UI frames remove it from the wait.
		r.updates.changed()
	}
	if r.unsubscribe != nil {
		r.unsubscribe()
	}
	if r.unsubscribePrefs != nil {
		r.unsubscribePrefs()
	}
	if r.unsubscribeNetwork != nil {
		r.unsubscribeNetwork()
	}
	if r.stopMemory != nil {
		r.stopMemory()
		r.stopMemory = nil
	}
	r.closeSettingsPages()
	for _, bound := range r.bindings {
		_ = bound.Close()
	}
	r.closeNetworkUI()
	for _, release := range r.conversationWatches {
		release()
	}
	r.closeNativeWindow()
	r.closeDocuments()
	r.closePluginViews()
	r.closePluginStatus()
	r.closePluginCards()
	r.closePluginMemory()
	r.closeCommands()
	r.closeAttachments()
	r.invalidateFilePreview(nil)
	if r.backupSettings.scheduleTimer != nil {
		r.backupSettings.scheduleTimer.Stop()
		r.backupSettings.scheduleTimer = nil
	}
	for id := range r.turnRetries {
		r.forgetTurnRetry(id)
	}
	for _, pending := range r.conversationCreations {
		pending.releaseAdmission()
	}
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
	r.beforeUpdateFrame()
	r.beginConversationFrame()
	r.syncNetwork()
	for _, bound := range r.bindings {
		bound.beginConversationFrame()
	}
	delivered := false
	if revision := r.Preferences.Revision(); revision != r.prefsRevision {
		r.applyPreferences()
		r.prefsRevision = revision
		r.project()
		delivered = true
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
	// After the results, which may write what an earlier one derived from.
	if delivered || dirty || len(queue) > 0 {
		r.delivered++
	}
	if dirty {
		r.observeConversationCreations()
		for _, bound := range r.bindings {
			bound.observeConversationCreations()
		}
	}
	r.resumeReadsOnReconnect()
	for _, bound := range r.bindings {
		bound.resumeReadsOnReconnect()
	}
	r.draftsShown = map[string]bool{}
	r.listSidebar()
	r.conversationPanelsFrame()
	if r.savedProject != "" {
		for _, project := range r.App.Data.Projects {
			if project.ID == r.savedProject {
				r.savedProject = ""
				r.App.Sidebar.Reveal(project.ID)
				r.openProjectOverview(project)
				break
			}
		}
	}
	owner := r.selectedRuntime()
	if owner != r {
		r.clearWorkspaceSelection()
	}
	for _, bound := range r.bindings {
		if owner != bound {
			bound.clearWorkspaceSelection()
		}
	}
	if owner != nil {
		owner.workspaceFrame(c)
	} else {
		r.App.Panel.Actions = nil
		r.App.Panel.Data = &viewmodel.Workspace{}
		r.App.Panel.Session = false
	}
	r.resourceWorkspaceFrame(owner)
	r.App.Panel.Worktree = panelTerminalWorktree(owner) != (protocol.WorktreeID{})
	r.nodeSettingsFrame()
	r.workspaceSettingsFrame()
	r.terminalSettingsFrame()
	r.backupSettingsFrame(c)
	r.memoryMapFrame(c)
	r.localModelsFrame()
	r.dictationFrame(c)
	r.permissionsFrame()
	r.packagesFrame(c)
	r.pluginViewsFrame(c)
	r.terminalsFrame(c)
	r.pluginCardsFrame(c)
	r.networkFrame(c)
	r.frameNativeWindowResources()
	r.documentFrame()
	r.aboutFrame(c)
	r.ensureConversationSelection()
	r.attentionFrame()
}

// attentionFrame marks the conversation on screen read: a session whose
// turn ended is unread until it is shown in the focused window, as the
// reference acknowledges it (shell/session_scope/attention.rs). One
// acknowledgement goes out per attention revision; one that lost to a newer
// revision is sent again for that revision.
func (r *Runtime) attentionFrame() {
	if r.parent != nil || r.App.Page != shell.ConversationPage || r.updatesFocused != nil && !r.updatesFocused() {
		return
	}
	cv := r.App.Data.Find(r.App.FocusedConversation())
	if cv == nil || cv.ID == r.keptUnread {
		return
	}
	r.keptUnread = ""
	owner := r.conversationRuntime(cv, nil)
	if owner == nil {
		return
	}
	session, ok := owner.session(cv.ID)
	if !ok || !session.Activity.Attention.Unread {
		return
	}
	revision := session.Activity.Attention.Revision
	if sent, ok := owner.acknowledged[cv.ID]; ok && sent == revision {
		return
	}
	if owner.acknowledged == nil {
		owner.acknowledged = map[string]uint64{}
	}
	owner.acknowledged[cv.ID] = revision
	owner.execute(protocol.Command{Kind: "set_session_read", Data: protocol.CommandSetSessionReadPayload{Session: session.ID, ExpectedRevision: revision, Read: true}}, func(*ui.Context, protocol.Output, error) {})
}

func (r *Runtime) afterFrame() {
	for _, bound := range r.bindings {
		bound.finishFrame()
		bound.finishConversationFrame()
		bound.releaseComposerCatalogs()
	}
	r.finishFrame()
	r.finishConversationFrame()
	r.releaseComposerCatalogs()
	r.countFrame()
	r.pluginViewsAfterFrame()
	r.finishNativeWindowResources()
	r.pruneDrafts()
	r.pruneIdleComposers()
	r.pruneClosedChildTabs()
	// Each conversation has its own search bar: one whose conversation is no
	// longer shown closes alone.
	for _, id := range r.App.Messages.SearchSessions() {
		if session, err := protocol.ParseSessionID(id); err == nil && !r.seenConversation(session) {
			r.App.Messages.CloseSearchIn(id)
		}
	}
	r.savePreferences()
	r.afterUpdateFrame()
}

func (r *Runtime) finishFrame() {
	r.pluginStatusAfterFrame()
	r.pluginControlsAfterFrame()
	r.pluginCardsAfterFrame()
	r.pruneAttachmentImages()
	r.pruneDocumentPreviews()
	r.pruneCommands()
	for id, release := range r.conversationWatches {
		if !r.conversationSeen[id] || r.conversationCopies[id].state.Removed {
			release()
			delete(r.conversationWatches, id)
		}
	}
	r.pruneWorkspaceCheckpoints()
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
	removed := r.reconcileSessionRemovals(snapshot.Sessions)
	r.projectAdmission(&snapshot)
	listed := snapshot
	listed.Sessions = r.filterRemovedSessions(snapshot.Sessions)
	d := Sidebar(listed, r.Preferences.Data)
	d.Notices = Notices(r.controller.Notices())
	var peerRemovals map[*Runtime][]protocol.SessionID
	for _, bound := range r.bindings {
		peerSnapshot := bound.snapshot()
		if ids := bound.reconcileSessionRemovals(peerSnapshot.Sessions); len(ids) != 0 {
			if peerRemovals == nil {
				peerRemovals = make(map[*Runtime][]protocol.SessionID)
			}
			peerRemovals[bound] = ids
		}
		d.Notices = append(d.Notices, Notices(bound.controller.Notices())...)
		bound.projectModels()
		bound.prunePins(peerSnapshot)
		bound.pruneSessions(peerSnapshot)
	}
	r.appendResourceDirectory(d)
	r.projectHosts(d)
	previous := r.App.Data
	r.applyConversationData(d)
	r.forgetRemovedSessions(removed)
	for owner, ids := range peerRemovals {
		owner.forgetRemovedSessions(ids)
	}
	r.carryConversationPresentations(previous)
	for _, bound := range r.bindings {
		bound.carryConversationPresentations(previous)
	}
	r.projectModels()
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
	maps.DeleteFunc(r.composerObservedConfig, func(id string, _ protocol.SessionConfig) bool { return gone(id) })
	maps.DeleteFunc(r.composerRevisions, func(id string, _ uint64) bool { return gone(id) })
	maps.DeleteFunc(r.composerCatalogs, func(id string, _ *composerCatalog) bool { return gone(id) })
	maps.DeleteFunc(r.composerSaving, func(id string, saving bool) bool { return !saving && gone(id) })
	maps.DeleteFunc(r.conversationEdits, func(id string, _ conversationEdit) bool { return gone(id) })
	maps.DeleteFunc(r.composerOutgoing, func(id string, _ *conversationOutgoing) bool { return gone(id) })
	maps.DeleteFunc(r.composerTrayOutgoing, func(id string, _ *conversationOutgoing) bool { return gone(id) })
	maps.DeleteFunc(r.acknowledged, func(id string, _ uint64) bool { return gone(id) })
	maps.DeleteFunc(r.queueHalts, func(id string, _ protocol.ConversationQueuePause) bool { return gone(id) })
	maps.DeleteFunc(r.usageFailed, func(id string, _ bool) bool { return gone(id) })
	maps.DeleteFunc(r.guidanceSent, func(id string, _ map[protocol.TurnID]sentGuidance) bool { return gone(id) })
	maps.DeleteFunc(r.queuedSends, func(id protocol.SessionID, _ map[protocol.TurnID]*queuedSend) bool { return !known[id] })
	maps.DeleteFunc(r.interactionFlows, func(id string, flow *interactionFlow) bool {
		return gone(id) && (flow == nil || flow.approval == nil && len(flow.questions) == 0)
	})
	maps.DeleteFunc(r.connectionReferences, func(key string, entry *referenceCatalog) bool {
		if entry != nil && entry.loading {
			return false
		}
		if session, _, ok := strings.Cut(key, "\x00"); ok {
			return gone(session)
		}
		return false
	})
	if r.attachments != nil {
		maps.DeleteFunc(r.attachments.files, func(_ string, desc attachmentDescriptor) bool {
			return desc.session != (protocol.SessionID{}) && !known[desc.session]
		})
	}
	if r.App != nil && !r.App.Demo() {
		maps.DeleteFunc(r.composerBases, func(comp *composer.State, _ protocol.SessionConfig) bool { return !r.App.HasDraft(comp) })
		maps.DeleteFunc(r.sessionBases, func(comp *composer.State, _ *sessionBaseStart) bool { return !r.App.HasDraft(comp) })
		if r.parent == nil {
			maps.DeleteFunc(r.usage.dictated, func(comp *composer.State, _ bool) bool { return !r.App.HasDraft(comp) })
		}
	}
	for slot, retry := range r.turnRetries {
		if r.busy[retry.key] {
			continue
		}
		if slot != retry.session && retry.admitted(snapshot.Turns, nil) {
			r.confirmTurnRetry(slot, retry)
		} else if gone(retry.session) {
			r.forgetTurnRetry(slot)
		}
	}
	if r.teams != nil {
		maps.DeleteFunc(r.teams.childParents, func(id string, _ protocol.SessionID) bool { return gone(id) })
		maps.DeleteFunc(r.teams.composers, func(id string, state *teamComposer) bool {
			return !state.inFlight() && gone(id)
		})
		if r.currentSettingsPage(settings.Teams) == nil {
			knownRoles := make(map[protocol.RoleID]uint64, len(snapshot.Roles))
			for _, role := range snapshot.Roles {
				knownRoles[role.ID] = role.Revision
			}
			maps.DeleteFunc(r.teams.roles, func(_ string, role protocol.RoleProfile) bool {
				rev, ok := knownRoles[role.ID]
				return !ok || rev != role.Revision
			})
			knownTeams := make(map[protocol.TeamID]uint64, len(snapshot.Teams))
			for _, team := range snapshot.Teams {
				knownTeams[team.ID] = team.Revision
			}
			maps.DeleteFunc(r.teams.teams, func(_ string, team protocol.TeamProfile) bool {
				rev, ok := knownTeams[team.ID]
				return !ok || rev != team.Revision
			})
		}
	}
	r.pruneProviderDraftBases(snapshot)
}

func (r *Runtime) pruneIdleComposers() {
	if r.parent != nil || r.App == nil || r.App.Demo() || r.App.Sidebar == nil {
		return
	}
	open := r.App.OpenSessionIDs()
	inUseByAttachment := func(owner *Runtime, comp *composer.State) bool {
		if owner.attachments == nil {
			return false
		}
		for _, record := range owner.attachments.items {
			if record != nil && record.composer == comp {
				return true
			}
		}
		return false
	}
	inFlight := func(owner *Runtime, id string) bool {
		return owner.teams != nil && owner.teams.composers[id].inFlight() ||
			owner.conversationCreating[id] ||
			owner.composerOutgoing[id] != nil ||
			owner.composerTrayOutgoing[id] != nil ||
			owner.composerSaving[id] ||
			len(owner.composerWaiters[id]) > 0
	}
	dropped := r.App.PruneComposers(func(id string) bool {
		if id == r.App.Sidebar.Selected || r.draftsShown[id] || r.openDrafts[id] || open[id] {
			return true
		}
		session, err := protocol.ParseSessionID(id)
		if err != nil {
			return true
		}
		if r.seenConversation(session) || inFlight(r, id) {
			return true
		}
		for _, bound := range r.bindings {
			if inFlight(bound, id) {
				return true
			}
		}
		comp := r.App.DraftComposer(id)
		if comp != nil {
			if inUseByAttachment(r, comp) {
				return true
			}
			for _, bound := range r.bindings {
				if inUseByAttachment(bound, comp) {
					return true
				}
			}
		}
		return false
	})
	for id, comp := range dropped {
		r.releaseComposerMetadata(id, comp)
	}
}

func (r *Runtime) trimMemory() {
	if r.parent != nil {
		return
	}
	r.pruneIdleComposers()
	r.pruneDocumentPreviews()
	if r.pluginViews != nil {
		clear(r.pluginViews.icons)
	}
	maps.DeleteFunc(r.connectionReferences, func(_ string, entry *referenceCatalog) bool {
		return entry == nil || !entry.loading
	})
	for _, bound := range r.bindings {
		maps.DeleteFunc(bound.connectionReferences, func(_ string, entry *referenceCatalog) bool {
			return entry == nil || !entry.loading
		})
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
		if target := r.App.ResourceTarget(); target.Project != "" {
			owner = r.runtimeFor(target.Node)
		} else if r.App.Page == shell.HostPage {
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
			} else {
				r.retryFailedReads()
			}
			r.readSettings()
		})
	})
}

// execute captures immutable input and retains its request ID while its
// outcome is open: the Node still runs it or could not be asked. The same
// command, sent again, then carries the same ID and resolves it. Once the
// Client reports the request applied, failed or released (client.Released),
// the ID is dropped and the next attempt is a new request. All outcome
// delivery and draft changes run on the UI thread.
func (r *Runtime) execute(command protocol.Command, finished func(*ui.Context, protocol.Output, error)) {
	r.executeScoped("", command, finished)
}

func (r *Runtime) executeScoped(scope string, command protocol.Command, finished func(*ui.Context, protocol.Output, error), delivery ...func(func(*ui.Context))) protocol.RequestID {
	return r.executeObserved(scope, command, nil, finished, delivery...)
}

func (r *Runtime) executeObserved(scope string, command protocol.Command, prepared func(protocol.RequestID), finished func(*ui.Context, protocol.Output, error), delivery ...func(func(*ui.Context))) protocol.RequestID {
	deliver := r.enqueue
	if len(delivery) != 0 {
		deliver = delivery[0]
	}
	if r.parent != nil && !r.controller.Connected() {
		deliver(func(c *ui.Context) {
			finished(c, protocol.Output{}, resourceOfflineFault())
		})
		return protocol.RequestID{}
	}
	b, err := json.Marshal(command)
	if err != nil {
		deliver(func(c *ui.Context) { finished(c, protocol.Output{}, err) })
		return protocol.RequestID{}
	}
	key := scope + "\x00" + string(b)
	if r.busy[key] {
		return protocol.RequestID{}
	}
	r.busy[key] = true
	r.commandRevision++
	request, exists := r.uncertain[key]
	if !exists {
		request = r.controller.Prepare(command)
	}
	if prepared != nil {
		prepared(request.ID)
	}
	if !r.async(func() {
		output, err := r.controller.Settle(r.ctx, request)
		deliver(func(c *ui.Context) {
			delete(r.busy, key)
			r.commandRevision++
			code := faultCode(err)
			if client.Retained(err) || code == protocol.ErrorCodeUnavailable && !client.Released(err) && (strings.HasPrefix(scope, "teams/") || strings.HasPrefix(scope, "session-team/")) {
				r.uncertain[key] = request
			} else {
				delete(r.uncertain, key)
			}
			finished(c, output, err)
		})
	}) {
		delete(r.busy, key)
		return protocol.RequestID{}
	}
	return request.ID
}

// forgetCommand drops the request kept for an unscoped command whose outcome
// was not confirmed, so the same command sent later is a new request.
func (r *Runtime) forgetCommand(command protocol.Command) {
	r.forgetScoped("", command)
}

// forgetScoped drops the request kept for command under scope.
func (r *Runtime) forgetScoped(scope string, command protocol.Command) {
	if b, err := json.Marshal(command); err == nil {
		r.dropUncertainCommand(scope + "\x00" + string(b))
	}
}

func (r *Runtime) dropUncertainCommand(key string) {
	if _, exists := r.uncertain[key]; exists {
		delete(r.uncertain, key)
		r.commandRevision++
	}
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

func resourceOfflineFault() *protocol.Fault {
	return &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "resource host is offline", Details: &protocol.FaultDetails{Kind: "network"}}
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
	if faultMessage(err) == "resource host is offline" {
		key = "resource_offline_action"
	}
	kit.Toast(c, o, i18n.T(c, key))
	kit.RecordFailure("desktop", "chat_action_failed", o, err)
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

// unarchive returns an archived session to its project's rows.
func (r *Runtime) unarchive(cv *viewmodel.Conversation) {
	if owner := r.conversationRuntime(cv, nil); owner == nil {
		r.Report(fileActionFault(protocol.ErrorCodeUnavailable, "the execution host is unavailable"))
		return
	} else if owner != r {
		owner.unarchive(cv)
		return
	}
	id, err := protocol.ParseSessionID(cv.ID)
	if err != nil {
		return
	}
	r.command(protocol.Command{Kind: "set_session_archived", Data: protocol.CommandSetSessionArchivedPayload{Session: id, ExpectedRevision: cv.Revision, Archived: false}}, "session_changed", nil)
}
func (r *Runtime) read(cv *viewmodel.Conversation, read bool) {
	if !read {
		r.localRuntime().keptUnread = cv.ID
	}
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

// forgetRemovedSessions takes sessions this runtime's Node confirmed removed
// out of the window's tabs and splits: its command's answer, or its
// sessions_removed event (Client.RemovedSessions), which covers a removal by
// another client. A session only absent, as one not loaded, of a host
// offline, archived or hidden by a withdrawn permission, never reaches here.
// The window's tabs name a session by ID alone, so one another host still
// lists keeps them.
func (r *Runtime) forgetRemovedSessions(ids []protocol.SessionID) {
	if len(ids) == 0 {
		return
	}
	removed := make(map[protocol.SessionID]bool, len(ids))
	for _, id := range ids {
		removed[id] = true
	}
	for _, owner := range r.terminalOwners() {
		if owner.controller.Target() == r.controller.Target() {
			continue
		}
		confirmed := owner.localRuntime().confirmedRemovals[owner.controller.Target()]
		for _, session := range owner.snapshot().Sessions {
			if !confirmed[session.ID] {
				delete(removed, session.ID)
			}
		}
		for _, session := range owner.admittedSessions {
			if !confirmed[session.ID] {
				delete(removed, session.ID)
			}
		}
	}
	for _, id := range ids {
		if removed[id] {
			r.App.ForgetSession(id.String())
			r.localRuntime().releaseDraftComposer(id.String())
		}
	}
}

func expectedProject(p *viewmodel.Project) (protocol.Project, error) {
	id, err := protocol.ParseProjectID(p.ID)
	return protocol.Project{ID: id, Name: p.Name, Path: p.Path, Appearance: protocol.ProjectsAppearance{Icon: p.Icon, Color: p.Color}, Owner: p.Owner, Visibility: p.Visibility}, err
}

// removeProject removes a project from its Node's inventory. The outcome is
// a toast: removed, or why it stayed; after a failure the confirmation shows
// again, as a retry is a new attempt.
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
	root := r.localRuntime()
	r.execute(protocol.Command{Kind: "remove_project", Data: protocol.CommandRemoveProjectPayload{Expected: expected}}, func(c *ui.Context, _ protocol.Output, err error) {
		if err == nil {
			kit.Toast(c, kit.Done, i18n.T(c, "project_removed"))
			if node, project, shown := root.App.ProjectOwner(); shown && node == p.Host && project == p.ID {
				root.App.Host = p.Host
				root.App.Navigate(shell.HostPage)
			}
			return
		}
		outcome, key := projectRemovalFailure(err)
		kit.Toast(c, outcome, i18n.T(c, key))
		root.App.Sidebar.AskRemove(c, p)
	})
}

// projectRemovalFailure names why a project stayed: an unknown outcome
// first, then what the Node says of its tasks and terminals, then the code.
func projectRemovalFailure(err error) (kit.Outcome, string) {
	code := faultCode(err)
	var removal *protocol.FaultDetailsProjectRemovalPayload
	details := false
	var fault *link.FaultError
	if errors.As(err, &fault) && fault.Fault.Details != nil {
		details = true
		if value, ok := detail[protocol.FaultDetailsProjectRemovalPayload](fault.Fault.Details.Data); ok {
			removal = &value
		}
	}
	switch {
	case code == protocol.ErrorCodeOutcomeUnknown || code == "" || code == protocol.ErrorCodeUnavailable && !details:
		return kit.Uncertain, "project_remove_unknown"
	case removal != nil && removal.TasksActive && removal.TerminalsMayBeClosed:
		return kit.Failed, "project_remove_tasks_partial"
	case removal != nil && removal.TasksActive:
		return kit.Failed, "project_remove_tasks"
	case removal != nil && removal.TerminalsMayBeClosed && code == protocol.ErrorCodeRevisionConflict:
		return kit.Conflict, "project_remove_changed_partial"
	case removal != nil && removal.TerminalsMayBeClosed:
		return kit.Failed, "project_remove_failed"
	case removal != nil:
		return kit.Failed, "live_request_failed"
	case code == protocol.ErrorCodeBusy:
		return kit.Failed, "project_remove_busy"
	case code == protocol.ErrorCodeRevisionConflict || code == protocol.ErrorCodeNotFound:
		return kit.Conflict, "project_changed"
	}
	return kit.Failed, "live_request_failed"
}

// saveProject creates or updates a project on its host with the editor's
// name, path and appearance; a clone goes into the destination folder under
// the project's name. The outcome is a toast, and the draft stays on failure.
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
	clone := e.Clone == 1 && e.Project == nil
	appearance := protocol.ProjectsAppearance{Icon: e.Icon, Color: e.Color}
	if appearance.Icon == "" || appearance.Color == "" {
		appearance = protocol.DefaultProjectsAppearance()
	}
	path := strings.TrimSpace(e.Path)
	source := protocol.ProjectsSource{Kind: "local"}
	if clone {
		var branch *string
		if strings.TrimSpace(e.Branch) != "" {
			value := strings.TrimSpace(e.Branch)
			branch = &value
		}
		path = sidebar.Destination(path, e.Name)
		source = protocol.ProjectsSource{Kind: "clone", Data: protocol.ProjectsSourceClonePayload{Url: strings.TrimSpace(e.Repo), Branch: branch}}
	}
	visibility := protocol.ProjectVisibility(e.Visibility)
	command := protocol.Command{Kind: "create_project", Data: protocol.ProjectsDraft{Name: e.Name, Path: path, Appearance: appearance, Source: source, Visibility: visibility}}
	if e.Project != nil {
		expected, err := expectedProject(e.Project)
		if err != nil {
			done(false)
			return
		}
		var updatedVisibility *protocol.ProjectVisibility
		if e.Visibility != "" {
			updatedVisibility = &visibility
		}
		command = protocol.Command{Kind: "update_project", Data: protocol.CommandUpdateProjectPayload{Expected: expected, Name: e.Name, Path: path, Appearance: appearance, Visibility: updatedVisibility}}
	}
	root := r.localRuntime()
	r.execute(command, func(c *ui.Context, output protocol.Output, err error) {
		if err != nil {
			outcome, key := kit.Failed, "live_request_failed"
			switch code := faultCode(err); {
			case code == protocol.ErrorCodeOutcomeUnknown || code == protocol.ErrorCodeUnavailable || code == "":
				outcome, key = kit.Uncertain, "project_outcome_unknown"
			case code == protocol.ErrorCodeRevisionConflict:
				outcome, key = kit.Conflict, "project_changed"
			case code == protocol.ErrorCodeBusy:
				key = "project_path_in_use"
			case clone && code == protocol.ErrorCodePermissionDenied:
				key = "git_auth_failed"
			case clone:
				key = "project_clone_failed"
			}
			kit.Toast(c, outcome, i18n.T(c, key))
			done(false)
			return
		}
		done(true)
		// The saved project opens once the Node's snapshot holds it.
		if saved, ok := detail[protocol.Project](output.Data); ok {
			root.savedProject = saved.ID.String()
		}
	})
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

// moveSession saves the manual order of a project's sessions with source
// before or after target. The Node checks the order it is given against
// its own, so the order comes from the snapshot, which also holds the
// sessions the sidebar does not list.
func (r *Runtime) moveSession(source, target *viewmodel.Conversation, after bool) {
	if owner := r.conversationRuntime(target, nil); owner == nil {
		r.Report(fileActionFault(protocol.ErrorCodeUnavailable, "the execution host is unavailable"))
		return
	} else if owner != r {
		owner.moveSession(source, target, after)
		return
	}
	command, ok := sessionOrderCommand(r.snapshot().Sessions, source.ID, target.ID, after)
	if !ok {
		return
	}
	r.execute(command, func(c *ui.Context, _ protocol.Output, err error) {
		if err != nil {
			r.reportKeys(c, err, "live_request_failed", "session_changed", "project_outcome_unknown")
			return
		}
		r.project()
	})
}

// sessionOrderCommand is the command moving source before or after target
// among their project's sessions, false when they share no project or
// nothing moves.
func sessionOrderCommand(sessions []protocol.Session, source, target string, after bool) (protocol.Command, bool) {
	var project *protocol.ProjectID
	for _, session := range sessions {
		if session.ID.String() == target {
			project = session.Project
		}
	}
	if project == nil {
		return protocol.Command{}, false
	}
	var expected []protocol.SessionID
	var ids []string
	for _, session := range sessions {
		if session.Project != nil && *session.Project == *project {
			expected, ids = append(expected, session.ID), append(ids, session.ID.String())
		}
	}
	moved, ok := sidebar.MovePin(ids, source, target, after)
	if !ok {
		return protocol.Command{}, false
	}
	order := make([]protocol.SessionID, len(moved))
	for i, id := range moved {
		order[i] = expected[slices.Index(ids, id)]
	}
	return protocol.Command{Kind: "set_session_order", Data: protocol.CommandSetSessionOrderPayload{Project: *project, Expected: expected, Sessions: order}}, true
}

// movePin saves the pinned order with source before or after target and
// shows it.
func (r *Runtime) movePin(source, target *viewmodel.Conversation, after bool) {
	if r.savePinOrder(source, target, after) {
		r.project()
	}
}

// savePinOrder saves the pinned order with source before or after target,
// reporting whether it changed. The pins are the sessions themselves, so each
// order is saved for its own host even where two hosts list a session of one
// ID.
func (r *Runtime) savePinOrder(source, target *viewmodel.Conversation, after bool) bool {
	var pins []*viewmodel.Conversation
	add := func(list []*viewmodel.Conversation) {
		for _, cv := range list {
			if cv.Pinned {
				pins = append(pins, cv)
			}
		}
	}
	add(r.App.Data.Unassigned)
	for _, p := range r.App.Data.Projects {
		add(p.Conversations)
	}
	pins = sidebar.OrderPinsFunc(pins, func(cv *viewmodel.Conversation) sidebar.Pin {
		return sidebar.Pin{ID: cv.ID, Order: cv.PinOrder, LastRun: cv.Updated, Created: cv.Created}
	})
	moved, ok := sidebar.MovePin(pins, source, target, after)
	if !ok {
		return false
	}
	for i, cv := range moved {
		order := uint64(i)
		if owner := r.conversationRuntime(cv, nil); owner != nil {
			r.Preferences.Data.SetPin(owner.controller.Target(), cv.ID, preferences.Pin{Pinned: true, Order: &order})
		}
	}
	return true
}

// OpenSession and OpenNotice are called by window and notification callbacks on
// the UI thread; asynchronous work still returns through the same queue.
func (r *Runtime) OpenSession(id string) {
	// The window's root projects the shared data, so it resolves the request.
	r = r.localRuntime()
	r.pendingSession, r.pendingSelected = id, r.App.Sidebar.Selected
	r.selectPendingSession()
}

// A session opened before it is projected is selected when it appears, unless
// a later navigation replaced the selection the request was made from.
func (r *Runtime) selectPendingSession() {
	if r.pendingSession != "" && r.App.Sidebar.Selected != r.pendingSelected {
		r.pendingSession = ""
	}
	if r.pendingSession == "" || r.App.Data.Find(r.pendingSession) == nil {
		return
	}
	r.App.OpenConversation(r.pendingSession)
	r.pendingSession = ""
}

// fallBackSelection keeps a pending OpenSession across a selection the runtime
// chose because the previous one was unavailable; it is not a navigation.
func (r *Runtime) fallBackSelection(before string) {
	r = r.localRuntime()
	if r.pendingSession != "" && r.pendingSelected == before {
		r.pendingSelected = r.App.Sidebar.Selected
	}
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
