package live

import (
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type memorySettingsState struct {
	loaded           bool
	loading          bool
	refreshQueued    bool
	generation       uint64
	observedRev      uint64
	observedProjects string
	view             string
	scope            string
	query            string
	config           settings.Stored[settings.MemoryConfig]
	configError      string
	configRetry      bool
	configRequest    *protocol.Request
	entries          []settings.MemoryEntryItem
	allActive        []settings.MemoryEntryItem
	projects         []settings.MemoryProjectOption
	loadError        string
	pendingRequest   *protocol.Request
	actions          settings.MemoryActions
}

func memoryErrorKey(code protocol.ErrorCode) string {
	switch code {
	case protocol.ErrorCodeRevisionConflict, protocol.ErrorCodeNotFound:
		return "memory_conflict"
	case protocol.ErrorCodeOutcomeUnknown, protocol.ErrorCodeUnavailable:
		return "memory_unknown"
	case protocol.ErrorCodeInvalidRequest:
		return "memory_invalid"
	case protocol.ErrorCodeBusy:
		return "memory_capacity"
	case protocol.ErrorCodeConflict:
		return "memory_duplicate"
	default:
		return "memory_failed"
	}
}

func memoryUncertain(code protocol.ErrorCode) bool {
	return code == protocol.ErrorCodeOutcomeUnknown || code == protocol.ErrorCodeUnavailable
}

func (r *Runtime) memorySettingsFrame() {
	root := r.localRuntime()
	m := root.App.Settings
	if root.App.Page != shell.SettingsPage || m.Section != settings.Memory {
		return
	}
	owner := root.managedSettingsNode()
	if owner == nil {
		m.MemorySettings = settings.MemorySettingsState{Status: "connections_offline"}
		m.MemoryActions = nil
		return
	}
	st := &owner.memorySettings
	if st.view == "" {
		st.view = "active"
	}
	if st.scope == "" {
		st.scope = "all"
	}
	if st.actions.Refresh == nil {
		st.actions = settings.MemoryActions{
			Refresh: func() {
				st.view = m.MemorySettings.View
				st.scope = m.MemorySettings.Scope
				st.query = m.MemorySettings.Query
				owner.refreshMemoryCatalog()
			},
			SaveConfig: func() {
				st.config.Draft = m.MemorySettings.Config.Draft
				owner.saveMemoryConfig()
			},
			ReloadConfig: func() {
				st.config.Draft = st.config.Saved
				st.configError = ""
				st.configRetry = false
				st.configRequest = nil
				owner.refreshMemoryCatalog()
			},
			LoadEntry:   owner.loadMemoryEntry,
			SaveEntry:   owner.saveMemoryEntry,
			DeleteEntry: owner.deleteMemoryEntry,
			ForgetPending: func() {
				st.pendingRequest = nil
			},
		}
	}

	snap := owner.snapshot()
	projects := make([]settings.MemoryProjectOption, 0, len(snap.Projects))
	var projKey strings.Builder
	for _, p := range snap.Projects {
		id := p.ID.String()
		projects = append(projects, settings.MemoryProjectOption{ID: id, Name: p.Name})
		projKey.WriteString(id)
		projKey.WriteByte(':')
		projKey.WriteString(p.Name)
		projKey.WriteByte('\x00')
	}
	st.projects = projects

	if (!st.loaded || snap.MemoryRevision != st.observedRev || projKey.String() != st.observedProjects) && !st.loading {
		st.observedProjects = projKey.String()
		owner.refreshMemoryCatalog()
	}

	m.MemorySettings = settings.MemorySettingsState{
		Config:      st.config,
		ConfigError: st.configError,
		ConfigRetry: st.configRetry,
		Loaded:      st.loaded,
		Loading:     st.loading,
		Connected:   owner.controller.Connected(),
		Error:       st.loadError,
		View:        st.view,
		Scope:       st.scope,
		Query:       st.query,
		Projects:    st.projects,
		Entries:     st.entries,
		AllActive:   st.allActive,
	}
	m.MemoryActions = &st.actions
}

func (r *Runtime) refreshMemoryCatalog() {
	st := &r.memorySettings
	if !r.controller.Connected() {
		return
	}
	if st.loading {
		st.refreshQueued = true
		return
	}
	st.loading = true
	st.loadError = ""
	st.generation++
	gen := st.generation
	st.observedRev = r.snapshot().MemoryRevision
	view := st.view
	if view == "" {
		view = "active"
	}
	scope := st.scope
	if scope == "" {
		scope = "all"
	}
	query := st.query
	payload := protocol.CommandReadMemoryCatalogPayload{View: view, Query: query}
	switch scope {
	case "all":
	case "global":
		payload.GlobalOnly = true
	default:
		p := scope
		payload.Project = &p
	}
	needAllActive := view != "active" || scope != "all" || strings.TrimSpace(query) != ""
	r.async(func() {
		out, err := r.controller.Do(r.ctx, protocol.Command{Kind: "read_memory_catalog", Data: payload})
		var activeOut protocol.Output
		var activeErr error
		if err == nil && needAllActive {
			activeOut, activeErr = r.controller.Do(r.ctx, protocol.Command{Kind: "read_memory_catalog", Data: protocol.CommandReadMemoryCatalogPayload{View: "active"}})
		}
		r.enqueue(func(c *ui.Context) {
			if st.generation != gen {
				return
			}
			st.loading = false
			defer func() {
				if st.refreshQueued {
					st.refreshQueued = false
					r.refreshMemoryCatalog()
				}
			}()
			if err != nil {
				key := "memory_failed"
				if faultCode(err) == protocol.ErrorCodeInvalidRequest {
					key = "memory_search_too_long"
				}
				st.loadError = key
				kit.Toast(c, kit.Failed, i18n.T(c, key))
				return
			}
			cat, ok := detail[protocol.MemoryCatalog](out.Data)
			if !ok || out.Kind != "memory_catalog" {
				st.loadError = "memory_failed"
				kit.Toast(c, kit.Failed, i18n.T(c, "memory_failed"))
				return
			}
			cfg := settings.MemoryConfig{
				Enabled:         cat.Settings.Enabled,
				AutoWrite:       cat.Settings.AutoWrite,
				ContextBytes:    cat.Settings.ContextBytes,
				ReviewAfterDays: cat.Settings.ReviewAfterDays,
			}
			if !st.loaded || (st.config.Draft == st.config.Saved && !st.config.Pending && !st.configRetry) {
				st.config.Draft = cfg
			}
			st.config.Saved = cfg
			st.config.Revision = cat.Settings.Revision
			st.loaded = true
			st.entries = convertMemorySummaries(cat.Entries)
			if !needAllActive {
				st.allActive = slices.Clone(st.entries)
			} else if activeErr == nil {
				if actCat, valid := detail[protocol.MemoryCatalog](activeOut.Data); valid && activeOut.Kind == "memory_catalog" {
					st.allActive = convertMemorySummaries(actCat.Entries)
				}
			}
		})
	})
}

func convertMemorySummaries(items []protocol.MemorySummary) []settings.MemoryEntryItem {
	out := make([]settings.MemoryEntryItem, 0, len(items))
	for _, item := range items {
		out = append(out, convertMemorySummary(item))
	}
	return out
}

func convertMemorySummary(item protocol.MemorySummary) settings.MemoryEntryItem {
	project := ""
	if item.Project != nil {
		project = *item.Project
	}
	dup := ""
	if item.DuplicateOf != nil {
		dup = *item.DuplicateOf
	}
	return settings.MemoryEntryItem{
		ID:          item.ID,
		Project:     project,
		Title:       item.Title,
		Kind:        item.Kind,
		Archived:    item.Archived,
		Revision:    item.Revision,
		UpdatedAtMs: item.UpdatedAtMs,
		DuplicateOf: dup,
		NeedsReview: item.NeedsReview,
	}
}

func (r *Runtime) saveMemoryConfig() {
	st := &r.memorySettings
	if st.config.Pending || !r.controller.Connected() {
		return
	}
	st.config.Pending = true
	st.configError = ""
	var req protocol.Request
	if st.configRequest != nil {
		req = *st.configRequest
	} else {
		req = r.controller.Prepare(protocol.Command{
			Kind: "save_memory_settings",
			Data: protocol.CommandSaveMemorySettingsPayload{
				ExpectedRevision: st.config.Revision,
				Enabled:          st.config.Draft.Enabled,
				AutoWrite:        st.config.Draft.AutoWrite,
				ContextBytes:     st.config.Draft.ContextBytes,
				ReviewAfterDays:  st.config.Draft.ReviewAfterDays,
			},
		})
		copied := req
		st.configRequest = &copied
	}
	r.async(func() {
		out, err := r.controller.Execute(r.ctx, req)
		r.enqueue(func(c *ui.Context) {
			st.config.Pending = false
			if err != nil {
				code := faultCode(err)
				key := memoryErrorKey(code)
				if code == protocol.ErrorCodeRevisionConflict {
					key = "memory_settings_conflict"
				}
				st.configError = key
				if memoryUncertain(code) {
					st.configRetry = true
				} else {
					st.configRetry = false
					st.configRequest = nil
				}
				kit.Toast(c, kit.Failed, i18n.T(c, key))
				return
			}
			st.configRequest = nil
			st.configRetry = false
			st.configError = ""
			if saved, ok := detail[protocol.MemorySettings](out.Data); ok && out.Kind == "memory_settings" {
				cfg := settings.MemoryConfig{
					Enabled:         saved.Enabled,
					AutoWrite:       saved.AutoWrite,
					ContextBytes:    saved.ContextBytes,
					ReviewAfterDays: saved.ReviewAfterDays,
				}
				st.config.Saved = cfg
				st.config.Draft = cfg
				st.config.Revision = saved.Revision
			}
		})
	})
}

func (r *Runtime) loadMemoryEntry(id string, done func(entry settings.MemoryEntryItem, body string, errKey string)) {
	r.async(func() {
		out, err := r.controller.Do(r.ctx, protocol.Command{
			Kind: "read_memory_entry",
			Data: protocol.CommandReadMemoryEntryPayload{ID: id},
		})
		r.enqueue(func(c *ui.Context) {
			if err != nil {
				key := memoryErrorKey(faultCode(err))
				kit.Toast(c, kit.Failed, i18n.T(c, key))
				done(settings.MemoryEntryItem{}, "", key)
				return
			}
			entry, ok := detail[protocol.MemoryEntry](out.Data)
			if !ok || out.Kind != "memory_entry" {
				kit.Toast(c, kit.Failed, i18n.T(c, "memory_failed"))
				done(settings.MemoryEntryItem{}, "", "memory_failed")
				return
			}
			done(convertMemorySummary(entry.Summary), entry.Body, "")
		})
	})
}

func (r *Runtime) saveMemoryEntry(draft settings.MemorySaveDraft, done func(ok bool, retry bool, errKey string)) {
	st := &r.memorySettings
	var req protocol.Request
	if st.pendingRequest != nil {
		req = *st.pendingRequest
	} else {
		payload := protocol.CommandSaveMemoryEntryPayload{
			ExpectedRevision: draft.ExpectedRevision,
			Title:            draft.Title,
			Kind:             draft.Kind,
			Body:             draft.Body,
			Archived:         draft.Archived,
		}
		if draft.ID != "" {
			id := draft.ID
			payload.ID = &id
		}
		if draft.Project != "" {
			proj := draft.Project
			payload.Project = &proj
		}
		if len(draft.MergeSources) > 0 {
			payload.MergeSources = make([]protocol.MemoryMergeSource, 0, len(draft.MergeSources))
			for _, src := range draft.MergeSources {
				payload.MergeSources = append(payload.MergeSources, protocol.MemoryMergeSource{ID: src.ID, Revision: src.Revision})
			}
		}
		req = r.controller.Prepare(protocol.Command{Kind: "save_memory_entry", Data: payload})
		copied := req
		st.pendingRequest = &copied
	}
	r.async(func() {
		_, err := r.controller.Execute(r.ctx, req)
		r.enqueue(func(c *ui.Context) {
			if err != nil {
				code := faultCode(err)
				key := memoryErrorKey(code)
				retry := memoryUncertain(code)
				if !retry {
					st.pendingRequest = nil
				}
				kit.Toast(c, kit.Failed, i18n.T(c, key))
				done(false, retry, key)
				return
			}
			st.pendingRequest = nil
			r.refreshMemoryCatalog()
			done(true, false, "")
		})
	})
}

func (r *Runtime) deleteMemoryEntry(id string, revision uint64, done func(ok bool, retry bool, errKey string)) {
	st := &r.memorySettings
	var req protocol.Request
	if st.pendingRequest != nil {
		req = *st.pendingRequest
	} else {
		req = r.controller.Prepare(protocol.Command{
			Kind: "delete_memory_entry",
			Data: protocol.CommandDeleteMemoryEntryPayload{ID: id, ExpectedRevision: revision},
		})
		copied := req
		st.pendingRequest = &copied
	}
	r.async(func() {
		_, err := r.controller.Execute(r.ctx, req)
		r.enqueue(func(c *ui.Context) {
			if err != nil {
				code := faultCode(err)
				key := memoryErrorKey(code)
				retry := memoryUncertain(code)
				if !retry {
					st.pendingRequest = nil
				}
				kit.Toast(c, kit.Failed, i18n.T(c, key))
				done(false, retry, key)
				return
			}
			st.pendingRequest = nil
			r.refreshMemoryCatalog()
			done(true, false, "")
		})
	})
}
