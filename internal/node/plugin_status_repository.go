package node

import (
	"context"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func (runner *pluginStatusRunner) watchRepository() error {
	required := false
	for _, entry := range runner.packages {
		required = required || entry.changes(protocol.PluginHostChangeRepository)
	}
	if !required || runner.effective.Worktree == nil {
		runner.closeRepository()
		return nil
	}
	worktree := *runner.effective.Worktree
	root, err := runner.service.node.workspaceRoot(runner.watcher.ctx, worktree, false)
	if err != nil {
		return err
	}
	if runner.repository != nil && runner.repository.root == root {
		return nil
	}
	runner.closeRepository()
	lifetime, cancel := context.WithCancel(runner.watcher.ctx)
	stream, err := runner.service.node.watches.subscribe(lifetime, root, runner.service.node.ID(), worktree, true)
	if err != nil {
		cancel()
		return err
	}
	current, err := runner.service.node.workspaceRoot(lifetime, worktree, false)
	if err == nil && current != root {
		err = unavailable()
	}
	if err == nil {
		_, err = stream.Next(lifetime)
	}
	if err != nil {
		cancel()
		_ = stream.Close()
		return err
	}
	runner.repositoryID++
	generation := runner.repositoryID
	repository := &pluginStatusRepository{root: root, stream: stream, cancel: cancel, done: make(chan struct{})}
	runner.repository = repository
	go func() {
		defer close(repository.done)
		for lifetime.Err() == nil {
			_, err := stream.Next(lifetime)
			select {
			case <-lifetime.Done():
				return
			case runner.repositories <- pluginStatusRepositoryUpdate{generation: generation, err: err}:
			}
			if err != nil {
				return
			}
		}
	}()
	return nil
}

func (runner *pluginStatusRunner) closeRepository() {
	if runner.repository != nil {
		repository := runner.repository
		runner.repository = nil
		repository.cancel()
		_ = repository.stream.Close()
		<-repository.done
	}
}
