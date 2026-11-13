package plugins

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"image/png"
	"math"
	"slices"
	"strings"

	"github.com/MoeForge/moego/base64"
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

func (mounted *Mounted) Render(context *ui.Context) (ui.Element, error) {
	if mounted.suspended && !mounted.closed {
		if mounted.last == nil {
			return ui.Box(context), nil
		}
		element, err := mounted.renderElement(context, mounted.last)
		if err == nil && element != (ui.Element{}) {
			element.Disabled(true).DimDisabled(false)
		}
		return element, err
	}
	if err := mounted.pump(false); err != nil {
		return ui.Element{}, err
	}
	bounds := context.Root().Bounds()
	if bounds.W > 0 && bounds.H > 0 {
		mounted.viewport = map[string]float32{"width": bounds.W, "height": bounds.H}
	}
	if _, err := mounted.call("setViewport", mounted.viewport); err != nil {
		return ui.Element{}, err
	}
	if _, err := mounted.call("setTheme", runtimeTheme(context)); err != nil {
		return ui.Element{}, err
	}
	element, err := mounted.snapshot()
	if err != nil {
		return ui.Element{}, err
	}
	return mounted.renderElement(context, element)
}

func (mounted *Mounted) renderElement(context *ui.Context, element *NativeElement) (ui.Element, error) {
	if element == nil {
		return ui.Element{}, errors.New("plugin element is missing")
	}
	context = context.Key(mounted.widgetKey(element))
	element.RenderChild = mounted.renderElement
	var result ui.Element
	var resultErr error
	parent := mounted.scrollAxes(element)
	children := func() {
		mounted.scrolling = append(mounted.scrolling, parent)
		truncating := mounted.truncating
		mounted.truncating = truncating || slices.ContainsFunc(element.Styles, func(style ElementStyle) bool {
			return style.Name == "truncate" || style.Name == "whitespace_nowrap" || style.Name == "text_ellipsis"
		})
		defer func() {
			mounted.scrolling, mounted.truncating = mounted.scrolling[:len(mounted.scrolling)-1], truncating
		}()
		if box := mounted.scrollBoxes[parent.id]; (parent.x && box.W == 0 || parent.y && box.H == 0) &&
			slices.ContainsFunc(element.Children, func(child *NativeElement) bool { return scrollPercentage(child, parent) }) {
			// The viewport is known a frame after the container first
			// shows: its content waits for it rather than move once laid out.
			return
		}
		for _, child := range element.Children {
			if _, err := mounted.renderElement(context, child); err != nil && resultErr == nil {
				resultErr = err
			}
		}
	}
	emit := func(callback CallbackRef, event json.RawMessage) {
		if mounted.suspended {
			return
		}
		if err := mounted.Dispatch(callback, event); err != nil {
			mounted.failure = err
		}
	}
	if element.Module == "sailry" && element.Export == "Image" {
		var err error
		result, err = mounted.renderCapturedImage(context, element)
		if err != nil {
			return ui.Element{}, err
		}
	} else if component, exists := mounted.runtime.components[element.Module+"/"+element.Export]; exists {
		var err error
		result, err = component.Render(context, *element, children, emit)
		if err != nil {
			return ui.Element{}, err
		}
	} else if element.Module == "gpui-kit" || element.Module == "gpui-base" || element.Module == "gpui-component" {
		var err error
		result, err = mounted.renderCore(context, element, children)
		if err != nil {
			return ui.Element{}, err
		}
	} else {
		return ui.Element{}, fmt.Errorf("plugin native component %s.%s is unavailable", element.Module, element.Export)
	}
	if resultErr != nil {
		return ui.Element{}, resultErr
	}
	if err := mounted.applyStyles(context, result, element); err != nil {
		return ui.Element{}, err
	}
	if callback, exists := element.Callbacks["on_click"]; exists {
		result.OnClick(func() { emit(callback, json.RawMessage(`{}`)) })
	}
	if bind := mounted.runtime.bindings.BindActions; bind != nil && !mounted.suspended {
		bind(context, result, *element, emit)
	}
	return result, nil
}

