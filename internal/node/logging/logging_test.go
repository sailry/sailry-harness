package logging

import (
	"encoding/json"
	"log/slog"
	"os"
	"path/filepath"
	"reflect"
	"regexp"
	"strings"
	"testing"
	"time"
)

func TestLogPrivacyFilter(t *testing.T) {
	t.Run("line", func(t *testing.T) {
		at := time.Date(2026, 11, 11, 8, 9, 10, 123456000, time.FixedZone("", 3600))
		got := line(at, slog.LevelWarn, "desktop", Fields{"code": "not_found", "area": "command", "detail": "two words", "count": uint64(3), "durable": true, "wait_ms": int64(-1), "reason": "true"})
		want := `2026-11-11T07:09:10.123456Z WARN desktop area=command detail="two words" reason="true" wait_ms=-1 code=not_found count=3 durable=true` + "\n"
		if got != want {
			t.Fatalf("line = %q", got)
		}
		for level, name := range map[slog.Level]string{slog.LevelDebug: "DEBUG", slog.LevelInfo: "INFO", slog.LevelWarn: "WARN", slog.LevelError: "ERROR"} {
			if levelName(level) != name {
				t.Errorf("level %v is %q", level, levelName(level))
			}
		}
	})
	t.Run("remote", func(t *testing.T) {
		got := Remote(Fields{
			"area": "command", "phase": "failed", "operation": "submit_turn", "code": "internal", "cause_code": "Not A Token",
			"duration_us": uint64(12), "count": int64(2), "elapsed_us": uint64(7), "app_version": "1.2.3",
			"session": "0195f1a0-0000-7000-8000-000000000001", "request": "r", "node": "n", "caller": "c", "turn": "t",
			"provider": "p", "provider_name": "My Provider", "model": "private-model", "detail": "/Users/someone/project/file.go",
			"reason": "http_rejected", "upstream_status": uint64(401), "package": "someone.package", "digest": "abc", "command": "submit_turn",
			"bytes": uint64(9), "durable": true, "stage": "provider_http",
		})
		want := Fields{"area": "command", "phase": "failed", "operation": "submit_turn", "code": "internal", "duration_us": uint64(12),
			"count": int64(2), "elapsed_us": uint64(7), "app_version": "1.2.3", "package": "third_party"}
		if !reflect.DeepEqual(got, want) {
			t.Fatalf("remote = %v", got)
		}
		for _, refused := range []Fields{{"app_version": "1.2"}, {"app_version": "v1.2.3 /Users/x"}, {"area": "Command"}, {"operation": "a..b"}, {"code": strings.Repeat("a", 65)}, {"session": uint64(1)}, {"detail": true}} {
			if got := Remote(refused); len(got) != 0 {
				t.Errorf("%v passed as %v", refused, got)
			}
		}
	})
	t.Run("file", func(t *testing.T) {
		profile, err := filepath.EvalSymlinks(t.TempDir())
		if err != nil {
			t.Fatal(err)
		}
		Emit(slog.LevelError, slog.String("area", "before")) // nothing is installed
		if _, err := Install(profile, "other"); err == nil {
			t.Fatal("an unknown application was installed")
		}
		directory := filepath.Join(profile, "logs", "desktop")
		if err := os.MkdirAll(directory, 0o700); err != nil {
			t.Fatal(err)
		}
		for _, name := range []string{"2026-11-09.log", "2026-11-10.log", "notes.log"} {
			if err := os.WriteFile(filepath.Join(directory, name), []byte("earlier\n"), 0o600); err != nil {
				t.Fatal(err)
			}
		}
		closeLog, err := Install(profile, "desktop")
		if err != nil {
			t.Fatal(err)
		}
		closed := false
		defer func() {
			if !closed {
				closeLog()
			}
		}()
		if _, err := Install(profile, "desktop"); err == nil {
			t.Fatal("a second writer was installed")
		}
		if Directory() != filepath.Join(profile, "logs") {
			t.Fatalf("directory = %q", Directory())
		}
		if file := installed.Load().file; file.MaxSize != 10 || file.MaxBackups != 9 || file.MaxAge != 0 || file.Compress || file.Filename != filepath.Join(directory, "current.log") {
			t.Fatalf("rotation = %+v", file)
		}
		// Before the first record the latest earlier file is the current one.
		if path, err := CurrentFile(); err != nil || path != filepath.Join(directory, "2026-11-10.log") {
			t.Fatalf("current = %q, %v", path, err)
		}
		if err := Retain(filepath.Join(profile, "elsewhere")); err != nil {
			t.Fatal(err)
		}
		if _, err := os.Stat(filepath.Join(directory, "2026-11-10.log")); err != nil {
			t.Fatal("another profile's owner retired the earlier files")
		}
		if err := Retain(profile); err != nil {
			t.Fatal(err)
		}
		entries, _ := os.ReadDir(directory)
		var names []string
		for _, entry := range entries {
			names = append(names, entry.Name())
		}
		if !reflect.DeepEqual(names, []string{"notes.log", "writer.lock"}) {
			t.Fatalf("after retiring: %v", names)
		}

		var reported []Fields
		var levels []slog.Level
		SetReport(func(level slog.Level, values Fields) {
			levels, reported = append(levels, level), append(reported, values)
		})
		defer SetReport(nil)
		Emit(slog.LevelDebug, slog.String("area", "command"), slog.String("phase", "begin"), slog.String("session", "local-session"),
			slog.String("prompt", "SECRET PROMPT"), slog.Any("body", map[string]string{"text": "SECRET BODY"}), slog.String("code", ""))
		Emit(slog.LevelInfo, slog.String("area", "command"), slog.String("detail", "line\nbreak\x00"+strings.Repeat("é", 300)))
		Emit(slog.LevelDebug-4, slog.String("area", "trace"))
		Emit(slog.LevelWarn, slog.String("area", "command"), slog.String("phase", "failed"), slog.String("code", "not_found"), slog.Uint64("duration_us", 5))
		Emit(slog.LevelError, slog.String("area", "command"), slog.String("phase", "failed"), slog.String("code", "internal"), slog.String("package", "someone.package"))
		slog.Info("another library's record", "area", "foreign")
		path, err := CurrentFile()
		if err != nil || path != filepath.Join(directory, "current.log") {
			t.Fatalf("current = %q, %v", path, err)
		}
		data, err := os.ReadFile(path)
		if err != nil {
			t.Fatal(err)
		}
		lines := strings.Split(strings.TrimSuffix(string(data), "\n"), "\n")
		stamp := regexp.MustCompile(`^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d\.\d{6}Z `)
		if len(lines) != 4 {
			t.Fatalf("file has %d records: %s", len(lines), data)
		}
		for index, want := range []string{
			"DEBUG desktop area=command phase=begin session=local-session",
			"INFO desktop area=command detail=linebreak" + strings.Repeat("é", 247),
			"WARN desktop area=command phase=failed code=not_found duration_us=5",
			"ERROR desktop area=command phase=failed package=someone.package code=internal",
		} {
			if !stamp.MatchString(lines[index]) || stamp.ReplaceAllString(lines[index], "") != want {
				t.Errorf("record %d = %q", index, lines[index])
			}
		}
		if strings.Contains(string(data), "SECRET") || strings.Contains(string(data), "foreign") {
			t.Fatalf("the file kept a payload or another library's record: %s", data)
		}
		if info, err := os.Stat(path); err != nil || info.Mode().Perm() != 0o600 {
			t.Fatalf("file mode = %v, %v", info.Mode(), err)
		}
		if !reflect.DeepEqual(levels, []slog.Level{slog.LevelWarn, slog.LevelError}) || reported[1]["package"] != "someone.package" {
			t.Fatalf("reported %v %v", levels, reported)
		}

		// The attachment keeps only what may leave the process.
		tail, err := Tail(path)
		if err != nil {
			t.Fatal(err)
		}
		var objects []map[string]any
		for _, entry := range strings.Split(strings.TrimSuffix(string(tail), "\n"), "\n") {
			var object map[string]any
			if err := json.Unmarshal([]byte(entry), &object); err != nil {
				t.Fatalf("attachment line %q: %v", entry, err)
			}
			if _, err := time.Parse(time.RFC3339Nano, object["timestamp"].(string)); err != nil {
				t.Fatal(err)
			}
			delete(object, "timestamp")
			objects = append(objects, object)
		}
		if !reflect.DeepEqual(objects, []map[string]any{
			{"level": "DEBUG", "area": "command", "phase": "begin"},
			{"level": "INFO", "area": "command"},
			{"level": "WARN", "area": "command", "phase": "failed", "code": "not_found", "duration_us": float64(5)},
			{"level": "ERROR", "area": "command", "phase": "failed", "code": "internal", "package": "third_party"},
		}) {
			t.Fatalf("attachment = %s", tail)
		}
		if strings.Contains(string(tail), "local-session") || strings.Contains(string(tail), "linebreak") || strings.Contains(string(tail), "someone") {
			t.Fatalf("the attachment kept local values: %s", tail)
		}
		// Lines this package did not write are not records.
		foreign := filepath.Join(profile, "foreign.log")
		if err := os.WriteFile(foreign, []byte("free text /Users/x\n2026-11-11T07:09:10.123456Z INFO other area=command\n2026-11-11T07:09:10.123456Z INFO host area=command area=again\n"+
			"2026-11-11T07:09:10.123456Z INFO host area=command count=4\r\n"), 0o600); err != nil {
			t.Fatal(err)
		}
		if tail, err = Tail(foreign); err != nil || string(tail) != `{"area":"command","count":4,"level":"INFO","timestamp":"2026-11-11T07:09:10.123456Z"}`+"\n" {
			t.Fatalf("foreign tail = %q, %v", tail, err)
		}
		if tail, err = Tail(""); err != nil || len(tail) != 0 {
			t.Fatalf("empty tail = %q, %v", tail, err)
		}

		// A full file rotates; the active name never changes.
		filler := slog.String("detail", strings.Repeat("x", 256))
		for range 10*1024*1024/300 + 2000 {
			Emit(slog.LevelInfo, slog.String("area", "fill"), filler)
		}
		entries, _ = os.ReadDir(directory)
		backups := 0
		for _, entry := range entries {
			info, err := entry.Info()
			if err != nil {
				t.Fatal(err)
			}
			if info.Size() > 10*1024*1024 {
				t.Errorf("%s holds %d bytes", entry.Name(), info.Size())
			}
			if strings.HasPrefix(entry.Name(), "current-") && strings.HasSuffix(entry.Name(), ".log") {
				backups++
			}
		}
		if backups != 1 {
			t.Fatalf("rotated files = %d", backups)
		}
		// The attachment reads at most 500 records of the active file.
		if tail, err = Tail(path); err != nil || strings.Count(string(tail), "\n") > 500 || len(tail) == 0 {
			t.Fatalf("tail has %d lines, %v", strings.Count(string(tail), "\n"), err)
		}

		closeLog()
		closed = true
		if Directory() != "" {
			t.Fatal("the log stayed installed")
		}
		before, _ := os.ReadFile(path)
		Emit(slog.LevelError, slog.String("area", "after"))
		if after, _ := os.ReadFile(path); len(after) != len(before) {
			t.Fatal("a record was written after the log closed")
		}
		// The lock is free for the next process.
		again, err := Install(profile, "desktop")
		if err != nil {
			t.Fatal(err)
		}
		again()
	})
}
