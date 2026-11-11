package models

import (
	"context"
	"errors"

	"github.com/sailry/sailry-harness/internal/localmodels/assets"
	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/sailry/sailry-harness/internal/speech"
)

// Start downloads a model's missing files, verifies each against its pinned
// size and SHA-256 and validates the result. Complete files are kept and
// partial ones resume. A mirror replaces the Hugging Face origin of a
// registered model. Starting a model that is already downloading does
// nothing; one in use is ErrBusy and an unregistered one ErrMissing.
func (m *Manager) Start(id *protocol.ModelID, mirror *string) error {
	key := name(id)
	m.mu.Lock()
	if m.active[key] > 0 {
		m.mu.Unlock()
		return ErrBusy
	}
	if current := m.jobs[key]; current != nil && current.state.Status == Downloading {
		m.mu.Unlock()
		return nil
	}
	var definition *protocol.ModelsDefinition
	if id != nil {
		for _, record := range m.records {
			if record.ID == *id {
				definition = &record.Definition
			}
		}
		if definition == nil {
			m.mu.Unlock()
			return ErrMissing
		}
	}
	ctx, cancel := context.WithCancel(m.ctx)
	m.jobs[key] = &job{cancel: cancel, state: State{Status: Downloading}}
	m.workers.Add(1)
	m.mu.Unlock()
	m.changed()
	go func() {
		defer m.workers.Done()
		defer cancel()
		m.finish(id, m.install(ctx, id, definition, mirror), ctx)
	}()
	return nil
}

// install acquires and validates the model's files.
func (m *Manager) install(ctx context.Context, id *protocol.ModelID, definition *protocol.ModelsDefinition, mirror *string) error {
	downloads, err := recommendedDownloads()
	if definition != nil {
		downloads, err = nil, nil
		for _, asset := range definition.Assets {
			address, invalid := assets.URL(asset, mirror)
			if invalid != nil {
				return speech.ErrInvalidModel
			}
			downloads = append(downloads, assets.Download{File: assets.File{Name: asset.Name, Size: asset.Bytes, SHA256: asset.Sha256}, URL: address})
		}
	}
	if err != nil {
		return err
	}
	path := m.path(id)
	if err := assets.Prepare(ctx, m.client, path, downloads, func(progress assets.Progress) { m.progress(id, progress) }); err != nil {
		return err
	}
	if ctx.Err() != nil {
		return assets.ErrCancelled
	}
	m.progress(id, assets.Progress{Phase: assets.Validating})
	if m.options.Validate != nil {
		if err := m.options.Validate(paths(path, definition != nil)); err != nil {
			return err
		}
	}
	// The receipt of this verification lets a dictation start without
	// hashing the model again.
	if list, marker := pinned(definition); !ready(path, list, marker) {
		return assets.ErrInvalidAsset
	}
	return nil
}

func (m *Manager) progress(id *protocol.ModelID, progress assets.Progress) {
	m.mu.Lock()
	if job := m.jobs[name(id)]; job != nil {
		job.state = State{Status: Downloading, Progress: progress}
	}
	m.mu.Unlock()
	m.changed()
}

// finish records a download's outcome: a registered model is marked
// installed in the index, a cancelled one is missing again and a failure
// keeps its reason.
func (m *Manager) finish(id *protocol.ModelID, result error, ctx context.Context) {
	if id != nil && result == nil && ctx.Err() == nil {
		m.progress(id, assets.Progress{Phase: assets.Installing})
	}
	m.mu.Lock()
	if ctx.Err() != nil {
		result = assets.ErrCancelled
	}
	if id != nil && result == nil {
		for index := range m.records {
			if record := &m.records[index]; record.ID == *id {
				previous := record.Installed
				record.Installed = true
				if err := m.save(); err != nil {
					record.Installed = previous
					result = err
				}
			}
		}
	}
	if job := m.jobs[name(id)]; job != nil {
		switch {
		case result == nil:
			job.state = State{Status: Installed}
		case errors.Is(result, assets.ErrCancelled):
			job.state = State{Status: Missing}
		default:
			job.state = State{Status: Failed, Err: result}
		}
		job.cancelled = false
	}
	m.mu.Unlock()
	m.changed()
}
