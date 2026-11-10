package git

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"io"
	"net/url"
	"os"
	"os/exec"
	"sort"
	"strings"
	"sync"
	"time"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type captureWriter struct {
	mu        sync.Mutex
	data      []byte
	truncated bool
}

func (w *captureWriter) Write(p []byte) (int, error) {
	w.mu.Lock()
	defer w.mu.Unlock()
	n := min(len(p), 64*1024-len(w.data))
	w.data = append(w.data, p[:n]...)
	w.truncated = w.truncated || n < len(p)
	return len(p), nil
}
func nativeRunner(environment []string) Runner {
	if environment == nil {
		environment = os.Environ()
	}
	environment = append([]string(nil), environment...)
	return func(ctx context.Context, root string, args []string, input []byte) (CommandResult, error) {
		cmd := exec.Command("git", append([]string{"--no-pager"}, args...)...)
		cmd.Dir = root
		env := map[string]string{}
		for _, value := range environment {
			k, v, ok := strings.Cut(value, "=")
			if ok && !strings.HasPrefix(strings.ToUpper(k), "GIT_") {
				env[k] = v
			}
		}
		env["LC_ALL"] = "C"
		env["GIT_TERMINAL_PROMPT"] = "0"
		env["GIT_EDITOR"] = "true"
		env["GIT_SEQUENCE_EDITOR"] = "true"
		keys := []string{}
		for k := range env {
			keys = append(keys, k)
		}
		sort.Strings(keys)
		for _, k := range keys {
			cmd.Env = append(cmd.Env, k+"="+env[k])
		}
		if input != nil {
			cmd.Stdin = bytes.NewReader(input)
		}
		stdout, stderr := &captureWriter{}, &captureWriter{}
		outRead, outWrite, err := os.Pipe()
		if err != nil {
			return CommandResult{}, err
		}
		defer outRead.Close()
		defer outWrite.Close()
		errRead, errWrite, err := os.Pipe()
		if err != nil {
			return CommandResult{}, err
		}
		defer errRead.Close()
		defer errWrite.Close()
		cmd.Stdout, cmd.Stderr = outWrite, errWrite

		cleanup, err := prepareNative(cmd)
		if err != nil {
			return CommandResult{}, err
		}
		defer cleanup()
		if err := cmd.Start(); err != nil {
			return CommandResult{}, fault(protocol.ErrorCodeUnavailable, "Git executable unavailable on execution Node")
		}
		if err := ownNative(cmd); err != nil {
			_ = cmd.Process.Kill()
			_ = cmd.Wait()
			return CommandResult{}, err
		}
		outWrite.Close()
		errWrite.Close()
		drained := make(chan error, 2)
		go func() { _, err := io.Copy(stdout, outRead); drained <- err }()
		go func() { _, err := io.Copy(stderr, errRead); drained <- err }()
		done := make(chan error, 1)
		go func() { done <- cmd.Wait() }()
		select {
		case err = <-done:
		case <-ctx.Done():
			killNative(cmd)
			select {
			case <-done:
			case <-time.After(2 * time.Second):
			}
			return CommandResult{}, ctx.Err()
		}
		killNative(cmd)
		deadline := time.NewTimer(2 * time.Second)
		defer deadline.Stop()
		for i := 0; i < 2; i++ {
			select {
			case drainErr := <-drained:
				if drainErr != nil {
					return CommandResult{}, drainErr
				}
			case <-deadline.C:
				outRead.Close()
				errRead.Close()
				return CommandResult{}, fault(protocol.ErrorCodeOutcomeUnknown, "Git output could not be read")
			}
		}

		code := 0
		var exit *exec.ExitError
		if errors.As(err, &exit) {
			code = exit.ExitCode()
		} else if err != nil {
			return CommandResult{}, err
		}
		return CommandResult{Stdout: stdout.data, Stderr: stderr.data, ExitCode: code, Truncated: stdout.truncated || stderr.truncated, InvalidUTF8: !utf8.Valid(stdout.data) || !utf8.Valid(stderr.data)}, nil
	}
}
func (s *Service) command(ctx context.Context, root string, args []string, input []byte, read bool) (CommandResult, error) {
	duration := 120 * time.Second
	if read {
		duration = readTimeout
	}
	limited, cancel := context.WithTimeout(ctx, duration)
	defer cancel()
	result, err := s.run(limited, root, args, input)
	s.record(root, args, result)
	if err != nil {
		if errors.Is(err, context.DeadlineExceeded) && !read {
			return result, fault(protocol.ErrorCodeOutcomeUnknown, "Git operation timed out; inspect repository before retrying")
		}
		return result, mapError(err)
	}
	if result.ExitCode != 0 {
		code := protocol.ErrorCodeInvalidRequest
		message := strings.ToLower(string(result.Stderr))
		switch {
		case strings.Contains(message, ".lock"):
			code = protocol.ErrorCodeBusy
		case strings.Contains(message, "gpg") || strings.Contains(message, "signing failed"):
			code = protocol.ErrorCodeUnavailable
		case strings.Contains(message, "empty ident") || strings.Contains(message, "user.email") || strings.Contains(message, "user.name"):
			code = protocol.ErrorCodeNotConfigured
		case strings.Contains(message, "conflict") || strings.Contains(message, "would be overwritten"):
			code = protocol.ErrorCodeConflict
		case strings.Contains(message, "authentication") || strings.Contains(message, "permission denied"):
			code = protocol.ErrorCodePermissionDenied
		}
		return result, fault(code, fmt.Sprintf("Git operation failed (exit %d)", result.ExitCode))
	}
	return result, nil
}
func redact(text string) string {
	var result strings.Builder
	for _, word := range strings.FieldsFunc(text, func(r rune) bool { return r == '\x00' }) {
		for _, token := range strings.SplitAfter(word, " ") {
			if start := strings.Index(token, "://"); start >= 0 {
				end := len(token)
				for i := start + 3; i < len(token); i++ {
					if strings.ContainsRune("/?#\n\r\t", rune(token[i])) {
						end = i
						break
					}
				}
				if at := strings.LastIndex(token[start+3:end], "@"); at >= 0 {
					token = token[:start+3] + "<redacted>" + token[start+3+at:]
				}
				if q := strings.IndexByte(token, '?'); q >= 0 {
					token = token[:q] + "?<redacted>"
				}
			}
			lower := strings.ToLower(token)
			for _, key := range []string{"password=", "token=", "authorization=", "secret="} {
				if strings.Contains(lower, key) {
					if eq := strings.IndexByte(token, '='); eq >= 0 {
						token = token[:eq+1] + "<redacted>"
					}
					break
				}
			}
			result.WriteString(token)
		}
	}
	return result.String()
}
func (s *Service) record(root string, args []string, result CommandResult) {
	safe := append([]string(nil), args...)
	for i := 1; i < len(safe); i++ {
		if safe[i-1] == "--message" || safe[i-1] == "-m" {
			safe[i] = "<message>"
		}
	}
	entry := redact("> git " + strings.Join(safe, " ") + "\n" + string(result.Stdout) + string(result.Stderr) + fmt.Sprintf("\n[exit %d]\n", result.ExitCode))
	entry, _ = bounded(entry, 64*1024)
	s.mu.Lock()
	defer s.mu.Unlock()
	if _, ok := s.journal[root]; !ok && len(s.journal) >= 32 {
		keys := []string{}
		for k := range s.journal {
			keys = append(keys, k)
		}
		sort.Strings(keys)
		delete(s.journal, keys[0])
	}
	current := s.journal[root] + entry
	if len(current) > 64*1024 {
		current = current[len(current)-64*1024:]
		for len(current) > 0 && !utf8.RuneStart(current[0]) {
			current = current[1:]
		}
	}
	s.journal[root] = current
}
func (s *Service) Output(root string) string {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.journal[root]
}
func publicRemoteURL(value string) string {
	parsed, err := url.Parse(value)
	if err != nil {
		return ""
	}
	parsed.User = nil
	parsed.RawQuery = ""
	parsed.Fragment = ""
	return parsed.String()
}
