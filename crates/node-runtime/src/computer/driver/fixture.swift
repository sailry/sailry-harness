// Isolated AppKit input fixture. Output contains only this fixture's state.
import AppKit

final class PointerSurface: NSView {
    var moved: ((NSEvent) -> Void)?
    private var tracking: NSTrackingArea?

    override func updateTrackingAreas() {
        if let tracking { removeTrackingArea(tracking) }
        tracking = NSTrackingArea(rect: bounds,
            options: [.mouseMoved, .activeAlways, .inVisibleRect], owner: self, userInfo: nil)
        addTrackingArea(tracking!)
        super.updateTrackingAreas()
    }
    override func mouseMoved(with event: NSEvent) { moved?(event) }
}

func pointValues(_ point: CGPoint) -> [Any] {
    [point.x, point.y].map { $0.isFinite ? $0 as Any : NSNull() }
}

private let activityLibrary = dlopen(
    "/System/Library/PrivateFrameworks/SkyLight.framework/SkyLight", RTLD_LAZY | RTLD_LOCAL)
private typealias FrontProcess = @convention(c) (UnsafeMutableRawPointer) -> Int32
private typealias ProcessForPID = @convention(c) (Int32, UnsafeMutableRawPointer) -> Int32

func windowServerForeground(_ pid: pid_t) -> Bool? {
    guard activityLibrary != nil,
          let frontSymbol = dlsym(UnsafeMutableRawPointer(bitPattern: -2), "_SLPSGetFrontProcess"),
          let processSymbol = dlsym(UnsafeMutableRawPointer(bitPattern: -2), "GetProcessForPID") else { return nil }
    let getFront = unsafeBitCast(frontSymbol, to: FrontProcess.self)
    let getProcess = unsafeBitCast(processSymbol, to: ProcessForPID.self)
    var front: UInt64 = 0
    var process: UInt64 = 0
    guard withUnsafeMutablePointer(to: &front, { getFront(UnsafeMutableRawPointer($0)) }) == 0,
          withUnsafeMutablePointer(to: &process, { getProcess(pid, UnsafeMutableRawPointer($0)) }) == 0 else { return nil }
    return front == process
}

// A physically interactive custom control exposes only accessibility raise.
// Do not implement accessibilityPerformPress: AppKit adds AXPress for it.
final class PointerButton: NSView {
    var click: (() -> Void)?
    var inputEvent: ((String) -> Void)?
    var unsupportedPress: (() -> Void)?
    private var pressed = false

    override func isAccessibilityElement() -> Bool { true }
    override func accessibilityRole() -> NSAccessibility.Role? { .button }
    override func accessibilityLabel() -> String? { "Pointer Count" }
    override func isAccessibilityEnabled() -> Bool { true }
    override func accessibilityParent() -> Any? { superview }
    override func accessibilityFrame() -> NSRect {
        window?.convertToScreen(convert(bounds, to: nil)) ?? .zero
    }
    override func accessibilityActionNames() -> [NSAccessibility.Action] { [.raise] }
    override func isAccessibilitySelectorAllowed(_ selector: Selector) -> Bool {
        if selector == #selector(NSAccessibilityProtocol.accessibilityPerformPress) {
            return false
        }
        if selector == #selector(NSAccessibilityProtocol.accessibilityPerformRaise) {
            return true
        }
        return super.isAccessibilitySelectorAllowed(selector)
    }
    override func accessibilityPerformAction(_ action: NSAccessibility.Action) {
        if action == .press { unsupportedPress?() }
    }
    override func accessibilityPerformRaise() -> Bool { true }
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }
    override func mouseDown(with event: NSEvent) {
        inputEvent?("mouse_down")
        pressed = bounds.contains(convert(event.locationInWindow, from: nil))
        needsDisplay = true
    }
    override func mouseUp(with event: NSEvent) {
        inputEvent?("mouse_up")
        let clicked = pressed && bounds.contains(convert(event.locationInWindow, from: nil))
        pressed = false
        needsDisplay = true
        if clicked { click?() }
    }
    override func draw(_ dirtyRect: NSRect) {
        (pressed ? NSColor.selectedControlColor : NSColor.controlColor).setFill()
        NSBezierPath(roundedRect: bounds, xRadius: 6, yRadius: 6).fill()
        let label = "Pointer Count" as NSString
        let attributes: [NSAttributedString.Key: Any] = [
            .font: NSFont.systemFont(ofSize: 13), .foregroundColor: NSColor.controlTextColor
        ]
        let size = label.size(withAttributes: attributes)
        label.draw(at: NSPoint(x: bounds.midX - size.width / 2, y: bounds.midY - size.height / 2),
                   withAttributes: attributes)
    }
}

