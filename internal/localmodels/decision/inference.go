package decision

import (
	"math"
	"slices"
	"strconv"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// option is one answer a question offers, in the order the model scores it.
type option struct {
	key         string
	description *string
}

// query is one typed question prepared for a model layout.
type query struct {
	name         string
	kind         string
	instructions string
	levels       []string
	options      []option
}

func newQuery(name string, question protocol.DecisionQuestion, algorithm Algorithm) *query {
	instructions, binary, choices, levels, _ := content(question)
	result := &query{name: name, kind: question.Kind, instructions: instructions, levels: levels}
	switch question.Kind {
	case "noul":
		for _, key := range []string{"false", "true"} {
			item := option{key: key}
			if description, present := binary[key]; present {
				item.description = &description
			}
			result.options = append(result.options, item)
		}
		if algorithm == OpenJev {
			slices.Reverse(result.options)
		}
	case "choice":
		keys := make([]string, 0, len(choices))
		for key := range choices {
			keys = append(keys, key)
		}
		slices.Sort(keys)
		for _, key := range keys {
			result.options = append(result.options, option{key: key, description: choices[key]})
		}
	case "score":
		for index, description := range levels {
			result.options = append(result.options, option{key: strconv.Itoa(index), description: &description})
		}
	}
	return result
}

// column is the laya head output trained for this question type.
func (q *query) column() int {
	switch q.kind {
	case "choice":
		return 0
	case "score":
		return 1
	}
	return 2
}

// answer calibrates one raw score per option into the typed answer.
func (q *query) answer(metadata *Metadata, scores []float32) (protocol.DecisionAnswer, error) {
	count := len(q.options)
	if len(scores) != count || slices.ContainsFunc(scores, func(value float32) bool { return !finite(value) }) {
		return protocol.DecisionAnswer{}, native("decision scores are invalid")
	}
	if count == 0 {
		return protocol.DecisionAnswer{}, native("decision has no options")
	}
	bucket := "11"
	switch {
	case count <= 2:
		bucket = "2"
	case count <= 5:
		bucket = "3_5"
	case count <= 10:
		bucket = "6_10"
	}
	temperature, calibrated := metadata.temperatures[q.kind+"."+bucket]
	if !calibrated {
		if temperature, calibrated = metadata.temperatures[q.kind]; !calibrated {
			temperature = 1
		}
	}
	var bias []float32
	if q.kind == "score" {
		bias = metadata.bias[count]
	}
	probabilities := make([]float64, count)
	maximum := math.Inf(-1)
	for index, score := range scores {
		probabilities[index] = float64(score)
		if bias != nil {
			probabilities[index] += float64(bias[index])
		}
		maximum = max(maximum, probabilities[index])
	}
	sum := 0.0
	for index, logit := range probabilities {
		probabilities[index] = math.Exp((logit - maximum) / float64(temperature))
		sum += probabilities[index]
	}
	best := 0
	for index := range probabilities {
		probabilities[index] /= sum
		if probabilities[index] > probabilities[best] {
			best = index
		}
	}
	switch q.kind {
	case "noul":
		for index, item := range q.options {
			if item.key == "true" {
				return protocol.DecisionAnswer{Kind: "noul", Data: protocol.DecisionAnswerNoulPayload{
					Noul: float32(probabilities[index]),
				}}, nil
			}
		}
		return protocol.DecisionAnswer{}, native("binary option is missing")
	case "choice":
		confidence := 1.0
		if count >= 2 {
			uniform := 1 / float64(count)
			confidence = max((probabilities[best]-uniform)/(1-uniform), 0)
		}
		mapped := make(map[string]float32, count)
		for index, item := range q.options {
			mapped[item.key] = float32(probabilities[index])
		}
		return protocol.DecisionAnswer{Kind: "choice", Data: protocol.DecisionAnswerChoicePayload{
			Choice: q.options[best].key, Probabilities: mapped, Confidence: float32(confidence),
		}}, nil
	}
	score, distance, uniform := 0.0, 0.0, 0.0
	levels := make([]float32, count)
	for index, probability := range probabilities {
		score += float64(index) * probability
		distance += probability * math.Abs(float64(index-best))
		uniform += math.Abs(float64(index)-float64(count-1)/2) / float64(count)
		levels[index] = float32(probability)
	}
	return protocol.DecisionAnswer{Kind: "score", Data: protocol.DecisionAnswerScorePayload{
		Score: float32(score), Legend: q.levels, Probabilities: levels, Confidence: float32(max(1-distance/uniform, 0)),
	}}, nil
}

// laya rebuilds a prompt within the trained head budgets and returns the
// position of each option marker. The state is never truncated to fit.
func laya(tokens []int32, count int, marker, separator int32, head int) ([]int32, []int, error) {
	var markers []int
	for index, token := range tokens {
		if token == marker {
			markers = append(markers, index)
		}
	}
	if count == 0 || len(markers) != count || markers[0] < 2 || tokens[markers[0]-1] != separator ||
		tokens[len(tokens)-1] != separator {
		return nil, nil, invalid("unexpected laya prompt layout")
	}
	headEnd := markers[0] - 1
	end := markers[count-1] + slices.Index(tokens[markers[count-1]:], separator)
	if end+1 >= len(tokens) {
		return nil, nil, invalid("laya state is missing")
	}
	options := make([][]int32, count)
	optionTokens := 0
	for index, start := range markers {
		stop := end
		if index+1 < count {
			stop = markers[index+1]
		}
		options[index] = tokens[start:min(stop, start+49)]
		optionTokens += len(options[index])
	}
	if optionTokens+16 > head {
		limit := max((head-min(head, 16))/count, 4)
		optionTokens = 0
		for index := range options {
			options[index] = options[index][:min(len(options[index]), limit)]
			optionTokens += len(options[index])
		}
	}
	question := max(head-min(head, optionTokens), 8)
	output := make([]int32, 0, len(tokens))
	output = append(output, tokens[0])
	output = append(output, tokens[1:min(headEnd, 1+question)]...)
	output = append(output, separator)
	positions := make([]int, 0, count)
	for _, item := range options {
		positions = append(positions, len(output))
		output = append(output, item...)
	}
	return append(output, tokens[end:]...), positions, nil
}
