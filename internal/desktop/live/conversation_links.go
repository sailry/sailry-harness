package live

import (
	"net/url"
	"path"
	"path/filepath"
	"strconv"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Links and file paths of a conversation's content: one normalization and
// routing decision. A session link opens that session, a web link opens in a
// browser tab of the right panel (in the system browser with the platform
// modifier), mail and telephone links go to the system, and the rest are
// knowledge sources or files resolved against the session's worktree: in the
// right panel, or with the modifier in the file's own app on this device.

// normalizeLink recovers the forms Markdown leaves without a usable scheme:
// a bare www host, and file:relative, which as a URL would read as absolute.
func normalizeLink(link string) string {
	link = strings.TrimSpace(link)
	if len(link) >= 4 && strings.EqualFold(link[:4], "www.") {
		return "https://" + link
	}
	if len(link) > 5 && strings.EqualFold(link[:5], "file:") && link[5] != '/' && link[5] != '\\' {
		return link[5:]
	}
	return link
}

// fileTarget is where a file link points: a path inside the worktree with an
// optional line (0 for none), or an absolute path outside it.
type fileTarget struct {
	workspace bool
	path      string
	line      int
}

func decimal(value string) bool {
	if value == "" {
		return false
	}
	for _, b := range []byte(value) {
		if b < '0' || b > '9' {
			return false
		}
	}
	return true
}

// linkParts splits a file link into its path and line: #L12, #L12-L20 and
// #L12C4 name a line, as path:line and path:line:column do; #Heading does
// not, and a query is dropped.
func linkParts(link string) (string, int, bool) {
	target, fragment, _ := strings.Cut(link, "#")
	line := 0
	if value, ok := strings.CutPrefix(fragment, "L"); ok && value != "" && (value[0] == '-' || value[0] >= '0' && value[0] <= '9') {
		end := 0
		for end < len(value) && value[end] >= '0' && value[end] <= '9' {
			end++
		}
		if rest := value[end:]; rest != "" && rest[0] != '-' && rest[0] != 'C' {
			return "", 0, false
		}
		n, err := strconv.Atoi(value[:end])
		if err != nil || n < 1 {
			return "", 0, false
		}
		line = n
	}
	if index := strings.IndexByte(target, '?'); index >= 0 {
		target = target[:index]
	}
	if index := strings.LastIndexByte(target, ':'); index >= 0 && decimal(target[index+1:]) {
		if line != 0 {
			return "", 0, false
		}
		file, suffix := target[:index], target[index+1:]
		if inner := strings.LastIndexByte(file, ':'); inner >= 0 && decimal(file[inner+1:]) {
			file, suffix = file[:inner], file[inner+1:]
		}
		n, err := strconv.Atoi(suffix)
		if err != nil || n < 1 {
			return "", 0, false
		}
		target, line = file, n
	}
	if target == "" || strings.ContainsRune(target, 0) {
		return "", 0, false
	}
	return target, line, true
}

// resolveFileLink resolves a Markdown path or file URL against root, the
// worktree's directory on its Node.
func resolveFileLink(link, root string) (fileTarget, bool) {
	target, line, ok := linkParts(link)
	if !ok {
		return fileTarget{}, false
	}
	root = path.Clean("/" + strings.Trim(strings.ReplaceAll(root, "\\", "/"), "/"))
	target = strings.ReplaceAll(target, "\\", "/")
	drive := func(value string) bool {
		return len(value) >= 2 && value[1] == ':' && (value[0]|0x20 >= 'a' && value[0]|0x20 <= 'z')
	}
	if !drive(target) {
		if parsed, err := url.Parse(target); err == nil && parsed.Scheme != "" {
			if !strings.EqualFold(parsed.Scheme, "file") || parsed.Host != "" {
				return fileTarget{}, false
			}
			target = parsed.Path
		} else if decoded, err := url.PathUnescape(target); err == nil {
			target = decoded
		}
	}
	if strings.ContainsRune(target, 0) {
		return fileTarget{}, false
	}
	absolute := target
	switch {
	case drive(target):
		absolute = "/" + target
	case !strings.HasPrefix(target, "/"):
		absolute = root + "/" + target
	}
	absolute = path.Clean(absolute)
	if absolute == root {
		return fileTarget{workspace: true, line: line}, true
	}
	if relative, inside := strings.CutPrefix(absolute, strings.TrimSuffix(root, "/")+"/"); inside {
		return fileTarget{workspace: true, path: relative, line: line}, true
	}
	return fileTarget{path: absolute, line: line}, true
}

// openChatLink opens a link of a reply: every link the application routes
// itself; false leaves a heading anchor to the transcript.
func (r *Runtime) openChatLink(c *ui.Context, session, link string) bool {
	if strings.HasPrefix(strings.TrimSpace(link), "#") {
		return false
	}
	link = normalizeLink(link)
	external := conversation.External(c)
	owner := r
	cv := r.App.Data.Find(session)
	if cv != nil {
		if bound := r.conversationRuntime(cv, nil); bound != nil {
			owner = bound
		}
	}
	if target := client.SessionLink(link); target != nil {
		id := target.String()
		switch {
		case r.App.Data.Find(id) != nil:
			r.OpenSession(id)
		case cv != nil:
			owner.openLiveChild(cv, id)
		default:
			kit.Toast(c, kit.Failed, i18n.T(c, "unavailable"))
		}
		return true
	}
	if parsed, err := url.Parse(link); err == nil {
		switch strings.ToLower(parsed.Scheme) {
		case "http", "https":
			// An address without a host or with credentials opens nowhere.
			if !browserAddress(link) {
				return true
			}
			if external || r.browser == nil || !r.browser.available {
				c.OpenURL(link)
			} else {
				r.browserNavigate(r.App.Panel.AddBrowser(true).ID, link)
				r.App.PanelOpen = true
			}
			return true
		case "mailto", "tel":
			c.OpenURL(link)
			return true
		case "sailry-knowledge":
			r.App.OpenKnowledge(c, session, link)
			return true
		}
	}
	owner.openFileLink(c, session, link, external, cv != nil)
	return true
}

// openFileLink opens a path of a reply resolved against its session's
// worktree: a folder in the file manager, a file in the right panel at its
// line, or, with the platform modifier on this device, in its own app.
func (r *Runtime) openFileLink(c *ui.Context, session, link string, external, listed bool) {
	failed := func(key string) { kit.Toast(c, kit.Failed, i18n.T(c, key)) }
	current, ok := r.session(session)
	if !ok {
		failed("files_open_failed")
		return
	}
	var worktree protocol.Worktree
	for _, candidate := range r.snapshot().Worktrees {
		if candidate.ID == current.Worktree {
			worktree = candidate
		}
	}
	target, ok := resolveFileLink(link, worktree.Path)
	if !ok || worktree.Path == "" {
		failed("files_open_failed")
		return
	}
	local := r.attachments != nil && r.attachments.native.LocalNode == r.controller.Target()
	switch {
	case target.workspace && (target.path == "" || strings.HasSuffix(link, "/")):
		if !local || r.attachments.native.RevealLocal == nil {
			failed("files_remote_path")
			return
		}
		r.attachments.native.RevealLocal(filepath.Join(worktree.Path, filepath.FromSlash(target.path)))
	case target.workspace && local && external:
		r.OpenWorkspaceExternal(worktree.ID, target.path, nil)
	case target.workspace:
		if !validWorkspaceRelative(target.path) {
			failed("files_open_failed")
			return
		}
		// A listed session shows its own workspace; a child panel's link
		// opens in the workspace already shown.
		if listed {
			r.App.Sidebar.Selected = session
			r.workspaceFrame(nil)
		}
		var project *workspaceProject
		if r.workspace != nil {
			project = r.workspace.projects[worktree.ID]
		}
		if project == nil {
			failed("files_open_failed")
			return
		}
		if !project.openFileKind(target.path) {
			project.openFileAt(target.path, target.line)
		}
	case local && r.attachments.native.OpenLocal != nil:
		// resolveFileLink keeps a Windows drive path as "/C:/…".
		localPath := target.path
		if filepath.VolumeName(localPath[1:]) != "" {
			localPath = localPath[1:]
		}
		r.attachments.native.OpenLocal(filepath.FromSlash(localPath))
	default:
		failed("files_remote_path")
	}
}