// An actionable, read-only counter stays visible in normal AX observations.
final class CounterView: NSView {
    var count: () -> Int = { 0 }

    override func isAccessibilityElement() -> Bool { true }
    override func accessibilityRole() -> NSAccessibility.Role? { .textField }
    override func accessibilityLabel() -> String? { "Pointer Clicks" }
    override func accessibilityValue() -> Any? { String(count()) }
    override func isAccessibilityEnabled() -> Bool { true }
    override func accessibilityParent() -> Any? { superview }
    override func accessibilityFrame() -> NSRect {
        window?.convertToScreen(convert(bounds, to: nil)) ?? .zero
    }
    override func accessibilityActionNames() -> [NSAccessibility.Action] { [.raise] }
    override func isAccessibilitySelectorAllowed(_ selector: Selector) -> Bool {
        if selector == #selector(NSAccessibilityProtocol.accessibilityPerformRaise) {
            return true
        }
        if selector == #selector(NSAccessibilityProtocol.accessibilityPerformPress)
            || NSStringFromSelector(selector).hasPrefix("setAccessibility") {
            return false
        }
        return super.isAccessibilitySelectorAllowed(selector)
    }
    override func accessibilityPerformRaise() -> Bool { true }
    func valueChanged() {
        needsDisplay = true
        NSAccessibility.post(element: self, notification: .valueChanged)
    }
    override func draw(_ dirtyRect: NSRect) {
        (String(count()) as NSString).draw(
            at: NSPoint(x: 0, y: 5),
            withAttributes: [.font: NSFont.systemFont(ofSize: 13), .foregroundColor: NSColor.labelColor])
    }
}

// The read-only AX value forces text insertion through real key events.
final class KeyboardDraft: NSView {
    var text = ""
    var keyEvents = 0
    var shortcuts = 0
    var inputEvent: ((NSEvent) -> Void)?

    override var acceptsFirstResponder: Bool { true }
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }
    override func mouseDown(with event: NSEvent) { window?.makeFirstResponder(self) }
    override func isAccessibilityElement() -> Bool { true }
    override func accessibilityRole() -> NSAccessibility.Role? { .textField }
    override func accessibilityLabel() -> String? { "Keyboard Input" }
    override func accessibilityValue() -> Any? { text }
    override func isAccessibilityEnabled() -> Bool { true }
    override func isAccessibilityFocused() -> Bool { window?.firstResponder === self }
    override func accessibilityParent() -> Any? { superview }
    override func accessibilityFrame() -> NSRect {
        window?.convertToScreen(convert(bounds, to: nil)) ?? .zero
    }
    override func accessibilitySelectedText() -> String? { "" }
    override func accessibilityActionNames() -> [NSAccessibility.Action] { [.raise] }
    override func isAccessibilitySelectorAllowed(_ selector: Selector) -> Bool {
        if selector == #selector(NSAccessibilityProtocol.accessibilityPerformRaise) {
            return true
        }
        if selector == #selector(NSAccessibilityProtocol.accessibilityPerformPress) {
            return false
        }
        if selector == #selector(NSAccessibilityProtocol.setAccessibilitySelectedText(_:))
            || selector == #selector(NSAccessibilityProtocol.setAccessibilityValue(_:))
            || selector == #selector(NSAccessibilityProtocol.setAccessibilityFocused(_:)) {
            return false
        }
        return super.isAccessibilitySelectorAllowed(selector)
    }
    override func accessibilityPerformRaise() -> Bool { true }
    override func keyDown(with event: NSEvent) {
        keyEvents += 1
        if event.modifierFlags.contains(.control) && event.keyCode == 37 {
            shortcuts += 1
            inputEvent?(event)
            return
        }
        guard let characters = event.characters, !characters.isEmpty else { return }
        text += characters
        needsDisplay = true
        inputEvent?(event)
        NSAccessibility.post(element: self, notification: .valueChanged)
    }
    override func draw(_ dirtyRect: NSRect) {
        NSColor.textBackgroundColor.setFill()
        bounds.fill()
        (text.isEmpty ? "Keyboard Input" : text as NSString).draw(
            at: NSPoint(x: 6, y: 5),
            withAttributes: [.font: NSFont.systemFont(ofSize: 13), .foregroundColor: NSColor.textColor])
    }
}

