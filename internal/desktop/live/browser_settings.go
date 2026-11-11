package live

import (
	"context"
	"encoding/json"
	"errors"
	"path/filepath"
	"runtime"
	"slices"
	"strings"
	"sync"
	"time"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/chrome"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// The browser package's settings page: whether browser tabs keep their
// website data, and importing a local Chrome profile's cookies into them.
// Profile paths, the Chrome Safe Storage password and cookie values stay in
// this process: the page receives opaque profile IDs, names and counts.

// Failures the page shows from its own catalog. They are plain errors, as the
// page finds the key at the end of the message.
var (
	errBrowserImportUnavailable = errors.New("browser_import_unavailable")
	errBrowserImportFailed      = errors.New("browser_import_failed")
	errBrowserViewClosed        = errors.New("plugin view is closed")
	// errBrowserImportCancelled is the user refusing the system's Keychain
	// prompt; the page receives null and shows nothing.
	errBrowserImportCancelled = errors.New("browser import cancelled")
)

// browserSources are what a Chrome import reads and writes. A test replaces
// them on the local Runtime; nothing else does.
type browserSources struct {
	supported bool
	profiles  func() ([]chrome.Profile, error)
	password  func() ([]byte, error)
	set       func(mygo.Cookie) error
	list      func() ([]mygo.Cookie, error)
}

// browserImport is one settings view's scan: the profiles it listed, by the
// opaque ID the page holds, and whether a scan or an import is under way.
type browserImport struct {
	mu       sync.Mutex
	busy     bool
	profiles map[string]chrome.Profile
}

// browserPrivate is the in-memory website data of browser tabs that keep
// nothing, shared by every window of the process.
var browserPrivate = sync.OnceValue(mygo.NewEphemeralSession)

// browserProfileSupported reports whether this system has a browser profile
// to keep and to import into: macOS, as in the reference implementation.
// Elsewhere browser tabs are always private.
func browserProfileSupported() bool { return runtime.GOOS == "darwin" }

// browserPersistent reports whether browser tabs keep cookies and other
// website data between runs: the browser_persistent preference, on by
// default, of a profile-backed desktop.
func (r *Runtime) browserPersistent() bool {
	store := r.localRuntime().Preferences
	return browserProfileSupported() && store.Path != "" && (store.Data.BrowserPersistent == nil || *store.Data.BrowserPersistent)
}

// browserSession is the website data store new browser tabs use: one kept on
// disk per profile directory, or the process's in-memory one.
func (r *Runtime) browserSession() *mygo.Session {
	if r.browserPersistent() {
		return mygo.NewSession("browser " + filepath.Dir(r.localRuntime().Preferences.Path))
	}
	return browserPrivate()
}

func (r *Runtime) browserImportSources() browserSources {
	root := r.localRuntime()
	if root.browserSources != nil {
		return *root.browserSources
	}
	session := root.browserSession()
	return browserSources{
		supported: browserProfileSupported(), profiles: chrome.Profiles, password: chromePassword,
		set:  session.Cookies.Set,
		list: func() ([]mygo.Cookie, error) { return session.Cookies.List(mygo.CookieFilter{}) },
	}
}

// chromePassword reads Chrome's cookie password, the generic password
// "Chrome Safe Storage" of account "Chrome" in the login Keychain. The
// system asks the user before giving another application's item.
func chromePassword() ([]byte, error) {
	value, err := mygo.Secrets.Service("Chrome Safe Storage").Get("Chrome")
	switch {
	case err == nil:
		return value, nil
	case strings.Contains(err.Error(), "(OSStatus -128)"):
		// errSecUserCanceled: the user refused the prompt.
		return nil, errBrowserImportCancelled
	}
	return nil, chrome.ErrKeyDenied
}

func (state *pluginNativeState) browserOperations() {
	operations := state.native.Operations
	settings := &browserImport{profiles: map[string]chrome.Profile{}}
	// permit is the reference's scope check: a settings view of a package
	// that declares the browser action.
	permit := func(action protocol.PluginAction) error {
		info, available := state.packageInfo()
		if state.scope.Surface.Kind != "settings" || !available || info.Extension == nil || !slices.Contains(info.Extension.Actions, action) {
			return pluginNativeFault(protocol.ErrorCodePermissionDenied, "browser capability is unavailable in this view")
		}
		return nil
	}
	enabled := func() bool { return state.runtime.localRuntime().Preferences.Path != "" }
	operations["sailry/ui.readBrowserSettings"] = func(context.Context, []json.RawMessage) (json.RawMessage, error) {
		if err := permit(protocol.PluginActionReadBrowser); err != nil {
			return nil, err
		}
		data := state.runtime.localRuntime().Preferences.Data
		return pluginNativeJSON(map[string]bool{
			"supported": state.runtime.browserImportSources().supported, "enabled": enabled(),
			"persistent": data.BrowserPersistent == nil || *data.BrowserPersistent,
		})
	}
	operations["sailry/ui.setBrowserPersistent"] = func(_ context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
		if err := permit(protocol.PluginActionControlBrowser); err != nil {
			return nil, err
		}
		value, err := pluginNativeArgument[bool](arguments, 0)
		if err != nil {
			return nil, err
		}
		root := state.runtime.localRuntime()
		if !enabled() || !root.browserImportSources().supported {
			return nil, errBrowserImportUnavailable
		}
		// The frame's preference save writes it; open tabs reload in the
		// other store on the next frame.
		root.Preferences.Data.BrowserPersistent = &value
		root.wake()
		return json.RawMessage("null"), nil
	}
	// begin runs on the UI thread: it checks the view and takes the one
	// scan or import a view runs at a time.
	begin := func(ctx context.Context, action protocol.PluginAction) (browserSources, error) {
		var sources browserSources
		_, err := state.update(ctx, func(*ui.Context) (json.RawMessage, error) {
			if err := permit(action); err != nil {
				return nil, err
			}
			sources = state.runtime.browserImportSources()
			settings.mu.Lock()
			defer settings.mu.Unlock()
			if !sources.supported || !enabled() || settings.busy {
				return nil, errBrowserImportUnavailable
			}
			settings.busy = true
			return nil, nil
		})
		return sources, err
	}
	end := func() {
		settings.mu.Lock()
		settings.busy = false
		settings.mu.Unlock()
	}
	operations["sailry/ui.listBrowserProfiles"] = func(ctx context.Context, _ []json.RawMessage) (json.RawMessage, error) {
		sources, err := begin(ctx, protocol.PluginActionReadBrowser)
		if err != nil {
			return nil, err
		}
		defer end()
		profiles, err := sources.profiles()
		if err != nil {
			return nil, err
		}
		if ctx.Err() != nil {
			return nil, errBrowserViewClosed
		}
		entries := make([]map[string]string, 0, len(profiles))
		settings.mu.Lock()
		clear(settings.profiles)
		for _, profile := range profiles {
			id := protocol.NewRequestID().String()
			settings.profiles[id] = profile
			entries = append(entries, map[string]string{"id": id, "name": profile.Name})
		}
		settings.mu.Unlock()
		return pluginNativeJSON(entries)
	}
	operations["sailry/ui.importBrowserProfile"] = func(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
		id, err := pluginNativeArgument[string](arguments, 0)
		if err != nil {
			return nil, err
		}
		sources, err := begin(ctx, protocol.PluginActionControlBrowser)
		if err != nil {
			return nil, err
		}
		defer end()
		settings.mu.Lock()
		profile, exists := settings.profiles[id]
		settings.mu.Unlock()
		if !exists {
			return nil, chrome.ErrMissing
		}
		count, skipped, err := browserImportProfile(ctx, sources, profile)
		if errors.Is(err, errBrowserImportCancelled) {
			return json.RawMessage("null"), nil
		}
		if err != nil {
			return nil, err
		}
		return pluginNativeJSON(map[string]int{"count": count, "skipped": skipped})
	}
}

// browserImportProfile reads a profile's cookies read-only, decrypts them and
// sets them in the browser's store. It reports the cookies the store then
// holds as given, and those left out: partitioned, expired, undecryptable or
// refused by the store. Values are overwritten before it returns.
func browserImportProfile(ctx context.Context, sources browserSources, profile chrome.Profile) (count, skipped int, err error) {
	sites, err := chrome.Sites(profile)
	if err != nil {
		return 0, 0, err
	}
	prepared, err := chrome.Prepare(profile, sites)
	if err != nil {
		return 0, 0, err
	}
	defer prepared.Clear()
	if ctx.Err() != nil {
		return 0, 0, errBrowserViewClosed
	}
	imported, err := prepared.Unlock(sources.password)
	if err != nil {
		return 0, 0, err
	}
	defer imported.Clear()
	type identity struct{ domain, path, name string }
	expected := map[identity]mygo.Cookie{}
	for _, source := range imported.Cookies {
		if ctx.Err() != nil {
			return 0, 0, errBrowserViewClosed
		}
		cookie := mygo.Cookie{
			Name: source.Name, Value: string(source.Value), Domain: strings.ToLower(source.Domain), Path: source.Path,
			Secure: source.Secure, HTTPOnly: source.HTTPOnly,
		}
		if cookie.Path == "" {
			cookie.Path = "/"
		}
		if source.Expires != nil {
			cookie.Expires = time.Unix(*source.Expires, 0)
		}
		switch source.SameSite {
		case 0:
			// The store refuses SameSite=None without Secure, as Chrome
			// does; such a cookie keeps the default.
			if cookie.Secure {
				cookie.SameSite = mygo.CookieSameSiteNone
			}
		case 2:
			cookie.SameSite = mygo.CookieSameSiteStrict
		case 1:
			cookie.SameSite = mygo.CookieSameSiteLax
		}
		// A cookie the store refuses is counted as skipped below.
		if sources.set(cookie) == nil {
			expected[identity{cookie.Domain, cookie.Path, cookie.Name}] = cookie
		}
	}
	stored, err := sources.list()
	if err != nil {
		return 0, 0, errBrowserImportFailed
	}
	for _, cookie := range stored {
		if wanted, exists := expected[identity{cookie.Domain, cookie.Path, cookie.Name}]; exists &&
			wanted.Value == cookie.Value && wanted.Secure == cookie.Secure && wanted.HTTPOnly == cookie.HTTPOnly {
			count++
		}
	}
	return count, imported.Skipped + len(imported.Cookies) - count, nil
}
