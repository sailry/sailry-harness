// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"bytes"
	"encoding/json"
	"fmt"
)

type ChartPoint struct {
	Label string  `json:"label"`
	Value float64 `json:"value"`
}

func DefaultChartPoint() ChartPoint {
	return ChartPoint{
		Label: "",
		Value: 0,
	}
}
func (value *ChartPoint) UnmarshalJSON(data []byte) error {
	type plain ChartPoint
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"label", "value"}, true); err != nil {
		return err
	}
	*value = ChartPoint(decoded)
	return nil
}
func (value ChartPoint) MarshalJSON() ([]byte, error) {
	type plain ChartPoint
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ChartCandle struct {
	Label string  `json:"label"`
	Open  float64 `json:"open"`
	High  float64 `json:"high"`
	Low   float64 `json:"low"`
	Close float64 `json:"close"`
}

func DefaultChartCandle() ChartCandle {
	return ChartCandle{
		Label: "",
		Open:  0,
		High:  0,
		Low:   0,
		Close: 0,
	}
}
func (value *ChartCandle) UnmarshalJSON(data []byte) error {
	type plain ChartCandle
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"label", "open", "high", "low", "close"}, true); err != nil {
		return err
	}
	*value = ChartCandle(decoded)
	return nil
}
func (value ChartCandle) MarshalJSON() ([]byte, error) {
	type plain ChartCandle
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ChartNode struct {
	ID    string `json:"id"`
	Label string `json:"label"`
}

func DefaultChartNode() ChartNode {
	return ChartNode{
		ID:    "",
		Label: "",
	}
}
func (value *ChartNode) UnmarshalJSON(data []byte) error {
	type plain ChartNode
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"id", "label"}, true); err != nil {
		return err
	}
	*value = ChartNode(decoded)
	return nil
}
func (value ChartNode) MarshalJSON() ([]byte, error) {
	type plain ChartNode
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ChartLink struct {
	Source string  `json:"source"`
	Target string  `json:"target"`
	Value  float64 `json:"value"`
}

func DefaultChartLink() ChartLink {
	return ChartLink{
		Source: "",
		Target: "",
		Value:  0,
	}
}
func (value *ChartLink) UnmarshalJSON(data []byte) error {
	type plain ChartLink
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"source", "target", "value"}, true); err != nil {
		return err
	}
	*value = ChartLink(decoded)
	return nil
}
func (value ChartLink) MarshalJSON() ([]byte, error) {
	type plain ChartLink
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ChartData struct {
	Kind string
	Data any
}
type ChartDataLinePayload struct {
	Data []ChartPoint `json:"data"`
}

func DefaultChartDataLinePayload() ChartDataLinePayload {
	return ChartDataLinePayload{
		Data: []ChartPoint{},
	}
}
func (value *ChartDataLinePayload) UnmarshalJSON(data []byte) error {
	type plain ChartDataLinePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"data"}, true); err != nil {
		return err
	}
	*value = ChartDataLinePayload(decoded)
	return nil
}
func (value ChartDataLinePayload) MarshalJSON() ([]byte, error) {
	type plain ChartDataLinePayload
	encoded := plain(value)
	encoded.Data = normalizeCollections(encoded.Data)
	return json.Marshal(encoded)
}

type ChartDataBarPayload struct {
	Data []ChartPoint `json:"data"`
}

func DefaultChartDataBarPayload() ChartDataBarPayload {
	return ChartDataBarPayload{
		Data: []ChartPoint{},
	}
}
func (value *ChartDataBarPayload) UnmarshalJSON(data []byte) error {
	type plain ChartDataBarPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"data"}, true); err != nil {
		return err
	}
	*value = ChartDataBarPayload(decoded)
	return nil
}
func (value ChartDataBarPayload) MarshalJSON() ([]byte, error) {
	type plain ChartDataBarPayload
	encoded := plain(value)
	encoded.Data = normalizeCollections(encoded.Data)
	return json.Marshal(encoded)
}

type ChartDataAreaPayload struct {
	Data []ChartPoint `json:"data"`
}

func DefaultChartDataAreaPayload() ChartDataAreaPayload {
	return ChartDataAreaPayload{
		Data: []ChartPoint{},
	}
}
func (value *ChartDataAreaPayload) UnmarshalJSON(data []byte) error {
	type plain ChartDataAreaPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"data"}, true); err != nil {
		return err
	}
	*value = ChartDataAreaPayload(decoded)
	return nil
}
func (value ChartDataAreaPayload) MarshalJSON() ([]byte, error) {
	type plain ChartDataAreaPayload
	encoded := plain(value)
	encoded.Data = normalizeCollections(encoded.Data)
	return json.Marshal(encoded)
}

type ChartDataPiePayload struct {
	Data []ChartPoint `json:"data"`
}

