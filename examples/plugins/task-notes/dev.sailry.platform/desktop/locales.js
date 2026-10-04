const en = { revision: "Revision", selected: "Selected", none: "None" };
const zh = { revision: "修订", selected: "当前", none: "无" };
export function messages(locale) { return locale.startsWith("zh") ? zh : en; }
