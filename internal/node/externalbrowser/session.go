package externalbrowser

import (
	"context"
	"os"
	"path/filepath"
	"sync"
	"time"

	"github.com/chromedp/cdproto/browser"
	"github.com/chromedp/cdproto/page"
	"github.com/chromedp/cdproto/target"
	"github.com/chromedp/chromedp"
	"github.com/jonboulle/clockwork"
	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const (
	viewportWidth  = 1280
	viewportHeight = 900
	pageTimeout    = 25 * time.Second
	scriptTimeout  = 15 * time.Second
	// stagedUploads is how many uploaded files a browser keeps for its pages.
	stagedUploads = 16
)

// session is one conversation session's browser process tree.
type session struct {
	ctx       context.Context // the browser; done when its process exits
	cancel    context.CancelFunc
	profile   string // user data directory, kept for the conversation
	temporary string // uploads and downloads, removed with the browser
	group     *processGroup
	closing   sync.Once
	clock     clockwork.Clock
	// resumed says the conversation already had a profile when this browser
	// started, so it may have had pages open before.
	resumed bool
	// uploads are the staged upload directories, oldest first. A page reads
	// a selected file when it submits or sends it, which is a later call, so
	// the copies stay until the browser closes. Operations of a session run
	// one at a time, so nothing else guards the list.
	uploads []string

	windows map[target.ID]*window
	current *window

	mu     sync.Mutex
	dialog dialogs
}

// window is one page target. Its frames lead from the page to the frame
// that selectors and scripts currently address.
type window struct {
	id     target.ID
	ctx    context.Context
	cancel context.CancelFunc
	frames []frame
}

type frame struct {
	// ctx is the target that renders the frame's document. A cross-site
	// frame is its own target; cancel detaches from it.
	ctx    context.Context
	cancel context.CancelFunc
	// owner is the frame element inside ctx. It is nil at a target's root.
	owner *chromedp.Node
}

// dialogs is how JavaScript dialogs are answered. A dialog blocks its page,
// so each one is answered as it opens.
type dialogs struct {
	accept bool
	text   string
	seen   bool // a dialog opened since the policy was last set
}

func (browsers *Browsers) start(ctx context.Context, id protocol.SessionID, executable string) (*session, error) {
	failed := func(err error) (*session, error) {
		return nil, unavailable("managed browser startup failed: " + err.Error())
	}
	profile := filepath.Join(browsers.sessionsRoot(), id.String(), "profile")
	_, absent := os.Stat(profile)
	if err := core.PrivateDirectory(profile); err != nil {
		return failed(err)
	}
	shared, err := core.Temporary(browsers.options.Profile)
	if err != nil {
		return nil, err
	}
	temporary, err := os.MkdirTemp(shared, "browser-"+id.String()+"-")
	if err != nil {
		return failed(err)
	}
	downloads := filepath.Join(temporary, "downloads")
	if err := os.Mkdir(downloads, 0o700); err != nil {
		_ = os.RemoveAll(temporary)
		return failed(err)
	}
	options := append(chromedp.DefaultExecAllocatorOptions[:],
		chromedp.ExecPath(executable),
		chromedp.UserDataDir(profile),
		chromedp.WindowSize(viewportWidth, viewportHeight),
		chromedp.ModifyCmdFunc(configureProcess),
	)
	// The browser lives as long as the Node, never as long as one call.
	allocator, cancel := chromedp.NewExecAllocator(browsers.processes, options...)
	root, _ := chromedp.NewContext(allocator)
	started := &session{ctx: root, cancel: cancel, profile: profile, temporary: temporary, windows: map[target.ID]*window{},
		clock: browsers.clock, resumed: absent == nil}
	ready := make(chan error, 1)
	go func() { ready <- chromedp.Do(root) }()
	select {
	case err = <-ready:
	case <-ctx.Done():
		started.close(false)
		<-ready
		return nil, fault(protocol.ErrorCodeCancelled, "external browser startup cancelled")
	case <-browsers.ctx.Done():
		started.close(false)
		<-ready
		return nil, unavailable("execution Node is shutting down")
	}
	if err == nil {
		started.group, err = retainProcess(chromedp.FromContext(root).Browser.Process())
	}
	if err == nil {
		_, err = chromedp.CallBrowser(root, browser.SetDownloadBehavior, browser.SetDownloadBehaviorParams{
			Behavior: browser.SetDownloadBehaviorBehaviorAllow, DownloadPath: downloads,
		})
	}
	if err == nil {
		first := &window{id: chromedp.FromContext(root).Target.TargetID, ctx: root, cancel: func() {}}
		err = started.adopt(first)
	}
	if err != nil {
		started.close(false)
		return failed(err)
	}
	return started, nil
}

// close stops the browser and reclaims everything but its profile. It
// returns once no process of the tree is left.
func (session *session) close(graceful bool) {
	session.closing.Do(func() {
		if graceful && session.ctx.Err() == nil {
			// A graceful exit lets the browser flush its profile.
			limited, cancel := context.WithTimeout(session.ctx, 5*time.Second)
			_ = chromedp.Cancel(limited)
			cancel()
		}
		session.cancel()
		if session.group != nil {
			session.group.kill()
		}
		if allocated := chromedp.FromContext(session.ctx); allocated != nil && allocated.Allocator != nil {
			allocated.Allocator.Wait()
		}
		// Helper processes can outlive the main process for a moment.
		deadline := time.Now().Add(3 * time.Second)
		for killProcessesUsing(session.profile) > 0 && time.Now().Before(deadline) {
			time.Sleep(50 * time.Millisecond)
		}
		_ = os.RemoveAll(session.temporary)
	})
}

// adopt registers a page target, makes it current and answers its dialogs.
func (session *session) adopt(opened *window) error {
	err := chromedp.Listen(opened.ctx, func(event any) {
		dialog, ok := event.(*page.EventJavascriptDialogOpening)
		if !ok {
			return
		}
		session.mu.Lock()
		session.dialog.seen = true
		answer := page.HandleJavaScriptDialogParams{Accept: session.dialog.accept}
		if dialog.Type == page.DialogTypePrompt && answer.Accept {
			answer.PromptText = session.dialog.text
		}
		session.mu.Unlock()
		// The listener runs on the reader of this target and must not wait.
		go func() { _, _ = chromedp.Call(opened.ctx, page.HandleJavaScriptDialog, answer) }()
	})
	if err != nil {
		return err
	}
	session.windows[opened.id] = opened
	session.current = opened
	return nil
}

// pages lists the page targets of the browser, including the ones a page
// opened itself.
func (session *session) pages(ctx context.Context) ([]*target.Info, error) {
	infos, err := chromedp.Targets(ctx)
	if err != nil {
		return nil, err
	}
	var listed []*target.Info
	for _, info := range infos {
		if info.Type == "page" {
			listed = append(listed, info)
		}
	}
	return listed, nil
}

// open creates a page target and switches to it.
func (session *session) open(separate bool) (*window, error) {
	ctx, cancel := chromedp.NewContext(session.ctx, chromedp.WithNewWindow(separate))
	if err := chromedp.Do(ctx); err != nil {
		cancel()
		return nil, err
	}
	opened := &window{id: chromedp.FromContext(ctx).Target.TargetID, ctx: ctx, cancel: cancel}
	if err := session.adopt(opened); err != nil {
		cancel()
		return nil, err
	}
	return opened, nil
}

// switchTo makes a page target current by handle, attaching to it when a
// page opened it.
func (session *session) switchTo(ctx context.Context, handle string) error {
	if known := session.windows[target.ID(handle)]; known != nil && known.ctx.Err() == nil {
		session.current = known
		return nil
	}
	infos, err := session.pages(ctx)
	if err != nil {
		return err
	}
	for _, info := range infos {
		if string(info.TargetID) != handle {
			continue
		}
		attached, cancel := chromedp.NewContext(session.ctx, chromedp.WithTargetID(info.TargetID))
		if err := chromedp.Do(attached); err != nil {
			cancel()
			return err
		}
		return session.adopt(&window{id: info.TargetID, ctx: attached, cancel: cancel})
	}
	return unavailable("Failed to switch window: no such window")
}

// scope is where selectors and scripts apply: the current frame's target and
// its owner element, or the page itself.
func (session *session) scope() (context.Context, *chromedp.Node, error) {
	current := session.current
	if current == nil || current.ctx.Err() != nil {
		return nil, nil, unavailable("no browser window is open; open a new tab")
	}
	if len(current.frames) == 0 {
		return current.ctx, nil, nil
	}
	innermost := current.frames[len(current.frames)-1]
	stale := innermost.ctx.Err() != nil
	if innermost.owner != nil {
		select {
		case <-innermost.owner.Invalidated:
			stale = true
		default:
		}
	}
	if stale {
		current.leave(0)
		return nil, nil, unavailable("the selected frame is gone; switched to default content")
	}
	return innermost.ctx, innermost.owner, nil
}

// leave drops the frames above depth.
func (window *window) leave(depth int) {
	for index := len(window.frames) - 1; index >= depth; index-- {
		if window.frames[index].cancel != nil {
			window.frames[index].cancel()
		}
	}
	window.frames = window.frames[:depth]
}