func DefaultChartDataPiePayload() ChartDataPiePayload {
	return ChartDataPiePayload{
		Data: []ChartPoint{},
	}
}
func (value *ChartDataPiePayload) UnmarshalJSON(data []byte) error {
	type plain ChartDataPiePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"data"}, true); err != nil {
		return err
	}
	*value = ChartDataPiePayload(decoded)
	return nil
}
func (value ChartDataPiePayload) MarshalJSON() ([]byte, error) {
	type plain ChartDataPiePayload
	encoded := plain(value)
	encoded.Data = normalizeCollections(encoded.Data)
	return json.Marshal(encoded)
}

type ChartDataRadarPayload struct {
	Data []ChartPoint `json:"data"`
}

func DefaultChartDataRadarPayload() ChartDataRadarPayload {
	return ChartDataRadarPayload{
		Data: []ChartPoint{},
	}
}
func (value *ChartDataRadarPayload) UnmarshalJSON(data []byte) error {
	type plain ChartDataRadarPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"data"}, true); err != nil {
		return err
	}
	*value = ChartDataRadarPayload(decoded)
	return nil
}
func (value ChartDataRadarPayload) MarshalJSON() ([]byte, error) {
	type plain ChartDataRadarPayload
	encoded := plain(value)
	encoded.Data = normalizeCollections(encoded.Data)
	return json.Marshal(encoded)
}

type ChartDataCandlestickPayload struct {
	Data []ChartCandle `json:"data"`
}

func DefaultChartDataCandlestickPayload() ChartDataCandlestickPayload {
	return ChartDataCandlestickPayload{
		Data: []ChartCandle{},
	}
}
func (value *ChartDataCandlestickPayload) UnmarshalJSON(data []byte) error {
	type plain ChartDataCandlestickPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"data"}, true); err != nil {
		return err
	}
	*value = ChartDataCandlestickPayload(decoded)
	return nil
}
func (value ChartDataCandlestickPayload) MarshalJSON() ([]byte, error) {
	type plain ChartDataCandlestickPayload
	encoded := plain(value)
	encoded.Data = normalizeCollections(encoded.Data)
	return json.Marshal(encoded)
}

type ChartDataSankeyPayload struct {
	Nodes []ChartNode `json:"nodes"`
	Links []ChartLink `json:"links"`
}

func DefaultChartDataSankeyPayload() ChartDataSankeyPayload {
	return ChartDataSankeyPayload{
		Nodes: []ChartNode{},
		Links: []ChartLink{},
	}
}
func (value *ChartDataSankeyPayload) UnmarshalJSON(data []byte) error {
	type plain ChartDataSankeyPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"nodes", "links"}, true); err != nil {
		return err
	}
	*value = ChartDataSankeyPayload(decoded)
	return nil
}
func (value ChartDataSankeyPayload) MarshalJSON() ([]byte, error) {
	type plain ChartDataSankeyPayload
	encoded := plain(value)
	encoded.Nodes = normalizeCollections(encoded.Nodes)
	encoded.Links = normalizeCollections(encoded.Links)
	return json.Marshal(encoded)
}

var chartDataVariants = variantFactories{"line": func() any { return new(ChartDataLinePayload) },
	"bar":         func() any { return new(ChartDataBarPayload) },
	"area":        func() any { return new(ChartDataAreaPayload) },
	"pie":         func() any { return new(ChartDataPiePayload) },
	"radar":       func() any { return new(ChartDataRadarPayload) },
	"candlestick": func() any { return new(ChartDataCandlestickPayload) },
	"sankey":      func() any { return new(ChartDataSankeyPayload) }}

func (value ChartData) MarshalJSON() ([]byte, error) {
	return MarshalUnion("type", "", value.Kind, value.Data, chartDataVariants)
}
func (value *ChartData) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "type", "", chartDataVariants, true)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type Chart struct {
	Title string    `json:"title"`
	Data  ChartData `json:"-"`
}

func DefaultChart() Chart {
	return Chart{
		Title: "",
		Data:  ChartData{},
	}
}
func (value *Chart) UnmarshalJSON(data []byte) error {
	type plain Chart
	decoded := plain{}
	var fields map[string]json.RawMessage
	if err := json.Unmarshal(data, &fields); err != nil {
		return err
	}
	if fields == nil {
		return fmt.Errorf("expected chart object")
	}
	if title, ok := fields["title"]; ok {
		if bytes.Equal(bytes.TrimSpace(title), []byte("null")) {
			return fmt.Errorf("field %q must not be null", "title")
		}
		if err := json.Unmarshal(title, &decoded.Title); err != nil {
			return err
		}
		delete(fields, "title")
	}
	remaining, err := json.Marshal(fields)
	if err != nil {
		return err
	}
	if err := json.Unmarshal(remaining, &decoded.Data); err != nil {
		return err
	}
	*value = Chart(decoded)
	return nil
}
func (value Chart) MarshalJSON() ([]byte, error) {
	data, err := json.Marshal(value.Data)
	if err != nil {
		return nil, err
	}
	var fields map[string]json.RawMessage
	if err := json.Unmarshal(data, &fields); err != nil {
		return nil, err
	}
	title, err := json.Marshal(value.Title)
	if err != nil {
		return nil, err
	}
	fields["title"] = title
	return json.Marshal(fields)
}
