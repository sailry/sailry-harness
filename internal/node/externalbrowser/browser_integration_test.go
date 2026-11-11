//go:build integration

package externalbrowser

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/shirou/gopsutil/v4/process"

	"github.com/sailry/sailry-harness/internal/protocol"
)

const integrationPage = `<!doctype html><html><head><title>Sailry fixture</title></head><body>
<h1 id="heading">Fixture heading</h1>
<input id="name"><button id="go" onclick="document.getElementById('out').textContent=document.getElementById('name').value">Go</button>
<p id="out"></p>
<select id="pick" onchange="document.getElementById('picked').textContent=this.value"><option value="a">Alpha</option><option value="b">Beta</option></select><p id="picked"></p>
<a id="other" href="/other">Other page</a>
<a id="popup" href="/other" target="_blank">Popup</a>
<a id="download" href="/file">Download</a>
<input id="up" type="file" onchange="document.getElementById('uploaded').textContent=this.files[0].name+':'+this.files[0].size"><p id="uploaded"></p>
<button id="send" onclick="var form=new FormData();form.append('file',document.getElementById('up').files[0]);var sent=document.getElementById('sent');sent.textContent='';fetch('/upload',{method:'POST',body:form}).then(function(r){return r.text()}).then(function(t){sent.textContent='received '+t},function(e){sent.textContent='failed '+e})">Send</button><p id="sent"></p>
<button id="ask" onclick="document.getElementById('confirmed').textContent=String(confirm('Proceed?'))">Ask</button><p id="confirmed"></p>
<div id="hover" onmouseover="this.textContent='hovered'">rest</div>
<input id="keys" onkeydown="document.getElementById('key').textContent=event.key"><p id="key"></p>
<div id="twice" ondblclick="this.textContent='twice'">once</div>
<div id="menu" oncontextmenu="this.textContent='menu';return false">none</div>
<input id="focus" onfocus="document.getElementById('focused').textContent='yes'"><p id="focused"></p>
<div id="from" onmousedown="window.dragging=true" style="width:40px;height:40px">from</div>
<div id="to" onmouseup="if(window.dragging)this.textContent='dropped'" style="width:40px;height:40px">to</div>
<iframe id="child" src="/frame"></iframe>
<iframe id="foreign" src="%s/frame"></iframe>
<div style="height:3000px"></div><p id="bottom">Bottom</p>
</body></html>`

// processesUsing counts the processes started with a user data directory
// below directory. It never signals them.
func processesUsing(t *testing.T, directory string) int {
	t.Helper()
	processes, err := process.Processes()
	if err != nil {
		t.Fatal(err)
	}
	count := 0
	for _, candidate := range processes {
		arguments, err := candidate.CmdlineSlice()
		if err != nil {
			continue
		}
		for _, argument := range arguments {
			if strings.HasPrefix(argument, "--user-data-dir="+directory) {
				count++
				break
			}
		}
	}
	return count
}

