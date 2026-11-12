import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
import '../../../runtime/notices.dart';
import '../../../ui/kit.dart';
import '../../../ui/theme.dart';
import '../../../ui/toast.dart';
import 'rules.dart';

TextStyle dimText(BuildContext context, [double size = 13]) => TextStyle(
  fontSize: size,
  color: Theme.of(context).colorScheme.onSurfaceVariant.withValues(alpha: .65),
);

/// Runs a Node edit and reports a failure as a toast.
Future<bool> attempt(
  BuildContext context,
  Future<bool> Function() action,
) async {
  try {
    return await action();
  } catch (error) {
    if (context.mounted) showFailure(context, error);
    return false;
  }
}

/// Reports a validation problem by localization key; returns whether valid.
bool accept(BuildContext context, String? problem) {
  if (problem != null) showToast(context, context.tr(problem));
  return problem == null;
}

/// Glass card with a title row and divided inner rows.
class SectionCard extends StatelessWidget {
  const SectionCard({
    super.key,
    required this.title,
    required this.children,
    this.count,
    this.actions = const [],
  });
  final String title;
  final int? count;
  final List<Widget> actions;
  final List<Widget> children;

  @override
  Widget build(BuildContext context) => Surface(
    padding: const EdgeInsets.fromLTRB(16, 12, 16, 4),
    child: Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        ConstrainedBox(
          constraints: const BoxConstraints(minHeight: 32),
          child: Row(
            children: [
              Text(
                title,
                style: const TextStyle(
                  fontSize: 15,
                  fontWeight: FontWeight.w600,
                ),
              ),
              if (count != null) ...[
                const SizedBox(width: 8),
                Text('$count', style: dimText(context)),
              ],
              const Spacer(),
              ...actions,
            ],
          ),
        ),
        for (final (index, child) in children.indexed)
          DecoratedBox(
            decoration: BoxDecoration(
              border: index < children.length - 1
                  ? Border(
                      bottom: BorderSide(
                        color: Theme.of(context).colorScheme.outlineVariant,
                      ),
                    )
                  : null,
            ),
            child: child,
          ),
      ],
    ),
  );
}

/// Label with a dim value; tapping opens the value's editor.
class FieldRow extends StatelessWidget {
  const FieldRow({
    super.key,
    required this.label,
    required this.value,
    this.onTap,
    this.trailing,
    this.mono = false,
  });
  final String label;
  final String value;
  final VoidCallback? onTap;
  final Widget? trailing;
  final bool mono;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    return InkWell(
      onTap: onTap,
      child: ConstrainedBox(
        constraints: const BoxConstraints(minHeight: 48),
        child: Row(
          children: [
            Text(label, style: const TextStyle(fontSize: 15)),
            const SizedBox(width: 16),
            Expanded(
              child: Text(
                value,
                textAlign: TextAlign.end,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: TextStyle(
                  fontSize: 14,
                  color: colors.onSurfaceVariant,
                  fontFamily: mono ? 'monospace' : null,
                ),
              ),
            ),
            ?trailing,
            if (onTap != null) ...[
              const SizedBox(width: 8),
              AppIcon('chevron', color: colors.onSurfaceVariant),
            ],
          ],
        ),
      ),
    );
  }
}

/// Label, optional icon and an adaptive switch.
class SwitchRow extends StatelessWidget {
  const SwitchRow({
    super.key,
    required this.label,
    required this.value,
    required this.onChanged,
    this.icon,
  });
  final String label;
  final bool value;
  final ValueChanged<bool>? onChanged;
  final String? icon;

  @override
  Widget build(BuildContext context) => ConstrainedBox(
    constraints: const BoxConstraints(minHeight: 48),
    child: Row(
      children: [
        if (icon != null) ...[
          AppIcon(
            icon!,
            size: 18,
            color: Theme.of(context).colorScheme.onSurfaceVariant,
          ),
          const SizedBox(width: 10),
        ],
        Expanded(child: Text(label, style: const TextStyle(fontSize: 15))),
        Switch.adaptive(value: value, onChanged: onChanged),
      ],
    ),
  );
}

/// Icons of the image, tool and reasoning capabilities a model has.
class CapabilityIcons extends StatelessWidget {
  const CapabilityIcons(this.model, {super.key});
  final Json model;

  @override
  Widget build(BuildContext context) {
    final color = Theme.of(context).colorScheme.onSurfaceVariant;
    return Row(
      mainAxisSize: MainAxisSize.min,
      spacing: 6,
      children: [
        for (final name in capabilityNames)
          if (model[name] == true)
            Semantics(
              label: context.tr(capabilityLabel(name)),
              child: AppIcon(capabilityIcon(name), size: 15, color: color),
            ),
      ],
    );
  }
}

String capabilityIcon(String name) => switch (name) {
  'vision' => 'image',
  'tools' => 'tool',
  _ => 'bulb',
};

String capabilityLabel(String name) => switch (name) {
  'vision' => 'providerVision',
  'tools' => 'providerTools',
  _ => 'providerReasoning',
};

/// Accent tag marking the provider's default model.
class DefaultTag extends StatelessWidget {
  const DefaultTag({super.key});

  @override
  Widget build(BuildContext context) {
    final color = SailryTheme.accentColor(context, AccentTone.blue);
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 7, vertical: 1),
      decoration: BoxDecoration(
        color: color.withValues(alpha: .16),
        borderRadius: BorderRadius.circular(999),
      ),
      child: Text(
        context.tr('providerDefault'),
        style: TextStyle(fontSize: 11, color: color),
      ),
    );
  }
}

/// Round initial-letter avatar of a provider.
class ProviderAvatar extends StatelessWidget {
  const ProviderAvatar(this.name, {super.key});
  final String name;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final initial = name.trim().isEmpty
        ? '?'
        : String.fromCharCode(name.trim().runes.first).toUpperCase();
    return Container(
      width: 34,
      height: 34,
      alignment: Alignment.center,
      decoration: BoxDecoration(
        color: colors.surfaceContainerHigh,
        shape: BoxShape.circle,
      ),
      child: Text(
        initial,
        style: const TextStyle(fontSize: 15, fontWeight: FontWeight.w600),
      ),
    );
  }
}

/// Full-width glass action, such as a default or delete command.
class ActionCard extends StatelessWidget {
  const ActionCard({
    super.key,
    required this.label,
    required this.onTap,
    this.danger = false,
  });
  final String label;
  final VoidCallback? onTap;
  final bool danger;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final color = onTap == null
        ? Theme.of(context).disabledColor
        : danger
        ? SailryTheme.statusColor(context, StatusTone.danger)
        : colors.onSurface;
    return Surface(
      onTap: onTap,
      padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 13),
      child: Text(
        label,
        textAlign: TextAlign.center,
        style: TextStyle(fontSize: 15, color: color),
      ),
    );
  }
}

/// Asks before deleting [name]; the dialog names its target.
Future<bool> confirmDelete(BuildContext context, String name) async =>
    await showAppDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text(context.tr('delete')),
        content: Text(context.tr('configDelete').replaceAll('{name}', name)),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: Text(context.tr('cancel')),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(context, true),
            child: Text(context.tr('delete')),
          ),
        ],
      ),
    ) ==
    true;
