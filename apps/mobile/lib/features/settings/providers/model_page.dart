import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import '../../../ui/kit.dart';
import 'controller.dart';
import 'field_sheet.dart';
import 'parts.dart';
import 'reasoning_card.dart';
import 'rules.dart';

/// Settings of one model; every change saves the provider to its Node.
class ModelPage extends StatefulWidget {
  const ModelPage({super.key, required this.controller, required this.id});
  final ProviderController controller;
  final String id;

  @override
  State<ModelPage> createState() => _ModelPageState();
}

class _ModelPageState extends State<ModelPage> {
  /// Follows a renamed model.
  late String _id = widget.id;

  ProviderController get _controller => widget.controller;

  Future<bool> _save(Json model) async {
    if (!mounted) return false;
    final problem = limitsProblem(model);
    if (!accept(context, problem)) return false;
    final saved = await _controller.save(
      replaceModel(_controller.provider, _id, model),
    );
    if (saved && mounted) setState(() => _id = text(model['id']));
    return saved;
  }

  Future<void> _editId(Json model) => editText(
    context,
    title: context.tr('providerModelId'),
    initial: _id,
    check: (id) => id != _id && findModel(_controller.provider, id) != null
        ? 'providerModelExists'
        : null,
    onSave: (id) async => id == _id || await _save(renameModel(model, id)),
  );

  Future<void> _editName(Json model) => editText(
    context,
    title: context.tr('providerDisplayName'),
    initial: text(model['display_name']),
    optional: true,
    onSave: (name) async =>
        name == text(model['display_name']) ||
        await _save(
          name.isEmpty
              ? ({...model}..remove('display_name'))
              : {...model, 'display_name': name},
        ),
  );

  Future<void> _editTokens(Json model, String field, String label) async {
    final current = model[field];
    await editText(
      context,
      title: context.tr(label),
      initial: current is num ? '${current.toInt()}' : '',
      numeric: true,
      optional: field == 'compaction_tokens',
      hint: field == 'compaction_tokens' ? context.tr('providerDefault') : null,
      check: (value) =>
          parseTokens(value) == null ? 'providerNumberInvalid' : null,
      onSave: (value) async {
        final tokens = parseTokens(value);
        return tokens == current || await _save({...model, field: tokens});
      },
    );
  }

  Future<void> _remove(Json model) async {
    if (!await confirmDelete(context, modelLabel(model)) || !mounted) return;
    final removed = await attempt(
      context,
      () => _controller.save(removeModel(_controller.provider, _id)),
    );
    if (removed && mounted) Navigator.pop(context);
  }

  String _tokens(Object? value) =>
      value is num ? tokenLabel(value.toInt()) : context.tr('providerDefault');

  @override
  Widget build(BuildContext context) => ListenableBuilder(
    listenable: _controller,
    builder: (context, _) {
      final provider = _controller.provider;
      final model = findModel(provider, _id);
      if (model == null) return PageFrame(title: _id, child: const SizedBox());
      final idle = !_controller.busy;
      final isDefault = provider['default_model'] == _id;
      return PageFrame(
        title: modelLabel(model),
        loading: _controller.busy,
        subtitle: Text(
          '${text(provider['name'])} · $_id',
          maxLines: 1,
          overflow: TextOverflow.ellipsis,
          style: dimText(context, 12),
        ),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          spacing: 16,
          children: [
            SectionCard(
              title: context.tr('providerBasic'),
              children: [
                FieldRow(
                  label: context.tr('providerModelId'),
                  value: _id,
                  mono: true,
                  onTap: idle ? () => _editId(model) : null,
                ),
                FieldRow(
                  label: context.tr('providerDisplayName'),
                  value: text(model['display_name']),
                  onTap: idle ? () => _editName(model) : null,
                ),
              ],
            ),
            SectionCard(
              title: context.tr('providerContextTitle'),
              children: [
                for (final (field, label) in [
                  ('context', 'providerContextLength'),
                  ('output', 'providerMaxOutput'),
                  ('compaction_tokens', 'providerCompaction'),
                ])
                  FieldRow(
                    label: context.tr(label),
                    value: _tokens(model[field]),
                    onTap: idle ? () => _editTokens(model, field, label) : null,
                  ),
              ],
            ),
            SectionCard(
              title: context.tr('providerCapabilities'),
              children: [
                for (final name in capabilityNames)
                  SwitchRow(
                    icon: capabilityIcon(name),
                    label: context.tr(capabilityLabel(name)),
                    value: model[name] == true,
                    onChanged: idle
                        ? (value) => attempt(
                            context,
                            () => _save(setCapability(model, name, value)),
                          )
                        : null,
                  ),
              ],
            ),
            if (model['reasoning'] == true)
              ReasoningCard(
                model: model,
                onSave: idle
                    ? (model) => attempt(context, () => _save(model))
                    : null,
              ),
            ActionCard(
              label: context.tr(
                isDefault ? 'providerIsDefault' : 'providerSetDefault',
              ),
              onTap: idle && !isDefault
                  ? () => attempt(
                      context,
                      () =>
                          _controller.save({...provider, 'default_model': _id}),
                    )
                  : null,
            ),
            ActionCard(
              label: context.tr('providerDeleteModel'),
              danger: true,
              onTap: idle ? () => _remove(model) : null,
            ),
          ],
        ),
      );
    },
  );
}
