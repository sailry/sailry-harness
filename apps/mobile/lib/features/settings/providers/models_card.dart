import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import '../../../ui/kit.dart';
import 'parts.dart';
import 'rules.dart';

/// Models of a provider with fetch and manual add actions.
class ModelsCard extends StatelessWidget {
  const ModelsCard({
    super.key,
    required this.provider,
    required this.onFetch,
    required this.onAdd,
    required this.onOpen,
  });
  final Json provider;
  final VoidCallback? onFetch;
  final VoidCallback? onAdd;
  final ValueChanged<Json> onOpen;

  @override
  Widget build(BuildContext context) {
    final items = models(provider);
    final fallback = text(provider['default_model']);
    return SectionCard(
      title: context.tr('configModels'),
      count: items.length,
      actions: [
        _Action(icon: 'refresh', label: 'providerFetch', onPressed: onFetch),
        _Action(icon: 'plus', label: 'add', onPressed: onAdd),
      ],
      children: [
        if (items.isEmpty)
          Padding(
            padding: const EdgeInsets.symmetric(vertical: 14),
            child: Text(
              context.tr('configEmpty'),
              textAlign: TextAlign.center,
              style: dimText(context),
            ),
          ),
        for (final model in items)
          _ModelRow(
            model: model,
            isDefault: model['id'] == fallback,
            onTap: () => onOpen(model),
          ),
      ],
    );
  }
}

class _Action extends StatelessWidget {
  const _Action({
    required this.icon,
    required this.label,
    required this.onPressed,
  });
  final String icon;
  final String label;
  final VoidCallback? onPressed;

  @override
  Widget build(BuildContext context) => TextButton.icon(
    style: TextButton.styleFrom(
      padding: const EdgeInsets.symmetric(horizontal: 8),
      minimumSize: const Size(0, 32),
      tapTargetSize: MaterialTapTargetSize.shrinkWrap,
    ),
    onPressed: onPressed,
    icon: AppIcon(icon, size: 15),
    label: Text(context.tr(label), style: const TextStyle(fontSize: 13)),
  );
}

class _ModelRow extends StatelessWidget {
  const _ModelRow({
    required this.model,
    required this.isDefault,
    required this.onTap,
  });
  final Json model;
  final bool isDefault;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    return InkWell(
      onTap: onTap,
      child: Padding(
        padding: const EdgeInsets.symmetric(vertical: 10),
        child: Row(
          children: [
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Row(
                    children: [
                      Flexible(
                        child: Text(
                          modelLabel(model),
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: const TextStyle(fontSize: 15),
                        ),
                      ),
                      if (isDefault) ...[
                        const SizedBox(width: 8),
                        const DefaultTag(),
                      ],
                    ],
                  ),
                  const SizedBox(height: 2),
                  Text(
                    text(model['id']),
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: dimText(
                      context,
                      12,
                    ).copyWith(fontFamily: 'monospace'),
                  ),
                ],
              ),
            ),
            const SizedBox(width: 8),
            CapabilityIcons(model),
            const SizedBox(width: 8),
            AppIcon('chevron', color: colors.onSurfaceVariant),
          ],
        ),
      ),
    );
  }
}