// scrollAxes reports the axes a package's own container scrolls.
func (mounted *Mounted) scrollAxes(element *NativeElement) scrollParent {
	if element.Module != "gpui-kit" && element.Module != "gpui-base" && element.Module != "gpui-component" {
		return scrollParent{}
	}
	var parent scrollParent
	for _, style := range element.Styles {
		switch style.Name {
		case "overflow_scroll", "overflow_scrollbar":
			parent.x, parent.y = true, true
		case "overflow_x_scroll", "overflow_x_scrollbar":
			parent.x = true
		case "overflow_y_scroll", "overflow_y_scrollbar":
			parent.y = true
		}
	}
	if parent.x || parent.y {
		parent.id = mounted.widgetID(element)
	}
	return parent
}

// scrollPercentage reports whether an element sizes itself by a percentage
// of an axis its parent scrolls.
func scrollPercentage(element *NativeElement, parent scrollParent) bool {
	for _, style := range element.Styles {
		name := style.Name
		var argument any
		if len(style.Arguments) > 0 && json.Unmarshal(style.Arguments[0], &argument) != nil {
			continue
		}
		if tail, exists := runtimeScaleStyle(name); exists {
			name, argument = tail.name, tail.value
		}
		horizontal, vertical := false, false
		switch name {
		case "w", "min_w", "max_w":
			horizontal = true
		case "h", "min_h", "max_h":
			vertical = true
		case "size":
			horizontal, vertical = true, true
		default:
			continue
		}
		if _, percentage, _, err := runtimeLength(argument); err == nil && percentage && (horizontal && parent.x || vertical && parent.y) {
			return true
		}
	}
	return false
}

// scrollViewport is the box a percentage on a scrolling axis of the parent
// being built resolves against, once that parent has been laid out.
func (mounted *Mounted) scrollViewport() (box ui.Rect, x, y bool) {
	if len(mounted.scrolling) == 0 {
		return ui.Rect{}, false, false
	}
	parent := mounted.scrolling[len(mounted.scrolling)-1]
	box = mounted.scrollBoxes[parent.id]
	return box, parent.x && box.W > 0, parent.y && box.H > 0
}

func (mounted *Mounted) renderCapturedImage(context *ui.Context, element *NativeElement) (ui.Element, error) {
	properties := runtimeProperties(element)
	resource := runtimeString(properties["path"])
	_, bitmap := mounted.bundle.Images[resource]
	_, source := mounted.bundle.Files[resource]
	if !bitmap && (!source || !strings.HasSuffix(resource, ".svg")) {
		return ui.Box(context), nil
	}
	image, err := mounted.image(resource)
	if err != nil {
		return ui.Element{}, err
	}
	fit := ui.Contain
	switch runtimeString(properties["fit"]) {
	case "fill":
		fit = ui.FillBox
	case "cover":
		fit = ui.Cover
	}
	result := ui.Image(context, image).Fill().Fit(fit)
	width, widthOK := properties["width"].(float64)
	height, heightOK := properties["height"].(float64)
	if widthOK && heightOK && width > 0 && height > 0 && width <= math.MaxFloat32 && height <= math.MaxFloat32 {
		result.Size(float32(width), float32(height)).AspectRatio(float32(width) / float32(height))
	}
	if runtimeBool(properties["circular"]) {
		result.Radius(9999)
	}
	return result, nil
}

