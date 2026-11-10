//go:build darwin || linux

// Adapted from sailry-code 67ae9fa0. See third_party_licenses/sailry-code-terminal.md.
package process

import (
	"bytes"
	"context"
	"crypto/rand"
	"encoding/hex"
	"io"
	"maps"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"syscall"
	"time"
)

func captureLogin(ctx context.Context, environment Environment) Environment {
	ctx, cancel := context.WithTimeout(ctx, 5*time.Second)
	defer cancel()
	type resolved struct {
		account account
		err     error
	}
	result := make(chan resolved, 1)
	go func() { account, err := currentAccount(); result <- resolved{account, err} }()
	var account account
	select {
	case <-ctx.Done():
		return environment
	case value := <-result:
		if value.err != nil {
			return environment
		}
		account = value.account
	}
	apply := func(values map[string]string) {
		values["HOME"], values["USER"], values["LOGNAME"], values["SHELL"] = account.home, account.name, account.name, account.shell
	}
	environment.values["HOME"], environment.scoped["HOME"], environment.scoped["USER"] = account.home, account.home, account.name
	apply(environment.unrestricted)
	apply(environment.terminal)
	if values := captureShell(ctx, account.shell, environment.terminal); values != nil {
		captured := NewEnvironment(environmentValues(values))
		apply(captured.terminal)
		maps.Copy(environment.values, captured.values)
		maps.Copy(environment.terminal, captured.terminal)
		maps.Copy(environment.scoped, captured.scoped)
		maps.Copy(environment.unrestricted, captured.unrestricted)
	}
	return environment
}

func captureShell(ctx context.Context, shell string, environment map[string]string) map[string]string {
	var nonce [16]byte
	if _, err := rand.Read(nonce[:]); err != nil {
		return nil
	}
	marker := "SAILRY_ENV_" + hex.EncodeToString(nonce[:])
	script := "printf '\\0" + marker + "\\0'; /usr/bin/env -0 && printf '\\0" + marker + "-end\\0'"
	args := []string{"-l", "-c", script}
	switch filepath.Base(shell) {
	case "fish":
		args = []string{"-l", "-i", "-c", script}
	case "bash", "zsh", "sh":
		args = []string{"-ilc", script}
	}
	command := exec.Command(shell, args...)
	command.Env, command.Dir = environmentValues(environment), environment["HOME"]
	command.SysProcAttr = &syscall.SysProcAttr{Setpgid: true}
	reader, writer, err := os.Pipe()
	if err != nil {
		return nil
	}
	defer reader.Close()
	defer writer.Close()
	command.Stdout = writer
	if ctx.Err() != nil || command.Start() != nil {
		return nil
	}
	_ = writer.Close()
	defer syscall.Kill(-command.Process.Pid, syscall.SIGKILL)
	type captured struct {
		bytes []byte
		err   error
	}
	output := make(chan captured, 1)
	go func() { value, err := io.ReadAll(io.LimitReader(reader, 1024*1024+1)); output <- captured{value, err} }()
	waited := make(chan error, 1)
	go func() { err := command.Wait(); _ = syscall.Kill(-command.Process.Pid, syscall.SIGKILL); waited <- err }()
	deadline, _ := ctx.Deadline()
	reserve := min(time.Until(deadline)/5, 100*time.Millisecond)
	probe, stop := context.WithDeadline(ctx, deadline.Add(-reserve))
	defer stop()
	var value []byte
	readDone, rootDone, success := false, false, true
	for !readDone || !rootDone {
		select {
		case <-probe.Done():
			success = false
			goto cleanup
		case result := <-output:
			readDone = true
			value = result.bytes
			if result.err != nil || len(value) > 1024*1024 {
				success = false
				goto cleanup
			}
		case err := <-waited:
			rootDone = true
			success = success && err == nil
		}
	}
cleanup:
	_ = syscall.Kill(-command.Process.Pid, syscall.SIGKILL)
	if !rootDone {
		select {
		case <-waited:
		case <-ctx.Done():
		}
	}
	if !success {
		return nil
	}
	return parseEnvironment(value, marker)
}

func parseEnvironment(value []byte, marker string) map[string]string {
	start := bytes.Index(value, []byte("\x00"+marker+"\x00"))
	if start < 0 {
		return nil
	}
	value = value[start+len(marker)+2:]
	end := bytes.Index(value, []byte("\x00"+marker+"-end\x00"))
	if end < 0 {
		return nil
	}
	result := map[string]string{}
	for _, record := range bytes.Split(value[:end], []byte{0}) {
		if len(record) == 0 {
			continue
		}
		key, value, ok := strings.Cut(string(record), "=")
		if !ok || key == "" {
			return nil
		}
		result[key] = value
	}
	return result
}
