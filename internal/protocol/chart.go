// Hand-maintained v1 protocol declarations; keep them consistent with the fixtures and tolerance tests.

package protocol

import (
	"encoding/json"
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