func TestManagedBrowserIntegration(t *testing.T) {
	if Installed() == "" {
		t.Skip("no Chrome, Chromium or Edge is installed on this host; install one to run this test")
	}
	// The other site is the same loopback listener family under another host
	// name, so Chrome isolates its frame in a separate process.
	foreign := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, _ *http.Request) {
		fmt.Fprint(writer, `<!doctype html><p id="inner">foreign frame</p>`)
	}))
	defer foreign.Close()
	foreignAddress := strings.Replace(foreign.URL, "127.0.0.1", "localhost", 1)
	release := make(chan struct{})
	mux := http.NewServeMux()
	mux.HandleFunc("/", func(writer http.ResponseWriter, _ *http.Request) {
		fmt.Fprintf(writer, integrationPage, foreignAddress)
	})
	mux.HandleFunc("/frame", func(writer http.ResponseWriter, _ *http.Request) {
		fmt.Fprint(writer, `<!doctype html><p id="inner">inside frame</p><input id="deep">`)
	})
	mux.HandleFunc("/other", func(writer http.ResponseWriter, _ *http.Request) {
		fmt.Fprint(writer, `<!doctype html><title>Other</title><p id="where">other page</p>`)
	})
	mux.HandleFunc("/file", func(writer http.ResponseWriter, _ *http.Request) {
		writer.Header().Set("Content-Disposition", `attachment; filename="report.txt"`)
		fmt.Fprint(writer, "downloaded report")
	})
	mux.HandleFunc("/upload", func(writer http.ResponseWriter, request *http.Request) {
		file, _, err := request.FormFile("file")
		if err != nil {
			http.Error(writer, err.Error(), http.StatusBadRequest)
			return
		}
		defer file.Close()
		_, _ = io.Copy(writer, file)
	})
	mux.HandleFunc("/slow", func(_ http.ResponseWriter, request *http.Request) {
		select {
		case <-request.Context().Done():
		case <-release:
		}
	})
	server := httptest.NewServer(mux)
	defer server.Close()
	defer close(release)

	profile, worktree := t.TempDir(), t.TempDir()
	// The temporary directory is reached through a link on macOS.
	for _, directory := range []*string{&profile, &worktree} {
		resolved, err := filepath.EvalSymlinks(*directory)
		if err != nil {
			t.Fatal(err)
		}
		*directory = resolved
	}
	if err := os.WriteFile(filepath.Join(worktree, "upload.txt"), []byte("hello"), 0o600); err != nil {
		t.Fatal(err)
	}
	outside := filepath.Join(filepath.Dir(worktree), "outside.txt")
	if err := os.WriteFile(outside, []byte("secret"), 0o600); err != nil {
		t.Fatal(err)
	}
	defer os.Remove(outside)
	node := protocol.NodeID{}
	browsers := New(Options{Profile: profile, Node: node})
	defer browsers.Shutdown()
	if !browsers.Available() {
		t.Fatal("a Node with a profile must own browser sessions")
	}
	conversation := protocol.NewSessionID()
	use := func(ctx context.Context, action protocol.ExternalBrowserAction, arguments map[string]any) (map[string]any, error) {
		encoded, err := json.Marshal(arguments)
		if err != nil {
			t.Fatal(err)
		}
		output, err := browsers.Execute(ctx, Call{Request: protocol.NewRequestID(), Session: conversation, Root: worktree, Action: action, Arguments: encoded})
		if err != nil {
			return nil, err
		}
		var value map[string]any
		if err := json.Unmarshal(output.Data.(json.RawMessage), &value); err != nil {
			t.Fatal(err)
		}
		return value, nil
	}
	do := func(action protocol.ExternalBrowserAction, arguments map[string]any) map[string]any {
		t.Helper()
		value, err := use(context.Background(), action, arguments)
		if err != nil {
			t.Fatalf("%s %v: %v", action, arguments, err)
		}
		result, _ := value["result"].(map[string]any)
		if result == nil {
			return value
		}
		result["#output"] = value
		return result
	}
	text := func(selector string) string {
		t.Helper()
		value, _ := do(protocol.ExternalBrowserActionExtractText, map[string]any{"selector": selector})["text"].(string)
		return value
	}
	expect := func(got any, want any, what string) {
		t.Helper()
		if fmt.Sprint(got) != fmt.Sprint(want) {
			t.Fatalf("%s = %v, want %v", what, got, want)
		}
	}

	// Navigation and the page context it returns.
	opened := do(protocol.ExternalBrowserActionNavigate, map[string]any{"url": server.URL + "/"})
	expect(opened["title"], "Sailry fixture", "navigate title")
	if page, _ := opened["page"].(map[string]any); !strings.Contains(fmt.Sprint(page["page_text"]), "Fixture heading") {
		t.Fatalf("navigate page context = %v", opened["page"])
	}
	if processesUsing(t, profile) == 0 {
		t.Fatal("the browser must run with a user data directory below the Node profile")
	}
	expect(len(browsers.Sessions()), 1, "running sessions")

	// Typing, clicking and reading.
	typed := do(protocol.ExternalBrowserActionType, map[string]any{"selector": "#name", "text": "Sailry"})
	expect(typed["field_value"], "Sailry", "typed field value")
	clicked := do(protocol.ExternalBrowserActionClick, map[string]any{"selector": "#go"})
	expect(clicked["clicked_element"], "button: Go", "clicked element")
	expect(text("#out"), "Sailry", "clicked output")
	do(protocol.ExternalBrowserActionClear, map[string]any{"selector": "#name"})
	expect(do(protocol.ExternalBrowserActionEvaluateJs, map[string]any{"script": "return document.getElementById('name').value"})["result"], "", "cleared value")
	expect(do(protocol.ExternalBrowserActionSelect, map[string]any{"selector": "#pick", "text": "Beta"})["selected_text"], "Beta", "selected text")
	expect(text("#picked"), "b", "selected value")
	do(protocol.ExternalBrowserActionHover, map[string]any{"selector": "#hover"})
	expect(text("#hover"), "hovered", "hover")
	do(protocol.ExternalBrowserActionPressKey, map[string]any{"key": "ArrowDown", "selector": "#keys"})
	expect(text("#key"), "ArrowDown", "pressed key")
	do(protocol.ExternalBrowserActionDoubleClick, map[string]any{"selector": "#twice"})
	expect(text("#twice"), "twice", "double click")
	do(protocol.ExternalBrowserActionRightClick, map[string]any{"selector": "#menu"})
	expect(text("#menu"), "menu", "right click")
	do(protocol.ExternalBrowserActionFocus, map[string]any{"selector": "#focus"})
	expect(text("#focused"), "yes", "focus")
	do(protocol.ExternalBrowserActionDragAndDrop, map[string]any{"source_selector": "#from", "target_selector": "#to"})
	expect(text("#to"), "dropped", "drag and drop")
	expect(do(protocol.ExternalBrowserActionWait, map[string]any{"seconds": 0.05})["waited_seconds"], 0.05, "wait")
	state := do(protocol.ExternalBrowserActionElementState, map[string]any{"selector": "#go"})
	expect(state["is_clickable"], true, "element state")
	if _, err := use(context.Background(), protocol.ExternalBrowserActionExtractText, map[string]any{"selector": "#absent"}); err == nil || !strings.Contains(err.Error(), "Element not found: #absent") {
		t.Fatalf("missing element fault = %v", err)
	}

	// Scripts keep the function-body contract, synchronous and with a callback.
	expect(do(protocol.ExternalBrowserActionEvaluateJs, map[string]any{"script": "return 1 + 2"})["result"], 3, "evaluate")
	expect(do(protocol.ExternalBrowserActionEvaluateJs, map[string]any{"script": "var done = arguments[arguments.length - 1]; setTimeout(function () { done(7) }, 10)", "async": true})["result"], 7, "async evaluate")

	// Extraction.
	links := do(protocol.ExternalBrowserActionExtractLinks, map[string]any{})
	expect(links["count"], 3, "link count")
	expect(do(protocol.ExternalBrowserActionExtractAttribute, map[string]any{"selector": "#other", "attribute": "href"})["value"], "/other", "attribute")
	markup := do(protocol.ExternalBrowserActionPageSource, map[string]any{"max_length": 64})
	expect(markup["truncated"], true, "page source truncation")
	expect(markup["returned_length"], 64, "page source length")
	expect(do(protocol.ExternalBrowserActionPageInfo, map[string]any{})["title"], "Sailry fixture", "page info")
	do(protocol.ExternalBrowserActionScroll, map[string]any{"direction": "bottom"})
	do(protocol.ExternalBrowserActionWaitForText, map[string]any{"text": "Bottom", "timeout": 5})
	do(protocol.ExternalBrowserActionWaitForElement, map[string]any{"selector": "#bottom", "timeout": 5, "visible": true})
	do(protocol.ExternalBrowserActionWaitForPageLoad, map[string]any{"timeout": 5})
	do(protocol.ExternalBrowserActionScroll, map[string]any{"direction": "top"})

	// Screenshots return bounded inline media and are saved only on request.
	shot := do(protocol.ExternalBrowserActionScreenshot, map[string]any{})
	expect(shot["mime_type"], "image/png", "screenshot type")
	expect(shot["saved_to_artifacts"], false, "screenshot saved")
	if _, leaked := shot["base64_image"]; leaked {
		t.Fatal("the capture must leave the bounded result")
	}
	media, _ := shot["#output"].(map[string]any)["inline_data"].([]any)
	if len(media) != 1 || len(fmt.Sprint(media[0].(map[string]any)["data"])) < 1000 {
		t.Fatalf("screenshot inline data = %v", len(media))
	}
	saved := do(protocol.ExternalBrowserActionScreenshot, map[string]any{"selector": "#heading", "save_to_artifacts": true})
	if path, _ := saved["path"].(string); !strings.HasPrefix(path, "assets/generated/browser-") {
		t.Fatalf("saved screenshot path = %v", saved["path"])
	} else if _, err := os.Stat(filepath.Join(worktree, filepath.FromSlash(path))); err != nil {
		t.Fatal(err)
	}
	printed := do(protocol.ExternalBrowserActionPrintToPdf, map[string]any{"landscape": true})
	document, err := os.ReadFile(filepath.Join(worktree, filepath.FromSlash(fmt.Sprint(printed["path"]))))
	if err != nil || !strings.HasPrefix(string(document), "%PDF") {
		t.Fatalf("printed document = %v, %v", printed["path"], err)
	}

	// Cookies of the current page.
	do(protocol.ExternalBrowserActionAddCookie, map[string]any{"name": "token", "value": "abc"})
	found := do(protocol.ExternalBrowserActionGetCookie, map[string]any{"name": "token"})
	expect(found["cookie"].(map[string]any)["value"], "abc", "cookie value")
	expect(do(protocol.ExternalBrowserActionGetCookies, map[string]any{})["count"], 1, "cookie count")
	do(protocol.ExternalBrowserActionAddCookie, map[string]any{"name": "second", "value": "2"})
	do(protocol.ExternalBrowserActionDeleteCookie, map[string]any{"name": "token"})
	expect(do(protocol.ExternalBrowserActionGetCookies, map[string]any{})["count"], 1, "cookie count after delete")
	do(protocol.ExternalBrowserActionDeleteAllCookies, map[string]any{})
	expect(do(protocol.ExternalBrowserActionGetCookies, map[string]any{})["count"], 0, "cookie count after clearing")

	// Frames: same site by selector, another site by index, and back.
	do(protocol.ExternalBrowserActionSwitchToFrame, map[string]any{"selector": "#child"})
	expect(text("#inner"), "inside frame", "frame text")
	do(protocol.ExternalBrowserActionType, map[string]any{"selector": "#deep", "text": "nested"})
	expect(do(protocol.ExternalBrowserActionEvaluateJs, map[string]any{"script": "return document.getElementById('deep').value"})["result"], "nested", "frame typing")
	if _, err := use(context.Background(), protocol.ExternalBrowserActionExtractText, map[string]any{"selector": "#out"}); err == nil {
		t.Fatal("a frame must not see its parent's elements")
	}
	do(protocol.ExternalBrowserActionSwitchToParentFrame, map[string]any{})
	expect(text("#out"), "Sailry", "parent frame text")
	do(protocol.ExternalBrowserActionSwitchToFrame, map[string]any{"index": 1})
	expect(text("#inner"), "foreign frame", "cross-site frame text")
	do(protocol.ExternalBrowserActionSwitchToDefaultContent, map[string]any{})
	expect(text("#heading"), "Fixture heading", "default content text")

	// Dialogs are answered by the session's choice.
	do(protocol.ExternalBrowserActionHandleAlert, map[string]any{"action": "accept"})
	do(protocol.ExternalBrowserActionClick, map[string]any{"selector": "#ask"})
	expect(text("#confirmed"), "true", "accepted dialog")
	expect(do(protocol.ExternalBrowserActionHandleAlert, map[string]any{"action": "dismiss"})["had_active_alert"], true, "dialog seen")
	do(protocol.ExternalBrowserActionClick, map[string]any{"selector": "#ask"})
	expect(text("#confirmed"), "false", "dismissed dialog")

	// Uploads come from the worktree only.
	uploaded := do(protocol.ExternalBrowserActionFileUpload, map[string]any{"selector": "#up", "file_path": "upload.txt"})
	expect(uploaded["uploaded_file"], "upload.txt", "uploaded path")
	expect(text("#uploaded"), "upload.txt:5", "uploaded file")
	for _, escaping := range []string{"../outside.txt", outside} {
		if _, err := use(context.Background(), protocol.ExternalBrowserActionFileUpload, map[string]any{"selector": "#up", "file_path": escaping}); err == nil {
			t.Fatalf("upload of %q must be refused", escaping)
		}
	}
	expect(text("#uploaded"), "upload.txt:5", "upload after refusals")
	// The page reads the chosen file when it sends it, which is a later
	// call than the one that chose it.
	sent := func() string {
		t.Helper()
		do(protocol.ExternalBrowserActionClick, map[string]any{"selector": "#send"})
		for deadline := time.Now().Add(10 * time.Second); time.Now().Before(deadline); time.Sleep(50 * time.Millisecond) {
			if answer := text("#sent"); answer != "" {
				return answer
			}
		}
		t.Fatal("the page did not finish sending its file")
		return ""
	}
	expect(sent(), "received hello", "file sent after the upload call returned")
	// Without the staged copy the same send fails: the browser holds a
	// path, not the bytes. This is why the copy outlives its call.
	browsers.mu.Lock()
	staged := slices.Clone(browsers.sessions[conversation].session.uploads)
	browsers.mu.Unlock()
	expect(len(staged), 1, "staged uploads")
	if err := os.RemoveAll(staged[0]); err != nil {
		t.Fatal(err)
	}
	if answer := sent(); !strings.HasPrefix(answer, "failed ") {
		t.Errorf("a file whose staged copy is gone was sent: %q", answer)
	}

	// A wait that runs out is an ordinary answer and the browser stays,
	// however long the model asked to wait.
	began := time.Now()
	_, err = use(context.Background(), protocol.ExternalBrowserActionWaitForElement, map[string]any{"selector": "#absent", "timeout": 60})
	var absent *protocol.Fault
	if !errors.As(err, &absent) || absent.Code != protocol.ErrorCodeUnavailable || absent.Message != "Element not found after 30s: #absent" {
		t.Fatalf("a wait for an absent element = %v", err)
	}
	if spent := time.Since(began); spent < waitLimit-time.Second || spent > operationTimeout-5*time.Second {
		t.Fatalf("a wait of 60 seconds took %v", spent)
	}
	if _, err := use(context.Background(), protocol.ExternalBrowserActionWaitForText, map[string]any{"text": "never shown", "timeout": 1}); err == nil || !strings.Contains(err.Error(), "Text 'never shown' not found after 1s") {
		t.Fatalf("a wait for absent text = %v", err)
	}
	if _, err := use(context.Background(), protocol.ExternalBrowserActionClick, map[string]any{"selector": "#absent", "wait_timeout": 1}); err == nil || !strings.Contains(err.Error(), "Element not found: #absent") {
		t.Fatalf("a click on an absent element = %v", err)
	}
	expect(text("#uploaded"), "upload.txt:5", "page state after waits that ran out")
	expect(len(browsers.Sessions()), 1, "sessions after waits that ran out")

	// Downloads stay in the session's temporary directory until saved.
	do(protocol.ExternalBrowserActionClick, map[string]any{"selector": "#download"})
	deadline := time.Now().Add(10 * time.Second)
	for {
		listing := do(protocol.ExternalBrowserActionDownloads, map[string]any{})
		if files, _ := listing["files"].([]any); len(files) == 1 {
			expect(files[0].(map[string]any)["name"], "report.txt", "download name")
			break
		}
		if time.Now().After(deadline) {
			t.Fatalf("download did not complete: %v", listing)
		}
		time.Sleep(100 * time.Millisecond)
	}
	if entries, _ := os.ReadDir(worktree); len(entries) != 2 {
		t.Fatalf("a download must not reach the worktree by itself: %v", entries)
	}
	for _, name := range []string{"../report.txt", "report.txt.crdownload", "missing.txt", ""} {
		if _, err := use(context.Background(), protocol.ExternalBrowserActionSaveDownload, map[string]any{"name": name}); err == nil {
			t.Fatalf("saving download %q must be refused", name)
		}
	}
	kept := do(protocol.ExternalBrowserActionSaveDownload, map[string]any{"name": "report.txt"})
	content, err := os.ReadFile(filepath.Join(worktree, filepath.FromSlash(fmt.Sprint(kept["path"]))))
	if err != nil || string(content) != "downloaded report" || !strings.HasPrefix(fmt.Sprint(kept["path"]), "assets/generated/browser-") {
		t.Fatalf("saved download = %v %q %v", kept["path"], content, err)
	}

	// Windows: a new tab becomes current, a popup can be adopted.
	first := do(protocol.ExternalBrowserActionListWindows, map[string]any{})
	expect(first["count"], 1, "window count")
	tab := do(protocol.ExternalBrowserActionNewTab, map[string]any{"url": server.URL + "/other"})
	expect(text("#where"), "other page", "new tab text")
	listed := do(protocol.ExternalBrowserActionListWindows, map[string]any{})
	expect(listed["count"], 2, "window count with a tab")
	expect(listed["current_window"], tab["window_handle"], "current window")
	back := do(protocol.ExternalBrowserActionSwitchWindow, map[string]any{"handle": first["current_window"]})
	expect(back["title"], "Sailry fixture", "switched window title")
	do(protocol.ExternalBrowserActionSwitchWindow, map[string]any{"handle": tab["window_handle"]})
	do(protocol.ExternalBrowserActionCloseWindow, map[string]any{})
	expect(do(protocol.ExternalBrowserActionListWindows, map[string]any{})["count"], 1, "window count after close")
	expect(text("#heading"), "Fixture heading", "window after close")
	do(protocol.ExternalBrowserActionClick, map[string]any{"selector": "#popup"})
	var popup string
	for deadline := time.Now().Add(5 * time.Second); popup == "" && time.Now().Before(deadline); time.Sleep(50 * time.Millisecond) {
		for _, handle := range do(protocol.ExternalBrowserActionListWindows, map[string]any{})["windows"].([]any) {
			if handle != first["current_window"] {
				popup = handle.(string)
			}
		}
	}
	expect(do(protocol.ExternalBrowserActionSwitchWindow, map[string]any{"handle": popup})["title"], "Other", "popup title")
	do(protocol.ExternalBrowserActionCloseWindow, map[string]any{})
	do(protocol.ExternalBrowserActionSetWindowSize, map[string]any{"width": 1000, "height": 700})
	do(protocol.ExternalBrowserActionMaximizeWindow, map[string]any{})
	do(protocol.ExternalBrowserActionMinimizeWindow, map[string]any{})
	separate := do(protocol.ExternalBrowserActionNewWindow, map[string]any{"url": server.URL + "/other"})
	expect(text("#where"), "other page", "new window text")
	expect(do(protocol.ExternalBrowserActionListWindows, map[string]any{})["current_window"], separate["window_handle"], "new window is current")
	do(protocol.ExternalBrowserActionCloseWindow, map[string]any{})
	do(protocol.ExternalBrowserActionNavigate, map[string]any{"url": server.URL + "/other"})
	do(protocol.ExternalBrowserActionBack, map[string]any{})
	expect(text("#heading"), "Fixture heading", "back")
	do(protocol.ExternalBrowserActionForward, map[string]any{})
	expect(text("#where"), "other page", "forward")
	do(protocol.ExternalBrowserActionBack, map[string]any{})
	do(protocol.ExternalBrowserActionRefresh, map[string]any{})

	// Argument bounds are enforced before the browser is used.
	for _, action := range []protocol.ExternalBrowserAction{protocol.ExternalBrowserActionNavigate, protocol.ExternalBrowserActionNewTab, protocol.ExternalBrowserActionNewWindow} {
		if _, err := use(context.Background(), action, map[string]any{"url": "file:///etc/passwd"}); err == nil {
			t.Fatalf("%s to a file URL must be refused", action)
		}
	}
	// A page cannot reach local files either: the browser refuses both a
	// read and a navigation that a web page starts.
	expect(do(protocol.ExternalBrowserActionEvaluateJs, map[string]any{"async": true,
		"script": "var done = arguments[arguments.length - 1]; fetch('file:///etc/hosts').then(function () { done('read') }, function () { done('refused') })"})["result"], "refused", "page read of a local file")
	do(protocol.ExternalBrowserActionEvaluateJs, map[string]any{"script": "window.location.href = 'file:///etc/hosts'; return 1"})
	do(protocol.ExternalBrowserActionWait, map[string]any{"seconds": 0.5})
	if address := fmt.Sprint(do(protocol.ExternalBrowserActionPageInfo, map[string]any{})["url"]); !strings.HasPrefix(address, server.URL) {
		t.Fatalf("a page navigated itself to %s", address)
	}

	// Stopping a turn interrupts the call and closes the session's browser.
	interrupted, cancel := context.WithTimeout(context.Background(), 500*time.Millisecond)
	_, err = use(interrupted, protocol.ExternalBrowserActionNavigate, map[string]any{"url": server.URL + "/slow"})
	cancel()
	var stopped *protocol.Fault
	if !errors.As(err, &stopped) || stopped.Code != protocol.ErrorCodeCancelled || !strings.Contains(stopped.Message, "browser session closed") {
		t.Fatalf("interrupted read = %v", err)
	}
	if running, sessions := processesUsing(t, profile), browsers.Sessions(); running != 0 || len(sessions) != 0 {
		t.Fatalf("after an interrupted call %d processes and %d sessions remain", running, len(sessions))
	}
	if entries, _ := filepath.Glob(filepath.Join(profile, "tmp", "browser-*")); len(entries) != 0 {
		t.Fatalf("session temporary files remain: %v", entries)
	}

	// The next call starts a fresh browser; an interrupted control action
	// reports that its effects are unknown.
	do(protocol.ExternalBrowserActionNavigate, map[string]any{"url": server.URL + "/other"})
	interrupted, cancel = context.WithTimeout(context.Background(), 300*time.Millisecond)
	_, err = use(interrupted, protocol.ExternalBrowserActionEvaluateJs, map[string]any{"script": "", "async": true})
	cancel()
	if !errors.As(err, &stopped) || stopped.Code != protocol.ErrorCodeOutcomeUnknown {
		t.Fatalf("interrupted control = %v", err)
	}
	if running := processesUsing(t, profile); running != 0 {
		t.Fatalf("after an interrupted control action %d processes remain", running)
	}

	// At the session limit the browser idle longest gives up its place,
	// and its conversation keeps its profile: a cookie set before is there
	// when the conversation's next call starts a browser again.
	do(protocol.ExternalBrowserActionNavigate, map[string]any{"url": server.URL + "/other"})
	do(protocol.ExternalBrowserActionAddCookie, map[string]any{"name": "kept", "value": "yes", "expiry": float64(time.Now().Add(time.Hour).Unix())})
	others := []protocol.SessionID{protocol.NewSessionID(), protocol.NewSessionID(), protocol.NewSessionID(), protocol.NewSessionID()}
	for _, other := range others {
		if _, err := browsers.Execute(context.Background(), Call{Request: protocol.NewRequestID(), Session: other, Root: worktree, Action: protocol.ExternalBrowserActionPageInfo, Arguments: json.RawMessage(`{}`)}); err != nil {
			t.Fatal(err)
		}
	}
	directory := filepath.Join(profile, "browser", "sessions", conversation.String())
	if running := browsers.Sessions(); len(running) != maxSessions || slices.Contains(running, conversation) {
		t.Fatalf("sessions at the limit = %v, want the four later ones", running)
	}
	if running := processesUsing(t, directory); running != 0 {
		t.Fatalf("the browser that gave up its place keeps %d processes", running)
	}
	resumed, err := use(context.Background(), protocol.ExternalBrowserActionNavigate, map[string]any{"url": server.URL + "/other"})
	if err != nil || !strings.Contains(fmt.Sprint(resumed["notice"]), "a new browser was started") {
		t.Fatalf("resumed conversation = %v, %v", resumed["notice"], err)
	}
	expect(do(protocol.ExternalBrowserActionGetCookie, map[string]any{"name": "kept"})["cookie"].(map[string]any)["value"], "yes", "cookie kept across browsers")
	if running := browsers.Sessions(); len(running) != maxSessions || slices.Contains(running, others[0]) {
		t.Fatalf("sessions after the resumed conversation = %v", running)
	}

	// An explicit close keeps the profile, a removed conversation loses it,
	// and Node shutdown closes the rest.
	closed, err := use(context.Background(), protocol.ExternalBrowserActionCloseSession, map[string]any{})
	if err != nil || closed["closed"] != true {
		t.Fatalf("close session = %v, %v", closed, err)
	}
	expect(len(browsers.Sessions()), 3, "sessions after close")
	if running := processesUsing(t, directory); running != 0 {
		t.Fatalf("closed session keeps %d processes", running)
	}
	if _, err := os.Stat(filepath.Join(directory, "profile")); err != nil {
		t.Fatalf("a closed session lost its profile: %v", err)
	}
	removed := filepath.Join(profile, "browser", "sessions", others[1].String())
	browsers.Remove(conversation)
	browsers.Remove(others[1])
	for _, gone := range []string{directory, removed} {
		if _, err := os.Stat(gone); !errors.Is(err, os.ErrNotExist) {
			t.Fatalf("the profile of a removed conversation remains at %s: %v", gone, err)
		}
	}
	if running := processesUsing(t, removed); running != 0 || len(browsers.Sessions()) != 2 {
		t.Fatalf("a removed conversation keeps %d processes; sessions %v", running, browsers.Sessions())
	}
	browsers.Shutdown()
	if running := processesUsing(t, profile); running != 0 {
		t.Fatalf("after Node shutdown %d browser processes remain", running)
	}
	if _, err := use(context.Background(), protocol.ExternalBrowserActionPageInfo, map[string]any{}); err == nil {
		t.Fatal("a stopped Node must refuse browser calls")
	}

	// A Node that died leaves a browser behind; the next owner of the profile
	// reclaims it on start.
	abandoned := New(Options{Profile: profile, Node: node})
	if _, err := abandoned.Execute(context.Background(), Call{Request: protocol.NewRequestID(), Session: conversation, Root: worktree, Action: protocol.ExternalBrowserActionPageInfo, Arguments: json.RawMessage(`{}`)}); err != nil {
		t.Fatal(err)
	}
	if processesUsing(t, profile) == 0 {
		t.Fatal("the abandoned browser must be running")
	}
	recovered := New(Options{Profile: profile, Node: node})
	for deadline := time.Now().Add(5 * time.Second); processesUsing(t, profile) != 0 && time.Now().Before(deadline); time.Sleep(50 * time.Millisecond) {
	}
	if running := processesUsing(t, profile); running != 0 {
		t.Fatalf("recovery left %d browser processes", running)
	}
	recovered.Shutdown()
	abandoned.Shutdown()
}
