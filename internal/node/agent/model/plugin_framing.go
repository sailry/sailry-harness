package model

import (
	"bytes"
	"strings"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type pluginDecoder struct {
	framing protocol.PluginModelAdapterFraming
	pending []byte
	data    []string
	event   *string
	bytes   int
	limit   int
	first   bool
}

func (decoder *pluginDecoder) push(chunk []byte, done bool) ([]protocol.PluginModelAdapterWireEvent, error) {
	if len(chunk) > decoder.limit-decoder.bytes {
		return nil, pluginResourceFailure()
	}
	decoder.bytes += len(chunk)
	decoder.pending = append(decoder.pending, chunk...)
	if decoder.framing == protocol.PluginModelAdapterFramingJson {
		if !done {
			return []protocol.PluginModelAdapterWireEvent{}, nil
		}
		if !utf8.Valid(decoder.pending) {
			return nil, &Failure{Kind: InvalidRequest, Message: "model response is not UTF-8"}
		}
		data := string(decoder.pending)
		decoder.pending = nil
		return []protocol.PluginModelAdapterWireEvent{{Data: data}}, nil
	}
	events := []protocol.PluginModelAdapterWireEvent{}
	for {
		end := bytes.IndexAny(decoder.pending, "\r\n")
		if end < 0 {
			break
		}
		if decoder.pending[end] == '\r' && end+1 == len(decoder.pending) && !done {
			break
		}
		terminator := 1
		if decoder.pending[end] == '\r' && end+1 < len(decoder.pending) && decoder.pending[end+1] == '\n' {
			terminator = 2
		}
		if err := decoder.line(decoder.pending[:end], &events); err != nil {
			return nil, err
		}
		decoder.pending = decoder.pending[end+terminator:]
	}
	if done {
		if len(decoder.pending) != 0 {
			if err := decoder.line(decoder.pending, &events); err != nil {
				return nil, err
			}
			decoder.pending = nil
		}
		decoder.flush(&events)
	}
	return events, nil
}

func (decoder *pluginDecoder) line(raw []byte, events *[]protocol.PluginModelAdapterWireEvent) error {
	if !utf8.Valid(raw) {
		return &Failure{Kind: InvalidRequest, Message: "model response is not UTF-8"}
	}
	line := strings.TrimSuffix(string(raw), "\r")
	if decoder.first {
		line = strings.TrimPrefix(line, "\ufeff")
		decoder.first = false
	}
	if decoder.framing == protocol.PluginModelAdapterFramingNdjson {
		if strings.TrimSpace(line) != "" {
			*events = append(*events, protocol.PluginModelAdapterWireEvent{Data: line})
		}
	} else if line == "" {
		decoder.flush(events)
	} else if !strings.HasPrefix(line, ":") {
		field, value, _ := strings.Cut(line, ":")
		value = strings.TrimPrefix(value, " ")
		switch field {
		case "data":
			decoder.data = append(decoder.data, value)
		case "event":
			decoder.event = &value
		}
	}
	return nil
}

func (decoder *pluginDecoder) flush(events *[]protocol.PluginModelAdapterWireEvent) {
	if len(decoder.data) != 0 {
		*events = append(*events, protocol.PluginModelAdapterWireEvent{Event: decoder.event, Data: strings.Join(decoder.data, "\n")})
		decoder.data = nil
	}
	decoder.event = nil
}
