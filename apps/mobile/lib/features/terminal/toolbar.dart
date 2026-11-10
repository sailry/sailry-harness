import 'package:flutter/material.dart';

import '../../l10n/strings.dart';
import '../../ui/kit.dart';
import '../../ui/theme.dart';

class TerminalToolbar extends StatelessWidget {
  const TerminalToolbar({
    super.key,
    required this.modifiers,
    required this.onModifier,
    required this.onKey,
    required this.onKeyboard,
    required this.keyboardVisible,
  });

  final Set<String> modifiers;
  final ValueChanged<String>? onModifier;
  final ValueChanged<String>? onKey;
  final VoidCallback? onKeyboard;
  final bool keyboardVisible;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    Widget button(
      String label,
      VoidCallback? action, {
      String? icon,
      bool active = false,
    }) => TextButton(
      style: TextButton.styleFrom(
        minimumSize: const Size(40, 40),
        padding: const EdgeInsets.symmetric(horizontal: 10),
        tapTargetSize: MaterialTapTargetSize.shrinkWrap,
        backgroundColor: active
            ? SailryTheme.navigationSelection(context)
            : Colors.transparent,
        shape: const StadiumBorder(),
      ),
      onPressed: action,
      child: icon == null
          ? Text(label)
          : Tooltip(
              message: label,
              child: AppIcon(
                icon,
                size: 18,
                color: action == null
                    ? colors.onSurface.withValues(alpha: .38)
                    : active
                    ? colors.onSurface
                    : colors.onSurfaceVariant,
              ),
            ),
    );

    return Padding(
      padding: EdgeInsets.only(
        top: 12,
        bottom: 8 + MediaQuery.paddingOf(context).bottom,
      ),
      child: TextFieldTapRegion(
        child: Surface(
          kind: SurfaceKind.navigation,
          radius: 26,
          padding: const EdgeInsets.all(6),
          child: SingleChildScrollView(
            scrollDirection: Axis.horizontal,
            child: Row(
              mainAxisSize: MainAxisSize.min,
              spacing: 2,
              children: [
                button(
                  context.tr(
                    keyboardVisible
                        ? 'terminalHideKeyboard'
                        : 'terminalKeyboard',
                  ),
                  onKeyboard,
                  icon: 'keyboard',
                  active: keyboardVisible,
                ),
                for (final (key, label) in const [
                  ('escape', 'terminalEscape'),
                  ('tab', 'terminalTab'),
                ])
                  button(
                    context.tr(label),
                    onKey == null ? null : () => onKey!(key),
                  ),
                for (final key in ['Ctrl', 'Alt', 'Shift', 'Cmd'])
                  button(
                    context.tr('terminal$key'),
                    onModifier == null ? null : () => onModifier!(key),
                    active: modifiers.contains(key),
                  ),
                for (final (key, icon, label) in const [
                  ('arrow_left', 'arrow-left', 'terminalArrowLeft'),
                  ('arrow_up', 'arrow-up', 'terminalArrowUp'),
                  ('arrow_down', 'arrow-down', 'terminalArrowDown'),
                  ('arrow_right', 'arrow-right', 'terminalArrowRight'),
                  ('enter', 'enter', 'terminalEnter'),
                ])
                  button(
                    context.tr(label),
                    onKey == null ? null : () => onKey!(key),
                    icon: icon,
                  ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}
