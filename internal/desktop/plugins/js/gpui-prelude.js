
globalThis.__gpui = (() => {
  const methods = {};

  // Two prototypes, and a measured reason for having two.
  //
  // QuickJS reports a missing method as `TypeError: not a function` without
  // naming the property, so a mistyped style name would arrive with no clue —
  // and giving the call site a real diagnostic is the entire reason the style
  // surface is methods rather than a string of class names (§13.2).
  //
  // A Proxy prototype solves that, but the M0 benchmark measured it at ~30% of
  // the whole description pass (1.09 ms → 1.42 ms for 443 nodes). So the fast
  // prototype is the default, and a render that fails with "not a function" is
  // re-run once against the diagnostic prototype purely to produce the message.
  // Errors are rare; a 30% tax on every frame is not.
  const diagnostic = new Proxy(methods, {
    get(target, name, receiver) {
      const found = Reflect.get(target, name, receiver);
      if (found !== undefined) return found;
      if (typeof name !== "string" || name.startsWith("__")) return undefined;
      return () => __unknown(name);
    },
  });

  globalThis.__diagnostics = false;

  const element = (id) => {
    const object = Object.create(globalThis.__diagnostics ? diagnostic : methods);
    object.__id = id;
    return object;
  };

  const define = (name) => {
    methods[name] = function (...args) {
      __apply(this.__id, name, args);
      return this;
    };
  };

  // Styles do not go through `__apply`, and the reason is arithmetic. They are
  // most of what a description records, and the generic form above pays three
  // times over for information the prelude already has: it allocates a rest
  // array to hold arguments a style never has more than one of, it sends a
  // method name that has to be copied into a Rust string, and it arrives at a
  // dispatcher that has to look that string back up in a table. Closing over
  // the table index instead removes all three, and measured at roughly half
  // the cost of a recorded style call. `define` stays for the behaviours,
  // where the argument shapes vary and a second form would not repay itself.
  const defineNullaryStyle = (name, index) => {
    methods[name] = function () {
      __applyNullaryStyle(this.__id, index);
      return this;
    };
  };
  const defineParamStyle = (name, index) => {
    methods[name] = function (value) {
      __applyParamStyle(this.__id, index, value);
      return this;
    };
  };

  for (let i = 0; i < __nullaryStyles.length; i += 1) {
    defineNullaryStyle(__nullaryStyles[i], __nullaryStyleIndexes[i]);
  }
  for (let i = 0; i < __paramStyles.length; i += 1) {
    defineParamStyle(__paramStyles[i], i);
  }
  for (const name of __behaviorNames) define(name);

  // Attaching is the other call a description makes once per element, and it
  // carries no argument a `Bridged` could describe — two element ids, both
  // already numbers. It gets an entry point of its own for the same reason the
  // styles do.
  //
  // A retained child view is a child too — the same shape GPUI has, where an
  // `Entity<V>` is itself renderable — so `.child(handle)` mounts one. An
  // element always carries `__id`, so the branch costs the hot path one
  // `undefined` test and the slow side is the case that used to fail with
  // rquickjs' "Error converting from undefined to f64".
  const childId = (child) => {
    const id = child?.__id;
    if (id !== undefined) return id;
    // A string is an element. GPUI implements `IntoElement` for `&str`,
    // `String` and `SharedString`, so `.child("hello")` is how text is written
    // there, and the style comes from the element holding it.
    const kind = typeof child;
    if (kind === "string" || kind === "number" || kind === "boolean") {
      return __text(String(child));
    }
    // A template's sentinel, reached only while a template body is running.
    // Checked after elements and strings so the ordinary description pays
    // nothing for it.
    if (child?.__slot !== undefined) return __text_slot(child.__slot);
    if (child?.__entity) return __child_view(child.__handle);
    throw new TypeError(
      "child(value) expects an element, a string, or an entity from cx.new(Class, props)",
    );
  };
  methods.child = function (child) {
    __attach(this.__id, childId(child));
    return this;
  };
  methods.children = function (list) {
    for (const child of list) __attach(this.__id, childId(child));
    return this;
  };
  // A named slot. The element is consumed exactly as `child` consumes one, so
  // it cannot also be added to the tree — which is the point: the component
  // renders it somewhere of its own, or not at all.
  const slot = (name) =>
    function (element) {
      __apply(this.__id, name, [element.__id]);
      return this;
    };

  methods.content = slot("content");
  methods.trigger = slot("trigger");

  // A number input's three. Every element carries them, as it carries `content`
  // and `trigger`, because one prototype is shared by all of them.
  methods.input = slot("input");
  methods.decrement_button = slot("decrement_button");
  methods.increment_button = slot("increment_button");

  // An avatar's two. Base renders the image, or the fallback when there is no
  // image, and never both.
  methods.image = slot("image");
  methods.fallback = slot("fallback");

  // An accordion item's two. Both are read back for their own type rather than
  // rendered, so both must be the part they name.
  methods.header = slot("header");
  methods.footer = slot("footer");
  methods.panel = slot("panel");

  // Focus is held by handle, so the element records the handle rather than the
  // wrapper object around it — the same unwrapping `Input.new(state)` does.
  methods.track_focus = function (handle) {
    if (typeof handle?.__handle !== "number") {
      throw new TypeError(
        "track_focus(handle) expects a FocusHandle from cx.focus_handle(), not a name or an element",
      );
    }
    __apply(this.__id, "track_focus", [handle.__handle]);
    return this;
  };
  // A virtualized list's scroll position, unwrapped exactly as `track_focus`
  // unwraps a focus handle, and checked here for the same reason: a name or an
  // element would be dropped on the Rust side and the list would simply never
  // respond to `scroll_to_item`.
  methods.track_scroll = function (handle) {
    if (typeof handle?.__handle !== "number") {
      throw new TypeError(
        "track_scroll(handle) expects a VirtualListScrollHandle from VirtualListScrollHandle.new()",
      );
    }
    __apply(this.__id, "track_scroll", [handle.__handle]);
    return this;
  };
  // The second handle a combobox root needs: the one the keyboard moves to when
  // the surface opens. Checked here for the same reason `track_focus` is — a
  // name or an element would otherwise be dropped on the Rust side and the
  // focus would simply never move.
  methods.content_focus_handle = function (handle) {
    if (typeof handle?.__handle !== "number") {
      throw new TypeError(
        "content_focus_handle(handle) expects a FocusHandle from cx.focus_handle(), not a name or an element",
      );
    }
    __apply(this.__id, "content_focus_handle", [handle.__handle]);
    return this;
  };
  // State styles reuse the ordinary style methods on a detached element, so
  // there is no second grammar for "what a style is".
  const state = (name) =>
    function (declare) {
      const target = element(__state(this.__id, name));
      declare(target);
      return this;
    };

  methods.hover = state("hover");
  methods.active = state("active");
  methods.focus = state("focus");
  // Not a state: the filled part of a slider, declared the same way because
  // it is the same thing — a detached element collecting styles. The shell
  // positions the box; this says what it looks like.
  methods.range_style = state("range_style");
  // An OtpInput's cells. Not states either: the shell decides which template
  // a cell gets, from the state, on every frame — but they are declared the
  // same way, because what they collect is the same thing.
  methods.cell_style = state("cell_style");
  methods.cell_active_style = state("cell_active_style");
  methods.caret_style = state("caret_style");

  // The argument checks are here rather than only on the Rust side because a
  // list built with the pieces in the wrong order — a render function where the
  // sizes go — would otherwise fail as a type error naming neither.
  // The three checks every lazy list makes. Only the render hint differs:
  // `list` is called per item, the other two per visible range.
  const checkListArgs = (shape, item_count, get_key, render, renderHint) => {
    if (!Number.isInteger(item_count) || item_count < 0) {
      throw new TypeError(shape + " needs a whole, non-negative item_count");
    }
    if (typeof get_key !== "function") {
      throw new TypeError(
        shape + " needs get_key(index) to return each item's stable string key",
      );
    }
    if (typeof render !== "function") {
      throw new TypeError(shape + " needs a render function; it is called " + renderHint);
    }
  };

  const RANGE_HINT = "once per visible range, not once per item";

  const virtualList = (build, name) => (id, item_count, item_sizes, get_key, render) => {
    const shape = name + "(id, item_count, item_sizes, get_key, render)";
    checkListArgs(shape, item_count, get_key, render, RANGE_HINT);
    if (Array.isArray(item_sizes) && item_sizes.length !== item_count) {
      throw new TypeError(
        shape + " was given " + item_sizes.length + " item sizes for " + item_count +
          " items; pass one number for a uniform extent, or one per item",
      );
    }
    return element(build(String(id), item_count, item_sizes, get_key, render));
  };

  // `list` and `uniform_list`: GPUI's own lazy lists. Both cross the boundary
  // the way a virtual list does -- one renderer per visible range -- so a
  // `list` renderer written per item is folded into a range here, once, rather
  // than teaching the host a second calling convention.
  const lazyList = (build, name, perItem) => (id, item_count, get_key, render) => {
    const shape = name + "(id, item_count, get_key, render)";
    checkListArgs(
      shape,
      item_count,
      get_key,
      render,
      perItem ? "once per item on screen, with the item's index" : RANGE_HINT,
    );
    const describe = perItem
      ? (range, cx) => {
          const items = [];
          for (let index = range.start; index < range.end; index++) {
            items.push(render(index, cx));
          }
          return items;
        }
      : render;
    return element(build(String(id), item_count, get_key, describe));
  };

  const finiteNonNegative = (value, name) => {
    if (typeof value !== "number" || !Number.isFinite(value) || value < 0) {
      throw new TypeError(name + " must be a finite non-negative number");
    }
    return value;
  };

  const finitePositive = (value, name) => {
    if (typeof value !== "number" || !Number.isFinite(value) || value <= 0) {
      throw new TypeError(name + " must be a finite positive number");
    }
    return value;
  };

  // A table index is one-based and whole. 0 and 1.5 are not values a screen
  // reader rounds off; they are cells announced in the wrong column, so they
  // are refused at the call site rather than cast quietly on the Rust side.
  const oneBased = (value, name) => {
    if (!Number.isInteger(value) || value < 1) {
      throw new TypeError(name + " must be a whole number of at least 1");
    }
    return value;
  };

  methods.transition = function (property, options) {
    property = String(property);
    if (!["opacity", "width", "height", "left", "top"].includes(property)) {
      throw new TypeError(
        "transition(property, policy) supports opacity, width, height, left or top; got " +
          JSON.stringify(property),
      );
    }
    const policy = typeof options === "number" ? { duration: options } : (options ?? {});
    const duration = finiteDuration(policy.duration ?? 0, "transition duration");
    const delay = finiteDuration(policy.delay ?? 0, "transition delay");
    const easing = policy.easing ?? "ease-out";
    if (!["linear", "ease-in", "ease-out", "ease-in-out"].includes(easing)) {
      throw new TypeError(
        "transition easing must be linear, ease-in, ease-out or ease-in-out; got " +
          JSON.stringify(easing),
      );
    }
    __apply(this.__id, "transition", [
      property,
      duration,
      delay,
      easing,
    ]);
    return this;
  };

  methods.spring = function (property, options) {
    property = String(property);
    if (!["opacity", "width", "height", "left", "top"].includes(property)) {
      throw new TypeError(
        "spring(property, policy) supports opacity, width, height, left or top; got " +
          JSON.stringify(property),
      );
    }
    const policy = options ?? {};
    const response = finiteDuration(policy.response ?? 250, "spring response");
    const damping = finiteNonNegative(policy.damping ?? 1, "spring damping");
    const epsilon = finitePositive(policy.epsilon ?? 0.001, "spring epsilon");
    __apply(this.__id, "spring", [
      property,
      response,
      damping,
      epsilon,
    ]);
    return this;
  };

  // Announced, not laid out: `axis` sets the semantic orientation of a
  // grouping container and never turns it into a row or a column. Checked here
  // so a typo reports at the call site instead of silently announcing the
  // container's default.
  methods.axis = function (value) {
    value = String(value);
    if (!["horizontal", "vertical"].includes(value)) {
      throw new TypeError(
        "axis(value) must be horizontal or vertical; got " + JSON.stringify(value),
      );
    }
    __apply(this.__id, "axis", [value]);
    return this;
  };

  // A bar's visibility policy. Unset follows the theme, which is what every
  // other scrollbar in the application does; the three named modes are checked
  // here so a typo reports at the call site instead of silently falling back.
  methods.mode = function (value) {
    value = String(value);
    if (!["scrolling", "hover", "always"].includes(value)) {
      throw new TypeError(
        "mode(value) must be scrolling, hover or always; got " + JSON.stringify(value),
      );
    }
    __apply(this.__id, "mode", [value]);
    return this;
  };

  // The content size a bar measures its thumb against. Both halves are
  // required: one axis sized by the script and the other by the scroll area is
  // a thumb that lies about one of them.
  methods.scroll_size = function (width, height) {
    __apply(this.__id, "scroll_size", [
      finiteNonNegative(width, "scroll_size width"),
      finiteNonNegative(height, "scroll_size height"),
    ]);
    return this;
  };

  // Which corner of an anchored surface is pinned to its trigger. The names
  // come from the host so that the check here, the parser behind it and the
  // union in gpui-kit.d.ts cannot disagree. Checked at the call site because an
  // unrecognized anchor would otherwise open the surface in the component's
  // default corner, which looks like a positioning bug rather than a typo.
  methods.anchor = function (value) {
    value = String(value);
    if (!__anchorNames.includes(value)) {
      throw new TypeError(
        "anchor(value) must be one of " +
          __anchorNames.join(", ") +
          "; got " +
          JSON.stringify(value),
      );
    }
    __apply(this.__id, "anchor", [value]);
    return this;
  };

  methods.frame_budget = function (milliseconds) {
    __apply(this.__id, "frame_budget", [
      finitePositive(milliseconds, "frame_budget"),
    ]);
    return this;
  };

  // A popover opened by the wrong button is silence, not a visual mistake, so
  // an unknown button name is refused rather than falling back to the left one.
  methods.mouse_button = function (value) {
    value = String(value);
    if (!["left", "right", "middle"].includes(value)) {
      throw new TypeError(
        "mouse_button(value) must be left, right or middle; got " + JSON.stringify(value),
      );
    }
    __apply(this.__id, "mouse_button", [value]);
    return this;
  };

  // Milliseconds, as everywhere else a script names a duration.
  const finiteDuration = (value, name) => {
    value = finiteNonNegative(value, name);
    if (value > 86400000) throw new RangeError(name + " must not exceed 86400000 milliseconds");
    return value;
  };
  const delay = (name) =>
    function (ms) {
      __apply(this.__id, name, [finiteDuration(ms, name)]);
      return this;
    };

  methods.open_delay = delay("open_delay");
  methods.close_delay = delay("close_delay");

  // Two arguments rather than a range literal, which JavaScript has no spelling
  // for. The floor is required — a panel always has one, and base's own is
  // 100px — while the ceiling is optional, because most panels have none.
  methods.size_range = function (min, max) {
    const args = [finiteNonNegative(min, "size_range min")];
    if (max !== undefined && max !== null) {
      args.push(finiteNonNegative(max, "size_range max"));
    }
    __apply(this.__id, "size_range", args);
    return this;
  };

  // `size` and `visible` on a resizable panel are base's own inherent builders
  // — the initial size along the group's axis, and whether the panel is drawn —
  // and in Rust each shadows the `Styled` method of the same name for that one
  // type. Own properties on the panel object shadow the shared prototype by the
  // same mechanism, so a script writes what the Rust writes and `.size(200)`
  // still means width-and-height, `.visible()` still means `visibility`,
  // everywhere else.
  const resizablePanel = () => {
    const object = element(__resizable_panel());
    object.size = function (pixels) {
      __apply(this.__id, "panel_size", [finiteNonNegative(pixels, "resizable_panel size")]);
      return this;
    };
    object.visible = function (value) {
      __apply(this.__id, "panel_visible", [Boolean(value)]);
      return this;
    };
    return object;
  };

  const coordinate = (value, name) => {
    if (typeof value === "number" && Number.isFinite(value)) return value;
    if (typeof value === "string" && /^-?(?:\d+(?:\.\d*)?|\.\d+)%$/.test(value)) return value;
    throw new TypeError(name + " must be a finite pixel number or percentage string");
  };

  const background = (kind, values, opacityFactor = 1, colorSpace = "srgb") => Object.freeze({
    __background: true,
    kind,
    values: Object.freeze(values),
    opacityFactor,
    colorSpace,
    opacity(factor) {
      return background(kind, values, finiteNonNegative(factor, "background opacity"), colorSpace);
    },
    color_space(space) {
      space = String(space).toLowerCase();
      if (!['srgb', 'oklab'].includes(space)) throw new TypeError("background color_space must be srgb or oklab");
      return background(kind, values, opacityFactor, space);
    },
  });

  const asBackground = (value) => value?.__background
    ? value
    : background("solid", [String(value)]);

  const pathBuilder = (fill, width) => {
    const commands = [];
    const builder = {};
    const command = (name, arity, coordinateCount = arity) => (...args) => {
      if (args.length < arity) throw new TypeError(name + " expects at least " + arity + " argument(s)");
      for (let index = 0; index < coordinateCount; index++) coordinate(args[index], name + " coordinate");
      commands.push(Object.freeze([name, ...args]));
      return builder;
    };
    builder.move_to = command("move_to", 2);
    builder.line_to = command("line_to", 2);
    builder.curve_to = command("curve_to", 4);
    builder.cubic_bezier_to = command("cubic_bezier_to", 6);
    builder.arc_to = (...args) => {
      if (args.length < 7) throw new TypeError("arc_to expects at least 7 argument(s)");
      coordinate(args[0], "arc x radius");
      coordinate(args[1], "arc y radius");
      if (typeof args[2] !== "number" || !Number.isFinite(args[2])) throw new TypeError("arc rotation must be finite");
      coordinate(args[5], "arc destination x");
      coordinate(args[6], "arc destination y");
      commands.push(Object.freeze(["arc_to", ...args]));
      return builder;
    };
    builder.close = () => { commands.push(Object.freeze(["close"])); return builder; };
    builder.dash_array = (values) => {
      if (fill) throw new TypeError("dash_array is only available on stroke paths");
      if (!Array.isArray(values) || values.some((value) => typeof value !== "number" || !Number.isFinite(value) || Math.fround(value) <= 0)) {
        throw new TypeError("dash_array(values) expects positive finite pixel numbers");
      }
      commands.push(Object.freeze(["dash_array", ...values]));
      return builder;
    };
    builder.add_polygon = (points, closed = true) => {
      if (!Array.isArray(points) || points.length === 0) throw new TypeError("add_polygon(points) expects a non-empty array");
      points.forEach((point, index) => {
        if (!Array.isArray(point) || point.length < 2) throw new TypeError("each polygon point must be [x, y]");
        command(index === 0 ? "move_to" : "line_to", 2)(point[0], point[1]);
      });
      if (closed) builder.close();
      return builder;
    };
    builder.build = () => Object.freeze({
      __path: true,
      fill,
      width,
      commands: Object.freeze(commands.slice()),
    });
    return builder;
  };

  const paintPath = (pathValue, paintValue) => {
    if (!pathValue?.__path) throw new TypeError("window.paint_path(path, background) expects a Path built by PathBuilder");
    const paint = asBackground(paintValue);
    const object = element(__path(
      pathValue.fill,
      paint.kind,
      paint.values.map(String),
      paint.opacityFactor,
      paint.colorSpace,
      pathValue.width,
    ));
    for (const [name, ...args] of pathValue.commands) __apply(object.__id, name, args);
    return object;
  };

  // Mirrors GPUI's `FluentBuilder::map`: unlike an ordinary element method,
  // the callback decides the return type. Keeping this in JavaScript also
  // avoids a host crossing for a purely fluent control-flow helper.
  methods.map = function (transform) {
    return transform(this);
  };

  methods.when = function (condition, branch) {
    if (!condition) return this;
    const produced = branch(this);
    if (produced === undefined || produced === null) {
      throw new Error("when(...) must return the element");
    }
    return produced;
  };

  // Retained state is held by handle; the methods close over it so nothing has
  // to read it back off `this`.
  const tokenStateMethods = (handle, invoke) => Object.fromEntries([
    "content", "tokens", "replace_with_token", "replace_range_with_token",
    "set_selected_range", "replace"
  ].map(name => [name, (...args) => invoke(handle, name, args)]));
  // A value is plain text or a content snapshot with tokens.
  const setValue = (handle, setText, invoke) => (next) =>
    next !== null && typeof next === "object"
      ? invoke(handle, "set_value", [next])
      : setText(handle, String(next ?? ""));
  const inputState = (handle) => ({
    ...tokenStateMethods(handle, __input_token_call),
    __handle: handle,
    value: () => __input_value(handle),
    set_value: setValue(handle, __input_set_value, __input_token_call),
    on: (event, handler) => __input_on(handle, String(event), handler),
    // What makes a text state a number state. There is no `NumberInputState`:
    // the step, the bounds and the mask are fields on this one, so a plain
    // input becomes a numeric one by being told about them.
    set_step: (step) => __input_set_step(handle, step === null || step === undefined ? null : Number(step)),
    set_min: (min) => __input_set_min(handle, min === null || min === undefined ? null : Number(min)),
    set_max: (max) => __input_set_max(handle, max === null || max === undefined ? null : Number(max)),
    set_masked: (masked) => __input_set_masked(handle, Boolean(masked)),
    set_loading: (loading) => __input_set_loading(handle, Boolean(loading)),
    release: () => __input_release(handle),
  });

  // The multi-line state shares almost all of its surface with the single-line
  // one, and adds the calls that only mean anything once text has more than
  // one line.
  const textareaState = (handle) => ({
    ...tokenStateMethods(handle, __textarea_token_call),
    __handle: handle,
    value: () => __textarea_value(handle),
    set_value: setValue(handle, __textarea_set_value, __textarea_token_call),
    on: (event, handler) => __textarea_on(handle, String(event), handler),
    set_rows: (rows) => __textarea_set_rows(handle, oneBased(rows, "set_rows(rows)")),
    set_auto_grow: (min_rows, max_rows) =>
      __textarea_set_auto_grow(
        handle,
        oneBased(min_rows, "set_auto_grow(min_rows, max_rows) min_rows"),
        oneBased(max_rows, "set_auto_grow(min_rows, max_rows) max_rows"),
      ),
    set_soft_wrap: (wrap) => __textarea_set_soft_wrap(handle, Boolean(wrap)),
    set_submit_on_enter: (submit) => __textarea_set_submit_on_enter(handle, Boolean(submit)),
    release: () => __textarea_release(handle),
  });

  // A slider's value crosses as an array either way, because a bare number
  // cannot say whether the script meant one thumb or two.
  const sliderValue = (values) => (values.length === 1 ? values[0] : values);
  const sliderValues = (value, api) => {
    const finite = (each) => typeof each === "number" && Number.isFinite(each);
    if (Array.isArray(value)) {
      if (value.length !== 2 || !value.every(finite)) {
        throw new TypeError(api + " expects a finite number, or a pair [start, end] of them");
      }
      return [value[0], value[1]];
    }
    if (!finite(value)) {
      throw new TypeError(api + " expects a finite number, or a pair [start, end] of them");
    }
    return [value];
  };

  const sliderState = (handle) => ({
    __handle: handle,
    value: () => sliderValue(__slider_value(handle)),
    set_value: (next) => __slider_set_value(handle, sliderValues(next, "set_value(value)")),
    min_value: () => __slider_bounds(handle)[0],
    max_value: () => __slider_bounds(handle)[1],
    step_value: () => __slider_bounds(handle)[2],
    on: (event, handler) => __slider_on(handle, String(event), handler),
    release: () => __slider_release(handle),
  });

  // A one-time code. `len` is read rather than set: base fixes it when the
  // state is created and offers no setter, because it is what the state is.
  const otpState = (handle) => ({
    __handle: handle,
    value: () => __otp_value(handle),
    set_value: (next) => __otp_set_value(handle, String(next ?? "")),
    len: () => __otp_len(handle),
    is_masked: () => __otp_is_masked(handle),
    set_masked: (masked) => __otp_set_masked(handle, Boolean(masked)),
    focus: () => __otp_focus(handle),
    on: (event, handler) => __otp_on(handle, String(event), handler),
    release: () => __otp_release(handle),
  });

  // A calendar's month, and the date chosen in it.
  //
  // `month_days()` is the reason this is bound at all: which dates fall in
  // which week, where the neighbouring months' days go, and how many weeks
  // this month needs. Everything else here is what it takes to move that grid
  // and read what was picked from it.
  //
  // The wire is a flat two-slot array either way; the narrowing to `null`, a
  // string or a pair happens here so a script never sees the flat form.
  // Reads the variant off the slot count, exactly as `calendarParts` writes
  // it. Looking at whether the second slot is null instead would collapse a
  // range whose end is not chosen yet into a bare string — losing the one
  // thing that distinguishes it from a single date, and losing it only in the
  // half-finished state a range picker spends most of its time in.
  const calendarDate = (parts) => {
    if (parts.length === 2) return [parts[0] ?? null, parts[1] ?? null];
    return parts[0] ?? null;
  };
  // How many slots go over is what says which `Date` was meant, and it has to:
  // a single day and a range whose end is not chosen yet are different states
  // to base — `is_single`, `is_complete` and `is_in_range` all branch on it —
  // but they read back as the same string, so the wire cannot recover the
  // difference from the values alone.
  const calendarParts = (value, api) => {
    if (value === null || value === undefined) return [null];
    if (Array.isArray(value)) {
      if (value.length !== 2) {
        throw new TypeError(`${api} range expects a two-element array [start, end]`);
      }
      return [value[0] ?? null, value[1] ?? null];
    }
    if (typeof value !== "string") {
      throw new TypeError(`${api} expects null, a "YYYY-MM-DD" string, or a pair of those`);
    }
    return [value];
  };
  const calendarState = (handle) => ({
    __handle: handle,
    month_days: () => __calendar_month_days(handle),
    year: () => __calendar_year(handle),
    month: () => __calendar_month(handle),
    today: () => __calendar_today(handle),
    value: () => calendarDate(__calendar_value(handle)),
    set_value: (next) => __calendar_set_value(handle, calendarParts(next, "set_value(value)")),
    next_month: () => __calendar_next_month(handle),
    prev_month: () => __calendar_prev_month(handle),
    on: (event, handler) =>
      __calendar_on(handle, String(event), (parts, cx) => handler(calendarDate(parts), cx)),
    release: () => __calendar_release(handle),
  });

  const focusHandle = (handle) => ({
    __handle: handle,
    focus: () => __focus_focus(handle),
    is_focused: () => __focus_is_focused(handle),
    release: () => __focus_release(handle),
  });

  // `__entity` is the discriminant `.child()` needs: every retained handle in
  // this API is a `{__handle: number}` wrapper, so a focus handle and an entity
  // are otherwise indistinguishable, and mounting the wrong one would report a
  // released view rather than the mistake that was made.
  const entity = (handle) => ({
    __entity: true,
    __handle: handle,
    set_props: (props) => __view_set_props(handle, props),
    release: () => __view_release(handle),
  });

  // A dockable layout, and the commands its chrome carries.
  //
  // Retained for a reason none of the other handles share: the layout is what
  // the *user* changed. A drag, a resize, a closed tab and a collapsed dock all
  // happen without this script rendering, so a dock rebuilt from a description
  // would put every one of them back the way the last render described it.
  const dockPlacement = (value, api) => {
    const name = String(value ?? "center");
    if (!["center", "left", "right", "bottom"].includes(name)) {
      throw new TypeError(api + ' expects "center", "left", "right" or "bottom"');
    }
    return name;
  };

  const wholeAt = (value, api) => {
    if (!Number.isSafeInteger(value) || value < 0) {
      throw new TypeError(api + " expects a whole, non-negative position");
    }
    return value;
  };

  const finiteDockNumber = (value, api) => {
    if (typeof value !== "number" || !Number.isFinite(value)) {
      throw new TypeError(api + " expects a finite number");
    }
    return value;
  };

  const nonNegativeDockNumber = (value, api) => {
    const number = finiteDockNumber(value, api);
    if (number < 0) throw new RangeError(api + " expects a non-negative number");
    return number;
  };

  const dockBounds = (value) => {
    const api = "add_panel(view, options) bounds";
    if (value === undefined || value === null) return null;
    if (typeof value !== "object") throw new TypeError(api + " expects an object");
    return {
      x: finiteDockNumber(value.x, api + ".x"),
      y: finiteDockNumber(value.y, api + ".y"),
      width: nonNegativeDockNumber(value.width, api + ".width"),
      height: nonNegativeDockNumber(value.height, api + ".height"),
    };
  };

  // Every chrome handler is given base's own state for one container, with the
  // area it belongs to added on this side — the commands need it, and this side
  // already knows it, so it never has to cross.
  const dockTarget = (value, api) => {
    const handle = value?.__dock;
    if (typeof handle !== "number") {
      throw new TypeError(
        api + " expects the group or dock your chrome handler was given as its first argument",
      );
    }
    return handle;
  };

  const groupNode = (group, api) => {
    if (typeof group?.node !== "number") {
      throw new TypeError(api + " expects a tab group, which is what tab_bar and empty_group are given");
    }
    return group.node;
  };

  // Commands, not callbacks. A chrome handler runs once per frame for as long
  // as the dock is on screen, so a handler registered inside one would pile up
  // exactly the way a virtual list's row handlers would. A command carries no
  // script value at all: it names a container and what to ask it, and base does
  // the rest.
  methods.select_tab = function (group, index) {
    const api = "select_tab(group, index)";
    __apply(this.__id, "select_tab", [dockTarget(group, api), groupNode(group, api), wholeAt(index, api)]);
    return this;
  };
  methods.close_panel = function (group, panel) {
    const api = "close_panel(group, panel_id)";
    __apply(this.__id, "close_panel", [dockTarget(group, api), groupNode(group, api), Number(panel)]);
    return this;
  };
  methods.toggle_zoom = function (group) {
    const api = "toggle_zoom(group)";
    __apply(this.__id, "toggle_zoom", [dockTarget(group, api), groupNode(group, api)]);
    return this;
  };
  methods.drag_tab = function (group, index) {
    const api = "drag_tab(group, index)";
    __apply(this.__id, "drag_tab", [dockTarget(group, api), groupNode(group, api), wholeAt(index, api)]);
    return this;
  };
  // The one command with an optional argument: a tab bar that names no slot
  // means "append", which is what a drop past the last tab is.
  methods.drop_tab = function (group, index) {
    const api = "drop_tab(group, index)";
    const at = index === undefined || index === null ? null : wholeAt(index, api);
    __apply(this.__id, "drop_tab", [dockTarget(group, api), groupNode(group, api), at]);
    return this;
  };
  methods.toggle_dock = function (dock) {
    const api = "toggle_dock(dock)";
    __apply(this.__id, "toggle_dock", [dockTarget(dock, api), dockPlacement(dock?.placement, api)]);
    return this;
  };
  methods.resize_dock = function (dock) {
    const api = "resize_dock(dock)";
    __apply(this.__id, "resize_dock", [dockTarget(dock, api), dockPlacement(dock?.placement, api)]);
    return this;
  };
  const DOCK_CHROME = [
    "tab_bar",
    "empty_group",
    "drop_indicator",
    "dock",
  ];

  // The four hooks are own properties of the one element that has them, rather
  // than prototype methods: every other element in the tree would otherwise
  // carry a `dock` and a `tab_bar` that mean nothing on it.
  const dockAreaElement = (area) => {
    const handle = area?.__dock;
    if (typeof handle !== "number") {
      throw new TypeError("dock_area(area) expects a DockArea from DockArea.new(id)");
    }
    const object = element(__dock_area_element(handle));
    for (const hook of DOCK_CHROME) {
      object[hook] = function (handler) {
        if (typeof handler !== "function") {
          throw new TypeError(hook + "(handler) expects a function returning an element");
        }
        __apply(this.__id, hook, [
          (payload, cx) => {
            payload.__dock = handle;
            return handler(payload, cx);
          },
        ]);
        return this;
      };
    }
    return object;
  };

  const dockArea = (handle) => ({
    __dock: handle,
    __handle: handle,
    add_panel: (view, options) => {
      if (typeof view?.__handle !== "number" || !view.__entity) {
        throw new TypeError(
          "add_panel(view, options) expects a view from cx.new(Class): a panel's body is a view, not an element",
        );
      }
      const settings = options ?? {};
      if (typeof settings.name !== "string" || settings.name.length === 0) {
        throw new TypeError(
          "add_panel(view, options) needs a name: it is what the panel is filed under in a saved layout, and what register_panel finds it again by",
        );
      }
      // No id comes back: the view is still being constructed when this is
      // called, so the panel it will hold does not exist yet. `panels()` names
      // every panel once the call that added them has returned.
      __dock_add_panel(handle, view.__handle, {
        name: settings.name,
        placement: dockPlacement(settings.placement, "add_panel placement"),
        size:
          settings.size === undefined || settings.size === null
            ? null
            : nonNegativeDockNumber(settings.size, "add_panel(view, options) size"),
        bounds: dockBounds(settings.bounds),
        closable: settings.closable === undefined ? true : Boolean(settings.closable),
        zoomable: settings.zoomable === undefined ? true : Boolean(settings.zoomable),
        visible: settings.visible === undefined ? true : Boolean(settings.visible),
      });
    },
    remove_panel: (id) => __dock_remove_panel(handle, wholeAt(id, "remove_panel(id)")),
    panels: () => JSON.parse(__dock_panels(handle)),
    // The layout as plain data, and back. `load` takes effect once this call
    // has returned: rebuilding a panel constructs a view, and a view cannot be
    // constructed while script is running.
    dump: () => JSON.parse(__dock_dump(handle)),
    load: (state) => __dock_load(handle, state),
    has_dock: (placement) => __dock_has(handle, dockPlacement(placement, "has_dock(placement)")),
    is_dock_open: (placement) =>
      __dock_is_open(handle, dockPlacement(placement, "is_dock_open(placement)")),
    toggle_dock: (placement) =>
      __dock_toggle(handle, dockPlacement(placement, "toggle_dock(placement)")),
    remove_dock: (placement) =>
      __dock_remove(handle, dockPlacement(placement, "remove_dock(placement)")),
    dock_size: (placement) => __dock_size(handle, dockPlacement(placement, "dock_size(placement)")),
    set_dock_size: (placement, size) =>
      __dock_set_size(
        handle,
        dockPlacement(placement, "set_dock_size(placement, size)"),
        nonNegativeDockNumber(size, "set_dock_size(placement, size)"),
      ),
    set_dock_collapsible: (placement, collapsible) =>
      __dock_set_collapsible(
        handle,
        dockPlacement(placement, "set_dock_collapsible(placement, collapsible)"),
        Boolean(collapsible),
      ),
    is_locked: () => __dock_is_locked(handle),
    set_locked: (locked) => __dock_set_locked(handle, Boolean(locked)),
    is_zoomed: () => __dock_is_zoomed(handle),
    zoom_out: () => __dock_zoom_out(handle),
    on: (event, handler) => __dock_on(handle, String(event), handler),
    release: () => __dock_release(handle),
  });

  const virtualScrollHandle = (handle) => ({
    __handle: handle,
    // The strategy is base's own word for where the item lands. `top` puts it
    // at the near edge, `center` in the middle; base's default is `top`.
    scroll_to_item: (index, strategy) => {
      if (!Number.isInteger(index) || index < 0) {
        throw new TypeError("scroll_to_item(index) needs a whole, non-negative index");
      }
      __virtual_scroll_to_item(handle, index, String(strategy ?? "top"));
    },
    scroll_to_bottom: () => __virtual_scroll_to_bottom(handle),
    release: () => __virtual_scroll_release(handle),
  });

  let deferInit = false;
  globalThis.__construct = (Class) => {
    deferInit = true;
    try {
      return new Class();
    } finally {
      deferInit = false;
    }
  };
  // `init` gets the async flavor, and both of its paths get the same one. That
  // is the honest shape: `init` exists to set up things that outlive the call —
  // tasks, timers, retained handles — so the context it hands to them must
  // outlive it too. A call-scoped `cx` here could not be given to the very work
  // `init` is for.
  globalThis.__initialize = (instance, props) => {
    if (typeof instance.init === "function") instance.init(props, __async_cx());
  };
  // This journals ordinary reachable object and callable descriptors only. Restoration succeeds
  // only while post-update descriptors remain legally redefinable/deletable.
  // Reflection cannot see private/internal slots or undo non-configurable
  // additions/hardening; the public declaration documents that boundary.
  globalThis.__checkpoint_view = (instance) => {
    const snapshots = [];
    const seen = new Set();
    const pending = [instance];
    let propertyCount = 0;
    while (pending.length > 0) {
      const value = pending.pop();
      if (
        value === null ||
        (typeof value !== "object" && typeof value !== "function") ||
        seen.has(value)
      ) continue;
      if (snapshots.length >= 10_000) {
        throw new RangeError("a nested view update reached the 10,000-object rollback limit");
      }
      seen.add(value);
      const descriptors = Object.getOwnPropertyDescriptors(value);
      const keys = Reflect.ownKeys(descriptors);
      propertyCount += keys.length;
      if (propertyCount > 100_000) {
        throw new RangeError("a nested view update reached the 100,000-property rollback limit");
      }
      snapshots.push([value, descriptors]);
      for (const key of keys) {
        const descriptor = descriptors[key];
        if (Object.prototype.hasOwnProperty.call(descriptor, "value")) {
          pending.push(descriptor.value);
        }
      }
    }
    return () => {
      for (let index = snapshots.length - 1; index >= 0; index -= 1) {
        const [value, descriptors] = snapshots[index];
        const saved = new Set(Reflect.ownKeys(descriptors));
        for (const key of Reflect.ownKeys(value)) {
          if (!saved.has(key)) {
            const current = Object.getOwnPropertyDescriptor(value, key);
            if (current?.configurable) delete value[key];
          }
        }
        Object.defineProperties(value, descriptors);
      }
    };
  };

  class View {
    constructor(props) {
      // `new MyView(props)` from script reaches `init` without the host's
      // generation, so the context here is the async flavor — it resolves
      // whichever call is running, and says so if there is none.
      if (!deferInit && typeof this.init === "function") this.init(props, __async_cx());
    }
  }

  // A dialog and a sheet are views whose `render` is the author's function.
  // That is the whole of the wrapping: a script view is an object with a
  // `render`, so a content function already is one, once it is given the name.
  const contentView = (build, api) => {
    if (typeof build !== "function") {
      throw new TypeError(
        api + " takes a function returning an element, not an element and not a view class",
      );
    }
    return { render: () => build() };
  };

  // `setItem` converts its argument, and `getItem` answers a string or `null` —
  // the Web Storage API exactly, so an application that wants structure reaches
  // for `JSON.stringify` as it would on the web. `length` is a getter because
  // it is a property there, not a call.
  const storage = (session) => ({
    get length() {
      return __storage_length(session);
    },
    // The host answers a missing entry with `undefined`; the Web Storage API
    // answers `null`, and `if (saved === null)` is how scripts test for it.
    key: (index) => {
      const at = Number(index);
      return Number.isInteger(at) && at >= 0 ? (__storage_key(session, at) ?? null) : null;
    },
    getItem: (key) => __storage_get(session, String(key)) ?? null,
    setItem: (key, value) => __storage_set(session, String(key), String(value)),
    removeItem: (key) => __storage_remove(session, String(key)),
    clear: () => __storage_clear(session),
    flush: () => __storage_flush(session),
  });

  // Overlays are window-level, not view-level: `cx.notify()` re-renders this
  // view, `window.open_dialog()` changes what the user is looking at. Grouped
  // under `window` because that is where `gpui-component` puts them — the
  // script API reads the same as the Rust it sits beside — and because it is
  // somewhere to grow: `Window` in Rust also answers focus, size and
  // appearance.
  //
  // A global rather than a module export. It names the window the
  // script is already inside, which is not something a file opts into by
  // importing it, and `window` is the one identifier every JavaScript author
  // already reaches for. Nothing collides: this runtime has no DOM.
  globalThis.window = {
    open_dialog: (build, options) =>
      __open_dialog(contentView(build, "window.open_dialog"), options ?? undefined),
    close_dialog: () => __close_dialog(),
    close_all_dialogs: () => __close_all_dialogs(),
    has_active_dialog: () => __has_active_dialog(),

    open_sheet: (build) => __open_sheet(undefined, contentView(build, "window.open_sheet")),
    open_sheet_at: (placement, build) =>
      __open_sheet(String(placement), contentView(build, "window.open_sheet_at")),
    close_sheet: () => __close_sheet(),
    has_active_sheet: () => __has_active_sheet(),

    push_toast: (options) => __push_toast(options),
    remove_toast: (id) => __remove_toast(String(id)),
    clear_toasts: () => __clear_toasts(),

    // The Web Storage API, where a browser keeps it. The two differ only in how
    // long they last: `localStorage` is a file the host placed, `sessionStorage`
    // is memory that goes with the process.
    localStorage: storage(false),
    sessionStorage: storage(true),

    // `Window::paint_path` in GPUI, so `window` here. It is the one element
    // constructor that is not a free function, and it is one because the thing
    // it mirrors is a method on the window rather than on the app.
    paint_path: paintPath,

    // What the window measures. All legal from `render`: a view that sizes
    // itself from the viewport, or spaces itself in rems, has to ask during
    // the pass that draws it.
    rem_size: () => __window_rem_size(),
    line_height: () => __window_line_height(),
    viewport_size: () => __window_viewport_size(),
    bounds: () => __window_bounds(),
    mouse_position: () => __window_mouse_position(),
    appearance: () => __window_appearance(),
    is_window_active: () => __window_is_active(),
    is_fullscreen: () => __window_is_fullscreen(),
    is_maximized: () => __window_is_maximized(),

    // What the window can be told. Refused from `render` for the reason
    // `cx.notify()` is: a frame that changes the window it is drawing into is
    // a frame arguing with itself.
    // `Window::dispatch_action` in GPUI, so `window` here — it walks the focus
    // path of *this* window. `cx.bind_keys` is the other half and is on `cx`
    // for the same reason: the keymap is `App`'s.
    dispatch_action: (action) => __dispatch_action(String(action)),

    set_rem_size: (size) => __window_set_rem_size(Number(size)),
    refresh: () => __window_refresh(),
    focus_next: () => __window_focus_next(),
    focus_prev: () => __window_focus_prev(),
    activate_window: () => __window_activate(),
    minimize_window: () => __window_minimize(),
    zoom_window: () => __window_zoom(),
    start_window_move: () => __window_start_move(),
    toggle_fullscreen: () => __window_toggle_fullscreen(),
  };

  globalThis.localStorage = globalThis.window.localStorage;
  globalThis.sessionStorage = globalThis.window.sessionStorage;

  let cachedThemeSource;
  let cachedTheme;
  let cachedThemeRevision = -1;
  globalThis.__theme_dirty = true;
  const refreshTheme = () => {
    const revision = __theme_revision();
    if (!globalThis.__theme_dirty && revision === cachedThemeRevision) return;
    const source = __theme_snapshot();
    if (source !== cachedThemeSource) {
      cachedThemeSource = source;
      cachedTheme = JSON.parse(source);
      Object.freeze(cachedTheme.colors);
      Object.freeze(cachedTheme.spacing);
      Object.freeze(cachedTheme.radius);
      Object.freeze(cachedTheme);
    }
    cachedThemeRevision = revision;
    globalThis.__theme_dirty = false;
  };
  globalThis.__prepare_theme = refreshTheme;
  const currentTheme = () => {
    if (globalThis.__theme_dirty || cachedTheme === undefined) refreshTheme();
    return cachedTheme;
  };

  // Every member of `cx` gates on `check()` and then does ordinary ambient
  // work. That gate is the whole difference between a call-scoped `cx` and an
  // async one, so a member added here is right for both flavors at once and
  // the two cannot drift.
  const contextMembers = (check) => ({
    theme: () => {
      check();
      return currentTheme();
    },
    open_url: (url) => {
      check();
      return __open_url(String(url));
    },
    open_path: (path) => {
      check();
      return __open_path(String(path));
    },
    read_from_clipboard: () => {
      check();
      return __clipboard_read_text();
    },
    write_to_clipboard: (text) => {
      check();
      return __clipboard_write_text(String(text));
    },
    focus_handle: () => {
      check();
      return focusHandle(__focus_handle_new());
    },
    // `App::bind_keys`, so `cx`. The keymap belongs to the application rather
    // than to a window, which is why binding a chord in one view makes it live
    // everywhere its `context` predicate matches.
    bind_keys: (bindings) => {
      check();
      if (!Array.isArray(bindings)) {
        throw new TypeError(
          "cx.bind_keys(bindings) expects an array of { keystroke, action, context? }",
        );
      }
      return __bind_keys(bindings);
    },
    new: (Class, props) => {
      check();
      if (typeof Class !== "function" || !(Class.prototype instanceof View)) {
        throw new TypeError("cx.new(Class, props) expects a View subclass");
      }
      return entity(__view_new(Class, props));
    },
    spawn: (body, opts) => {
      check();
      return __spawn(body, opts);
    },
    sleep: (ms) => {
      check();
      return __sleep(ms);
    },
    timer: {
      after: (ms, handler, opts) => {
        check();
        return __timer_after(ms, handler, opts);
      },
      every: (ms, handler, opts) => {
        check();
        return __timer_every(ms, handler, opts);
      },
    },
  });

  // A description recorded once and filled per call.
  //
  // **Not part of the script surface**, and deliberately so. Asking an author
  // to mark their hot paths is a performance annotation in the source: two ways
  // to write the same interface, restrictions that only report at first call,
  // and a decision nobody should have to make while describing a panel. The
  // machinery is kept because the runtime is meant to apply it *itself* — see
  // `engine/quickjs/template.rs` — and `globalThis.__template` is how the tests
  // that pin its behaviour reach it, the same standing `__apply` has.
  //
  // The body runs a single time, with a sentinel in each parameter position;
  // wherever a sentinel comes to rest in what it describes is a slot, and what
  // is left over is structure. Every call after that grafts the structure and
  // writes its arguments into the slots, entering no builder method at all.
  //
  // The sentinel refuses to become a primitive. `${price}` inside a body would
  // otherwise consume it and bake this first call's value into the structure,
  // which is a panel that silently stops updating — so it throws where it was
  // written instead.
  const templateSlot = (index) => {
    const refuse = () => {
      throw new TypeError(
        "a template argument can be passed to a builder call but not computed on. " +
          "Format or compare the value where the template is called, and pass the result",
      );
    };
    return {
      __slot: index,
      toString: refuse,
      valueOf: refuse,
      [Symbol.toPrimitive]: refuse,
    };
  };

  const template = (build) => {
    if (typeof build !== "function") {
      throw new TypeError("template(build) expects a function that builds one element");
    }
    let id = -1;
    return (...args) => {
      if (id < 0) {
        const slots = [];
        for (let i = 0; i < build.length; i += 1) slots.push(templateSlot(i));
        __template_begin(build.length);
        let root;
        try {
          root = build(...slots);
        } catch (error) {
          __template_abort();
          throw error;
        }
        id = __template_end(root?.__id);
      }
      return element(__template_instantiate(id, args));
    };
  };

  globalThis.__template = template;

  return {
    __element: element,
    View,
    div: () => element(__div()),
    h_flex: () => element(__h_flex()),
    v_flex: () => element(__v_flex()),
    svg: (path) => element(__svg(String(path))),
    image: (path) => element(__image(String(path))),
    PathBuilder: {
      fill: () => pathBuilder(true, 0),
      stroke: (width) => pathBuilder(false, finitePositive(width, "stroke width")),
    },
    Background: {
      solid: (color) => background("solid", [String(color)]),
      stop: (color, percentage) => {
        if (typeof percentage !== "number" || !Number.isFinite(percentage)) throw new TypeError("background stop percentage must be finite");
        return Object.freeze({ __backgroundStop: true, color: String(color), percentage });
      },
      linear_gradient: (angle, from, to) => {
        angle = Number(angle);
        if (!Number.isFinite(angle)) throw new TypeError("linear gradient angle must be finite");
        const stop = (value, fallback, name) => {
          if (typeof value === "string") return [value, fallback];
          if (!value?.__backgroundStop) throw new TypeError(name + " must be a color or Background.stop(color, percentage)");
          return [value.color, value.percentage];
        };
        const a = stop(from, 0, "gradient from stop");
        const b = stop(to, 1, "gradient to stop");
        return background("linear-gradient", [String(angle), a[0], String(a[1]), b[0], String(b[1])]);
      },
      pattern_slash: (color, width, interval) => background("pattern-slash", [
        String(color),
        String(finitePositive(width, "pattern width")),
        String(finitePositive(interval, "pattern interval")),
      ]),
      checkerboard: (color, size) => background("checkerboard", [
        String(color),
        String(finitePositive(size, "checkerboard size")),
      ]),
    },
    component: (module, name) => ({
      new: (id, props) => {
        if (typeof id !== "string" || id.length === 0) {
          throw new TypeError("a component needs a non-empty string id");
        }
        return element(__component(String(module), String(name), id, props ?? {}));
      },
    }),
    __context_members: contextMembers,
    Button: { new: (id) => element(__button(String(id))) },
    Link: { new: (id) => element(__link(String(id))) },
    Checkbox: { new: (id) => element(__checkbox(String(id))) },
    Switch: { new: (id) => element(__switch(String(id))) },
    Tabs: { new: (id) => element(__tabs(String(id))) },
    Tab: { new: (id) => element(__tab(String(id))) },
    Progress: { new: (id) => element(__progress(String(id))) },
    // Base's own shape: a root that chooses between two slots, and two slot
    // types that are not elements on their own.
    Accordion: { new: (id) => element(__accordion(String(id))) },
    AccordionItem: { new: () => element(__accordion_item()) },
    // `AccordionHeader::new` takes the trigger, exactly as `Popup.new` takes
    // its own: a heading whose button arrived later would be a heading that
    // announced nothing for a frame.
    AccordionHeader: {
      new: (trigger) => {
        if (typeof trigger?.__id !== "number") {
          throw new TypeError(
            "AccordionHeader.new(trigger) expects an AccordionTrigger element: the heading owns the button that opens the item, and base has none of its own",
          );
        }
        return element(__accordion_header()).trigger(trigger);
      },
    },
    AccordionPanel: { new: () => element(__accordion_panel()) },
    AccordionTrigger: { new: (id) => element(__accordion_trigger(String(id))) },
    Pagination: { new: (id) => element(__pagination(String(id))) },
    // Not a component: the one thing base contributes that a script cannot
    // write for itself is which page numbers to show, and that is arithmetic.
    // Called during `render`, once, and the buttons are built from what it
    // answers.
    pagination_items: (current_page, total_pages, visible_pages) =>
      __pagination_items(
        Number(current_page),
        Number(total_pages),
        visible_pages === undefined ? 7 : Number(visible_pages),
      ),
    Avatar: { new: () => element(__avatar()) },
    AvatarImage: { new: (path) => element(__avatar_image(String(path))) },
    AvatarFallback: { new: () => element(__avatar_fallback()) },
    ProgressTrack: { new: () => element(__progress_track()) },
    ProgressIndicator: { new: () => element(__progress_indicator()) },
    fps_monitor: () => element(__fps_monitor()),
    show_fps_monitor: (options) => __show_fps_monitor(options),
    hide_fps_monitor: () => __hide_fps_monitor(),
    fps_monitor_visible: () => __fps_monitor_visible(),
    Radio: { new: (id) => element(__radio(String(id))) },
    Toggle: { new: (id) => element(__toggle(String(id))) },
    RadioGroup: { new: (id) => element(__radio_group(String(id))) },
    ToggleGroup: { new: (id) => element(__toggle_group(String(id))) },
    Table: { new: (id) => element(__table(String(id))) },
    TableHeader: { new: (id) => element(__table_header(String(id))) },
    TableBody: { new: (id) => element(__table_body(String(id))) },
    TableCaption: { new: (id) => element(__table_caption(String(id))) },
    // Free functions, not `Type.new(...)`, because that is what base exports:
    // the group has no type a script ever names.
    h_resizable: (id) => element(__h_resizable(String(id))),
    v_resizable: (id) => element(__v_resizable(String(id))),
    resizable_panel: resizablePanel,
    TableRow: {
      new: (id, row_index) =>
        element(__table_row(String(id), oneBased(row_index, "TableRow.new row index"))),
    },
    TableHead: {
      new: (id, column_index) =>
        element(__table_head(String(id), oneBased(column_index, "TableHead.new column index"))),
    },
    TableCell: {
      new: (id, column_index) =>
        element(__table_cell(String(id), oneBased(column_index, "TableCell.new column index"))),
    },
    Collapsible: { new: () => element(__collapsible()) },
    Popover: { new: (id) => element(__popover(String(id))) },
    HoverCard: { new: (id) => element(__hover_card(String(id))) },
    // The trigger is a constructor argument, as it is in base: a popup with no
    // trigger has no bounds to anchor to, so there is no useful moment between
    // `new` and the trigger being known.
    Popup: {
      new: (id, trigger) => {
        if (typeof trigger?.__id !== "number") {
          throw new TypeError(
            "Popup.new(id, trigger) expects the trigger element; a popup anchors its content to the trigger's bounds, so it cannot be built without one",
          );
        }
        return element(__popup(String(id))).trigger(trigger);
      },
    },
    Select: { new: (id) => element(__select(String(id))) },
    Combobox: { new: (id) => element(__combobox(String(id))) },
    DatePicker: {
      new: (id, focus_handle) => {
        if (typeof focus_handle?.__handle !== "number") {
          throw new TypeError(
            "DatePicker.new(id, focus_handle) expects a FocusHandle from cx.focus_handle(); the picker takes the keyboard through that handle, and base has no builder to supply one later",
          );
        }
        return element(__date_picker(String(id), focus_handle.__handle));
      },
    },
    // Free functions, not `VirtualList.new(...)`, because that is what base
    // exports: `v_virtual_list` and `h_virtual_list` are the whole of its
    // public surface, and the list has no type a script ever names.
    //
    // The count is a separate argument from the sizes, which base does not
    // separate — its one vector is both. See the `.d.ts` for why: mirroring it
    // would put one number per row across the boundary on every render.
    v_virtual_list: virtualList(__v_virtual_list, "v_virtual_list"),
    h_virtual_list: virtualList(__h_virtual_list, "h_virtual_list"),
    list: lazyList(__list, "list", true),
    uniform_list: lazyList(__uniform_list, "uniform_list", false),
    VirtualListScrollHandle: { new: () => virtualScrollHandle(__virtual_scroll_new()) },
    Scrollbar: {
      new: (id) => element(__scrollbar(String(id))),
      // `horizontal` and `vertical` are `new` plus the orientation the group
      // containers already spell `axis`, so there is one word for orientation
      // in the whole API rather than a second one here.
      horizontal: (id) => element(__scrollbar(String(id))).axis("horizontal"),
      vertical: (id) => element(__scrollbar(String(id))).axis("vertical"),
    },
    InputState: {
      new: (options) =>
        inputState(__input_state_new(options?.placeholder ?? null, options?.value ?? null)),
    },
    Input: { new: (state) => element(__input_element(state.__handle)) },

    NumberInput: {
      new: (state) => element(__number_input_element(state.__handle)),
    },
    TextareaState: {
      new: (options) =>
        textareaState(
          __textarea_state_new(
            options?.placeholder ?? null,
            options?.value ?? null,
            options?.rows === undefined || options?.rows === null
              ? null
              : oneBased(options.rows, "TextareaState.new rows"),
          ),
        ),
    },
    Textarea: { new: (state) => element(__textarea_element(state.__handle)) },
    TextView: {
      html: (id, text) => element(__text_view(String(id), String(text), "html")),
      markdown: (id, text) => element(__text_view(String(id), String(text), "markdown")),
    },
    CalendarState: { new: () => calendarState(__calendar_state_new()) },
    SliderState: {
      new: (options) => {
        const settings = options ?? {};
        const min = settings.min ?? 0;
        const max = settings.max ?? 100;
        const step = settings.step ?? 1;
        const scale = String(settings.scale ?? "linear");
        for (const [name, value] of [["min", min], ["max", max], ["step", step]]) {
          if (typeof value !== "number" || !Number.isFinite(value)) {
            throw new TypeError("SliderState.new " + name + " must be a finite number");
          }
        }
        if (max <= min) throw new TypeError("SliderState.new needs a max greater than its min");
        if (step <= 0) throw new TypeError("SliderState.new step must be greater than 0");
        if (!["linear", "logarithmic"].includes(scale)) {
          throw new TypeError("SliderState.new scale must be linear or logarithmic");
        }
        // A logarithmic scale maps through log(value / min), which has no
        // answer at or below zero. Base asserts on it, and an assertion in the
        // host is a lost application rather than a reported mistake.
        if (scale === "logarithmic" && min <= 0) {
          throw new TypeError("SliderState.new with a logarithmic scale needs a min greater than 0");
        }
        return sliderState(
          __slider_state_new(
            min,
            max,
            step,
            scale,
            sliderValues(settings.value ?? min, "SliderState.new value"),
          ),
        );
      },
    },
    Slider: { new: (state) => element(__slider_element(state.__handle)) },
    SliderTrack: { new: (state) => element(__slider_track_element(state.__handle)) },
    SliderIndicator: { new: (state) => element(__slider_indicator_element(state.__handle)) },
    SliderThumb: { new: (state) => element(__slider_thumb_element(state.__handle)) },
    OtpState: {
      new: (length, options) => {
        // A code of no cells accepts no keystroke and shows nothing, and a
        // typed-in length of six hundred thousand is a frozen window. Neither
        // is something base refuses, so it is refused here.
        if (!Number.isInteger(length) || length < 1 || length > 64) {
          throw new TypeError("OtpState.new(length) expects a whole number between 1 and 64");
        }
        const settings = options ?? {};
        return otpState(
          __otp_state_new(length, settings.value ?? null, Boolean(settings.masked)),
        );
      },
    },
    OtpInput: { new: (state) => element(__otp_element(state.__handle)) },
    DockArea: {
      new: (id, options) => {
        const version = options?.version;
        if (version !== undefined && version !== null && (!Number.isSafeInteger(version) || version < 0)) {
          throw new TypeError("DockArea.new(id, options) version expects a whole, non-negative safe integer");
        }
        return dockArea(__dock_area_new(String(id), version ?? null));
      },
      // Not a method on an area: a builder is registered for the whole
      // application, and a layout is restored into whichever area asks for it.
      // Registering the same name twice replaces the class, which is what a hot
      // reload does.
      register_panel: (name, Class) => {
        if (typeof name !== "string" || name.length === 0) {
          throw new TypeError(
            "DockArea.register_panel(name, Class) needs the name the panel is added under",
          );
        }
        if (typeof Class !== "function" || !(Class.prototype instanceof View)) {
          throw new TypeError(
            "DockArea.register_panel(name, Class) expects the View subclass the panel is rebuilt from",
          );
        }
        return __dock_register_panel(name, Class);
      },
    },
    // Free functions, not `DockArea.element(...)`: the area is the state and
    // this is one description of it, the same split `v_virtual_list` has.
    dock_area: dockAreaElement,
    dock_content: () => element(__dock_content()),
  };
})();
