package python

import (
	"context"
	"errors"
	"path/filepath"
	"sync"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type Execution struct {
	Base    *Ready
	Skills  []Access
	Session string
	Cache   string
	Index   *string
	once    sync.Once
}

func (manager *Manager) Execution(ctx context.Context, session protocol.SessionID, skills []Access) (*Execution, error) {
	base, err := manager.Ensure(ctx, nil)
	if err != nil {
		return nil, err
	}
	execution := &Execution{Base: base, Cache: filepath.Join(manager.root, "cache")}
	success := false
	defer func() {
		if !success {
			_ = execution.Close()
		}
	}()
	for _, skill := range skills {
		refreshed, err := manager.Refresh(ctx, skill.Ready)
		if err != nil {
			return nil, err
		}
		execution.Skills = append(execution.Skills, Access{Ready: refreshed, Directory: skill.Directory})
	}
	files, err := core.OpenFiles(manager.profile, core.ManualFiles)
	if err != nil {
		return nil, err
	}
	directory := filepath.Join("workspaces", "sessions", session.String())
	if err := files.CreatePrivateDirectories(filepath.ToSlash(directory)); err != nil {
		_ = files.Close()
		return nil, err
	}
	if err := files.Close(); err != nil {
		return nil, storage(err)
	}
	execution.Session = filepath.Join(manager.profile, directory)
	manager.mu.Lock()
	execution.Index = copyString(manager.index)
	manager.mu.Unlock()
	success = true
	return execution, nil
}

func (execution *Execution) Variables() map[string]string {
	ready := execution.Base
	if len(execution.Skills) > 0 {
		ready = execution.Skills[len(execution.Skills)-1].Ready
	}
	values := map[string]string{
		"SAILRY_PYTHON": ready.Python, "SAILRY_SESSION_DIR": execution.Session,
		"PIP_CACHE_DIR": execution.Cache, "PYTHONDONTWRITEBYTECODE": "1",
	}
	if execution.Index != nil {
		values["PIP_INDEX_URL"] = *execution.Index
	}
	return values
}

func (execution *Execution) Close() error {
	var failures []error
	execution.once.Do(func() {
		failures = append(failures, execution.Base.Close())
		for _, skill := range execution.Skills {
			failures = append(failures, skill.Ready.Close())
		}
	})
	return errors.Join(failures...)
}
