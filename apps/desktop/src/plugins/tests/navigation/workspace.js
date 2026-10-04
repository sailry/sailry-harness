import { View, div } from "gpui-kit";
import { Workspace, PanelHeader, IconButton, nextControlEvent } from "sailry/ui";
import { Anchor } from "sailry/test";
import { Header, context } from "sailry";
import { prepareRequest, completeRequest, forgetRequest } from "sailry/sdk";

export default class Example extends View {
  init(_props, cx) {
    this.scope = JSON.parse(context());
    this.count = 0;
    this.available = true;
    this.navigation = "default";
    this.sidebar = true;
    this.sideCount = 0;
    this.minimum = 480;
    this.text = "loading";
    cx.spawn(async cx => {
      const request = prepareRequest({kind: "read_file", data: {worktree: this.scope.worktree, path: "notes.txt"}});
      try {
        const result = await completeRequest(request);
        this.text = result.Ok?.kind === "file_content" ? result.Ok.data.text : "unexpected";
      } finally { forgetRequest(request); }
      cx.notify();
    });
    cx.spawn(async cx => {
      while (true) {
        const event = await nextControlEvent();
        if (event.id === "workspace-availability") this.available = !this.available;
        else if (event.id === "workspace-navigation") {
          const modes = ["default", "none", "resource"];
          this.navigation = modes[(modes.indexOf(this.navigation) + 1) % modes.length];
        }
        else if (event.id === "workspace-sidebar-availability") this.sidebar = !this.sidebar;
        else if (event.id === "workspace-sidebar-action") this.sideCount++;
        else if (event.id === "workspace-minimum") this.minimum = this.minimum === 480 ? 1000 : 480;
        else this.count += event.id === "workspace-increment" ? 1 : 10;
        cx.notify();
      }
    });
  }

  render() {
    return Workspace.new("example-workspace", {has_details: this.available, default_details_width:320, details_label: "Workspace details", min_navigation_width:280, min_details_width:300, navigation: this.navigation}).children([
      Anchor.new("workspace-main").child(div().v_flex().size_full()
        .child(Header.new("workspace-header", {content: JSON.stringify({min_content_width:this.minimum})}))
        .child(div().id(`main-minimum-${this.minimum}`).child(String(this.minimum)))
        .child(IconButton.new("workspace-minimum", {icon: "panel-right", label: "Toggle minimum width"}))
        .child(div().id(`main-count-${this.count}`).child(this.text))
        .child(div().id(`scope-${this.scope.worktree}`).child(this.scope.worktree))
        .child(div().id(`availability-${this.available}`).child(String(this.available)))
        .child(IconButton.new("workspace-availability", {icon: "panel-right", label: "Toggle availability"}))
        .child(div().id(`navigation-${this.navigation}`).child(this.navigation))
        .child(div().id(`sidebar-${this.sidebar}`).child(String(this.sidebar)))
        .child(IconButton.new("workspace-navigation", {icon: "panel-left", label: "Change navigation"}))
        .child(IconButton.new("workspace-sidebar-availability", {icon: "panel-left", label: "Toggle navigation availability"}))
        .child(IconButton.new("workspace-increment", {icon: "plus", label: "Increment"}))),
      Anchor.new("workspace-details").child(div().v_flex().size_full()
        .child(PanelHeader.new("example-details-header").child(div().child("Details")))
        .child(div().id(`details-count-${this.count}`).child(String(this.count)))
        .child(IconButton.new("workspace-add-ten", {icon: "plus", label: "Add ten"}))),
      ...(this.navigation === "resource" && this.sidebar ? [
        Anchor.new("workspace-navigation-slot").child(div().v_flex().size_full()
          .child(PanelHeader.new("workspace-navigation-header").child(div().child("Resources")))
          .child(div().id(`sidebar-count-${this.sideCount}`).child(String(this.sideCount)))
          .child(IconButton.new("workspace-sidebar-action", {icon: "plus", label: "Add one"})))
      ] : [])
    ]);
  }
}
