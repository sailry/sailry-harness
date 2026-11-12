import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import '../../../runtime/session.dart' show HostConnection;
import '../../../ui/kit.dart';
import 'api.dart' as provider_api;
import 'controller.dart';
import 'field_sheet.dart';
import 'parts.dart';
import 'rules.dart';

/// Collects connection fields and saves a new provider; returns the Node's
/// copy, or null when dismissed.
Future<Json?> createProvider(
  BuildContext context, {
  required HostConnection host,
  required Set<String> names,
}) => showAppSheet<Json>(
  context,
  context.tr('providerNew'),
  child: _CreateForm(host: host, names: names),
);

class _CreateForm extends StatefulWidget {
  const _CreateForm({required this.host, required this.names});
  final HostConnection host;
  final Set<String> names;

  @override
  State<_CreateForm> createState() => _CreateFormState();
}

class _CreateFormState extends State<_CreateForm> {
  final _name = TextEditingController();
  final _endpoint = TextEditingController();
  final _key = TextEditingController();
  String _api = provider_api.values.first;
  bool _hidden = true;
  bool _busy = false;

  @override
  void dispose() {
    _name.dispose();
    _endpoint.dispose();
    _key.dispose();
    super.dispose();
  }

  String? _problem() {
    final name = _name.text.trim();
    if (name.isEmpty) return 'settingsRequired';
    if (widget.names.contains(name)) return 'configDuplicate';
    if (provider_api.fixedEndpoint(_api) == null &&
        _endpoint.text.trim().isEmpty) {
      return 'settingsRequired';
    }
    return null;
  }

  Future<void> _save() async {
    if (_busy || !accept(context, _problem())) return;
    setState(() => _busy = true);
    final key = _key.text.trim();
    final provider = newProvider(
      id: newId(),
      name: _name.text.trim(),
      api: _api,
      endpoint: provider_api.fixedEndpoint(_api) ?? _endpoint.text.trim(),
    );
    Json? saved;
    await attempt(context, () async {
      saved = await ProviderController.create(
        widget.host,
        provider,
        secret: key.isEmpty ? null : key,
      );
      return true;
    });
    if (!mounted) return;
    if (saved != null) {
      Navigator.pop(context, saved);
    } else {
      setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) => LoadingOverlay(
    loading: _busy,
    child: FormBody(
      children: [
        TextField(
          controller: _name,
          enabled: !_busy,
          decoration: InputDecoration(labelText: context.tr('configName')),
        ),
        SelectField<String>(
          value: _api,
          label: context.tr('settingsApi'),
          options: [
            for (final api in provider_api.values)
              (api, apiLabel(context, api)),
          ],
          onChanged: _busy ? null : (api) => setState(() => _api = api),
        ),
        if (provider_api.fixedEndpoint(_api) == null)
          TextField(
            controller: _endpoint,
            enabled: !_busy,
            keyboardType: TextInputType.url,
            autocorrect: false,
            decoration: InputDecoration(
              labelText: context.tr('configEndpoint'),
            ),
          ),
        TextField(
          controller: _key,
          enabled: !_busy,
          obscureText: _hidden,
          autocorrect: false,
          enableSuggestions: false,
          decoration: InputDecoration(
            labelText: context.tr('settingsApiKey'),
            suffixIcon: IconButton(
              tooltip: context.tr(
                _hidden ? 'providerShowKey' : 'providerHideKey',
              ),
              onPressed: () => setState(() => _hidden = !_hidden),
              icon: const AppIcon('eye', size: 18),
            ),
          ),
        ),
        FilledButton(
          onPressed: _busy ? null : _save,
          child: Text(context.tr('save')),
        ),
      ],
    ),
  );
}
