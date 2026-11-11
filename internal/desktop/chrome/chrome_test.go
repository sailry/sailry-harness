package chrome

import (
	"bytes"
	"database/sql"
	"encoding/hex"
	"errors"
	"io/fs"
	"log"
	"log/slog"
	"os"
	"path/filepath"
	"runtime"
	"testing"
)

// fixtureCipher is "fixture-login" for .example.test under the password
// "isolated-password", made by an independent oracle (Python hashlib PBKDF2,
// OpenSSL AES-128-CBC) for the reference implementation's tests.
func fixtureCipher(t *testing.T) []byte {
	t.Helper()
	value, err := hex.DecodeString("763130cbe0d3934b1d9921743baffaf66d630303d3497336a024272f8327f166749f1af504654cc643d5543c05231db3432f4c")
	if err != nil {
		t.Fatal(err)
	}
	return value
}

// fixtureDatabase writes a version 24 cookie database with one importable
// cookie, one of another host, and an expired, a partitioned and a v20 one.
func fixtureDatabase(t *testing.T, path string) {
	t.Helper()
	db, err := sql.Open("sqlite", path)
	if err != nil {
		t.Fatal(err)
	}
	defer db.Close()
	if _, err := db.Exec(`CREATE TABLE meta(key LONGVARCHAR, value LONGVARCHAR); INSERT INTO meta VALUES('version',24);
CREATE TABLE cookies(host_key TEXT,name TEXT,value TEXT,encrypted_value BLOB,path TEXT,is_secure INTEGER,is_httponly INTEGER,samesite INTEGER,expires_utc INTEGER,has_expires INTEGER,top_frame_site_key TEXT);`); err != nil {
		t.Fatal(err)
	}
	for _, row := range []struct {
		domain, name string
		expiry       int64
		partition    string
		cipher       []byte
	}{
		{".example.test", "login", 0, "", fixtureCipher(t)},
		{".other.test", "excluded", 0, "", fixtureCipher(t)},
		{".example.test", "expired", 1, "", fixtureCipher(t)},
		{".example.test", "partitioned", 0, "https://other.test", fixtureCipher(t)},
		{".example.test", "unsupported", 0, "", []byte("v20unsupported")},
	} {
		if _, err := db.Exec("INSERT INTO cookies VALUES(?,?,'',?,'/',1,1,1,?,?,?)", row.domain, row.name, row.cipher, row.expiry, row.expiry != 0, row.partition); err != nil {
			t.Fatal(err)
		}
	}
}