func (mounted *Mounted) renderCore(context *ui.Context, element *NativeElement, children func()) (ui.Element, error) {
	properties := runtimeProperties(element)
	label := runtimeString(properties["label"])
	if label == "" {
		label = element.Text
	}
	var result ui.Element
	switch element.Export {
	case "Text", "Label", "ShimmerText", "Kbd", "TableHead", "TableCell":
		result = ui.Text(context, label)
		if mounted.truncating {
			// MyGo keeps a line to itself on the text, not on its container.
			result.SingleLine().Shrink(1).MinWidth(0)
		}
		if element.Export == "ShimmerText" {
			result = kit.Shimmer(context, label, 14, theme.Of(context).Foreground)
		}
	case "Div", "VFlex", "Form", "VForm", "Table", "TableBody", "AccordionItem", "AccordionPanel", "ResizablePanel":
		axes := mounted.scrollAxes(element)
		switch {
		case axes.x && axes.y:
			result = ui.ScrollBoth(context)
		case axes.x:
			result = ui.ScrollHorizontal(context)
		case axes.y:
			result = ui.Scroll(context)
		default:
			result = ui.Column(context)
		}
		if _, sized := properties["size"]; element.Export == "ResizablePanel" && element.panelAxis != 0 && !sized {
			result.Grow(1).Basis(0).MinWidth(0).MinHeight(0)
		}
		if axes.x || axes.y {
			// MyGo leaves a percentage of a scrolling axis unresolved, where
			// the reference resolves it against the scroll viewport: keep
			// the viewport for the children to resolve theirs against.
			box, known := result.LayoutBounds(), false
			if mounted.scrollBoxes == nil {
				mounted.scrollBoxes = map[uint64]ui.Rect{}
			} else {
				_, known = mounted.scrollBoxes[axes.id]
			}
			if !known || box != mounted.scrollBoxes[axes.id] {
				mounted.scrollBoxes[axes.id] = box
				context.Invalidate()
			}
		}
	case "HFlex", "ToggleGroup", "TableHeader", "TableRow":
		result = ui.Row(context)
	case "HResizable", "VResizable", "Resizable":
		// A resizable group fills its place and lays its panels along its
		// axis: a panel with a size keeps it there, the others share the
		// rest. The panels are not dragged: MyGo's Split holds two panes
		// and sizes the first, where a group sizes any of several.
		vertical := element.Export == "VResizable" || element.Export == "Resizable" && runtimeString(properties["axis"]) == "vertical"
		result = ui.Row(context)
		axis := panelHorizontal
		if vertical {
			result, axis = ui.Column(context), panelVertical
		}
		result.Fill().MinWidth(0).MinHeight(0).AlignItems(ui.Stretch)
		for _, child := range element.Children {
			child.panelAxis = axis
		}
	case "Tabs", "TabBar":
		return mounted.renderTabs(context, element, properties)
	case "HorizontalRadioGroup", "RadioGroup":
		return mounted.renderRadioGroup(context, element, properties)
	case "Field":
		return mounted.renderField(context, element, properties, children)
	case "Accordion":
		return mounted.renderAccordion(context, element, properties)
	case "Collapsible":
		return mounted.renderCollapsible(context, element, properties, children)
	case "StatusBar":
		return mounted.renderStatusBar(context, element, children)
	case "Pagination":
		return mounted.renderPagination(context, element, properties)
	case "Button", "Link", "Tab", "AccordionTrigger":
		if element.Module == "gpui-base" {
			// The reference's Base controls have no look: the package's own
			// styles are all they draw, selected or not. A Button and a Tab
			// center their content on a line as high as its text; a Link
			// and an AccordionTrigger are plain blocks.
			result = ui.ButtonBase(context).Label(label).Shrink(1).DimDisabled(false)
			if element.Export == "Button" || element.Export == "Tab" {
				result.LineHeight(1)
			} else {
				result.Column().AlignItems(ui.Stretch).Justify(ui.Start)
			}
			if len(element.Children) == 0 && label != "" {
				result.Children(func() { ui.Text(context, label) })
			}
			break
		}
		variant := runtimeString(properties["variant"])
		// labeled is whether the Kit button built its own label.
		labeled := true
		// The Kit look's edges, which the package's own styles refine edge
		// by edge as the reference refines its Button's.
		palette := theme.Of(context)
		look := &elementLook{padding: [4]float32{0, 12, 0, 12}, radius: [4]float32{palette.Radius, palette.Radius, palette.Radius, palette.Radius}}
		element.look = look
		switch {
		case variant == "primary" || runtimeBool(properties["primary"]):
			result = kit.Primary(context, label, runtimeBool(properties["disabled"]))
			labeled = false
		case variant == "outline" || runtimeBool(properties["outline"]) || runtimeBool(properties["secondary"]):
			result = kit.Secondary(context, label)
			look.border = [4]float32{1, 1, 1, 1}
		case variant == "danger" || runtimeBool(properties["danger"]):
			result = kit.Danger(context, label)
		case runtimeBool(properties["success"]):
			result = kit.Primary(context, label, runtimeBool(properties["disabled"])).Background(theme.Of(context).Success)
			labeled = false
		case runtimeBool(properties["warning"]):
			result = kit.Primary(context, label, runtimeBool(properties["disabled"])).Background(theme.Of(context).Warning)
			labeled = false
		case runtimeBool(properties["link"]):
			result = kit.Ghost(context, label, false).TextColor(theme.Of(context).Link)
			labeled = false
			look.padding = [4]float32{}
		case element.Export != "Button" || runtimeBool(properties["ghost"]) || runtimeBool(properties["text"]) || variant == "ghost":
			result = kit.Ghost(context, label, runtimeBool(properties["selected"])).Padding(6, 10)
			labeled = false
			look.padding = [4]float32{6, 10, 6, 10}
		default:
			// The reference's Button without a variant is Kit's default
			// button, as the application's own secondary actions.
			result = kit.Secondary(context, label)
			look.border = [4]float32{1, 1, 1, 1}
		}
		if !labeled && len(element.Children) == 0 && label != "" {
			result.Children(func() { ui.Text(context, label).FontSize(14).SingleLine() })
		}
	case "Checkbox", "Radio", "Toggle", "Switch":
		identifier := mounted.widgetID(element)
		value := mounted.toggles[identifier]
		if value == nil {
			value = new(bool)
			mounted.toggles[identifier] = value
		}
		*value = runtimeBool(properties["checked"]) || runtimeBool(properties["selected"])
		switch element.Export {
		case "Checkbox", "Radio":
			result = ui.Checkbox(context, value, label)
		case "Toggle":
			result = ui.Toggle(context, value, label)
		case "Switch":
			result = ui.Switch(context, value).Label(label)
		}
		if callback, exists := element.Callbacks["on_change"]; exists {
			result.OnChange(func() {
				data, _ := json.Marshal(*value)
				if err := mounted.Dispatch(callback, data); err != nil {
					mounted.failure = err
				}
			})
		}
	case "Spinner":
		result = kit.Busy(context)
	case "Progress", "ProgressTrack", "ProgressIndicator", "SliderTrack", "SliderIndicator":
		value := runtimeFloat(properties["value"])
		result = ui.Progress(context, float64(value)/100)
	case "Icon":
		name := runtimeString(properties["icon"])
		if name == "" {
			if arguments, ok := properties["arguments"].([]any); ok && len(arguments) != 0 {
				name = runtimeString(arguments[0])
			}
		}
		icon, err := icons.Plugin(name)
		if err != nil {
			return ui.Element{}, err
		}
		if icons.Colored(name) {
			// A brand mark in its own colors, at an icon's default size.
			result = ui.Image(context, icon).Size(16, 16)
		} else {
			result = ui.Icon(context, icon)
		}
	case "Tag", "Badge":
		result = kit.Tag(context, label)
	case "Editor", "Input", "Textarea", "NumberInput", "OtpInput":
		state, _ := properties["state"].(map[string]any)
		if state == nil {
			if arguments, ok := properties["arguments"].([]any); ok && len(arguments) > 0 {
				state, _ = arguments[0].(map[string]any)
			}
		}
		identifier := mounted.widgetID(element)
		value := mounted.inputs[identifier]
		if value == nil {
			value = new(string)
			mounted.inputs[identifier] = value
		}
		*value = runtimeString(state["value"])
		if element.Export == "Editor" {
			result = ui.Column(context)
			leaf := kit.Code(context, result, *value, kit.CodeOptions{
				Editable: !runtimeBool(properties["read_only"]),
				Numbers:  runtimeBool(properties["line_numbers"]),
				Changed: func(text string) {
					*value = text
					mounted.changeState(state, text)
				},
			})
			element.Focus = func() bool {
				leaf.Focus()
				return true
			}
		} else {
			if element.Export == "Textarea" {
				result = ui.TextArea(context, value)
			} else {
				result = ui.TextInput(context, value)
			}
			result.Placeholder(runtimeString(state["placeholder"])).Label(label)
			result.OnChange(func() { mounted.changeState(state, *value) })
		}
	case "Diff":
		return mounted.renderDiff(context, element, properties)
	case "TextView":
		result = ui.Column(context)
		if runtimeString(properties["format"]) == "markdown" {
			kit.Markdown(context, element.Text, kit.MarkdownProps{})
		} else {
			ui.Text(context, element.Text).Selectable()
		}
	case "Svg", "Image", "AvatarImage":
		resource := runtimeString(properties["path"])
		if resource == "" {
			resource = runtimeString(properties["id"])
		}
		image, err := mounted.image(resource)
		if err != nil {
			return ui.Element{}, err
		}
		result = ui.Image(context, image).Fit(ui.Contain)
	case "Avatar", "AvatarFallback", "Attachment", "AccordionHeader", "TableCaption", "Scrollbar":
		result = ui.Box(context)
	case "Popover", "HoverCard", "Popup", "DropdownMenu":
		identifier := mounted.widgetID(element)
		open := mounted.popovers[identifier]
		if open == nil {
			open = new(bool)
			mounted.popovers[identifier] = open
		}
		trigger := element.Slots["trigger"]
		if len(trigger) == 0 {
			result = kit.Ghost(context, label, *open)
		} else {
			var err error
			result, err = mounted.renderElement(context, trigger[0])
			if err != nil {
				return ui.Element{}, err
			}
		}
		result.OnClick(func() { *open = !*open })
		kit.Popover(context, result, open, func() {
			for _, child := range element.Slots["content"] {
				if _, err := mounted.renderElement(context, child); err != nil {
					mounted.failure = err
				}
			}
			children()
		})
		return result, nil
	case "Path":
		return mounted.renderPath(context, element, properties)
	default:
		return ui.Element{}, fmt.Errorf("plugin core element %q has no native renderer", element.Export)
	}
	if len(element.Children) != 0 {
		result.Children(children)
	}
	for _, name := range []string{"header", "content", "input", "panel", "footer", "image", "fallback"} {
		if slot := element.Slots[name]; len(slot) != 0 {
			result.Children(func() {
				for _, child := range slot {
					if _, err := mounted.renderElement(context, child); err != nil {
						mounted.failure = err
					}
				}
			})
		}
	}
	return result, nil
}

