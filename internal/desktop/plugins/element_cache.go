package plugins

import (
	"encoding/json"
	"encoding/json/jsontext"
	jsonv2 "encoding/json/v2"
	"fmt"
	"strconv"
)

// elementCache is what rendering parses from an element's props and styles.
// A snapshot's elements keep their props until the script renders a new
// snapshot, which decodes new elements, so the parse lasts as long as the
// element. Parsing on every frame allocated the whole tree's props again
// while a page sat idle. Callers share the cached values and must not
// modify them.
type elementCache struct {
	properties map[string]any
	widgetKey  string
	native     *nativeParse
	// chart is the chart source derived from the props, by the chart kind.
	chart any
}

type nativeParse struct {
	identity string
	props    map[string]json.RawMessage
	err      error
}

// cached is the element's cache, made on first use. Components receive the
// element by value, so the cache is made before they are called and the
// copies share it.
func (element *NativeElement) cached() *elementCache {
	if element.cache == nil {
		element.cache = &elementCache{}
	}
	return element.cache
}

// nativeElementProps is nativeProps for an element of the tree, which keeps
// the parse.
func nativeElementProps(element *NativeElement) (string, map[string]json.RawMessage, error) {
	element.cached()
	return nativeProps(*element)
}

// runtimeProperties are the element's props with its styles over them.
func runtimeProperties(element *NativeElement) map[string]any {
	cache := element.cached()
	if cache.properties == nil {
		cache.properties = parseRuntimeProperties(element)
	}
	return cache.properties
}

func parseRuntimeProperties(element *NativeElement) map[string]any {
	properties := map[string]any{}
	_ = json.Unmarshal(element.Props, &properties)
	if properties == nil {
		properties = map[string]any{}
	}
	for _, style := range element.Styles {
		if len(style.Arguments) == 1 {
			var value any
			if json.Unmarshal(style.Arguments[0], &value) == nil {
				properties[style.Name] = value
			}
		} else if len(style.Arguments) == 0 {
			properties[style.Name] = true
		}
	}
	return properties
}

func (mounted *Mounted) widgetKey(element *NativeElement) string {
	cache := element.cached()
	if cache.widgetKey == "" {
		cache.widgetKey = element.Module + "/" + element.Export + "/" + widgetIdentity(element)
	}
	return cache.widgetKey
}

// widgetIdentity reads only the props that name a widget: its id, motion
// id, core state or position, else the element's ID.
func widgetIdentity(element *NativeElement) string {
	var identity struct {
		ID       any `json:"id"`
		MotionID any `json:"motion_id"`
		State    any `json:"state"`
		Ordinal  any `json:"__ordinal"`
	}
	// Names match exactly, as the props map is read; the options keep the
	// map decoding's tolerance of repeated names and invalid text.
	_ = jsonv2.Unmarshal(element.Props, &identity, jsontext.AllowDuplicateNames(true), jsontext.AllowInvalidUTF8(true))
	// A style of the same name replaces the prop, as in runtimeProperties.
	for _, style := range element.Styles {
		var target *any
		switch style.Name {
		case "id":
			target = &identity.ID
		case "motion_id":
			target = &identity.MotionID
		case "state":
			target = &identity.State
		case "__ordinal":
			target = &identity.Ordinal
		default:
			continue
		}
		if len(style.Arguments) == 1 {
			var value any
			if json.Unmarshal(style.Arguments[0], &value) == nil {
				*target = value
			}
		} else if len(style.Arguments) == 0 {
			*target = true
		}
	}
	if text := runtimeString(identity.ID); text != "" {
		return text
	}
	if text := runtimeString(identity.MotionID); text != "" {
		return text
	}
	if state, ok := identity.State.(map[string]any); ok && runtimeFloat(state["__coreState"]) > 0 {
		return fmt.Sprintf("state:%v", state["__coreState"])
	}
	if ordinal := runtimeFloat(identity.Ordinal); ordinal > 0 {
		return fmt.Sprintf("position:%v", ordinal)
	}
	return strconv.FormatUint(element.ID, 10)
}
