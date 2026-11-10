import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import 'icons.dart';
import 'surface.dart';
import 'theme.dart';

/// Shared spacing for stacked settings and resource forms.
class FormBody extends StatelessWidget {
  const FormBody({super.key, required this.children});

  final List<Widget> children;

  @override
  Widget build(BuildContext context) => Column(
    mainAxisSize: MainAxisSize.min,
    crossAxisAlignment: CrossAxisAlignment.stretch,
    spacing: 16,
    children: children,
  );
}

/// Uses Flutter's menu and focus lifecycle with the application theme.
class SelectField<T> extends StatefulWidget {
  const SelectField({
    super.key,
    required this.label,
    required this.value,
    required this.options,
    required this.onChanged,
  });

  final String label;
  final T value;
  final List<(T, String)> options;
  final ValueChanged<T>? onChanged;

  @override
  State<SelectField<T>> createState() => _SelectFieldState<T>();
}

class _SelectFieldState<T> extends State<SelectField<T>> {
  final _controller = TextEditingController();
  final _focus = FocusNode();
  final _menu = MenuController();

  bool get enabled => widget.onChanged != null && widget.options.isNotEmpty;

  void toggle() {
    if (!enabled) return;
    _menu.isOpen ? _menu.close() : _menu.open();
  }

  String name(SelectField<T> field) =>
      field.options.where((item) => item.$1 == field.value).firstOrNull?.$2 ??
      '';

  @override
  void initState() {
    super.initState();
    _controller.text = name(widget);
  }

  @override
  void didUpdateWidget(SelectField<T> oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.value != widget.value || name(oldWidget) != name(widget)) {
      _controller.text = name(widget);
    }
    if (!enabled) _menu.close();
  }

  @override
  void dispose() {
    _controller.dispose();
    _focus.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    // DropdownMenu has no menu-surface builder. MenuAnchor keeps Flutter's
    // placement, focus, navigation and dismissal around the shared glass surface.
    return LayoutBuilder(
      builder: (context, constraints) => MenuAnchor(
        controller: _menu,
        childFocusNode: _focus,
        crossAxisUnconstrained: false,
        clipBehavior: Clip.none,
        style: const MenuStyle(
          backgroundColor: WidgetStatePropertyAll(Colors.transparent),
          surfaceTintColor: WidgetStatePropertyAll(Colors.transparent),
          elevation: WidgetStatePropertyAll(0),
          padding: WidgetStatePropertyAll(EdgeInsets.zero),
          shape: WidgetStatePropertyAll(RoundedRectangleBorder()),
        ),
        menuChildren: [
          SizedBox(
            width: constraints.maxWidth,
            child: Surface(
              kind: SurfaceKind.sheet,
              radius: 16,
              padding: const EdgeInsets.all(6),
              child: ConstrainedBox(
                constraints: const BoxConstraints(maxHeight: 308),
                child: SingleChildScrollView(
                  primary: false,
                  child: Column(
                    mainAxisSize: MainAxisSize.min,
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: [
                      for (final (index, (id, label)) in widget.options.indexed)
                        Semantics(
                          selected: id == widget.value,
                          child: MenuItemButton(
                            autofocus: index == 0,
                            trailingIcon: id == widget.value
                                ? const AppIcon('check', size: 16)
                                : null,
                            onPressed: () {
                              _controller.text = label;
                              widget.onChanged?.call(id);
                            },
                            child: Text(
                              label,
                              maxLines: 2,
                              overflow: TextOverflow.ellipsis,
                            ),
                          ),
                        ),
                    ],
                  ),
                ),
              ),
            ),
          ),
        ],
        builder: (context, menu, _) => CallbackShortcuts(
          bindings: enabled
              ? {
                  const SingleActivator(LogicalKeyboardKey.enter): toggle,
                  const SingleActivator(LogicalKeyboardKey.space): toggle,
                  const SingleActivator(LogicalKeyboardKey.arrowDown):
                      _menu.open,
                  const SingleActivator(LogicalKeyboardKey.arrowUp): _menu.open,
                }
              : const {},
          child: Semantics(
            button: true,
            enabled: enabled,
            label: widget.label,
            value: _controller.text,
            expanded: menu.isOpen,
            onTap: enabled ? toggle : null,
            onExpand: enabled && !menu.isOpen ? menu.open : null,
            onCollapse: menu.isOpen ? menu.close : null,
            excludeSemantics: true,
            child: TextField(
              controller: _controller,
              focusNode: _focus,
              enabled: enabled,
              readOnly: true,
              enableInteractiveSelection: false,
              keyboardType: TextInputType.none,
              style: theme.textTheme.bodyLarge,
              onTap: toggle,
              decoration: InputDecoration(
                labelText: widget.label,
                suffixIcon: IconButton(
                  onPressed: enabled ? toggle : null,
                  icon: RotatedBox(
                    quarterTurns: menu.isOpen ? 2 : 0,
                    child: const AppIcon('down'),
                  ),
                ),
              ),
            ),
          ),
        ),
      ),
    );
  }
}
