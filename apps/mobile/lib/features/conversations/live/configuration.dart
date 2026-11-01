import 'package:flutter/material.dart';
import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import '../../../runtime/session.dart';
import '../../../ui/toast.dart';
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
  late final _team = TeamChoices(
    listenable: Listenable.merge([_session, widget.host]),
    read: () =>
        SessionTeam(_session.value, objects(widget.host.snapshot['teams'])),
    onTeam: _selectTeam,
    onMember: (role, model) => _run(
      'set_session_member_model',
      (revision) => memberCommand(_id, revision, role, model),
    ),
  );
  bool _saving = false;
  String? _pending;

  String get _id => widget.session['id'] as String;

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
      _pending = null;
    });
  }

  /// Admits one revision-checked session command; false when it was not applied.
  Future<bool> _run(
    String kind,
    Map<String, dynamic> Function(int revision) data, {
    Map<String, dynamic>? config,
  }) async {
    if (_saving || _pending != null || !widget.host.connected) return false;
    setState(() {
      if (config != null) _config = config;
      _saving = true;
    });
    try {
      final response = await widget.host.command(
        kind,
        data(_session.value['revision'] as int),
      );
      if (mounted) _accept(response);
      return mounted;
    } catch (error) {
      if (mounted) {
        if (error is CommandFailure && error.code == 'outcome_unknown') {
          setState(() => _pending = error.request);
        }
        showToast(context, failureLabel(error, translate: context.tr));
      }
      return false;
    } finally {
      if (mounted) setState(() => _saving = false);
    }
  }

  Future<void> _save(Map<String, dynamic> config) async {
    await _run(
      'set_session_config',
      (revision) => {
        'session': _id,
        'expected_revision': revision,
        'config': config,
      },
      config: config,
    );
  }

  /// A team that fixes a Lead model also changes the configuration; the Node
  /// never changes it implicitly.
  Future<void> _selectTeam(Map<String, dynamic>? team) async {
    final selected = await _run(
      'set_session_team',
      (revision) => selectCommand(_id, revision, team),
    );
    if (!selected || team == null) return;
    final config = leadConfig(_config, team, _providers);
    if (config != null) await _save(config);
  }

  List<Map<String, dynamic>> get _providers => objects(
    widget.host.snapshot['providers'],
  ).where((provider) => provider['enabled'] == true).toList();

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
    final providers = _providers;
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
