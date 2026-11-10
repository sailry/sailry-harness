package plugins

import (
	"encoding/json"
	"strconv"
	"time"

	"github.com/egoist/mygo/ui"
)

type dateDraft struct {
	original  int64
	label     string
	date      time.Time
	timestamp time.Time
	mounted   bool
}

func (binding *NativeBinding) dateOperation(name string, arguments []json.RawMessage) (json.RawMessage, error) {
	binding.state.mu.Lock()
	defer binding.state.mu.Unlock()
	if name == "createDateTime" {
		value, err := hostValue[int64](arguments, 0)
		if err != nil || value < -9_007_199_254_740_991 || value > 9_007_199_254_740_991 {
			return nil, hostInvalid("invalid local date or time")
		}
		at := time.UnixMilli(value).In(time.Local)
		if at.Year() < -262_143 || at.Year() > 262_142 {
			return nil, hostInvalid("invalid local date or time")
		}
		label, err := hostValue[string](arguments, 1)
		if err != nil {
			return nil, err
		}
		if len(binding.state.dates) >= 64 {
			return nil, hostInvalid("plugin field capacity exhausted")
		}
		binding.state.sequence++
		identity := "date-" + strconv.FormatUint(binding.state.sequence, 10)
		binding.state.dates[identity] = &dateDraft{original: value, label: label, date: at, timestamp: at}
		return hostJSON(identity)
	}
	identity, err := hostValue[string](arguments, 0)
	if err != nil {
		return nil, err
	}
	if name == "releaseDateTime" {
		delete(binding.state.dates, identity)
		return json.RawMessage("null"), nil
	}
	draft := binding.state.dates[identity]
	if draft == nil {
		return nil, hostInvalid("plugin field is unavailable")
	}
	if !draft.mounted {
		return hostJSON(draft.original)
	}
	if draft.date.IsZero() || draft.timestamp.IsZero() {
		return nil, hostInvalid("invalid local date or time")
	}
	year, month, day := draft.date.Date()
	hour, minute, second := draft.timestamp.Clock()
	wall := time.Date(year, month, day, hour, minute, second, 0, time.UTC)
	possible := map[int64]bool{}
	for _, observed := range []time.Time{wall.Add(-48 * time.Hour), wall, wall.Add(48 * time.Hour)} {
		_, offset := observed.In(time.Local).Zone()
		at := wall.Add(-time.Duration(offset) * time.Second).In(time.Local)
		currentYear, currentMonth, currentDay := at.Date()
		currentHour, currentMinute, currentSecond := at.Clock()
		if currentYear == year && currentMonth == month && currentDay == day && currentHour == hour && currentMinute == minute && currentSecond == second {
			possible[at.UnixMilli()] = true
		}
	}
	if len(possible) != 1 {
		return nil, hostInvalid("invalid local date or time")
	}
	for at := range possible {
		if time.UnixMilli(draft.original).Truncate(time.Second).UnixMilli() == at {
			at = draft.original
		}
		return hostJSON(at)
	}
	return nil, hostInvalid("invalid local date or time")
}

func (binding *NativeBinding) renderDateTime(view *ui.Context, element NativeElement, _ func(), _ func(CallbackRef, json.RawMessage)) (ui.Element, error) {
	identity, props, err := nativeProps(element)
	if err != nil {
		return ui.Element{}, err
	}
	binding.state.mu.Lock()
	draft := binding.state.dates[identity]
	binding.state.mu.Unlock()
	if draft == nil {
		return ui.Box(view), nil
	}
	draft.mounted = true
	disabled := nativeProp(props, "disabled", false)
	row := ui.Row(view.Key(identity)).FillWidth().Gap(8)
	row.Children(func() {
		ui.DateInput(view.Key("date"), &draft.date).Grow(1).MinWidth(0).Disabled(disabled)
		ui.TimeInput(view.Key("time"), &draft.timestamp).Label(draft.label).Width(128).Disabled(disabled)
	})
	return row, nil
}
