import 'package:flutter/material.dart';
import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import '../../../runtime/session.dart';
import '../../../ui/toast.dart';
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
  String? _pending;

  void _accept(Map<String, dynamic> response) {
    final session = object(response['data']);
    setState(() {
      _revision = session['revision'] as int;
      _config = Map.of(object(session['config']));
      _pending = null;
    });
  }

  Future<void> _save(Map<String, dynamic> config) async {
    if (_saving || _pending != null || !widget.host.connected) return;
    setState(() {
      _config = config;
      _saving = true;
    });
    try {
      final response = await widget.host.command('set_session_config', {
        'session': widget.session['id'],
        'expected_revision': _revision,
        'config': config,
      });
      if (mounted) _accept(response);
    } catch (error) {
      if (mounted) {
        if (error is CommandFailure && error.code == 'outcome_unknown') {
          setState(() => _pending = error.request);
        }
        showToast(context, failureLabel(error, translate: context.tr));
      }
    } finally {
      if (mounted) setState(() => _saving = false);
    }
  }

  Future<void> _check() async {
    final request = _pending;
    if (_saving || request == null || !widget.host.connected) return;
    setState(() => _saving = true);
    try {
      final response = await widget.host.execute(request);
      if (mounted) _accept(response);
    } catch (error) {
      if (mounted) {
        if (error is CommandFailure && error.rejected) {
          setState(() => _pending = null);
        }
        showToast(context, failureLabel(error, translate: context.tr));
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
    final enabled = widget.host.connected && _pending == null;
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
          if (_pending != null)
            TextButton(
              onPressed: widget.host.connected ? _check : null,
              child: Text(context.tr('conversationCheckResult')),
            ),
        ],
      ),
    );
  }
}
