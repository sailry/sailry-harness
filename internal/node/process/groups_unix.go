//go:build darwin || linux

package process

import (
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"syscall"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// A command's process group outlives a Node that crashes, so each running
// group is recorded in the profile and the next Node kills what is left.
const groupPrefix = "command-group-"

type groupRecord struct {
	Group int `json:"group"`
	// Started identifies the group leader, so a reused process ID is never
	// taken for the command.
	Started string `json:"started"`
}

func groupPath(profile string, id protocol.RequestID) string {
	return filepath.Join(profile, "tmp", groupPrefix+id.String())
}

// recordGroup notes a started command's process group and returns the
// function that forgets it once the group has been killed.
func recordGroup(profile string, id protocol.RequestID, group int) func() {
	if profile == "" || group <= 1 {
		return func() {}
	}
	started, err := processStarted(group)
	if err != nil {
		return func() {}
	}
	body, err := json.Marshal(groupRecord{Group: group, Started: started})
	if err != nil {
		return func() {}
	}
	path := groupPath(profile, id)
	if err := os.MkdirAll(filepath.Dir(path), 0700); err != nil {
		return func() {}
	}
	if err := os.WriteFile(path, body, 0600); err != nil {
		return func() {}
	}
	return func() { _ = os.Remove(path) }
}

// reapGroups kills the command groups a previous Node left running. A record
// whose leader is gone or was started at another time names no command of
// ours and is only forgotten.
func reapGroups(profile string) error {
	directory := filepath.Join(profile, "tmp")
	entries, err := os.ReadDir(directory)
	if errors.Is(err, os.ErrNotExist) {
		return nil
	}
	if err != nil {
		return err
	}
	for _, entry := range entries {
		if !strings.HasPrefix(entry.Name(), groupPrefix) || !entry.Type().IsRegular() {
			continue
		}
		path := filepath.Join(directory, entry.Name())
		var record groupRecord
		if body, err := os.ReadFile(path); err == nil && json.Unmarshal(body, &record) == nil && record.Group > 1 && record.Started != "" {
			if started, err := processStarted(record.Group); err == nil && started == record.Started {
				_ = syscall.Kill(-record.Group, syscall.SIGKILL)
			}
		}
		if err := os.Remove(path); err != nil {
			return err
		}
	}
	return nil
}
