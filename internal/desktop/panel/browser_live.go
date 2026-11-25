package panel

import (
	"net/url"
	"strings"

	"github.com/egoist/mygo/ui"
)

// BrowserPage is what a live browser tab's chrome shows of its page.
type BrowserPage struct {
	// Address is the page's address, "" before a first one.
	Address string
	// Title is the page's own title, "" until it gives one.
	Title         string
	Back, Forward bool
	// Loading is set from the start of a navigation until it finishes, fails
	// or is stopped.
	Loading bool
	// Error is the i18n key of what failed, "" while the page is fine. The
	// body shows it in place of the page.
	Error string
	// Picking is set while the page takes the element selection.
	Picking bool
}

// BrowserActions connects the browser tabs to their pages. View builds a
// tab's page and reports whether there is one to show.
type BrowserActions struct {
	View     func(*ui.Context, string) bool
	Page     func(string) BrowserPage
	Navigate func(id, address string) bool // whether the request was accepted
	Back     func(string)
	Forward  func(string)
	Reload   func(string)
	Stop     func(string)
	// Pick starts the element selection in a tab's page, or cancels the one
	// under way.
	Pick func(string)
	// Inspect opens or closes the page's web inspector.
	Inspect func(string)
	// Settings opens the browser's settings.
	Settings func()
	Release  func(string)
}

// AddBrowser opens another browser tab, shown or behind the active one, and
// returns it.
func (s *State) AddBrowser(show bool) Tab { return s.AddNumberedTab(Browser, show) }

// SetCreateResource lets the live window handle a Browser launch that cannot
// mount its platform service.
func (s *State) SetCreateResource(create func(Kind) bool) { s.createResource = create }

// OpenBrowserTab opens a fresh tab and gives focus to its address field.
func (s *State) OpenBrowserTab() {
	if _, ok := s.resource(Browser); !ok {
		return
	}
	if s.createResource != nil && s.createResource(Browser) {
		return
	}
	s.focusAddress = s.AddBrowser(true).ID
}

// BrowserAddress is the editable address of one tab, including unsubmitted text.
type BrowserAddress struct {
	Text, Shown string
	Field       ui.Handle
}

func (s *State) BrowserAddress(c *ui.Context, tab Tab) *BrowserAddress {
	address := &s.pageOf(tab.ID).live
	if tab == s.Active && tab.ID == s.focusAddress {
		s.focusAddress = ""
		address.Field.Focus(c)
	}
	return address
}

func BrowserTitle(s *State, tab Tab) string {
	if actions := s.BrowserActions; s.Live && actions != nil && actions.Page != nil {
		return actions.Page(tab.ID).Title
	}
	if state := s.pagesOf[tab.ID]; state != nil {
		if page := s.sample(state); page != nil {
			return page.Title
		}
	}
	return ""
}

func BrowserBusy(s *State, tab Tab) bool {
	return s.Live && s.BrowserActions != nil && s.BrowserActions.Page != nil && s.BrowserActions.Page(tab.ID).Loading
}

func ReleaseBrowserTab(s *State, tab Tab, _ TabRelease) {
	delete(s.pagesOf, tab.ID)
	if s.focusAddress == tab.ID {
		s.focusAddress = ""
	}
	if actions := s.BrowserActions; actions != nil && actions.Release != nil {
		actions.Release(tab.ID)
	}
}

// BrowserDestination is where text typed in the address field goes: an
// address with a scheme as typed, a host as https:// (http:// on this
// machine), and anything else as a web search. It reports false for nothing
// and for the schemes a browser tab never opens.
func BrowserDestination(text string) (string, bool) {
	text = strings.TrimSpace(text)
	lower := strings.ToLower(text)
	if text == "" || strings.HasPrefix(lower, "javascript:") || strings.HasPrefix(lower, "file:") || strings.HasPrefix(lower, "data:") {
		return "", false
	}
	if strings.Contains(text, "://") {
		return text, true
	}
	local := strings.HasPrefix(lower, "localhost") || strings.HasPrefix(lower, "127.") || strings.HasPrefix(lower, "[::1]")
	if !strings.ContainsAny(text, " \t\r\n") && (strings.Contains(text, ".") || local || strings.HasPrefix(text, "[")) {
		if local {
			return "http://" + text, true
		}
		return "https://" + text, true
	}
	return "https://www.google.com/search?q=" + url.QueryEscape(text), true
}