final class Fixture: NSObject, NSApplicationDelegate, NSTextFieldDelegate {
    var window: NSWindow!
    let field = NSTextField(frame: NSRect(x: 30, y: 140, width: 440, height: 50))
    let search = NSTextField(frame: NSRect(x: 30, y: 220, width: 440, height: 26))
    let pointerCounter = CounterView(frame: .zero)
    let keyboard = KeyboardDraft(frame: NSRect(x: 30, y: 196, width: 440, height: 24))
    var clicks = 0
    var menuClicks = 0
    var pointerClicks = 0
    var mouseMoves = 0
    var viewMouseMoves = 0
    var moveSourcePID: Int64 = -1
    var moveWindowID = -1
    var movePoint = CGPoint.zero
    var keySourcePID: Int64 = -1
    var keyWindowID = -1
    var unsupportedPresses = 0
    var submissions = 0
    var lastSubmitted = ""
    var clickForegroundPID: pid_t = -1
    var clickPointer = CGPoint.zero
    let output = CommandLine.arguments[1]
    let background = CommandLine.arguments.dropFirst(2).contains("background")
    var commandTimer: Timer?
    var commandRevision = 0
    var monitoring = false
    var monitorEvents: [[String: Any]] = []
    var monitorOverflow = false
    var monitorSamples = 0
    var lastSnapshot: Data?
    var monitorTimer: Timer?
    var notificationObservers: [NSObjectProtocol] = []
    var pointerMonitor: Any?

