// Package chrome reads the cookies of a local Chrome profile without changing
// it. It follows Chromium's cookie database v24 and macOS v10 encryption:
// net/extras/sqlite/sqlite_persistent_cookie_store.cc and
// components/os_crypt/async/browser/keychain_key_provider.mm (main, 2026-09-16).
//
// Cookie values, ciphertexts, the Safe Storage password and its derived key
// are held in byte slices that Clear and Unlock overwrite. Nothing here logs.
package chrome

import (
	"bytes"
	"database/sql"
	"encoding/json"
	"errors"
	"net/url"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"time"

	_ "modernc.org/sqlite"
)

// Error is a failure's message key in the browser package's catalog.
type Error string

func (e Error) Error() string { return string(e) }

const (
	ErrMissing      Error = "browser_chrome_missing"
	ErrAccessDenied Error = "browser_chrome_access_denied"
	ErrFailed       Error = "browser_chrome_failed"
	ErrSchema       Error = "browser_chrome_schema"
	ErrKeyDenied    Error = "browser_chrome_key_denied"
	ErrCipher       Error = "browser_chrome_cipher"
)

// Profile is a Chrome profile that has a cookie database.
type Profile struct {
	Name     string
	Database string
}

// Cookie is one decrypted cookie. Expires is in Unix seconds, nil for a
// cookie without an expiry. SameSite is Chromium's value: 0 none, 1 lax,
// 2 strict, -1 unspecified.
type Cookie struct {
	Domain   string
	Name     string
	Value    []byte
	Path     string
	Secure   bool
	HTTPOnly bool
	SameSite int64
	Expires  *int64
}

// Import is the cookies read from a profile and the number left out.
type Import struct {
	Cookies []Cookie
	Skipped int
}

// Clear overwrites the cookie values.
func (i *Import) Clear() {
	for index := range i.Cookies {
		clear(i.Cookies[index].Value)
	}
	i.Cookies = nil
}

// Prepared is a profile's cookies as stored: plain values and ciphertexts.
type Prepared struct {
	cookies []pending
	skipped int
}

type pending struct {
	cookie    Cookie
	encrypted []byte
}

// NeedsKey reports whether any cookie is encrypted.
func (p *Prepared) NeedsKey() bool {
	return slices.ContainsFunc(p.cookies, func(cookie pending) bool { return len(cookie.encrypted) != 0 })
}

// Unlock decrypts the cookies. It asks for the Safe Storage password only
// when a cookie is encrypted, overwrites the password and the derived key
// before returning, and counts a cookie that does not decrypt as skipped. A
// failed password leaves p as it was, so Unlock may be called again.
func (p *Prepared) Unlock(password func() ([]byte, error)) (Import, error) {
	var secret []byte
	if p.NeedsKey() {
		value, err := password()
		if err != nil {
			return Import{}, err
		}
		secret = key(value)
		clear(value)
		defer clear(secret)
	}
	result := Import{Skipped: p.skipped}
	for _, current := range p.cookies {
		cookie := current.cookie
		if len(current.encrypted) != 0 {
			value, err := decrypt(secret, cookie.Domain, current.encrypted)
			if err != nil {
				result.Skipped++
				continue
			}
			cookie.Value = value
		} else {
			cookie.Value = slices.Clone(cookie.Value)
		}
		result.Cookies = append(result.Cookies, cookie)
	}
	return result, nil
}

// Clear overwrites the stored values and ciphertexts.
func (p *Prepared) Clear() {
	for index := range p.cookies {
		clear(p.cookies[index].cookie.Value)
		clear(p.cookies[index].encrypted)
	}
	p.cookies = nil
}

// Profiles lists the profiles of the user's Chrome. Reading Chrome's data is
// what lets macOS apply its own protection of another application's data.
func Profiles() ([]Profile, error) {
	home, err := os.UserHomeDir()
	if err != nil {
		return nil, ErrMissing
	}
	return Discover(filepath.Join(home, "Library", "Application Support", "Google", "Chrome"))
}