func TestChromeImport(t *testing.T) {
	password := func() ([]byte, error) { return []byte("isolated-password"), nil }
	denied := func() ([]byte, error) { return nil, ErrKeyDenied }

	t.Run("verifies key, host and ciphertext", func(t *testing.T) {
		secret := key([]byte("isolated-password"))
		if want := []byte{0x2f, 0x83, 0xf5, 0xd4, 0x15, 0x36, 0x05, 0x66, 0x72, 0xfa, 0xa0, 0x5b, 0xd4, 0xce, 0x17, 0xd6}; !bytes.Equal(secret, want) {
			t.Fatalf("derived key %x", secret)
		}
		value, err := decrypt(secret, ".example.test", fixtureCipher(t))
		if err != nil || string(value) != "fixture-login" {
			t.Fatalf("decrypted %q: %v", value, err)
		}
		for name, attempt := range map[string]func() ([]byte, error){
			"another host":    func() ([]byte, error) { return decrypt(secret, ".other.test", fixtureCipher(t)) },
			"another key":     func() ([]byte, error) { return decrypt(make([]byte, 16), ".example.test", fixtureCipher(t)) },
			"another version": func() ([]byte, error) { return decrypt(secret, ".example.test", []byte("v20unsupported")) },
			"a partial block": func() ([]byte, error) { return decrypt(secret, ".example.test", fixtureCipher(t)[:20]) },
			"no ciphertext":   func() ([]byte, error) { return decrypt(secret, ".example.test", []byte("v10")) },
		} {
			if value, err := attempt(); !errors.Is(err, ErrCipher) || value != nil {
				t.Errorf("%s decrypted to %q: %v", name, value, err)
			}
		}
	})

	t.Run("imports the selected hosts read-only without logging secrets", func(t *testing.T) {
		var logged bytes.Buffer
		previous, output := slog.Default(), log.Writer()
		slog.SetDefault(slog.New(slog.NewTextHandler(&logged, &slog.HandlerOptions{Level: slog.LevelDebug})))
		t.Cleanup(func() { slog.SetDefault(previous); log.SetOutput(output) })
		log.SetOutput(&logged)

		root := t.TempDir()
		path := filepath.Join(root, "Cookies")
		fixtureDatabase(t, path)
		original, err := os.ReadFile(path)
		if err != nil {
			t.Fatal(err)
		}
		profile := Profile{Name: "Fixture", Database: path}
		sites, err := Sites(profile)
		if err != nil || len(sites) != 2 {
			t.Fatalf("sites %q: %v", sites, err)
		}
		prepared, err := Prepare(profile, []string{".example.test"})
		if err != nil || !prepared.NeedsKey() {
			t.Fatalf("prepared %+v: %v", prepared, err)
		}
		// Preparation reads the profile only; a refused credential keeps the
		// prepared cookies for another attempt.
		if _, err := prepared.Unlock(denied); !errors.Is(err, ErrKeyDenied) {
			t.Fatalf("refused key: %v", err)
		}
		given := []byte("isolated-password")
		result, err := prepared.Unlock(func() ([]byte, error) { return given, nil })
		if err != nil || len(result.Cookies) != 1 || result.Skipped != 3 {
			t.Fatalf("import %+v: %v", result, err)
		}
		if !bytes.Equal(given, make([]byte, len(given))) {
			t.Fatalf("the password was left in memory: %q", given)
		}
		cookie := result.Cookies[0]
		if string(cookie.Value) != "fixture-login" || cookie.Domain != ".example.test" || cookie.Name != "login" || cookie.Path != "/" ||
			!cookie.Secure || !cookie.HTTPOnly || cookie.SameSite != 1 || cookie.Expires != nil {
			t.Fatalf("cookie %+v", cookie)
		}
		again, err := prepared.Unlock(password)
		if err != nil || len(again.Cookies) != 1 || string(again.Cookies[0].Value) != "fixture-login" {
			t.Fatalf("second import %+v: %v", again, err)
		}
		value := cookie.Value
		result.Clear()
		prepared.Clear()
		if !bytes.Equal(value, make([]byte, len(value))) || result.Cookies != nil || prepared.NeedsKey() {
			t.Fatalf("cleared import kept %q", value)
		}
		empty, err := Prepare(profile, nil)
		if err != nil {
			t.Fatal(err)
		}
		none, err := empty.Unlock(func() ([]byte, error) {
			t.Error("unselected hosts asked for the credential")
			return nil, ErrKeyDenied
		})
		if err != nil || len(none.Cookies) != 0 || none.Skipped != 0 {
			t.Fatalf("unselected import %+v: %v", none, err)
		}
		// The profile is as Chrome left it: same bytes, no journal beside it.
		if current, err := os.ReadFile(path); err != nil || !bytes.Equal(current, original) {
			t.Fatalf("the cookie database changed: %v", err)
		}
		if entries, err := os.ReadDir(root); err != nil || len(entries) != 1 {
			t.Fatalf("files were written beside the cookie database: %v %v", entries, err)
		}
		for _, secret := range []string{"fixture-login", "isolated-password", "2f83f5d4"} {
			if bytes.Contains(logged.Bytes(), []byte(secret)) {
				t.Fatalf("a secret was logged: %s", logged.String())
			}
		}
		if logged.Len() != 0 {
			t.Fatalf("the import logged: %s", logged.String())
		}
	})

	t.Run("refuses another schema version", func(t *testing.T) {
		path := filepath.Join(t.TempDir(), "Cookies")
		fixtureDatabase(t, path)
		db, err := sql.Open("sqlite", path)
		if err != nil {
			t.Fatal(err)
		}
		if _, err := db.Exec("UPDATE meta SET value=25 WHERE key='version'"); err != nil {
			t.Fatal(err)
		}
		db.Close()
		if _, err := Sites(Profile{Database: path}); !errors.Is(err, ErrSchema) {
			t.Fatalf("schema 25: %v", err)
		}
		if _, err := Prepare(Profile{Database: path}, []string{".example.test"}); !errors.Is(err, ErrSchema) {
			t.Fatalf("schema 25: %v", err)
		}
	})

	t.Run("confines profile discovery", func(t *testing.T) {
		root := t.TempDir()
		write := func(name string, data string) {
			t.Helper()
			if err := os.MkdirAll(filepath.Dir(filepath.Join(root, name)), 0o755); err != nil {
				t.Fatal(err)
			}
			if err := os.WriteFile(filepath.Join(root, name), []byte(data), 0o644); err != nil {
				t.Fatal(err)
			}
		}
		write("Default/Cookies", "")
		write("Profile 1/Network/Cookies", "unopened fixture")
		write("Local State", `{"profile":{"info_cache":{"Profile 1":{"name":"Network"},"Default":{"name":"Fixture"},"Absent":{"name":"Absent"},"Unnamed":{},"../outside":{"name":"Outside"},"Default/Network":{"name":"Nested"}}}}`)
		write("Unnamed/Cookies", "")
		profiles, err := Discover(root)
		want := []Profile{
			{Name: "Fixture", Database: filepath.Join(root, "Default", "Cookies")},
			{Name: "Network", Database: filepath.Join(root, "Profile 1", "Network", "Cookies")},
			{Name: "Unnamed", Database: filepath.Join(root, "Unnamed", "Cookies")},
		}
		if err != nil || len(profiles) != len(want) {
			t.Fatalf("profiles %+v: %v", profiles, err)
		}
		for index := range want {
			if profiles[index] != want[index] {
				t.Fatalf("profile %d is %+v, want %+v", index, profiles[index], want[index])
			}
		}
		// Discovery opened no database.
		if data, err := os.ReadFile(want[1].Database); err != nil || string(data) != "unopened fixture" {
			t.Fatalf("discovery changed a cookie database: %q %v", data, err)
		}
	})

	t.Run("distinguishes missing, denied and unreadable data", func(t *testing.T) {
		for err, want := range map[error]Error{fs.ErrNotExist: ErrMissing, fs.ErrPermission: ErrAccessDenied, fs.ErrInvalid: ErrFailed} {
			if got := discoveryError(&fs.PathError{Op: "open", Path: "fixture", Err: err}); got != want {
				t.Errorf("%v is %v, want %v", err, got, want)
			}
		}
		root := t.TempDir()
		if _, err := Discover(root); !errors.Is(err, ErrMissing) {
			t.Fatalf("missing Local State: %v", err)
		}
		if err := os.Mkdir(filepath.Join(root, "Local State"), 0o755); err != nil {
			t.Fatal(err)
		}
		if _, err := Discover(root); !errors.Is(err, ErrFailed) {
			t.Fatalf("Local State directory: %v", err)
		}
		other := t.TempDir()
		for _, state := range []string{"not json", `{"profile":{}}`, `{"profile":{"info_cache":[]}}`} {
			if err := os.WriteFile(filepath.Join(other, "Local State"), []byte(state), 0o644); err != nil {
				t.Fatal(err)
			}
			if _, err := Discover(other); !errors.Is(err, ErrFailed) {
				t.Fatalf("Local State %q: %v", state, err)
			}
		}
		path := filepath.Join(t.TempDir(), "Cookies")
		if _, err := database(path); !errors.Is(err, ErrMissing) {
			t.Fatalf("missing database: %v", err)
		}
		if err := os.WriteFile(path, []byte("unreadable fixture"), 0o000); err != nil {
			t.Fatal(err)
		}
		// Windows has no owner read bit, and root reads any file.
		if runtime.GOOS != "windows" && os.Geteuid() != 0 {
			if _, err := database(path); !errors.Is(err, ErrAccessDenied) {
				t.Fatalf("unreadable database: %v", err)
			}
		}
		if err := os.Chmod(path, 0o600); err != nil {
			t.Fatal(err)
		}
		if _, err := database(path); !errors.Is(err, ErrFailed) {
			t.Fatalf("database that is not SQLite: %v", err)
		}
		if data, err := os.ReadFile(path); err != nil || string(data) != "unreadable fixture" {
			t.Fatalf("a refused database changed: %q %v", data, err)
		}
	})
}