    func applicationDidFinishLaunching(_ notification: Notification) {
        let menu = NSMenu()
        let application = NSMenuItem(title: "Fixture", action: nil, keyEquivalent: "")
        application.submenu = NSMenu(title: "Fixture")
        menu.addItem(application)
        let edit = NSMenuItem(title: "Edit", action: nil, keyEquivalent: "")
        let actions = NSMenu(title: "Edit")
        let menuCount = NSMenuItem(title: "Menu Count", action: #selector(countMenu), keyEquivalent: "")
        menuCount.target = self
        actions.addItem(menuCount)
        actions.addItem(withTitle: "Select All", action: #selector(NSText.selectAll(_:)), keyEquivalent: "a")
        edit.submenu = actions
        menu.addItem(edit)
        NSApp.mainMenu = menu
        let width: CGFloat = CommandLine.arguments.dropFirst(2).first == "wide" ? 1000 : 500
        window = NSWindow(contentRect: NSRect(x: 140, y: 180, width: width, height: 280),
                          styleMask: [.titled, .closable, .miniaturizable, .resizable], backing: .buffered, defer: false)
        window.title = "Sailry CUA Fixture"
        window.isReleasedWhenClosed = false
        let surface = PointerSurface(frame: window.contentView!.frame)
        surface.moved = { event in
            self.viewMouseMoves += 1
            self.persist()
        }
        window.contentView = surface
        window.acceptsMouseMovedEvents = true
        // Raw mouseMoved goes through the application's event pipeline even
        // when its first responder does not forward it to this content view.
        // Keep native receipt distinct from tracking/hover behavior.
        pointerMonitor = NSEvent.addLocalMonitorForEvents(matching: .mouseMoved) { event in
            if event.windowNumber == self.window.windowNumber {
                self.mouseMoves += 1
                self.moveSourcePID = event.cgEvent?.getIntegerValueField(.eventSourceUnixProcessID) ?? -1
                self.moveWindowID = event.windowNumber
                self.movePoint = event.locationInWindow
                self.sample("mouse_move")
                self.persist()
            }
            return event
        }
        field.delegate = self
        field.setAccessibilityLabel("Draft")
        field.stringValue = "Initial fixture text"
        window.contentView!.addSubview(field)
        search.delegate = self
        search.setAccessibilityLabel("Search")
        window.contentView!.addSubview(search)
        keyboard.inputEvent = { event in
            self.keySourcePID = event.cgEvent?.getIntegerValueField(.eventSourceUnixProcessID) ?? -1
            self.keyWindowID = event.windowNumber
            self.sample("key_down")
            self.persist()
        }
        window.contentView!.addSubview(keyboard)
        let button = NSButton(frame: NSRect(x: 40, y: 40, width: 120, height: 40))
        button.title = "Count"
        button.target = self
        button.action = #selector(count)
        window.contentView!.addSubview(button)
        let pointerButton = PointerButton(frame: NSRect(x: 200, y: 40, width: 180, height: 40))
        pointerButton.click = { self.pointerCount() }
        pointerButton.inputEvent = { self.sample($0) }
        pointerButton.unsupportedPress = {
            self.unsupportedPresses += 1
            self.persist()
        }
        window.contentView!.addSubview(pointerButton)
        pointerCounter.frame = NSRect(x: 200, y: 90, width: 180, height: 24)
        pointerCounter.count = { self.pointerClicks }
        window.contentView!.addSubview(pointerCounter)
        if background {
            window.orderBack(nil)
        } else {
            window.makeKeyAndOrderFront(nil)
        }
        window.makeFirstResponder(nil)
        for name in [NSApplication.didBecomeActiveNotification, NSApplication.didResignActiveNotification,
                     NSWindow.didBecomeKeyNotification, NSWindow.didResignKeyNotification,
                     NSWindow.didBecomeMainNotification, NSWindow.didResignMainNotification] {
            notificationObservers.append(NotificationCenter.default.addObserver(
                forName: name, object: nil, queue: .main) { _ in self.sample(name.rawValue) })
        }
        for name in [NSWorkspace.didActivateApplicationNotification, NSWorkspace.didDeactivateApplicationNotification] {
            notificationObservers.append(NSWorkspace.shared.notificationCenter.addObserver(
                forName: name, object: nil, queue: .main) { notification in
                    let affected = notification.userInfo?[NSWorkspace.applicationUserInfoKey] as? NSRunningApplication
                    self.sample(name.rawValue, affectedPID: affected?.processIdentifier)
                })
        }
        monitorTimer = Timer(timeInterval: 0.005, repeats: true) { _ in self.sample("poll") }
        RunLoop.main.add(monitorTimer!, forMode: .common)
        // A file command controls only this disposable fixture, never user apps.
        commandTimer = Timer.scheduledTimer(withTimeInterval: 0.05, repeats: true) { _ in
            let path = self.output + ".command"
            guard let command = try? String(contentsOfFile: path, encoding: .utf8) else { return }
            try? FileManager.default.removeItem(atPath: path)
            switch command {
            case "monitor_start":
                self.monitorEvents = []
                self.monitorOverflow = false
                self.monitorSamples = 0
                self.lastSnapshot = nil
                self.monitoring = true
                self.sample("start")
            case "monitor_stop":
                self.sample("stop")
                self.monitoring = false
            case "prepare_text":
                self.window.makeFirstResponder(self.field)
                (self.field.currentEditor() as? NSTextView)?.setSelectedRange(
                    NSRange(location: self.field.stringValue.utf16.count, length: 0))
            case "focus_keyboard": self.window.makeFirstResponder(self.keyboard)
            case "reset_keyboard":
                self.keyboard.text = ""
                self.keyboard.needsDisplay = true
            case "hidden": NSApp.hide(nil)
            case "show":
                NSApp.unhideWithoutActivation()
                self.window.orderBack(nil)
            case "minimized": self.window.miniaturize(nil)
            case "closed": self.window.close()
            case "activate":
                self.activate()
            case "move":
                self.window.setFrameOrigin(NSPoint(x: 260, y: 260))
            case "resize":
                self.window.setContentSize(NSSize(width: width + 120, height: 360))
            case "restore":
                NSApp.unhideWithoutActivation()
                self.window.setContentSize(NSSize(width: width, height: 280))
                self.window.setFrameOrigin(NSPoint(x: 140, y: 180))
            default:
                if command.hasPrefix("type_keyboard:") {
                    self.typeKeyboard(String(command.dropFirst("type_keyboard:".count)))
                } else if command.hasPrefix("activate_and_type:") {
                    let text = String(command.dropFirst("activate_and_type:".count))
                    self.activate()
                    DispatchQueue.main.asyncAfter(deadline: .now() + 0.15) {
                        self.window.makeFirstResponder(self.keyboard)
                        self.typeKeyboard(text)
                    }
                }
            }
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) {
                self.commandRevision += 1
                self.persist()
            }
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.3) {
            switch CommandLine.arguments.dropFirst(2).first {
            case "hidden": NSApp.hide(nil)
            case "minimized": self.window.miniaturize(nil)
            default: break
            }
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) { self.persist() }
        }
    }
    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool {
        if background {
            window.orderBack(nil)
        } else {
            window.makeKeyAndOrderFront(nil)
        }
        persist()
        return true
    }
    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { false }
    @objc func count() {
        clicks += 1
        recordClick()
    }
    @objc func pointerCount() {
        pointerClicks += 1
        pointerCounter.valueChanged()
        recordClick()
    }
    func recordClick() {
        sample("click")
        clickForegroundPID = NSWorkspace.shared.frontmostApplication?.processIdentifier ?? -1
        clickPointer = CGEvent(source: nil)?.location ?? .zero
        window.makeFirstResponder(nil)
        persist()
    }
    @objc func countMenu() {
        menuClicks += 1
        persist()
    }
    func control(_ control: NSControl, textView: NSTextView, doCommandBy commandSelector: Selector) -> Bool {
        guard control === field && commandSelector == #selector(NSResponder.insertNewline(_:)) else {
            return false
        }
        submissions += 1
        lastSubmitted = field.stringValue
        persist()
        return true
    }
    func controlTextDidBeginEditing(_ notification: Notification) { sample("text_begin"); persist() }
    func controlTextDidChange(_ notification: Notification) { sample("text_change"); persist() }
    func controlTextDidEndEditing(_ notification: Notification) { sample("text_end"); persist() }
    func activate() {
        // Explicit fixture setup uses LaunchServices. A background callback's
        // NSApp activation request can be declined without an error.
        window.makeKeyAndOrderFront(nil)
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/open")
        process.arguments = ["-a", Bundle.main.bundleURL.path]
        try! process.run()
        process.waitUntilExit()
        precondition(process.terminationStatus == 0, "fixture activation failed")
    }
    func typeKeyboard(_ text: String) {
        // These local AppKit events model concurrent fixture typing. They
        // never enter the global HID queue or another process's event queue.
        for character in text {
            guard let event = NSEvent.keyEvent(
                with: .keyDown, location: .zero, modifierFlags: [],
                timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber,
                context: nil, characters: String(character), charactersIgnoringModifiers: String(character),
                isARepeat: false, keyCode: 0) else { fatalError("fixture key event unavailable") }
            NSApp.sendEvent(event)
        }
        sample("fixture_typed")
        persist()
    }
    func snapshot() -> [String: Any] {
        let pointer = CGEvent(source: nil)?.location ?? .zero
        let draftEditor = field.currentEditor()
        let searchEditor = search.currentEditor()
        return [
            "foreground_pid": NSWorkspace.shared.frontmostApplication?.processIdentifier ?? -1,
            "active": NSApp.isActive,
            "running_active": NSRunningApplication.current.isActive,
            "window_server_foreground": windowServerForeground(ProcessInfo.processInfo.processIdentifier) as Any? ?? NSNull(),
            "key_window": NSApp.keyWindow?.windowNumber ?? -1,
            "main_window": NSApp.mainWindow?.windowNumber ?? -1,
            "is_key": window.isKeyWindow,
            "is_main": window.isMainWindow,
            "draft_focused": draftEditor != nil && window.firstResponder === draftEditor,
            "search_focused": searchEditor != nil && window.firstResponder === searchEditor,
            "keyboard_focused": window.firstResponder === keyboard,
            "keyboard_length": keyboard.text.count,
            "pointer": pointValues(pointer),
            "pointer_valid": pointer.x.isFinite && pointer.y.isFinite
        ]
    }
    func sample(_ kind: String, affectedPID: pid_t? = nil) {
        guard monitoring else { return }
        monitorSamples += 1
        let current = snapshot()
        var comparison = current
        // Background-only acceptance allows concurrent user pointer motion.
        // Keep activity notifications and action samples without rewriting a
        // growing timeline on every unrelated hardware-pointer movement.
        if background { comparison.removeValue(forKey: "pointer") }
        let encoded = try! JSONSerialization.data(withJSONObject: comparison, options: [.sortedKeys])
        if kind == "poll" && encoded == lastSnapshot { return }
        lastSnapshot = encoded
        guard monitorEvents.count < 4096 else { monitorOverflow = true; return }
        var event = current
        event["kind"] = kind
        event["time"] = ProcessInfo.processInfo.systemUptime
        if kind == "mouse_move" {
            event["mouse_moves"] = mouseMoves
            event["move_source_pid"] = moveSourcePID
            event["move_window_id"] = moveWindowID
            event["move_point"] = pointValues(movePoint)
        }
        if kind == "key_down" {
            event["key_source_pid"] = keySourcePID
            event["key_window_id"] = keyWindowID
            event["key_events"] = keyboard.keyEvents
        }
        if let affectedPID { event["affected_pid"] = affectedPID }
        monitorEvents.append(event)
        persist()
    }
    func persist() {
        let editor = field.currentEditor()
        let data = try! JSONSerialization.data(withJSONObject: [
            "clicks": clicks,
            "menu_clicks": menuClicks,
            "pointer_clicks": pointerClicks,
            "mouse_moves": mouseMoves,
            "view_mouse_moves": viewMouseMoves,
            "move_source_pid": moveSourcePID,
            "move_window_id": moveWindowID,
            "move_point": pointValues(movePoint),
            "unsupported_presses": unsupportedPresses,
            "text": field.stringValue,
            "search": search.stringValue,
            "keyboard_text": keyboard.text,
            "key_events": keyboard.keyEvents,
            "shortcuts": keyboard.shortcuts,
            "key_source_pid": keySourcePID,
            "key_window_id": keyWindowID,
            "submissions": submissions,
            "last_submitted": lastSubmitted,
            "isDraftFocused": editor != nil && window.firstResponder === editor,
            "click_foreground_pid": clickForegroundPID,
            "foreground_pid": NSWorkspace.shared.frontmostApplication?.processIdentifier ?? -1,
            "click_pointer": pointValues(clickPointer),
            "pid": ProcessInfo.processInfo.processIdentifier,
            "window_id": window.windowNumber,
            "command_revision": commandRevision,
            "focus": snapshot(),
            "monitoring": monitoring,
            "monitor_events": monitorEvents,
            "monitor_samples": monitorSamples,
            "monitor_overflow": monitorOverflow,
            "window_frame": [window.frame.minX, window.frame.minY, window.frame.width, window.frame.height],
            "hidden": NSApp.isHidden,
            "minimized": window.isMiniaturized,
            "visible": window.isVisible
        ])
        try! data.write(to: URL(fileURLWithPath: output), options: .atomic)
    }
}

let app = NSApplication.shared
let delegate = Fixture()
app.delegate = delegate
app.setActivationPolicy(.regular)
app.run()
