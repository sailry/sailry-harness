import 'package:flutter/material.dart';

import 'icons.dart';

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
  }

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return DropdownMenu<T>(
      controller: _controller,
      initialSelection: widget.value,
      enabled: widget.onChanged != null && widget.options.isNotEmpty,
      expandedInsets: EdgeInsets.zero,
      menuHeight: 320,
      selectOnly: true,
      requestFocusOnTap: true,
      enableSearch: false,
      label: Text(widget.label),
      trailingIcon: const AppIcon('down'),
      selectedTrailingIcon: const RotatedBox(
        quarterTurns: 2,
        child: AppIcon('down'),
      ),
      textStyle: theme.textTheme.bodyLarge,
      dropdownMenuEntries: [
        for (final (id, name) in widget.options)
          DropdownMenuEntry<T>(
            value: id,
            label: name,
            labelWidget: Text(
              name,
              maxLines: 2,
              overflow: TextOverflow.ellipsis,
            ),
            trailingIcon: id == widget.value
                ? const AppIcon('check', size: 16)
                : null,
          ),
      ],
      onSelected: (selected) {
        if (selected != null) widget.onChanged?.call(selected);
      },
    );
  }
}
