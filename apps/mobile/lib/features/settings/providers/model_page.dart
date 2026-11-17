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
  String? _imageRoute;

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

  /// An Azure OpenAI model is named by its deployment, as on Desktop.
  String get _idLabel => context.tr(
    _controller.provider['api'] == 'azure_open_ai'
        ? 'providerDeploymentId'
        : 'providerModelId',
  );

  Future<void> _editId(Json model) => editText(
    context,
    title: _idLabel,
    initial: _id,
    check: (id) => id != _id && findModel(_controller.provider, id) != null
        ? 'providerModelExists'
        : null,
    onSave: (id) async => id == _id || await _save(renameModel(model, id)),
  );

  Future<void> _editContext(Json model) async {
    final current = model['context'];
    await editText(
      context,
      title: context.tr('providerContextLength'),
      initial: current is num ? '${current.toInt()}' : '',
      numeric: true,
      check: (value) =>
          parseTokens(value) == null ? 'providerNumberInvalid' : null,
      onSave: (value) async {
        final tokens = parseTokens(value);
        return tokens == current ||
            tokens != null && await _save(setContextLimit(model, tokens));
      },
    );
  }

  Future<bool> _setGeneration(Json model, String kind, bool enabled) async {
    final originalRoute = imageGeneration(model);
    final edited = setGeneration(
      model,
      kind,
      enabled,
      _controller.provider,
      imageRoute: _imageRoute,
    );
    final saved = await _save(edited);
    if (saved && kind == 'image') {
      _imageRoute = imageGeneration(edited) ?? originalRoute ?? _imageRoute;
    }
    return saved;
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
      Widget capability(String name) => SwitchRow(
        icon: capabilityIcon(name),
        label: context.tr(capabilityLabel(name)),
        value: model[name] == true,
        onChanged: idle
            ? (value) => attempt(
                context,
                () => _save(setCapability(model, name, value)),
              )
            : null,
      );
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
            // Desktop's model fields: the ID, the context limit, then the
            // capabilities in its three groups. The display name, the output
            // limit and the compaction threshold come from the catalog. A
            // smaller context bounds the hidden output limit when saved.
            SectionCard(
              title: context.tr('providerBasic'),
              children: [
                FieldRow(
                  label: _idLabel,
                  value: _id,
                  mono: true,
                  onTap: idle ? () => _editId(model) : null,
                ),
                FieldRow(
                  label: context.tr('providerContextLength'),
                  value: _tokens(model['context']),
                  onTap: idle ? () => _editContext(model) : null,
                ),
              ],
            ),
            SectionCard(
              title: context.tr('providerCapabilities'),
              children: [
                _GroupLabel(context.tr('providerCapUnderstands')),
                capability('vision'),
                _GroupLabel(context.tr('providerCapCan')),
                capability('tools'),
                capability('reasoning'),
                _GroupLabel(context.tr('providerCapGenerates')),
                for (final kind in generationKinds)
                  SwitchRow(
                    icon: generationIcon(kind),
                    label: context.tr(generationLabel(kind)),
                    value: generates(model, kind),
                    onChanged: idle
                        ? (value) => attempt(
                            context,
                            () => _setGeneration(model, kind, value),
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

/// A capability group's dim heading inside the capabilities card.
class _GroupLabel extends StatelessWidget {
  const _GroupLabel(this.label);
  final String label;

  @override
  Widget build(BuildContext context) => Padding(
    padding: const EdgeInsets.only(top: 12, bottom: 4),
    child: Text(label, style: dimText(context, 12)),
  );
}
