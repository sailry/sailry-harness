package testplugin

const TerminalName = "commands"

func Terminal() Package {
	return Package{Name: TerminalName, Platform: `{
  "api_version":"v1","scope":"host","actions":["terminals.read","terminals.control"],
  "ui":[{"id":"terminal-new","slot":"project","kind":"button","label":{"label":"Fixture terminal"},"handler":"create"}],
  "desktop":{"entry":"dev.sailry.platform/terminal.js","ui_entry":"dev.sailry.platform/terminal.js","resources":["dev.sailry.platform/terminal.js"],"renderers":[{"resource":"terminal","create":"terminal-new"}]}
}`, Files: map[string]string{"dev.sailry.platform/terminal.js": `import {View} from 'gpui-kit';export default class extends View{render(){throw new Error('The native terminal must not mount this renderer');}}`}}
}
