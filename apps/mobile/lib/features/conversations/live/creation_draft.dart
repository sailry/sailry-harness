import 'package:flutter/foundation.dart';

import '../../../runtime/json.dart';
import '../../../runtime/session.dart';

/// Captures a new session's resources until Node admission is resolved.
class CreationDraft extends ChangeNotifier {
  CreationDraft(this.host, {this.initialProject}) {
    _sync();
    host.addListener(_changed);
  }

  final HostConnection host;
  final String? initialProject;
  Map<String, dynamic>? project;
  Map<String, dynamic>? worktree;
  Map<String, dynamic> config = {};
  Map<String, dynamic>? _created;
  String? _pendingRequest;
  bool _busy = false;
  bool _uncertain = false;
  bool attaching = false;
  bool _defaultsCaptured = false;
  bool _configChosen = false;
  bool _disposed = false;

  Map<String, dynamic>? get created => _created;
  String? get pendingRequest => _pendingRequest;
  bool get busy => _busy;
  bool get uncertain => _uncertain;

  List<Map<String, dynamic>> get providers =>
      objects(host.snapshot['providers'])
          .where(
            (provider) =>
                provider['enabled'] == true &&
                objects(provider['models']).isNotEmpty,
          )
          .toList();

  List<Map<String, dynamic>> get worktrees => project == null
      ? []
      : objects(
          host.snapshot['worktrees'],
        ).where((tree) => tree['project'] == project!['id']).toList();

  bool get hasModel => providers.any(
    (provider) =>
        provider['id'] == config['provider'] &&
        objects(
          provider['models'],
        ).any((model) => model['id'] == config['model']),
  );

  void _changed() {
    if (busy || uncertain || attaching || created != null) return;
    _sync();
    notifyListeners();
  }

  void _sync() {
    final projects = objects(host.snapshot['projects']);
    project =
        projects.where((item) => item['id'] == project?['id']).firstOrNull ??
        projects.where((item) => item['id'] == initialProject).firstOrNull ??
        projects.firstOrNull;
    worktree =
        worktrees.where((tree) => tree['id'] == worktree?['id']).firstOrNull ??
        worktrees.firstOrNull;
    if (!_defaultsCaptured && !_configChosen) {
      final defaults = object(object(host.snapshot['defaults'])['config']);
      config = {
        'mode': defaults['mode'] ?? 'code',
        'permission': defaults['permission'] ?? 'ask',
        'options': defaults['options'] ?? <String, dynamic>{},
        'provider': defaults['provider'],
        'model': defaults['model'],
        'effort': defaults['effort'] ?? 'default',
        'credential': defaults['credential'],
      };
      _defaultsCaptured = host.snapshot.containsKey('defaults');
    }
    if (!hasModel && !_configChosen) {
      final provider = providers.firstOrNull;
      if (provider != null) {
        final models = objects(provider['models']);
        final model =
            models
                .where((model) => model['id'] == provider['default_model'])
                .firstOrNull ??
            models.first;
        config.addAll({
          'provider': provider['id'],
          'credential': provider['credential'],
          'model': model['id'],
          'effort': model['default_effort'] ?? 'default',
        });
      }
    }
  }

  void configure(Map<String, dynamic> value) {
    config = value;
    _configChosen = true;
  }

  Future<void> send({required bool hasInput}) async {
    if (busy || attaching || !host.connected) return;
    if (uncertain && pendingRequest == null) return;
    if (!uncertain &&
        (project != null && worktree == null || !hasModel || !hasInput)) {
      return;
    }
    _busy = true;
    notifyListeners();
    try {
      final result = uncertain
          ? await host.execute(pendingRequest!)
          : await host.command('create_session', {
              'project': project?['id'],
              'worktree': worktree?['id'],
              'config': Map<String, dynamic>.of(config),
            });
      if (!_disposed) _created = object(result['data']);
    } catch (error) {
      if (!_disposed) {
        if (error is CommandFailure && error.code == 'outcome_unknown') {
          _uncertain = true;
          _pendingRequest = error.request ?? pendingRequest;
        } else if (error is CommandFailure && error.rejected) {
          _uncertain = false;
          _pendingRequest = null;
        }
      }
      rethrow;
    } finally {
      if (!_disposed) {
        _busy = false;
        notifyListeners();
      }
    }
  }

  @override
  void dispose() {
    _disposed = true;
    host.removeListener(_changed);
    super.dispose();
  }
}
