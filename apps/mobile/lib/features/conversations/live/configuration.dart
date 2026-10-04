import 'package:flutter/material.dart';
import '../../../runtime/json.dart';
import '../../../runtime/session.dart';
import 'configuration_fields.dart';
import 'presentation.dart' show failureLabel;

class ConversationConfiguration extends StatefulWidget {
  const ConversationConfiguration({
    super.key,
    required this.host,
    required this.session,
  });
  final HostConnection host;
  final Map<String, dynamic> session;
  @override
  State<ConversationConfiguration> createState() =>
      _ConversationConfigurationState();
}

class _ConversationConfigurationState extends State<ConversationConfiguration> {
  late Map<String, dynamic> _config = Map.of(object(widget.session['config']));
  late int _revision = widget.session['revision'] as int;
  bool _saving = false;
  String? _error;

  Future<void> _save(Map<String, dynamic> config) async {
    if (_saving || !widget.host.connected) return;
    setState(() {
      _config = config;
      _saving = true;
      _error = null;
    });
    try {
      final response = await widget.host.command('set_session_config', {
        'session': widget.session['id'],
        'expected_revision': _revision,
        'config': config,
      });
      final session = object(response['data']);
      if (mounted) {
        setState(() {
          _revision = session['revision'] as int;
          _config = Map.of(object(session['config']));
        });
      }
    } catch (error) {
      if (mounted) {
        setState(() {
          _error = failureLabel(error);
        });
      }
    } finally {
      if (mounted) setState(() => _saving = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final providers = objects(
      widget.host.snapshot['providers'],
    ).where((provider) => provider['enabled'] == true).toList();
    final enabled = widget.host.connected;
    // Keep the controls' appearance stable during admission. Disabling every
    // control briefly on each selection makes the whole sheet flash.
    return AbsorbPointer(
      absorbing: _saving,
      child: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          ConfigurationFields(
            providers: providers,
            config: _config,
            enabled: enabled,
            onChanged: (config) {
              if (!_saving) setState(() => _config = config);
            },
            onCommitted: _save,
          ),
          if (_error != null)
            Padding(
              padding: const EdgeInsets.only(top: 12),
              child: Text(
                _error!,
                style: TextStyle(color: Theme.of(context).colorScheme.error),
              ),
            ),
        ],
      ),
    );
  }
}
