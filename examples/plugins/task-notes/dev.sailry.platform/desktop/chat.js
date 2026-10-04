import { View, div } from "gpui-kit";
import { Conversation, context } from "sailry";

export default class NotesConversation extends View {
  render() {
    const source = JSON.parse(context());
    const props = source.session ? undefined : { assistant: "notes" };
    return div().size_full().child(Conversation.new("notes-conversation", props));
  }
}
