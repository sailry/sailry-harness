import 'package:flutter/material.dart';
import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import '../../../runtime/session.dart';
import '../../../ui/toast.dart';
import 'configuration_commands.dart';
import 'configuration_fields.dart';
import 'presentation.dart' show failureLabel;
import 'team.dart';

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

  /// The latest admitted revision, shared by configuration and team commands.
  late final _session = ValueNotifier<Map<String, dynamic>>(widget.session);
  late final _commands = ConfigurationCommands(
    command: widget.host.command,
    execute: widget.host.execute,
    accept: _accept,
    active: () => mounted,
  );
  late final _team = TeamChoices(
    listenable: Listenable.merge([_session, widget.host]),
    read: () =>
        SessionTeam(_session.value, objects(widget.host.snapshot['teams'])),
    onTeam: _selectTeam,
    onMember: (role, model) => _run(
      () => _commands.send(
        'set_session_member_model',
        memberCommand(_id, _revision, role, model),
      ),
    ),
  );
  bool _saving = false;

  String get _id => widget.session['id'] as String;
  int get _revision => _session.value['revision'] as int;

  @override
  void dispose() {
    _session.dispose();
    super.dispose();
  }

  void _accept(Map<String, dynamic> response) {
    final session = object(response['data']);
    _session.value = session;
    setState(() {
      _config = Map.of(object(session['config']));
    });
  }

  Future<void> _run(
    Future<void> Function() action, {
    bool recovery = false,
  }) async {
    if (_saving || (!recovery && _commands.pending) || !widget.host.connected) {
      return;
    }
    setState(() => _saving = true);
    try {
      await action();
    } catch (error) {
      if (mounted) {
        showToast(context, failureLabel(error, translate: context.tr));
      }
    } finally {
      if (mounted) setState(() => _saving = false);
    }
  }

  Future<void> _save(Map<String, dynamic> config) =>
      _run(() => _sendConfig(config));

  Future<void> _sendConfig(Map<String, dynamic> config) async {
    setState(() => _config = config);
    await _commands.send('set_session_config', {
      'session': _id,
      'expected_revision': _revision,
      'config': config,
    });
  }

  /// The Node applies the execution mode and any fixed team lead model together.
  Future<void> _selectTeam(String execution, Map<String, dynamic>? team) =>
      _run(
        () => _commands.select(execution, team, session: () => _session.value),
      );

  List<Map<String, dynamic>> get _providers => objects(
    widget.host.snapshot['providers'],
  ).where((provider) => provider['enabled'] == true).toList();

  Future<void> _check() => _run(_commands.check, recovery: true);

  @override
  Widget build(BuildContext context) {
    final providers = _providers;
    final enabled = widget.host.connected && !_commands.pending;
    // Keep the controls' appearance stable during admission. Disabling every
    // control briefly on each selection makes the whole sheet flash.
    return AbsorbPointer(
      absorbing: _saving,
      child: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          ConfigurationFields(
            host: widget.host,
            providers: providers,
            config: _config,
            enabled: enabled,
            onChanged: (config) {
              if (!_saving) setState(() => _config = config);
            },
            onCommitted: _save,
            team: widget.session['delegation'] == null ? _team : null,
          ),
          if (_commands.pending)
            TextButton(
              onPressed: widget.host.connected ? _check : null,
              child: Text(context.tr('conversationCheckResult')),
            ),
        ],
      ),
    );
  }
}
