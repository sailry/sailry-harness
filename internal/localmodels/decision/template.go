package decision

import (
	"errors"
	"strings"
	"unicode/utf8"

	"github.com/nikolalohinski/gonja/v2/builtins"
	"github.com/nikolalohinski/gonja/v2/config"
	"github.com/nikolalohinski/gonja/v2/exec"
	"github.com/nikolalohinski/gonja/v2/loaders"
	"github.com/nikolalohinski/gonja/v2/nodes"
	"github.com/nikolalohinski/gonja/v2/parser"
)

const (
	// maxPrompt bounds the rendered prompt in bytes.
	maxPrompt = 256 * 1024
	// maxSteps bounds the work of one render: every write, executed control
	// structure and generated range item costs one step.
	maxSteps = 100_000
)

var errStopped = errors.New("render was cancelled")

// templates renders the systemone prompt of one model.
type templates struct {
	template *exec.Template
	source   string
}

// rendered is a prompt and the byte ranges that came from request values.
// Only the text between those ranges may spell control tokens.
type rendered struct {
	text     string
	literals [][2]int
}

// metered charges one step whenever a control structure executes.
type metered struct{ exec.ControlStructure }

func (m metered) Execute(renderer *exec.Renderer, block *nodes.ControlStructureBlock) error {
	if output, measured := renderer.Output.(*output); measured {
		if err := output.charge(1); err != nil {
			return err
		}
	}
	return m.ControlStructure.Execute(renderer, block)
}

// newTemplates parses a template in an environment without a file system,
// macros, inclusion, inheritance or output capture.
func newTemplates(source string) (*templates, error) {
	structures := map[string]parser.ControlStructureParser{}
	for _, name := range []string{"if", "for", "set", "with", "raw"} {
		parse, _ := builtins.ControlStructures.Get(name)
		structures[name] = func(p *parser.Parser, args *parser.Parser) (nodes.ControlStructure, error) {
			structure, err := parse(p, args)
			if executable, runs := structure.(exec.ControlStructure); err == nil && runs {
				return metered{executable}, nil
			}
			return structure, err
		}
	}
	globals := map[string]any{}
	for _, name := range []string{"namespace", "dict", "joiner", "cycler"} {
		globals[name], _ = builtins.GlobalFunctions.Get(name)
	}
	settings := config.New()
	settings.StrictUndefined = true
	environment := &exec.Environment{
		Context:           exec.NewContext(globals),
		Filters:           builtins.Filters,
		Tests:             builtins.Tests,
		ControlStructures: exec.NewControlStructureSet(structures),
		Methods:           builtins.Methods,
	}
	loader, err := loaders.NewMemoryLoader(map[string]string{"/systemone": source})
	if err != nil {
		return nil, invalid("unsupported systemone template")
	}
	template, err := exec.NewTemplate("/systemone", settings, loader, environment)
	if err != nil {
		return nil, invalid("unsupported systemone template")
	}
	return &templates{template: template, source: source}, nil
}

// output strips the value brackets while the template writes and enforces
// the prompt, step and cancellation bounds.
type output struct {
	open, shut string
	cancelled  func() bool
	text       []byte
	literals   [][2]int
	depth      int
	steps      int
	failure    error
}

func (o *output) charge(steps int) error {
	if o.failure == nil {
		if o.steps += steps; steps < 0 || o.steps > maxSteps {
			o.failure = errors.New("systemone template exceeds its step limit")
		} else if o.cancelled() {
			o.failure = errStopped
		}
	}
	return o.failure
}

func (o *output) fail(message string) error {
	if o.failure == nil {
		o.failure = errors.New(message)
	}
	return o.failure
}

func (o *output) emit(data string) error {
	if len(data) > maxPrompt-len(o.text) {
		return o.fail("systemone prompt exceeds its limit")
	}
	start := len(o.text)
	o.text = append(o.text, data...)
	if data == "" || o.depth == 0 {
		return nil
	}
	if last := len(o.literals) - 1; last >= 0 && o.literals[last][1] == start {
		o.literals[last][1] = len(o.text)
	} else {
		o.literals = append(o.literals, [2]int{start, len(o.text)})
	}
	return nil
}

func (o *output) Write(data []byte) (int, error) {
	if err := o.charge(1); err != nil {
		return 0, err
	}
	rest := string(data)
	for rest != "" {
		opened, closed := strings.Index(rest, o.open), strings.Index(rest, o.shut)
		next, opening := opened, true
		if opened < 0 || closed >= 0 && closed < opened {
			next, opening = closed, false
		}
		if next < 0 {
			next = len(rest)
		}
		if err := o.emit(rest[:next]); err != nil {
			return 0, err
		}
		if next == len(rest) {
			break
		}
		if opening {
			o.depth++
			rest = rest[next+len(o.open):]
			continue
		}
		if o.depth--; o.depth < 0 {
			return 0, o.fail("systemone value is unbalanced")
		}
		rest = rest[next+len(o.shut):]
	}
	return len(data), nil
}

