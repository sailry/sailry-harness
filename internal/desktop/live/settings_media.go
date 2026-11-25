package live

import (
	"fmt"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Settings › Media models: the model the Node Settings shows uses for
// vision and for generating images, video and speech. The assignments and
// the models offered are read again whenever the providers or the
// assignments change, and a choice saves all four at the revision read.

// mediaState is what the media rows were read for and the bindings their
// option keys stand for.
type mediaState struct {
	seen     string // the providers and assignments last read for
	wanted   string // the latest ones the snapshot showed
	reading  bool
	bindings map[string]protocol.MediaBinding
	saving   map[mediaSaveKey]func() bool
}

type mediaSaveKey struct {
	revision uint64
	bindings [4]string
}

func (r *Runtime) BindMediaSettings() settings.WindowBinding {
	m := r.App.Settings
	return settings.WindowBinding{
		Blocked: func(string) { m.Media = settings.Stored[[4]string]{Pending: true} },
		Mount: func(target settings.PageTarget) settings.PageBinding {
			owner := r.runtimeFor(target.Node)
			owner.media.seen = ""
			m.Media = settings.Stored[[4]string]{}
			m.SaveMedia = func() {
				if target.Current() {
					owner.saveMedia()
				}
			}
			var seen bool
			var providers, assignments uint64
			follow := func() {
				source, revision := owner.controller.ProviderRevision(), owner.snapshot().MediaSettings.Revision
				if !seen || providers != source || assignments != revision {
					seen, providers, assignments = true, source, revision
					owner.followMedia(owner.snapshot())
				}
			}
			return settings.PageBinding{
				Follow: follow,
				Recover: func() {
					owner.media.seen = ""
					seen = false
					follow()
				},
				Close: func() {
					m.Media, m.MediaOptions, m.MediaUnavailable = settings.Stored[[4]string]{}, nil, [4]bool{}
					m.SaveMedia = nil
				},
			}
		},
	}
}

// followMedia reads the media assignments and the models offered when the
// snapshot's providers or assignments are not the ones last read for. A
// change that arrives during a read is read for once that read ends.
func (r *Runtime) followMedia(snapshot protocol.Snapshot) {
	if r.currentSettingsPage(settings.Media) == nil {
		return
	}
	var seen strings.Builder
	fmt.Fprintf(&seen, "%d", snapshot.MediaSettings.Revision)
	for _, provider := range snapshot.Providers {
		fmt.Fprintf(&seen, " %s/%d", provider.ID, provider.Revision)
	}
	state := &r.media
	state.wanted = seen.String()
	if !state.reading && state.seen != state.wanted {
		r.readMedia()
	}
}

// readMedia reads for the latest providers and assignments. An answer older
// than the assignments shown, as one overtaken by a save, is dropped: the
// snapshot that carries the newer revision is read for in its turn.
func (r *Runtime) readMedia() {
	page := r.currentSettingsPage(settings.Media)
	if page == nil {
		return
	}
	state := &r.media
	state.seen = state.wanted
	state.reading = r.async(func() {
		saved, err := r.controller.Do(page.target.Context, protocol.Command{Kind: "read_media_settings"})
		var listed protocol.Output
		if err == nil {
			listed, err = r.controller.Do(page.target.Context, protocol.Command{Kind: "list_media_models"})
		}
		r.enqueue(func(c *ui.Context) {
			state.reading = false
			if !page.target.Current() {
				// A return to this Node may have waited for the earlier read.
				state.seen = ""
				r.followMedia(r.snapshot())
				return
			}
			if err != nil {
				r.reportKeys(c, err, "media_load_failed", "media_load_failed", "media_load_failed")
			} else if value, valid := detail[protocol.MediaSettings](saved.Data); valid && saved.Kind == "media_settings" && value.Revision >= r.App.Settings.Media.Revision {
				candidates, _ := detail[[]protocol.MediaCandidate](listed.Data)
				r.projectMedia(value, candidates, true)
			}
			if state.seen != state.wanted {
				r.readMedia()
			}
		})
	})
}

// projectMedia shows the assignments the Node holds. A draft that differs
// from what was saved (its save failed, conflicted or is in doubt) stays
// the rows' choice; listed replaces the models offered.
func (r *Runtime) projectMedia(value protocol.MediaSettings, candidates []protocol.MediaCandidate, listed bool) {
	m := r.App.Settings
	state := &r.media
	if listed {
		// A draft's model that is no longer offered stays its row's choice.
		kept := map[string]viewmodel.MediaOption{}
		for _, option := range m.MediaOptions {
			for _, key := range m.Media.Draft {
				if key == option.Key {
					kept[key] = viewmodel.MediaOption{Key: key, Label: option.Label}
				}
			}
		}
		previous := state.bindings
		defer func() {
			for key, option := range kept {
				if _, offered := state.bindings[key]; !offered {
					state.bindings[key] = previous[key]
					m.MediaOptions = append(m.MediaOptions, option)
				}
			}
		}()
		state.bindings = map[string]protocol.MediaBinding{}
		m.MediaOptions = nil
		for _, candidate := range candidates {
			binding := protocol.MediaBinding{Provider: candidate.Provider, Model: candidate.Model}
			option := viewmodel.MediaOption{Key: mediaKey(binding), Label: candidate.ProviderName + " · " + candidate.Model}
			for _, kind := range candidate.Kinds {
				option.Kinds = append(option.Kinds, string(kind))
			}
			state.bindings[option.Key] = binding
			m.MediaOptions = append(m.MediaOptions, option)
		}
	}
	var bound [4]string
	m.MediaUnavailable = [4]bool{}
	for i, kind := range viewmodel.MediaKinds {
		binding := value.Bindings[protocol.MediaKind(kind)]
		if binding == nil {
			continue
		}
		bound[i] = mediaKey(*binding)
		if _, offered := state.bindings[bound[i]]; !offered {
			// A saved model that is no longer offered stays its row's choice,
			// named by its provider where the Node still has it.
			label := binding.Model
			if provider, exists := r.provider(binding.Provider.String()); exists {
				label = provider.Name + " · " + binding.Model
			}
			state.bindings[bound[i]] = *binding
			m.MediaOptions = append(m.MediaOptions, viewmodel.MediaOption{Key: bound[i], Label: label})
		}
	}
	for _, kind := range value.Unavailable {
		for i, name := range viewmodel.MediaKinds {
			m.MediaUnavailable[i] = m.MediaUnavailable[i] || name == string(kind)
		}
	}
	field := &m.Media
	pristine := field.Draft == field.Saved
	field.Saved, field.Revision = bound, value.Revision
	if pristine {
		field.Draft = bound
	}
}

func mediaKey(binding protocol.MediaBinding) string {
	return modelKey(binding.Provider, binding.Model)
}

// saveMedia sends the rows' choices at the revision read. Failure, conflict
// and an unknown outcome are toasts and keep the choices as the draft; the
// assignments are then read again, so that the next choice saves at the
// Node's revision.
func (r *Runtime) saveMedia() {
	field := &r.App.Settings.Media
	page := r.currentSettingsPage(settings.Media)
	if page == nil || field.Pending {
		return
	}
	key := mediaSaveKey{revision: field.Revision}
	request := protocol.MediaSettings{Revision: field.Revision, Bindings: map[protocol.MediaKind]*protocol.MediaBinding{}}
	for i, kind := range viewmodel.MediaKinds {
		if binding, chosen := r.media.bindings[field.Draft[i]]; chosen {
			request.Bindings[protocol.MediaKind(kind)] = &binding
			key.bindings[i] = mediaKey(binding)
		}
	}
	field.Pending = true
	if r.media.saving == nil {
		r.media.saving = map[mediaSaveKey]func() bool{}
	}
	// A returning page can make the same choice at the same revision before
	// it sees the result. Attach its binding to the original write.
	_, pending := r.media.saving[key]
	r.media.saving[key] = page.target.Current
	if pending {
		return
	}
	r.saveSetting(protocol.Command{Kind: "save_media_settings", Data: request}, settings.MediaMessages, func(output protocol.Output, ok bool) {
		current := r.media.saving[key]
		delete(r.media.saving, key)
		if !current() {
			return
		}
		field.Pending = false
		value, valid := detail[protocol.MediaSettings](output.Data)
		if !ok || !valid || output.Kind != "media_settings" {
			r.media.seen = ""
			r.followMedia(r.snapshot())
			return
		}
		field.Draft = field.Saved
		if value.Revision >= field.Revision {
			r.projectMedia(value, nil, false)
		}
	})
}
