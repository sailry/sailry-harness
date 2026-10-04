import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import '../../../ui/kit.dart';

/// Edits a configuration draft; the owning form admits the revision to the Node.
class ModelControls extends StatelessWidget {
  const ModelControls({
    super.key,
    required this.providers,
    required this.config,
    required this.onChanged,
    this.onCommitted,
    this.enabled = true,
  });

  final List<Map<String, dynamic>> providers;
  final Map<String, dynamic> config;
  final ValueChanged<Map<String, dynamic>> onChanged;
  final ValueChanged<Map<String, dynamic>>? onCommitted;
  final bool enabled;

  void _commit(Map<String, dynamic> value) {
    onChanged(value);
    onCommitted?.call(value);
  }

  @override
  Widget build(BuildContext context) {
    final provider = providers
        .where((p) => p['id'] == config['provider'])
        .firstOrNull;
    final model = objects(
      provider?['models'],
    ).where((m) => m['id'] == config['model']).firstOrNull;
    final efforts = <String>{
      ...(model?['efforts'] as List? ?? []).whereType<String>(),
    }.toList();
    final initial = model?['default_effort'] ?? 'default';
    final current = config['effort'] ?? 'default';
    final selected = efforts.indexOf(current is String ? current : '');
    final budget = object(current)['budget'];
    final label = budget == null
        ? text(current)
        : '${tr('conversationBudget')}: $budget';
    final colors = Theme.of(context).colorScheme;
    return Column(
      mainAxisSize: MainAxisSize.min,
      children: [
        Row(
          children: [
            const SizedBox(width: 40),
            Expanded(
              child: Text(
                label,
                textAlign: TextAlign.center,
                style: Theme.of(
                  context,
                ).textTheme.titleMedium?.copyWith(color: colors.primary),
              ),
            ),
            IconButton(
              style: IconButton.styleFrom(
                minimumSize: const Size.square(40),
                tapTargetSize: MaterialTapTargetSize.shrinkWrap,
              ),
              tooltip: tr('resetReasoning'),
              onPressed: enabled && current != initial
                  ? () => _commit({...config, 'effort': initial})
                  : null,
              icon: const AppIcon('refresh', size: 20),
            ),
          ],
        ),
        TextButton(
          key: const ValueKey('choose-model'),
          style: TextButton.styleFrom(
            minimumSize: const Size(40, 36),
            padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 4),
            tapTargetSize: MaterialTapTargetSize.shrinkWrap,
          ),
          onPressed: !enabled
              ? null
              : () async {
                  final selection = await showAppSheet<Map<String, dynamic>>(
                    context,
                    tr('modelPicker'),
                    child: ModelChoices(providers: providers, config: config),
                  );
                  if (selection != null) _commit({...config, ...selection});
                },
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              Flexible(
                child: Text(
                  text(config['model']).isEmpty
                      ? tr('conversationNoModel')
                      : text(config['model']),
                  overflow: TextOverflow.ellipsis,
                ),
              ),
              const SizedBox(width: 8),
              const AppIcon('chevron', size: 16),
            ],
          ),
        ),
        if (efforts.isNotEmpty) ...[
          SliderTheme(
            data: SliderTheme.of(context).copyWith(
              trackHeight: 18,
              thumbShape: const RoundSliderThumbShape(enabledThumbRadius: 13),
              overlayShape: const RoundSliderOverlayShape(overlayRadius: 22),
            ),
            child: Slider(
              key: const ValueKey('reasoning-slider'),
              value: selected < 0 ? 0 : selected.toDouble(),
              max: efforts.length > 1 ? (efforts.length - 1).toDouble() : 1,
              divisions: efforts.length > 1 ? efforts.length - 1 : 1,
              semanticFormatterCallback: (value) =>
                  efforts[value.round().clamp(0, efforts.length - 1)],
              onChanged: enabled && efforts.length > 1
                  ? (value) =>
                        onChanged({...config, 'effort': efforts[value.round()]})
                  : null,
              onChangeEnd: enabled && efforts.length > 1
                  ? (value) => onCommitted?.call({
                      ...config,
                      'effort': efforts[value.round()],
                    })
                  : null,
            ),
          ),
        ],
        const SizedBox(height: 12),
      ],
    );
  }
}

class ModelChoices extends StatelessWidget {
  const ModelChoices({
    super.key,
    required this.providers,
    required this.config,
  });
  final List<Map<String, dynamic>> providers;
  final Map<String, dynamic> config;

  @override
  Widget build(BuildContext context) => Column(
    mainAxisSize: MainAxisSize.min,
    crossAxisAlignment: CrossAxisAlignment.stretch,
    children: [
      for (final provider in providers.where((p) => p['enabled'] == true))
        ExpansionTile(
          key: PageStorageKey('model-provider-${provider['id']}'),
          initiallyExpanded: provider['id'] == config['provider'],
          tilePadding: const EdgeInsets.symmetric(horizontal: 8),
          childrenPadding: const EdgeInsets.only(left: 12),
          title: Text(
            text(provider['name']),
            style: Theme.of(context).textTheme.labelLarge,
          ),
          children: [
            for (final model in objects(provider['models']))
              ListTile(
                title: Text(text(model['id'])),
                trailing:
                    provider['id'] == config['provider'] &&
                        model['id'] == config['model']
                    ? const AppIcon('check', size: 18)
                    : null,
                onTap: () => Navigator.pop(context, {
                  'provider': provider['id'],
                  'credential': provider['credential'],
                  'model': model['id'],
                  'effort': model['default_effort'] ?? 'default',
                }),
              ),
          ],
        ),
    ],
  );
}
