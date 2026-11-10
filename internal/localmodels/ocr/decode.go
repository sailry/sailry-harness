package ocr

import (
	"fmt"
	"math"
	"os"
	"strings"
	"unicode"
	"unicode/utf8"
)

// dictionary maps recognizer classes to text: the CTC blank, the entries of
// dictionary.txt in order, then a space.
type dictionary []string

func loadDictionary(path string) (dictionary, error) {
	info, err := os.Stat(path)
	if err != nil {
		return nil, fmt.Errorf("OCR storage failed: %w", err)
	}
	if info.Size() > 512*1024 {
		return nil, invalid("OCR dictionary is too large")
	}
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, fmt.Errorf("OCR storage failed: %w", err)
	}
	return parseDictionary(data)
}

func parseDictionary(data []byte) (dictionary, error) {
	failure := invalid("OCR dictionary is invalid")
	if len(data) == 0 || !utf8.Valid(data) {
		return nil, failure
	}
	// One entry per line; a final newline does not start an entry and a
	// carriage return before a newline belongs to the line ending.
	entries := strings.Split(strings.TrimSuffix(string(data), "\n"), "\n")
	if len(entries) > 65534 {
		return nil, failure
	}
	tokens := make(dictionary, 0, len(entries)+2)
	tokens = append(tokens, "")
	for _, entry := range entries {
		entry = strings.TrimSuffix(entry, "\r")
		if entry == "" || len(entry) > 64 || strings.ContainsFunc(entry, unicode.IsControl) {
			return nil, failure
		}
		tokens = append(tokens, entry)
	}
	return append(tokens, " "), nil
}

// decode is the greedy CTC decode of steps rows of class probabilities: the
// best class of each step, with repeats collapsed and blanks dropped. The
// confidence is the mean probability of the kept classes.
func (d dictionary) decode(values []float32, steps int) (string, float32, error) {
	if steps == 0 || len(values) != steps*len(d) {
		return "", 0, invalid("recognizer vocabulary does not match its dictionary")
	}
	var text strings.Builder
	var total float32
	count, previous := 0, -1
	for step := range steps {
		row := values[step*len(d) : (step+1)*len(d)]
		best := 0
		for index, value := range row {
			if math.IsNaN(float64(value)) || value < 0 || value > 1.001 {
				return "", 0, invalid("recognizer probabilities are invalid")
			}
			// The last of equal maxima wins.
			if value >= row[best] {
				best = index
			}
		}
		if best != 0 && best != previous {
			text.WriteString(d[best])
			total += row[best]
			count++
		}
		previous = best
	}
	if count == 0 {
		return "", 0, nil
	}
	return text.String(), total / float32(count), nil
}
