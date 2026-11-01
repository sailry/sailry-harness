import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import '../../../runtime/notices.dart';
import '../../../ui/kit.dart';
import 'controller.dart';
import 'parts.dart';
import 'rules.dart';

/// Discovered models picked for adding, with their catalog references.
typedef Discovered = ({List<Json> models, Map<String, Json> references});

/// Lists the models the saved provider reports and returns the picked ones.
Future<Discovered?> showDiscoverySheet(
  BuildContext context,
  ProviderController controller,
) => showAppSheet<Discovered>(
  context,
  context.tr('providerFetchTitle'),
  scroll: false,
  child: _Discovery(controller: controller),
);

class _Discovery extends StatefulWidget {
  const _Discovery({required this.controller});
  final ProviderController controller;

  @override
  State<_Discovery> createState() => _DiscoveryState();
}

class _DiscoveryState extends State<_Discovery> {
  List<Json>? _models;
  Map<String, Json> _references = {};
  Object? _error;
  final _picked = <String>{};
  final _search = TextEditingController();
  late final Set<Object?> _added = {
    for (final model in models(widget.controller.provider)) model['id'],
  };

  @override
  void initState() {
    super.initState();
    _search.addListener(() => setState(() {}));
    _load();
  }

  @override
  void dispose() {
    _search.dispose();
    super.dispose();
  }

  Future<void> _load() async {
    setState(() => _error = null);
    try {
      final found = await widget.controller.discover();
      final references = await widget.controller.references([
        for (final model in found)
          if (!_added.contains(model['id'])) text(model['id']),
      ]);
      if (!mounted) return;
      setState(() {
        _models = found;
        _references = references;
      });
    } catch (error) {
      if (mounted) setState(() => _error = error);
    }
  }

  List<Json> get _visible {
    final query = _search.text.trim().toLowerCase();
    return [
      for (final model in _models ?? const <Json>[])
        if (query.isEmpty ||
            text(model['id']).toLowerCase().contains(query) ||
            text(model['display_name']).toLowerCase().contains(query))
          model,
    ];
  }

  @override
  Widget build(BuildContext context) {
    final found = _models;
    if (found == null && _error != null) {
      return FailureState(
        icon: 'spark',
        message: failureText(_error!, translate: context.tr),
        onRetry: _load,
      );
    }
    if (found == null) {
      return const LoadingOverlay(
        loading: true,
        minHeight: 160,
        child: SizedBox.shrink(),
      );
    }
    final open = [
      for (final model in _visible)
        if (!_added.contains(model['id'])) text(model['id']),
    ];
    final all = open.isNotEmpty && open.every(_picked.contains);
    return Column(
      mainAxisSize: MainAxisSize.min,
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Row(
          children: [
            Expanded(
              child: Text(
                context
                    .tr('providerFetched')
                    .replaceAll('{count}', '${found.length}'),
                style: dimText(context),
              ),
            ),
            TextButton(
              onPressed: open.isEmpty
                  ? null
                  : () => setState(
                      () =>
                          all ? _picked.removeAll(open) : _picked.addAll(open),
                    ),
              child: Text(
                context.tr(all ? 'providerSelectNone' : 'providerSelectAll'),
              ),
            ),
          ],
        ),
        const SizedBox(height: 4),
        TextField(
          controller: _search,
          decoration: InputDecoration(
            isDense: true,
            hintText: context.tr('providerSearch'),
            prefixIcon: const Padding(
              padding: EdgeInsets.all(10),
              child: AppIcon('search', size: 18),
            ),
            prefixIconConstraints: const BoxConstraints(minWidth: 38),
          ),
        ),
        const SizedBox(height: 8),
        Flexible(
          child: ListView(
            shrinkWrap: true,
            children: [
              for (final model in _visible)
                _Candidate(
                  model: model,
                  reference: _references[text(model['id'])],
                  added: _added.contains(model['id']),
                  picked: _picked.contains(model['id']),
                  onChanged: (picked) => setState(
                    () => picked
                        ? _picked.add(text(model['id']))
                        : _picked.remove(model['id']),
                  ),
                ),
            ],
          ),
        ),
        const SizedBox(height: 12),
        FilledButton(
          onPressed: _picked.isEmpty
              ? null
              : () => Navigator.pop(context, (
                  models: [
                    for (final model in found)
                      if (_picked.contains(model['id'])) model,
                  ],
                  references: _references,
                )),
          child: Text(
            context
                .tr('providerAddCount')
                .replaceAll('{count}', '${_picked.length}'),
          ),
        ),
      ],
    );
  }
}

class _Candidate extends StatelessWidget {
  const _Candidate({
    required this.model,
    required this.reference,
    required this.added,
    required this.picked,
    required this.onChanged,
  });
  final Json model;
  final Json? reference;
  final bool added;
  final bool picked;
  final ValueChanged<bool> onChanged;

  @override
  Widget build(BuildContext context) {
    final configured = configure(model, reference);
    final detail = added
        ? context.tr('providerAdded')
        : context
              .tr('providerContext')
              .replaceAll(
                '{tokens}',
                tokenLabel(number(configured['context']).toInt()),
              );
    return Opacity(
      opacity: added ? .45 : 1,
      child: InkWell(
        onTap: added ? null : () => onChanged(!picked),
        borderRadius: BorderRadius.circular(12),
        child: Padding(
          padding: const EdgeInsets.symmetric(vertical: 6),
          child: Row(
            children: [
              Checkbox(
                value: added || picked,
                onChanged: added ? null : (value) => onChanged(value == true),
              ),
              const SizedBox(width: 4),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      text(model['id']),
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: const TextStyle(
                        fontSize: 14,
                        fontFamily: 'monospace',
                      ),
                    ),
                    const SizedBox(height: 2),
                    Text(detail, style: dimText(context, 12)),
                  ],
                ),
              ),
              if (!added) ...[
                const SizedBox(width: 8),
                CapabilityIcons(configured),
              ],
            ],
          ),
        ),
      ),
    );
  }
}
