import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import '../../../runtime/notices.dart';
import '../../../ui/kit.dart';
import 'api.dart' as provider_api;
import 'controller.dart';
import 'field_sheet.dart';
import 'parts.dart';
import 'rules.dart';

const _masked = '••••••••';

/// Connection fields of a provider; each row edits and saves one field.
/// Preset providers keep the API and endpoint their preset defines.
class ConnectionCard extends StatefulWidget {
  const ConnectionCard({
    super.key,
    required this.controller,
    required this.names,
  });
  final ProviderController controller;
  final Set<String> names;

  @override
  State<ConnectionCard> createState() => _ConnectionCardState();
}

class _ConnectionCardState extends State<ConnectionCard> {
  /// Revealed key, held only while this card shows it.
  String? _key;
  bool _reading = false;

  ProviderController get _controller => widget.controller;
  Json get _provider => _controller.provider;

  Future<void> _save(Json next, {String? secret}) async {
    setState(() => _key = null);
    await attempt(context, () => _controller.save(next, secret: secret));
  }

  Future<String?> _readKey() async {
    if (_provider['credential'] == null) return '';
    setState(() => _reading = true);
    try {
      return await _controller.readKey();
    } catch (error) {
      if (mounted) showFailure(context, error);
      return null;
    } finally {
      if (mounted) setState(() => _reading = false);
    }
  }

  Future<void> _reveal() async {
    if (_key != null) {
      setState(() => _key = null);
      return;
    }
    final key = await _readKey();
    if (mounted && key != null) setState(() => _key = key);
  }

  Future<void> _editName() async {
    final name = await editText(
      context,
      title: context.tr('configName'),
      initial: text(_provider['name']),
      check: (value) => widget.names.contains(value) ? 'configDuplicate' : null,
    );
    if (name != null && name != _provider['name'] && mounted) {
      await _save({..._provider, 'name': name});
    }
  }

  Future<void> _editApi() async {
    final api = await pickApi(context, text(_provider['api']));
    if (api == null || api == _provider['api'] || !mounted) return;
    await _save({
      ..._provider,
      'api': api,
      'endpoint': provider_api.fixedEndpoint(api) ?? _provider['endpoint'],
    });
  }

  Future<void> _editEndpoint() async {
    final endpoint = await editText(
      context,
      title: context.tr('configEndpoint'),
      initial: text(_provider['endpoint']),
    );
    if (endpoint != null && endpoint != _provider['endpoint'] && mounted) {
      await _save({..._provider, 'endpoint': endpoint});
    }
  }

  Future<void> _editKey() async {
    final current = _key ?? await _readKey();
    if (current == null || !mounted) return;
    final key = await editText(
      context,
      title: context.tr('settingsApiKey'),
      initial: current,
      optional: true,
      secret: true,
    );
    if (key == null || key == current || !mounted) return;
    await _save(
      key.isEmpty ? {..._provider, 'credential': null} : _provider,
      secret: key.isEmpty ? null : key,
    );
  }

  @override
  Widget build(BuildContext context) {
    final provider = _provider;
    final api = text(provider['api']);
    final preset = provider['preset'] != null;
    final idle = !_controller.busy && !_reading;
    final stored = provider['credential'] != null;
    return SectionCard(
      title: context.tr('providerConnection'),
      children: [
        FieldRow(
          label: context.tr('configName'),
          value: text(provider['name']),
          onTap: idle ? _editName : null,
        ),
        FieldRow(
          label: context.tr('settingsApi'),
          value: apiLabel(context, api),
          onTap: idle && !preset && provider_api.editable(api)
              ? _editApi
              : null,
        ),
        FieldRow(
          label: context.tr('configEndpoint'),
          value: text(provider['endpoint']),
          onTap: idle && !preset && provider_api.fixedEndpoint(api) == null
              ? _editEndpoint
              : null,
        ),
        if (usesKey(provider))
          FieldRow(
            label: context.tr('settingsApiKey'),
            value: _key ?? (stored ? _masked : context.tr('providerKeyUnset')),
            mono: _key != null,
            onTap: idle ? _editKey : null,
            trailing: stored
                ? IconButton(
                    tooltip: context.tr(
                      _key == null ? 'providerShowKey' : 'providerHideKey',
                    ),
                    visualDensity: VisualDensity.compact,
                    onPressed: idle ? _reveal : null,
                    icon: const AppIcon('eye', size: 18),
                  )
                : null,
          ),
        SwitchRow(
          label: context.tr('settingsEnabled'),
          value: provider['enabled'] != false,
          onChanged: idle
              ? (enabled) => _save({...provider, 'enabled': enabled})
              : null,
        ),
      ],
    );
  }
}
