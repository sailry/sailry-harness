package plugins

import (
	"fmt"
	"strings"
)

func coreModule(name string) bool {
	switch name {
	case "gpui", "gpui-kit", "gpui-base", "gpui-component":
		return true
	}
	return false
}

func coreModuleSource(name string) string {
	var names []string
	table := "__gpui"
	switch name {
	case "gpui", "gpui-kit":
		names = []string{"View", "div", "svg", "image", "list", "uniform_list", "PathBuilder", "Background"}
	case "gpui-base":
		names = []string{
			"h_flex", "v_flex", "Button", "Link", "Checkbox", "Switch", "Tabs", "Tab", "Progress",
			"ProgressTrack", "ProgressIndicator", "Avatar", "AvatarImage", "AvatarFallback", "Pagination",
			"pagination_items", "CalendarState", "Accordion", "AccordionItem", "AccordionHeader", "AccordionPanel",
			"AccordionTrigger", "Radio", "Toggle", "RadioGroup", "ToggleGroup", "Table", "TableHeader", "TableBody",
			"TableRow", "TableHead", "TableCell", "TableCaption", "h_resizable", "v_resizable", "resizable_panel",
			"Collapsible", "Popover", "HoverCard", "Popup", "Select", "Combobox", "DatePicker", "Scrollbar",
			"v_virtual_list", "h_virtual_list", "VirtualListScrollHandle", "Input", "InputState", "NumberInput",
			"Textarea", "TextView", "TextareaState", "SliderState", "Slider", "SliderTrack", "SliderIndicator",
			"SliderThumb", "OtpState", "OtpInput", "DockArea", "dock_area", "dock_content", "set_theme",
		}
	case "gpui-component":
		table = "__desktopComponents"
		names = []string{
			"Accordion", "AccordionItem", "Attachment", "Badge", "Button", "Checkbox", "Collapsible", "Diff",
			"DropdownMenu", "Editor", "Field", "Form", "HorizontalRadioGroup", "Icon", "Label", "Link", "Pagination",
			"Progress", "Radio", "Resizable", "ResizablePanel", "ShimmerText", "Spinner", "StatusBar", "Switch",
			"Tab", "TabBar", "Tag", "Text", "Toggle", "VForm", "EditorState", "DiffState", "InputState",
		}
	}
	var source strings.Builder
	for _, export := range names {
		fmt.Fprintf(&source, "export const %s=globalThis.%s.%s;\n", export, table, export)
	}
	return source.String()
}