func (mounted *Mounted) changeState(state map[string]any, value string) {
	if mounted.suspended {
		return
	}
	if identifier := runtimeFloat(state["__coreState"]); identifier > 0 {
		if _, err := mounted.call("stateChange", uint64(identifier), value); err != nil {
			mounted.failure = err
		}
	}
}

func (mounted *Mounted) image(name string) (ui.ImageSource, error) {
	if image, exists := mounted.images[name]; exists {
		return image, nil
	}
	if source, exists := mounted.bundle.Files[name]; exists && strings.HasSuffix(strings.ToLower(name), ".svg") {
		image, err := ui.ParseSVG([]byte(source))
		if err == nil {
			mounted.images[name] = image
		}
		return image, err
	}
	encoded, exists := mounted.bundle.Images[name]
	if !exists {
		return nil, fmt.Errorf("plugin image %q is not a declared resource", name)
	}
	data, err := base64.StdEncoding.DecodeString(encoded)
	if err != nil {
		return nil, err
	}
	dimensions, err := png.DecodeConfig(bytes.NewReader(data))
	if err != nil {
		return nil, err
	}
	if dimensions.Width > 2048 || dimensions.Height > 2048 || uint64(dimensions.Width)*uint64(dimensions.Height)*4 > 16*1024*1024 {
		return nil, errors.New("plugin image exceeds its decoded size limit")
	}
	image, err := ui.DecodeImage(data)
	if err == nil {
		mounted.images[name] = image
	}
	return image, err
}

func runtimeProperties(element *NativeElement) map[string]any {
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

func runtimeString(value any) string {
	if text, ok := value.(string); ok {
		return text
	}
	return ""
}

func runtimeFloat(value any) float32 {
	switch number := value.(type) {
	case float64:
		return float32(number)
	case float32:
		return number
	case int:
		return float32(number)
	}
	return 0
}

func runtimeBool(value any) bool {
	boolean, _ := value.(bool)
	return boolean
}

func runtimeTheme(context *ui.Context) map[string]any {
	palette := theme.Of(context)
	result := map[string]any{}
	_ = json.Unmarshal(ThemeSnapshot(palette), &result)
	result["dark"] = palette.Dark
	result["spacing"] = map[string]any{"base": 4}
	result["radius"] = map[string]any{"default": palette.Radius, "large": palette.RadiusLg, "sm": float32(4), "md": palette.Radius, "lg": palette.RadiusLg, "xl": float32(12)}
	return result
}
