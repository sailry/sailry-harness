package live

import (
	"net/url"
	"path"
	"strconv"
	"strings"
	"sync/atomic"
	"unicode"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func normalizeLink(link string) string {
	link = strings.TrimSpace(link)
	if len(link) >= 4 && strings.EqualFold(link[:4], "www.") {
		return "https://" + link
	}
	return link
}

// fileTarget is where a file link points: a path inside the worktree with an
// optional line (0 for none), or an absolute path outside it.
type fileTarget struct {
	workspace bool
	path      string
	line      int
	directory bool
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
	name, line, ok := linkParts(link)
	if !ok {
		return fileTarget{}, false
	}
	name = strings.ReplaceAll(name, "\\", "/")
	if len(name) >= 5 && strings.EqualFold(name[:5], "file:") {
		name = name[5:]
		if strings.HasPrefix(name, "///") {
			name = name[2:]
		}
	} else if parsed, err := url.Parse(name); err == nil && parsed.Scheme != "" && !fileLinkDrive(name) {
		return fileTarget{}, false
	}
	decoded, err := url.PathUnescape(name)
	if err != nil || decoded == "" || strings.ContainsFunc(decoded, unicode.IsControl) {
		return fileTarget{}, false
	}
	name = strings.ReplaceAll(decoded, "\\", "/")
	if strings.HasPrefix(name, "//") || fileLinkDrive(name) && (len(name) < 3 || name[2] != '/') {
		return fileTarget{}, false
	}
	root = path.Clean("/" + strings.Trim(strings.ReplaceAll(root, "\\", "/"), "/"))
	target := fileTarget{line: line, directory: strings.HasSuffix(name, "/")}
	absolute := name
	switch {
	case fileLinkDrive(name):
		absolute = "/" + name
	case !strings.HasPrefix(name, "/"):
		relative := path.Clean(name)
		if relative != "." && !validWorkspaceRelative(relative) {
			return fileTarget{}, false
		}
		target.workspace = true
		if relative != "." {
			target.path = relative
		}
		return target, true
	}
	// An explicit absolute path can name a local item. A traversal must not
	// turn a scoped link into one of those system paths.
	for part := range strings.SplitSeq(absolute, "/") {
		if part == ".." {
			return fileTarget{}, false
		}
	}
	absolute = path.Clean(absolute)
	if absolute == root {
		target.workspace = true
	} else if relative, inside := strings.CutPrefix(absolute, strings.TrimSuffix(root, "/")+"/"); inside {
		if !validWorkspaceRelative(relative) {
			return fileTarget{}, false
		}
		target.workspace, target.path = true, relative
	} else {
		target.path = absolute
	}
	return target, true
}

func fileLinkDrive(name string) bool {
	return len(name) >= 2 && name[1] == ':' && name[0]|0x20 >= 'a' && name[0]|0x20 <= 'z'
}

func absoluteFileLinkDrive(name string) bool {
	return fileLinkDrive(name) && len(name) > 2 && (name[2] == '/' || name[2] == '\\') &&
		(len(name) == 3 || name[3] != '/' && name[3] != '\\')
}

// openChatLink opens a link of a reply: every link the application routes
// itself; false leaves a heading anchor to the transcript.
func (r *Runtime) openChatLink(c *ui.Context, session, link string) bool {
	r = r.localRuntime()
	if strings.HasPrefix(strings.TrimSpace(link), "#") {
		return false
	}
	link = normalizeLink(link)
	owner, cv := r.conversationLinkOwner(session)
	target := shell.LinkTarget{Session: session, URL: link, External: conversation.External(c)}
	if owner != nil && owner.controller != nil {
		target.Node = owner.controller.Target()
		if current, found := owner.session(session); found {
			target.Worktree = current.Worktree
		}
	}
	if parsed, err := url.Parse(link); err == nil && parsed.Scheme != "" && parsed.Scheme != "file" && !absoluteFileLinkDrive(link) {
		if !validLinkURL(parsed) {
			return true
		}
		target.Route = shell.LinkRoute{Kind: shell.LinkScheme, Name: parsed.Scheme}
		var active atomic.Bool
		active.Store(true)
		actions := shell.LinkActions{OpenSession: func() error {
			if !active.Load() {
				return unavailableLink()
			}
			if id := client.SessionLink(link); id != nil {
				switch {
				case r.App.Data.Find(id.String()) != nil:
					r.OpenSession(id.String())
					return nil
				case cv != nil && owner != nil:
					owner.openLiveChild(cv, id.String())
					return nil
				}
			}
			return unavailableLink()
		}}
		handled, err := r.dispatchConversationLink(c, owner, target, actions)
		active.Store(false)
		if handled {
			r.reportLink(c, err, "unavailable")
			return true
		}
		// An unclaimed opaque numeric suffix can be Markdown's path:line.
		// An unclaimed general URI never reaches a file or system opener.
		line, column, hasColumn := strings.Cut(parsed.Opaque, ":")
		if !decimal(line) || hasColumn && !decimal(column) {
			r.reportLink(c, unavailableLink(), "unavailable")
			return true
		}
		target.Route = shell.LinkRoute{}
	}
	r.openMarkdownFileLink(c, owner, target)
	return true
}

func validLinkURL(target *url.URL) bool {
	if target.User != nil {
		return false
	}
	switch target.Scheme {
	case "javascript", "vbscript", "data", "blob", "filesystem", "about":
		return false
	case "http", "https":
		return target.Hostname() != "" && target.Opaque == ""
	case "mailto", "tel":
		return target.Host == "" && target.Opaque != ""
	}
	return true
}

func (r *Runtime) conversationLinkOwner(session string) (*Runtime, *viewmodel.Conversation) {
	d := r.App.Data
	if session != "" && r.App.Sidebar.Selected == session {
		if node := r.App.Sidebar.SelectedNode(); node != "" {
			cv := d.FindOn(node, session)
			if cv == nil {
				return nil, nil
			}
			return r.conversationRuntime(cv, nil), cv
		}
	}
	if cv := d.Find(session); cv != nil {
		ambiguous := func(list []*viewmodel.Conversation) bool {
			for _, other := range list {
				if other.ID == session && other.Node != cv.Node {
					return true
				}
			}
			return false
		}
		for _, project := range d.Projects {
			if ambiguous(project.Conversations) {
				return nil, nil
			}
		}
		if ambiguous(d.Unassigned) {
			return nil, nil
		}
		return r.conversationRuntime(cv, nil), cv
	}
	if r.teams != nil && r.App.HasPanelTab(panel.Tab{Kind: panel.Subagent, ID: session}) {
		if scope, ok := r.teams.childScopes.tabs[r.childTabKey(session)]; ok {
			owner := r.runtimeFor(scope.node.String())
			if owner != nil {
				current, exists := owner.session(session)
				if _, valid := scope.target.CheckScope(owner.snapshot(), scope.node); exists && valid && scope.target.MatchesSession(current) {
					return owner, nil
				}
			}
		}
	}
	return nil, nil
}

func (r *Runtime) ownsLinkConversation(session string) bool {
	root := r.localRuntime()
	node := r.controller.Target().String()
	if root.runtimeFor(node) != r {
		return false
	}
	if root.App.Data.FindOn(node, session) != nil {
		return true
	}
	owner, _ := root.conversationLinkOwner(session)
	return owner == r
}

func unavailableLink() error {
	return fileActionFault(protocol.ErrorCodeUnavailable, "link target is unavailable")
}

func (r *Runtime) reportLink(c *ui.Context, err error, key string) {
	if faultCode(err) == protocol.ErrorCodeUnavailable {
		kit.Toast(c, kit.Failed, i18n.T(c, key))
		return
	}
	r.reportKeys(c, err, key, "workspace_changed", key)
}