// Discover lists the profiles named in root's Local State whose directory,
// directly under root, has a cookie database. It opens no database.
func Discover(root string) ([]Profile, error) {
	data, err := os.ReadFile(filepath.Join(root, "Local State"))
	if err != nil {
		return nil, discoveryError(err)
	}
	var state struct {
		Profile struct {
			InfoCache map[string]json.RawMessage `json:"info_cache"`
		} `json:"profile"`
	}
	if err := json.Unmarshal(data, &state); err != nil || state.Profile.InfoCache == nil {
		return nil, ErrFailed
	}
	folders := make([]string, 0, len(state.Profile.InfoCache))
	for folder := range state.Profile.InfoCache {
		folders = append(folders, folder)
	}
	slices.Sort(folders)
	var profiles []Profile
	for _, folder := range folders {
		if folder == "." || !filepath.IsLocal(folder) || strings.ContainsAny(folder, `/\`) {
			continue
		}
		database, err := cookiePath(filepath.Join(root, folder))
		if err != nil {
			return nil, err
		}
		if database == "" {
			continue
		}
		var info struct {
			Name *string `json:"name"`
		}
		name := folder
		if json.Unmarshal(state.Profile.InfoCache[folder], &info) == nil && info.Name != nil {
			name = *info.Name
		}
		profiles = append(profiles, Profile{Name: name, Database: database})
	}
	return profiles, nil
}

func discoveryError(err error) Error {
	switch {
	case errors.Is(err, os.ErrNotExist):
		return ErrMissing
	case errors.Is(err, os.ErrPermission):
		return ErrAccessDenied
	}
	return ErrFailed
}

func cookiePath(directory string) (string, error) {
	for _, path := range []string{filepath.Join(directory, "Cookies"), filepath.Join(directory, "Network", "Cookies")} {
		info, err := os.Stat(path)
		switch {
		case err == nil && info.Mode().IsRegular():
			return path, nil
		case err != nil && !errors.Is(err, os.ErrNotExist):
			return "", discoveryError(err)
		}
	}
	return "", nil
}

// database opens a cookie database read-only in place, without copying it.
// SQLite waits two seconds for Chrome's lock; a database still locked then,
// or of another schema version, is refused.
func database(path string) (*sql.DB, error) {
	// The database can be protected separately from Local State. Opening the
	// file lets macOS enforce consent and keeps a refusal apart from SQLite's
	// own failures.
	access, err := os.Open(path)
	if err != nil {
		return nil, discoveryError(err)
	}
	defer access.Close()
	name := filepath.ToSlash(path)
	if filepath.VolumeName(path) != "" && !strings.HasPrefix(name, "/") {
		name = "/" + name
	}
	location := url.URL{Scheme: "file", Path: name, RawQuery: url.Values{"mode": {"ro"}, "_pragma": {"busy_timeout(2000)"}}.Encode()}
	db, err := sql.Open("sqlite", location.String())
	if err != nil {
		return nil, ErrFailed
	}
	db.SetMaxOpenConns(1)
	var version int64
	if err := db.QueryRow("SELECT CAST(value AS INTEGER) FROM meta WHERE key='version'").Scan(&version); err != nil {
		db.Close()
		return nil, ErrFailed
	}
	if version != 24 {
		db.Close()
		return nil, ErrSchema
	}
	return db, nil
}

// Sites lists the hosts that have cookies in a profile.
func Sites(profile Profile) ([]string, error) {
	db, err := database(profile.Database)
	if err != nil {
		return nil, err
	}
	defer db.Close()
	rows, err := db.Query("SELECT DISTINCT host_key FROM cookies ORDER BY host_key")
	if err != nil {
		return nil, ErrFailed
	}
	defer rows.Close()
	var sites []string
	for rows.Next() {
		var site string
		if rows.Scan(&site) != nil {
			return nil, ErrFailed
		}
		sites = append(sites, site)
	}
	if rows.Err() != nil {
		return nil, ErrFailed
	}
	return sites, nil
}

// Prepare reads the cookies of the given hosts. Partitioned and expired
// cookies and those not encrypted as v10 are counted as skipped; cookies of
// other hosts are not counted. It reads no credential.
func Prepare(profile Profile, sites []string) (*Prepared, error) {
	db, err := database(profile.Database)
	if err != nil {
		return nil, err
	}
	defer db.Close()
	rows, err := db.Query("SELECT host_key,name,value,CAST(encrypted_value AS BLOB),path,is_secure,is_httponly,samesite,expires_utc,has_expires,top_frame_site_key FROM cookies")
	if err != nil {
		return nil, ErrFailed
	}
	defer rows.Close()
	result := &Prepared{}
	now := time.Now().Unix()
	for rows.Next() {
		var cookie Cookie
		var plain, encrypted sql.RawBytes
		var secure, httpOnly, expires, hasExpires int64
		var partition string
		if rows.Scan(&cookie.Domain, &cookie.Name, &plain, &encrypted, &cookie.Path, &secure, &httpOnly, &cookie.SameSite, &expires, &hasExpires, &partition) != nil {
			result.Clear()
			return nil, ErrFailed
		}
		if !slices.Contains(sites, cookie.Domain) {
			continue
		}
		cookie.Secure, cookie.HTTPOnly = secure != 0, httpOnly != 0
		if hasExpires != 0 {
			// Chromium counts microseconds from 1601-01-01.
			seconds := expires/1_000_000 - 11_644_473_600
			cookie.Expires = &seconds
		}
		if partition != "" || cookie.Expires != nil && *cookie.Expires <= now ||
			len(encrypted) != 0 && !bytes.HasPrefix(encrypted, []byte("v10")) {
			result.skipped++
			continue
		}
		cookie.Value = slices.Clone([]byte(plain))
		result.cookies = append(result.cookies, pending{cookie: cookie, encrypted: slices.Clone([]byte(encrypted))})
	}
	if rows.Err() != nil {
		result.Clear()
		return nil, ErrFailed
	}
	return result, nil
}