// brackets returns two private-use characters that occur in none of the
// texts, so no request text is ever rejected or mistaken for a bracket.
func brackets(texts ...string) (string, string) {
	used := map[rune]bool{}
	for _, text := range texts {
		for _, character := range text {
			if character >= 0xE000 && character <= 0xF8FF || character >= 0xF0000 {
				used[character] = true
			}
		}
	}
	var free []string
	for character := rune(0xE000); len(free) < 2; character++ {
		if character == 0xF900 {
			character = 0xF0000
		}
		if !used[character] {
			free = append(free, string(character))
		}
	}
	return free[0], free[1]
}

// render evaluates the template for one question. Every request value is
// bracketed while it passes through the template and reported as a literal
// range afterwards.
func (t *templates) render(state string, query *query, stopped func() bool) (*rendered, error) {
	texts := []string{t.source, state, query.name, query.kind, query.instructions}
	for _, option := range query.options {
		texts = append(texts, option.key)
		if option.description != nil {
			texts = append(texts, *option.description)
		}
	}
	// The plane 15 fallback is three times larger than all bounded inputs.
	open, shut := brackets(texts...)
	wrap := func(value string) string {
		if value == "" {
			return value
		}
		return open + value + shut
	}
	options := make([]map[string]any, len(query.options))
	for index, option := range query.options {
		options[index] = map[string]any{"key": wrap(option.key), "description": nil}
		if option.description != nil {
			options[index]["description"] = wrap(*option.description)
		}
	}
	writer := &output{open: open, shut: shut, cancelled: stopped}
	err := t.template.Execute(writer, exec.NewContext(map[string]any{
		"id": wrap(query.name), "type": wrap(query.kind), "instructions": wrap(query.instructions),
		"state": wrap(state), "options": options, "images": []any{},
		// The stock range feeds a channel from a goroutine that an
		// abandoned loop would leak; this one is bounded and synchronous.
		"range": func(_ *exec.Evaluator, arguments *exec.VarArgs) ([]int, error) {
			return sequence(writer, arguments)
		},
	}))
	if writer.failure == errStopped {
		return nil, cancelled()
	}
	if err != nil || writer.failure != nil || writer.depth != 0 {
		return nil, invalid("systemone template could not be rendered")
	}
	if !utf8.Valid(writer.text) {
		return nil, invalid("systemone prompt is not UTF-8")
	}
	return &rendered{text: string(writer.text), literals: writer.literals}, nil
}

// sequence is range(stop), range(start, stop) or range(start, stop, step).
func sequence(writer *output, arguments *exec.VarArgs) ([]int, error) {
	bounds := []int{0, 0, 1}
	for index, argument := range arguments.Args {
		if index > 2 || !argument.IsInteger() {
			return nil, errors.New("range expects integers")
		}
		if len(arguments.Args) == 1 {
			index = 1
		}
		bounds[index] = argument.Integer()
	}
	start, stop, step := bounds[0], bounds[1], bounds[2]
	if len(arguments.Args) == 0 || len(arguments.KwArgs) != 0 || step == 0 {
		return nil, errors.New("range expects integers")
	}
	count := 0
	if step > 0 && start < stop {
		count = (stop - start + step - 1) / step
	} else if step < 0 && start > stop {
		count = (start - stop - step - 1) / -step
	}
	if count < 0 || count > maxSteps {
		return nil, writer.fail("systemone template exceeds its step limit")
	}
	if err := writer.charge(count); err != nil {
		return nil, err
	}
	values := make([]int, count)
	for index := range values {
		values[index] = start + index*step
	}
	return values, nil
}

// space is C isspace in the "C" locale.
func space(character byte) bool {
	return character == ' ' || character >= '\t' && character <= '\r'
}

// strip widens a control token over adjacent whitespace as the LSTRIP and
// RSTRIP attributes of the pinned tokenizer do.
func (r *rendered) strip(start, end int, left, right bool) (int, int) {
	for left && start > 0 && space(r.text[start-1]) {
		start--
	}
	for right && end < len(r.text) && space(r.text[end]) {
		end++
	}
	return start, end
}

// trusted returns the ranges written by the template itself.
func (r *rendered) trusted() [][2]int {
	var ranges [][2]int
	start := 0
	for _, literal := range r.literals {
		if start < literal[0] {
			ranges = append(ranges, [2]int{start, literal[0]})
		}
		start = literal[1]
	}
	if start < len(r.text) {
		ranges = append(ranges, [2]int{start, len(r.text)})
	}
	return ranges
}
