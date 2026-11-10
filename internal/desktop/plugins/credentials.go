package plugins

import (
	"context"
	"encoding/json"
	"strconv"
	"unicode/utf8"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type secretDraft struct {
	value       string
	label       string
	placeholder string
	multiline   bool
	imported    string
	picking     bool
	version     uint64
	loading     uint64
	focused     bool
}

func (binding *NativeBinding) credentialOperation(ctx context.Context, name string, arguments []json.RawMessage) (json.RawMessage, error) {
	if name == "nextSecretEvent" {
		return binding.next(ctx, "secret")
	}
	if name == "chooseSecretFile" {
		return binding.chooseSecret(ctx, arguments)
	}
	binding.state.mu.Lock()
	defer binding.state.mu.Unlock()
	if name == "createSecret" {
		options, err := hostValue[map[string]json.RawMessage](arguments, 0)
		if err != nil {
			return nil, err
		}
		if len(binding.state.secrets) >= 16 {
			return nil, hostInvalid("credential draft capacity exhausted")
		}
		binding.state.sequence++
		identity := "secret-" + strconv.FormatUint(binding.state.sequence, 10)
		binding.state.secrets[identity] = &secretDraft{
			label: nativeProp(options, "label", ""), placeholder: nativeProp(options, "placeholder", ""),
			multiline: nativeProp(options, "multiline", false),
		}
		return hostJSON(identity)
	}
	identity, err := hostValue[string](arguments, 0)
	if err != nil {
		return nil, err
	}
	draft := binding.state.secrets[identity]
	if name == "releaseSecret" {
		if draft != nil {
			draft.value = ""
		}
		delete(binding.state.secrets, identity)
		return json.RawMessage("null"), nil
	}
	if draft == nil {
		return nil, hostInvalid("credential draft is unavailable")
	}
	switch name {
	case "describeSecret":
		var filename *string
		if draft.imported != "" {
			filename = &draft.imported
		}
		return hostJSON(map[string]any{"filled": draft.value != "", "file": filename, "picking": draft.picking, "version": draft.version})
	case "clearSecret":
		draft.value, draft.imported = "", ""
		draft.version++
		if binding.Invalidate != nil {
			binding.Invalidate()
		}
		return json.RawMessage("null"), nil
	}
	return nil, hostUnavailable("credential operation is unavailable")
}

func (binding *NativeBinding) secret(identity string) (protocol.Secret, error) {
	binding.state.mu.Lock()
	defer binding.state.mu.Unlock()
	draft := binding.state.secrets[identity]
	if draft == nil {
		return protocol.Secret{}, hostInvalid("credential draft is unavailable")
	}
	if draft.picking {
		return protocol.Secret{}, hostInvalid("credential file selection is pending")
	}
	return protocol.NewSecret(draft.value), nil
}

func (binding *NativeBinding) update(ctx context.Context, apply func() error) error {
	if binding.Post == nil {
		return hostUnavailable("native view scheduling is unavailable")
	}
	reply := make(chan error, 1)
	if !binding.Post(func(*ui.Context) {
		if binding.state.ctx.Err() != nil {
			reply <- binding.state.ctx.Err()
			return
		}
		binding.state.mu.Lock()
		err := apply()
		binding.state.mu.Unlock()
		if err == nil && binding.Invalidate != nil {
			binding.Invalidate()
		}
		reply <- err
	}) {
		return hostUnavailable("native view scheduling is unavailable")
	}
	select {
	case <-ctx.Done():
		return ctx.Err()
	case <-binding.state.ctx.Done():
		return binding.state.ctx.Err()
	case err := <-reply:
		return err
	}
}

func (binding *NativeBinding) chooseSecret(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
	identity, err := hostValue[string](arguments, 0)
	if err != nil {
		return nil, err
	}
	prompt, err := hostValue[string](arguments, 1)
	if err != nil {
		return nil, err
	}
	if binding.ChooseSecretFile == nil {
		return nil, hostUnavailable("credential file selection is unavailable")
	}
	err = binding.update(ctx, func() error {
		draft := binding.state.secrets[identity]
		if draft == nil {
			return hostInvalid("credential draft is unavailable")
		}
		if draft.picking {
			return hostInvalid("credential file selection is pending")
		}
		draft.picking = true
		return nil
	})
	if err != nil {
		return nil, err
	}
	content, filename, selectedErr := binding.ChooseSecretFile(ctx, prompt)
	if selectedErr == nil && content != nil && (!utf8.Valid(content) || len(content) == 0 || len(content) > 65536) {
		selectedErr = hostInvalid("credential file must contain between 1 and 65536 UTF-8 bytes")
	}
	err = binding.update(ctx, func() error {
		draft := binding.state.secrets[identity]
		if draft == nil {
			return hostInvalid("credential draft is unavailable")
		}
		draft.picking = false
		if selectedErr == nil && content != nil {
			draft.value, draft.imported = string(content), filename
		}
		return selectedErr
	})
	clear(content)
	if err != nil {
		return nil, err
	}
	if content == nil {
		return json.RawMessage("null"), nil
	}
	return hostJSON(filename)
}

func (binding *NativeBinding) renderSecret(view *ui.Context, element NativeElement, _ func(), _ func(CallbackRef, json.RawMessage)) (ui.Element, error) {
	identity, props, err := nativeProps(element)
	if err != nil {
		return ui.Element{}, err
	}
	binding.state.mu.Lock()
	draft := binding.state.secrets[identity]
	binding.state.mu.Unlock()
	if draft == nil {
		return ui.Box(view), nil
	}
	var input ui.Element
	if draft.multiline {
		input = ui.TextArea(view.Key(identity), &draft.value).MinHeight(78).MaxHeight(144)
	} else {
		input = ui.TextInput(view.Key(identity), &draft.value).Password()
	}
	input.Label(draft.label).Placeholder(draft.placeholder).FillWidth().Disabled(draft.picking || nativeProp(props, "disabled", false))
	send := func(kind string) {
		binding.emit("secret", map[string]any{"id": identity, "kind": kind, "version": draft.version, "filled": draft.value != ""})
	}
	input.OnChange(func() {
		draft.imported = ""
		draft.version++
		send("change")
	}).OnSubmit(func() { send("enter") })
	if focused := input.Focused(); focused != draft.focused {
		draft.focused = focused
		if focused {
			send("focus")
		} else {
			send("blur")
		}
	}
	return input, nil
}
